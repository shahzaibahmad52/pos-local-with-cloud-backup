//! Importing many products at once from the owner's Excel sheet. The till
//! reads the sheet; this checks every row, shows what would happen, and
//! then saves the good rows in one go.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::User,
    db,
    error::{AppError, AppResult},
    products::{self, NewBarcode, NewProduct, NewUnit, ReceiveStock},
};

pub const MAX_ROWS: usize = 5000;

/// One sheet row. Money is in paisa; the till converts from rupees.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportRow {
    /// Row number in the sheet, for messages.
    pub row: i64,
    pub name: String,
    pub generic_name: Option<String>,
    pub company: Option<String>,
    pub category: Option<String>,
    /// `medicine` or `general`
    pub kind: String,
    /// `unit` or `weight`
    pub sell_mode: String,
    pub base_unit: String,
    pub base_price: Option<i64>,
    pub base_sellable: Option<bool>,
    pub pack2_name: Option<String>,
    pub pack2_qty: Option<i64>,
    pub pack2_price: Option<i64>,
    pub pack3_name: Option<String>,
    pub pack3_qty: Option<i64>,
    pub pack3_price: Option<i64>,
    pub price_per_kg: Option<i64>,
    pub barcode: Option<String>,
    pub barcode_pack: Option<String>,
    pub rack: Option<String>,
    pub tax_rate_bp: Option<i64>,
    pub rx_required: Option<bool>,
    pub reorder_level: Option<i64>,
    pub opening_qty: Option<i64>,
    pub batch_no: Option<String>,
    pub expiry: Option<String>,
    pub cost_per_base: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowResult {
    pub row: i64,
    pub name: String,
    /// `new`, `update` or `error`
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub committed: bool,
    pub new_count: i64,
    pub update_count: i64,
    pub error_count: i64,
    pub rows: Vec<RowResult>,
}

fn clean(s: &Option<String>) -> Option<String> {
    s.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// The pack levels a row describes, smallest first.
fn units_of(r: &ImportRow) -> Result<Vec<(NewUnit, bool)>, String> {
    let mut units = Vec::new();
    let base = r.base_unit.trim();
    if base.is_empty() {
        return Err("Smallest unit is missing (Tab, Cap, Btl, Pc…).".into());
    }
    let packs = [(&r.pack2_name, r.pack2_qty, r.pack2_price), (&r.pack3_name, r.pack3_qty, r.pack3_price)];
    let base_price = match r.base_price {
        Some(p) => p,
        None => {
            // Work the smallest unit's price out from the first pack's price.
            let first = packs.iter().find_map(|(n, q, p)| clean(n).and(q.zip(*p)));
            match first {
                Some((q, p)) if q > 0 => crate::pricing::round_div(p, q),
                _ => return Err(format!("Price is missing (price of one {} or of a pack).", base.to_lowercase())),
            }
        }
    };
    units.push((
        NewUnit { name: base.to_string(), name_ur: None, base_qty: 1, price: base_price },
        r.base_sellable.unwrap_or(true),
    ));
    for (name, qty, price) in packs {
        let Some(name) = clean(name) else { continue };
        let qty = qty.ok_or_else(|| format!("How many {base} in a {name}?"))?;
        if qty <= 1 {
            return Err(format!("A {name} must hold more than one {base}."));
        }
        let price = price.unwrap_or(base_price * qty);
        if units.iter().any(|(u, _): &(NewUnit, bool)| u.name.eq_ignore_ascii_case(&name)) {
            return Err(format!("Pack name {name} is used twice."));
        }
        units.push((NewUnit { name, name_ur: None, base_qty: qty, price }, true));
    }
    if units.iter().any(|(u, _)| u.price < 0) {
        return Err("Prices can't be negative.".into());
    }
    Ok(units)
}

fn to_product(r: &ImportRow) -> Result<NewProduct, String> {
    let kind = match r.kind.trim().to_lowercase().as_str() {
        "" | "medicine" | "med" | "m" => "medicine",
        "general" | "gen" | "g" => "general",
        other => return Err(format!("Type \"{other}\" should be Medicine or General.")),
    };
    let sell_mode = match r.sell_mode.trim().to_lowercase().as_str() {
        "" | "unit" | "pack" | "packs" => "unit",
        "weight" | "kg" => "weight",
        other => return Err(format!("Sold by \"{other}\" should be Pack or Weight.")),
    };
    let tax = r.tax_rate_bp.unwrap_or(0);
    if !(0..=10_000).contains(&tax) {
        return Err("Sales tax must be between 0% and 100%.".into());
    }
    let (units, base_unit) = if sell_mode == "weight" {
        if r.price_per_kg.unwrap_or(0) <= 0 {
            return Err("Price per kg is missing.".into());
        }
        (Vec::new(), "g".to_string())
    } else {
        let units = units_of(r)?;
        (units.into_iter().map(|(u, _)| u).collect(), r.base_unit.trim().to_string())
    };
    Ok(NewProduct {
        name: r.name.trim().to_string(),
        generic_name: clean(&r.generic_name),
        company: clean(&r.company),
        category: clean(&r.category),
        kind: kind.into(),
        sell_mode: sell_mode.into(),
        base_unit,
        price_per_kg: r.price_per_kg,
        tax_rate_bp: tax,
        rx_required: r.rx_required.unwrap_or(false),
        controlled: false,
        cold_chain: false,
        rack: clean(&r.rack),
        reorder_level: r.reorder_level.unwrap_or(0).max(0),
        units,
        barcodes: clean(&r.barcode)
            .map(|code| vec![NewBarcode { code, unit_name: clean(&r.barcode_pack) }])
            .unwrap_or_default(),
    })
}

/// What one row will do, before anything is saved.
enum Plan {
    New(NewProduct, Option<ReceiveStock>),
    Update { id: String, product: NewProduct, base_sellable: bool },
}

fn plan_row(conn: &Connection, r: &ImportRow) -> Result<Plan, String> {
    if r.name.trim().is_empty() {
        return Err("Product name is missing.".into());
    }
    let product = to_product(r)?;
    let barcode = clean(&r.barcode);
    let by_barcode: Option<(String, String)> = match &barcode {
        Some(code) => conn
            .query_row(
                "SELECT p.id, p.name FROM barcodes b JOIN products p ON p.id = b.product_id WHERE b.code = ?1",
                [code],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?,
        None => None,
    };
    let by_name: Option<String> = conn
        .query_row("SELECT id FROM products WHERE name = ?1 COLLATE NOCASE", [product.name.as_str()], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    let existing = match (by_barcode, by_name) {
        (Some((a, other)), Some(b)) if a != b => {
            return Err(format!("Barcode {} is already used by \"{other}\".", barcode.unwrap_or_default()))
        }
        // A new name with a barcode that is already on a product is most likely a mistake in the sheet.
        (Some((_, other)), None) => {
            return Err(format!("Barcode {} is already used by \"{other}\".", barcode.unwrap_or_default()))
        }
        (Some((a, _)), _) => Some(a),
        (None, b) => b,
    };
    let cost = r.cost_per_base.unwrap_or(0);
    if cost < 0 {
        return Err("Cost can't be negative.".into());
    }
    if cost > 0 {
        let lowest_per_base = product
            .units
            .iter()
            .map(|u| u.price as f64 / u.base_qty as f64)
            .chain(product.price_per_kg.map(|k| k as f64 / 1000.0))
            .fold(f64::INFINITY, f64::min);
        if (cost as f64) > lowest_per_base + 0.5 {
            return Err("Sale price is lower than cost.".into());
        }
    }
    if let Some(id) = existing {
        let mode: String = conn
            .query_row("SELECT sell_mode FROM products WHERE id = ?1", [&id], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if mode != product.sell_mode {
            return Err("Already exists, sold a different way (pack vs weight). Change it on the product page.".into());
        }
        // Only prices are updated; a pack holding a different number than the sheet says is flagged.
        for u in &product.units {
            let have: Option<i64> = conn
                .query_row(
                    "SELECT base_qty FROM product_units WHERE product_id = ?1 AND name = ?2 COLLATE NOCASE",
                    params![id, u.name],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            if let Some(q) = have.filter(|q| *q != u.base_qty) {
                return Err(format!(
                    "{} holds {q} in the store but {} in the sheet. Change the pack on the product page.",
                    u.name, u.base_qty
                ));
            }
        }
        return Ok(Plan::Update { id, product, base_sellable: r.base_sellable.unwrap_or(true) });
    }
    let stock = match r.opening_qty.unwrap_or(0) {
        0 => None,
        q if q < 0 => return Err("Opening stock can't be negative.".into()),
        q => {
            let expiry = clean(&r.expiry);
            if let Some(e) = &expiry {
                if !products::valid_month(e) {
                    return Err(format!("Expiry \"{e}\" should look like 2028-08."));
                }
            } else if product.kind == "medicine" {
                return Err("Expiry is missing for the opening stock.".into());
            }
            Some(ReceiveStock {
                product_id: String::new(),
                batch_no: clean(&r.batch_no).unwrap_or_else(|| "-".into()),
                expiry,
                base_qty: q,
                cost_per_base: cost,
                unit_prices: Vec::new(),
                reason: "opening".into(),
            })
        }
    };
    Ok(Plan::New(product, stock))
}

/// Updates an existing product's prices (and fills in rack, category and
/// reorder level when the sheet has them). Stock is never changed here.
fn apply_update(conn: &Connection, id: &str, p: &NewProduct, base_sellable: bool) -> AppResult<()> {
    if p.sell_mode == "weight" {
        conn.execute("UPDATE products SET price_per_kg = ?1 WHERE id = ?2", params![p.price_per_kg, id])?;
    }
    for (i, u) in p.units.iter().enumerate() {
        let sellable = i > 0 || base_sellable;
        let n = conn.execute(
            "UPDATE product_units SET price = ?1, sellable = ?2 WHERE product_id = ?3 AND name = ?4 COLLATE NOCASE",
            params![u.price, sellable, id, u.name],
        )?;
        if n == 0 {
            conn.execute(
                "INSERT INTO product_units (id, product_id, name, base_qty, price, sellable, sort)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![db::new_id(), id, u.name, u.base_qty, u.price, sellable, i as i64],
            )?;
        }
    }
    conn.execute(
        "UPDATE products SET rack = COALESCE(?1, rack), category = COALESCE(?2, category),
            reorder_level = CASE WHEN ?3 > 0 THEN ?3 ELSE reorder_level END, updated_at = ?4
         WHERE id = ?5",
        params![p.rack, p.category, p.reorder_level, db::now(), id],
    )?;
    for b in &p.barcodes {
        let unit_id: Option<String> = match &b.unit_name {
            Some(n) => conn
                .query_row(
                    "SELECT id FROM product_units WHERE product_id = ?1 AND name = ?2 COLLATE NOCASE",
                    params![id, n],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };
        conn.execute(
            "INSERT INTO barcodes (code, product_id, unit_id) VALUES (?1, ?2, ?3) ON CONFLICT (code) DO NOTHING",
            params![b.code, id, unit_id],
        )?;
    }
    db::outbox(conn, "product", id, "upsert", &json!({ "id": id, "name": p.name }))?;
    Ok(())
}

/// Checks every row; with `commit`, saves the good ones in one transaction.
pub fn run(conn: &mut Connection, user: &User, rows: &[ImportRow], commit: bool) -> AppResult<ImportReport> {
    if rows.is_empty() {
        return Err(AppError::BadRequest("The sheet has no product rows.".into()));
    }
    if rows.len() > MAX_ROWS {
        return Err(AppError::BadRequest(format!("Import at most {MAX_ROWS} rows at a time.")));
    }
    let tx = conn.transaction()?;
    let mut results = Vec::with_capacity(rows.len());
    let mut seen_names: HashMap<String, i64> = HashMap::new();
    let mut seen_codes: HashMap<String, i64> = HashMap::new();
    let (mut new_count, mut update_count, mut error_count) = (0, 0, 0);
    for r in rows {
        let name = r.name.trim().to_string();
        let mut outcome = plan_row(&tx, r);
        if outcome.is_ok() {
            if let Some(first) = seen_names.get(&name.to_lowercase()) {
                outcome = Err(format!("Same product as row {first}."));
            } else if let Some(first) = clean(&r.barcode).and_then(|c| seen_codes.get(&c).copied()) {
                outcome = Err(format!("Same barcode as row {first}."));
            }
        }
        seen_names.entry(name.to_lowercase()).or_insert(r.row);
        if let Some(code) = clean(&r.barcode) {
            seen_codes.entry(code).or_insert(r.row);
        }
        let (status, message) = match outcome {
            Err(m) => {
                error_count += 1;
                ("error", Some(m))
            }
            Ok(Plan::New(product, stock)) => {
                new_count += 1;
                if commit {
                    // Each row is checked again as it is saved, so a bad row
                    // can't stop the others.
                    let saved = tx.execute_batch("SAVEPOINT import_row").map_err(AppError::from).and_then(|_| {
                        let id = products::insert_product(&tx, &product, &user.id)?;
                        if let Some(mut s) = stock {
                            s.product_id = id;
                            products::insert_stock(&tx, &s, &user.id)?;
                        }
                        Ok(())
                    });
                    match saved {
                        Ok(()) => tx.execute_batch("RELEASE import_row")?,
                        Err(e) => {
                            tx.execute_batch("ROLLBACK TO import_row; RELEASE import_row")?;
                            new_count -= 1;
                            error_count += 1;
                            results.push(RowResult {
                                row: r.row,
                                name,
                                status: "error".into(),
                                message: Some(e.to_string()),
                            });
                            continue;
                        }
                    }
                }
                ("new", None)
            }
            Ok(Plan::Update { id, product, base_sellable }) => {
                update_count += 1;
                if commit {
                    apply_update(&tx, &id, &product, base_sellable)?;
                }
                let note = r.opening_qty.filter(|q| *q > 0).map(|_| {
                    "Already exists: prices will be updated. Opening stock is ignored; receive it in Purchases."
                        .to_string()
                });
                ("update", note)
            }
        };
        results.push(RowResult { row: r.row, name, status: status.into(), message });
    }
    if commit {
        db::audit(
            &tx,
            Some(&user.id),
            None,
            "product.import",
            &json!({ "new": new_count, "updated": update_count, "skipped": error_count }),
            None,
        )?;
        tx.commit()?;
    }
    // Without commit the transaction is dropped, which rolls it back.
    Ok(ImportReport { committed: commit, new_count, update_count, error_count, rows: results })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panadol() -> ImportRow {
        ImportRow {
            row: 2,
            name: "Panadol 500mg".into(),
            kind: "Medicine".into(),
            base_unit: "Tab".into(),
            pack2_name: Some("Strip".into()),
            pack2_qty: Some(10),
            pack2_price: Some(3000),
            pack3_name: Some("Box".into()),
            pack3_qty: Some(200),
            ..Default::default()
        }
    }

    #[test]
    fn prices_fill_in_from_the_strip() {
        let units = units_of(&panadol()).unwrap();
        let prices: Vec<(String, i64, i64)> =
            units.iter().map(|(u, _)| (u.name.clone(), u.base_qty, u.price)).collect();
        assert_eq!(prices, vec![("Tab".into(), 1, 300), ("Strip".into(), 10, 3000), ("Box".into(), 200, 60000)]);
    }

    #[test]
    fn rejects_bad_rows() {
        let mut r = panadol();
        r.pack2_qty = None;
        assert!(units_of(&r).unwrap_err().contains("price of one tab"));
        r.base_price = Some(300);
        assert!(units_of(&r).unwrap_err().contains("Strip"));
        let mut r = panadol();
        r.kind = "food".into();
        assert!(to_product(&r).is_err());
    }
}
