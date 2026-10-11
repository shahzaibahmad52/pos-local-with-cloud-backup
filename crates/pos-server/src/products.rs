use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    db,
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitView {
    pub id: String,
    pub name: String,
    pub name_ur: Option<String>,
    pub base_qty: i64,
    /// Price the till should charge now: the earliest-expiry batch's printed
    /// price if it has one, otherwise the product's default.
    pub price: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductView {
    pub id: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub company: Option<String>,
    pub category: Option<String>,
    pub kind: String,
    pub sell_mode: String,
    pub base_unit: String,
    pub price_per_kg: Option<i64>,
    pub tax_rate_bp: i64,
    pub rx_required: bool,
    pub controlled: bool,
    pub cold_chain: bool,
    pub rack: Option<String>,
    pub units: Vec<UnitView>,
    /// Sellable (not expired) stock in base units.
    pub stock_base: i64,
    pub next_batch_no: Option<String>,
    pub next_expiry: Option<String>,
}

/// The batch the next sale will take stock from: earliest expiry first,
/// batches without expiry last, expired batches never.
pub fn first_sellable_batch(
    conn: &Connection,
    product_id: &str,
) -> AppResult<Option<(String, String, Option<String>)>> {
    Ok(conn
        .query_row(
            "SELECT id, batch_no, expiry FROM batches
             WHERE product_id = ?1 AND qty_on_hand > 0 AND (expiry IS NULL OR expiry >= ?2)
             ORDER BY expiry IS NULL, expiry, received_at
             LIMIT 1",
            params![product_id, db::current_month()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

pub fn effective_unit_price(
    conn: &Connection,
    batch_id: Option<&str>,
    unit_id: &str,
    default_price: i64,
) -> AppResult<i64> {
    let Some(batch_id) = batch_id else { return Ok(default_price) };
    Ok(conn
        .query_row(
            "SELECT price FROM batch_unit_prices WHERE batch_id = ?1 AND unit_id = ?2",
            params![batch_id, unit_id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(default_price))
}

pub fn product_view(conn: &Connection, id: &str) -> AppResult<Option<ProductView>> {
    let row = conn
        .query_row(
            "SELECT id, name, generic_name, company, category, kind, sell_mode, base_unit,
                    price_per_kg, tax_rate_bp, rx_required, controlled, cold_chain, rack
             FROM products WHERE id = ?1 AND active = 1",
            [id],
            |r| {
                Ok(ProductView {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    generic_name: r.get(2)?,
                    company: r.get(3)?,
                    category: r.get(4)?,
                    kind: r.get(5)?,
                    sell_mode: r.get(6)?,
                    base_unit: r.get(7)?,
                    price_per_kg: r.get(8)?,
                    tax_rate_bp: r.get(9)?,
                    rx_required: r.get::<_, i64>(10)? != 0,
                    controlled: r.get::<_, i64>(11)? != 0,
                    cold_chain: r.get::<_, i64>(12)? != 0,
                    rack: r.get(13)?,
                    units: Vec::new(),
                    stock_base: 0,
                    next_batch_no: None,
                    next_expiry: None,
                })
            },
        )
        .optional()?;
    let Some(mut p) = row else { return Ok(None) };

    p.stock_base = conn.query_row(
        "SELECT COALESCE(SUM(qty_on_hand), 0) FROM batches
         WHERE product_id = ?1 AND (expiry IS NULL OR expiry >= ?2)",
        params![p.id, db::current_month()],
        |r| r.get(0),
    )?;

    let first = first_sellable_batch(conn, &p.id)?;
    if let Some((_, batch_no, expiry)) = &first {
        p.next_batch_no = Some(batch_no.clone());
        p.next_expiry = expiry.clone();
    }
    let first_id = first.as_ref().map(|b| b.0.as_str());

    let mut stmt = conn.prepare(
        "SELECT id, name, name_ur, base_qty, price FROM product_units
         WHERE product_id = ?1 AND sellable = 1 ORDER BY sort, base_qty",
    )?;
    let units = stmt
        .query_map([&p.id], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (uid, name, name_ur, base_qty, price) in units {
        let price = effective_unit_price(conn, first_id, &uid, price)?;
        p.units.push(UnitView { id: uid, name, name_ur, base_qty, price });
    }
    Ok(Some(p))
}

/// Search by barcode (exact), name or generic name (contains). Barcode hits
/// come first so a scan followed by Enter adds the right item.
pub fn search(conn: &Connection, query: &str, limit: i64) -> AppResult<Vec<ProductView>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let like = format!("%{}%", q.replace('%', "\\%").replace('_', "\\_"));
    let mut stmt = conn.prepare(
        "SELECT p.id FROM products p
         WHERE p.active = 1 AND (
             p.id IN (SELECT product_id FROM barcodes WHERE code = ?1)
             OR p.name LIKE ?2 ESCAPE '\\'
             OR p.generic_name LIKE ?2 ESCAPE '\\'
         )
         ORDER BY p.id IN (SELECT product_id FROM barcodes WHERE code = ?1) DESC,
                  p.name LIKE ?3 ESCAPE '\\' DESC,
                  p.name COLLATE NOCASE
         LIMIT ?4",
    )?;
    let prefix = format!("{}%", q.replace('%', "\\%").replace('_', "\\_"));
    let ids =
        stmt.query_map(params![q, like, prefix, limit], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(p) = product_view(conn, &id)? {
            out.push(p);
        }
    }
    Ok(out)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarcodeHit {
    pub product: ProductView,
    pub unit_id: Option<String>,
}

pub fn by_barcode(conn: &Connection, code: &str) -> AppResult<Option<BarcodeHit>> {
    let hit: Option<(String, Option<String>)> = conn
        .query_row("SELECT product_id, unit_id FROM barcodes WHERE code = ?1", [code.trim()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let Some((product_id, unit_id)) = hit else { return Ok(None) };
    Ok(product_view(conn, &product_id)?.map(|product| BarcodeHit { product, unit_id }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewUnit {
    pub name: String,
    pub name_ur: Option<String>,
    pub base_qty: i64,
    pub price: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewBarcode {
    pub code: String,
    /// Which pack level this barcode is printed on (e.g. "Box").
    pub unit_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProduct {
    pub name: String,
    pub generic_name: Option<String>,
    pub company: Option<String>,
    pub category: Option<String>,
    pub kind: String,
    #[serde(default = "default_sell_mode")]
    pub sell_mode: String,
    pub base_unit: String,
    pub price_per_kg: Option<i64>,
    #[serde(default)]
    pub tax_rate_bp: i64,
    #[serde(default)]
    pub rx_required: bool,
    #[serde(default)]
    pub controlled: bool,
    #[serde(default)]
    pub cold_chain: bool,
    pub rack: Option<String>,
    #[serde(default)]
    pub reorder_level: i64,
    #[serde(default)]
    pub units: Vec<NewUnit>,
    #[serde(default)]
    pub barcodes: Vec<NewBarcode>,
}

fn default_sell_mode() -> String {
    "unit".into()
}

pub fn create_product(conn: &mut Connection, input: &NewProduct, user_id: &str) -> AppResult<ProductView> {
    let tx = conn.transaction()?;
    let id = insert_product(&tx, input, user_id)?;
    tx.commit()?;
    product_view(conn, &id)?.ok_or_else(|| AppError::Internal("new product not found".into()))
}

/// Checks and saves a new product inside the caller's transaction. Returns its id.
pub fn insert_product(tx: &Connection, input: &NewProduct, user_id: &str) -> AppResult<String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("Product name is required.".into()));
    }
    if !matches!(input.kind.as_str(), "medicine" | "general") {
        return Err(AppError::BadRequest("Type must be medicine or general.".into()));
    }
    if !(0..=10_000).contains(&input.tax_rate_bp) {
        return Err(AppError::BadRequest("Tax rate must be between 0% and 100%.".into()));
    }
    match input.sell_mode.as_str() {
        "unit" => {
            if input.units.is_empty() {
                return Err(AppError::BadRequest("Add at least one pack size (e.g. Tab or Strip).".into()));
            }
            for u in &input.units {
                if u.name.trim().is_empty()
                    || u.base_qty <= 0
                    || u.price < 0
                    || u.base_qty > crate::pricing::MAX_QTY
                    || u.price > crate::pricing::MAX_AMOUNT
                {
                    return Err(AppError::BadRequest(format!(
                        "Pack size \"{}\" needs a name, a quantity above 0 and a price.",
                        u.name
                    )));
                }
            }
        }
        "weight" => {
            if input.price_per_kg.unwrap_or(0) <= 0 {
                return Err(AppError::BadRequest("Set a price per kg.".into()));
            }
        }
        _ => return Err(AppError::BadRequest("Sell mode must be unit or weight.".into())),
    }

    let id = db::new_id();
    let now = db::now();
    let base_unit = if input.sell_mode == "weight" { "g" } else { input.base_unit.trim() };
    tx.execute(
        "INSERT INTO products (id, name, generic_name, company, category, kind, sell_mode, base_unit,
            price_per_kg, tax_rate_bp, rx_required, controlled, cold_chain, rack, reorder_level,
            created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
        params![
            id,
            name,
            input.generic_name.as_deref().map(str::trim),
            input.company.as_deref().map(str::trim),
            input.category,
            input.kind,
            input.sell_mode,
            base_unit,
            if input.sell_mode == "weight" { input.price_per_kg } else { None },
            input.tax_rate_bp,
            input.rx_required as i64,
            input.controlled as i64,
            input.cold_chain as i64,
            input.rack,
            input.reorder_level,
            now
        ],
    )?;
    if input.sell_mode == "unit" {
        for (i, u) in input.units.iter().enumerate() {
            tx.execute(
                "INSERT INTO product_units (id, product_id, name, name_ur, base_qty, price, sort)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![db::new_id(), id, u.name.trim(), u.name_ur, u.base_qty, u.price, i as i64],
            )?;
        }
    }
    for b in &input.barcodes {
        let code = b.code.trim();
        if code.is_empty() {
            continue;
        }
        let unit_id: Option<String> = match &b.unit_name {
            Some(n) => tx
                .query_row("SELECT id FROM product_units WHERE product_id = ?1 AND name = ?2", params![id, n], |r| {
                    r.get(0)
                })
                .optional()?,
            None => None,
        };
        let taken: Option<String> =
            tx.query_row("SELECT product_id FROM barcodes WHERE code = ?1", [code], |r| r.get(0)).optional()?;
        if taken.is_some() {
            return Err(AppError::Conflict(format!("Barcode {code} is already used by another product.")));
        }
        tx.execute("INSERT INTO barcodes (code, product_id, unit_id) VALUES (?1, ?2, ?3)", params![code, id, unit_id])?;
    }
    db::audit(tx, Some(user_id), None, "product.create", &json!({ "productId": id, "name": name }), None)?;
    db::outbox(tx, "product", &id, "upsert", &json!({ "id": id, "name": name }))?;
    Ok(id)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitPrice {
    pub unit_id: String,
    pub price: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveStock {
    pub product_id: String,
    pub batch_no: String,
    /// `YYYY-MM`
    pub expiry: Option<String>,
    pub base_qty: i64,
    #[serde(default)]
    pub cost_per_base: i64,
    /// Prices printed on this batch's packs, if different from the defaults.
    #[serde(default)]
    pub unit_prices: Vec<UnitPrice>,
    /// `opening` or `purchase`
    #[serde(default = "default_receive_reason")]
    pub reason: String,
}

fn default_receive_reason() -> String {
    "purchase".into()
}

pub fn valid_month(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 7 || b[4] != b'-' {
        return false;
    }
    let (Ok(year), Ok(month)) = (s[..4].parse::<u32>(), s[5..].parse::<u32>()) else {
        return false;
    };
    (2000..=2100).contains(&year) && (1..=12).contains(&month)
}

/// Adds stock to a batch (creating the batch if it is new).
pub fn receive_stock(conn: &mut Connection, input: &ReceiveStock, user_id: &str) -> AppResult<ProductView> {
    let tx = conn.transaction()?;
    insert_stock(&tx, input, user_id)?;
    tx.commit()?;
    product_view(conn, &input.product_id)?.ok_or_else(|| AppError::Internal("product vanished".into()))
}

/// Adds stock inside the caller's transaction.
pub fn insert_stock(tx: &Connection, input: &ReceiveStock, user_id: &str) -> AppResult<()> {
    if input.base_qty <= 0 || input.base_qty > crate::pricing::MAX_QTY {
        return Err(AppError::BadRequest("Quantity must be more than 0.".into()));
    }
    if input.cost_per_base < 0 {
        return Err(AppError::BadRequest("Cost can't be negative.".into()));
    }
    if let Some(e) = &input.expiry {
        if !valid_month(e) {
            return Err(AppError::BadRequest("Expiry must look like 2028-08.".into()));
        }
    }
    if !matches!(input.reason.as_str(), "opening" | "purchase") {
        return Err(AppError::BadRequest("Reason must be opening or purchase.".into()));
    }
    let batch_no = input.batch_no.trim();
    if batch_no.is_empty() {
        return Err(AppError::BadRequest("Batch number is required (use \"-\" if the pack has none).".into()));
    }

    let exists: bool = tx
        .query_row("SELECT 1 FROM products WHERE id = ?1 AND active = 1", [&input.product_id], |_| Ok(()))
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::NotFound("Product not found.".into()));
    }
    // Same as a purchase: the cost is averaged and a different expiry is a different batch.
    let batch_id = crate::purchases::add_to_batch(
        tx,
        &input.product_id,
        batch_no,
        input.expiry.as_deref(),
        input.base_qty,
        input.cost_per_base,
    )?;
    for up in &input.unit_prices {
        let belongs: bool = tx
            .query_row(
                "SELECT 1 FROM product_units WHERE id = ?1 AND product_id = ?2",
                params![up.unit_id, input.product_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !belongs || up.price < 0 {
            return Err(AppError::BadRequest("Batch price is for a pack size of another product.".into()));
        }
        tx.execute(
            "INSERT INTO batch_unit_prices (batch_id, unit_id, price) VALUES (?1, ?2, ?3)
             ON CONFLICT (batch_id, unit_id) DO UPDATE SET price = excluded.price",
            params![batch_id, up.unit_id, up.price],
        )?;
    }
    tx.execute(
        "INSERT INTO stock_moves (id, product_id, batch_id, qty_delta, reason, user_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![db::new_id(), input.product_id, batch_id, input.base_qty, input.reason, user_id, db::now()],
    )?;
    db::outbox(
        tx,
        "stock",
        &batch_id,
        "receive",
        &json!({ "productId": input.product_id, "batchId": batch_id, "qty": input.base_qty }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_format() {
        assert!(valid_month("2028-08"));
        assert!(!valid_month("2028-13"));
        assert!(!valid_month("08/2028"));
        assert!(!valid_month("2028-8"));
    }
}

/// Everything an extra till needs to keep selling if the shop network drops.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    pub store: crate::sales::StoreHeader,
    pub products: Vec<ProductView>,
    /// Barcode → (product id, pack id).
    pub barcodes: Vec<(String, String, Option<String>)>,
    pub max_discount_bp: i64,
    pub weight_round_to: i64,
    /// The highest offline receipt number this till has sent in, so a till
    /// whose local counter was wiped doesn't reuse a number.
    pub last_offline_no: i64,
    pub generated_at: String,
}

pub fn catalogue(conn: &Connection, till_code: &str) -> AppResult<Catalogue> {
    let ids: Vec<String> = conn
        .prepare("SELECT id FROM products WHERE active = 1 ORDER BY name COLLATE NOCASE")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut products = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(p) = product_view(conn, &id)? {
            products.push(p);
        }
    }
    let barcodes = conn
        .prepare("SELECT b.code, b.product_id, b.unit_id FROM barcodes b JOIN products p ON p.id = b.product_id WHERE p.active = 1")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    Ok(Catalogue {
        store: crate::sales::store_header(conn)?,
        products,
        barcodes,
        max_discount_bp: db::setting_i64(conn, "cashier_max_discount_bp", 500)?,
        weight_round_to: db::setting_i64(conn, "weight_round_to_paisa", 100)?,
        last_offline_no: last_offline_no(conn, till_code)?,
        generated_at: db::now(),
    })
}

fn last_offline_no(conn: &Connection, till_code: &str) -> AppResult<i64> {
    let nos: Vec<String> = conn
        .prepare("SELECT receipt_no FROM sales WHERE till_code = ?1 AND receipt_no LIKE '%-X%'")?
        .query_map([till_code], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(nos.iter().filter_map(|n| n.rsplit_once("-X").and_then(|(_, d)| d.parse::<i64>().ok())).max().unwrap_or(0))
}
