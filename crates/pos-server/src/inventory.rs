//! Backoffice inventory: the product list with stock and value, one
//! product's full details, editing products and correcting stock.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::User,
    db,
    error::{AppError, AppResult},
    products::{self, NewBarcode, NewUnit},
    purchases::{self, LastBuy},
};

/// Medicines expiring within this many months (counting this one) are "expiring soon".
pub const EXPIRY_WARN_MONTHS: u32 = 3;

/// The month `months` after the current one, as `YYYY-MM`.
pub fn month_after(months: u32) -> String {
    use chrono::Datelike;
    let now = chrono::Local::now();
    let total = now.year() * 12 + now.month0() as i32 + months as i32;
    format!("{:04}-{:02}", total / 12, total % 12 + 1)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitPrice {
    pub id: String,
    pub name: String,
    pub base_qty: i64,
    /// What the till charges now (the next batch's printed price, or the default).
    pub price: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryRow {
    pub id: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub company: Option<String>,
    pub category: Option<String>,
    pub kind: String,
    pub sell_mode: String,
    pub base_unit: String,
    pub price_per_kg: Option<i64>,
    pub rack: Option<String>,
    pub active: bool,
    pub reorder_level: i64,
    /// Sellable stock (not expired), base units.
    pub stock_base: i64,
    /// Expired stock still on the shelf, base units.
    pub expired_base: i64,
    /// Value of unexpired stock at cost (paisa).
    pub stock_value: i64,
    /// Average cost per base unit of unexpired stock (paisa).
    pub avg_cost: i64,
    pub next_expiry: Option<String>,
    pub low: bool,
    pub expiring: bool,
    pub units: Vec<UnitPrice>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySummary {
    pub product_count: i64,
    pub stock_value: i64,
    pub low_count: i64,
    pub expiring_count: i64,
    pub expired_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryPage {
    pub summary: InventorySummary,
    pub rows: Vec<InventoryRow>,
    /// Rows matching the filters, before paging.
    pub total: i64,
    pub categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryQuery {
    #[serde(default)]
    pub q: String,
    /// `all`, `low`, `expiring`, `expired`, `medicine`, `general`, `archived`
    #[serde(default)]
    pub tab: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub offset: i64,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

/// Per-product stock figures, computed in one pass. `?1` = this month,
/// `?2` = last month of the expiry warning window.
const STOCK_SQL: &str = "
    SELECT p.id, p.name, p.generic_name, p.company, p.category, p.kind, p.sell_mode, p.base_unit,
           p.price_per_kg, p.rack, p.active, p.reorder_level,
           COALESCE(SUM(CASE WHEN b.expiry IS NULL OR b.expiry >= ?1 THEN b.qty_on_hand END), 0) AS stock,
           COALESCE(SUM(CASE WHEN b.expiry < ?1 AND b.qty_on_hand > 0 THEN b.qty_on_hand END), 0) AS expired,
           COALESCE(SUM(CASE WHEN (b.expiry IS NULL OR b.expiry >= ?1) AND b.qty_on_hand > 0
                             THEN b.qty_on_hand * b.cost_per_base END), 0) AS value,
           MIN(CASE WHEN b.qty_on_hand > 0 AND b.expiry >= ?1 THEN b.expiry END) AS next_expiry
    FROM products p LEFT JOIN batches b ON b.product_id = p.id
    GROUP BY p.id";

fn stock_row(r: &rusqlite::Row, warn_until: &str) -> rusqlite::Result<InventoryRow> {
    let stock: i64 = r.get(12)?;
    let value: i64 = r.get(14)?;
    let reorder_level: i64 = r.get(11)?;
    let next_expiry: Option<String> = r.get(15)?;
    Ok(InventoryRow {
        id: r.get(0)?,
        name: r.get(1)?,
        generic_name: r.get(2)?,
        company: r.get(3)?,
        category: r.get(4)?,
        kind: r.get(5)?,
        sell_mode: r.get(6)?,
        base_unit: r.get(7)?,
        price_per_kg: r.get(8)?,
        rack: r.get(9)?,
        active: r.get(10)?,
        reorder_level,
        stock_base: stock,
        expired_base: r.get(13)?,
        stock_value: value,
        avg_cost: if stock > 0 { crate::pricing::round_div(value, stock) } else { 0 },
        low: reorder_level > 0 && stock <= reorder_level,
        expiring: next_expiry.as_deref().is_some_and(|e| e <= warn_until),
        next_expiry,
        units: Vec::new(),
    })
}

pub fn all_rows(conn: &Connection) -> AppResult<Vec<InventoryRow>> {
    let warn_until = month_after(EXPIRY_WARN_MONTHS - 1);
    let mut stmt = conn.prepare(&format!("{STOCK_SQL} ORDER BY p.name COLLATE NOCASE"))?;
    let rows =
        stmt.query_map(params![db::current_month()], |r| stock_row(r, &warn_until))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn unit_prices(conn: &Connection, product_id: &str) -> AppResult<Vec<UnitPrice>> {
    Ok(products::product_view(conn, product_id)?
        .map(|p| {
            p.units
                .into_iter()
                .map(|u| UnitPrice { id: u.id, name: u.name, base_qty: u.base_qty, price: u.price })
                .collect()
        })
        .unwrap_or_default())
}

pub fn categories(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT category FROM products WHERE category IS NOT NULL AND category <> ''
         ORDER BY category COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list(conn: &Connection, q: &InventoryQuery) -> AppResult<InventoryPage> {
    let rows = all_rows(conn)?;
    let active: Vec<&InventoryRow> = rows.iter().filter(|r| r.active).collect();
    let summary = InventorySummary {
        product_count: active.len() as i64,
        stock_value: active.iter().map(|r| r.stock_value).sum(),
        low_count: active.iter().filter(|r| r.low).count() as i64,
        expiring_count: active.iter().filter(|r| r.expiring).count() as i64,
        expired_count: active.iter().filter(|r| r.expired_base > 0).count() as i64,
    };
    let needle = q.q.trim().to_lowercase();
    let barcode_hit: Option<String> = if needle.is_empty() {
        None
    } else {
        conn.query_row("SELECT product_id FROM barcodes WHERE code = ?1", [q.q.trim()], |r| r.get(0)).optional()?
    };
    let mut matching: Vec<InventoryRow> = rows
        .into_iter()
        .filter(|r| match q.tab.as_str() {
            "archived" => !r.active,
            _ => r.active,
        })
        .filter(|r| match q.tab.as_str() {
            "low" => r.low,
            "expiring" => r.expiring,
            "expired" => r.expired_base > 0,
            "medicine" => r.kind == "medicine",
            "general" => r.kind == "general",
            _ => true,
        })
        .filter(|r| q.category.is_empty() || r.category.as_deref() == Some(q.category.as_str()))
        .filter(|r| {
            needle.is_empty()
                || barcode_hit.as_deref() == Some(r.id.as_str())
                || r.name.to_lowercase().contains(&needle)
                || r.generic_name.as_deref().is_some_and(|g| g.to_lowercase().contains(&needle))
                || r.company.as_deref().is_some_and(|c| c.to_lowercase().contains(&needle))
        })
        .collect();
    let total = matching.len() as i64;
    let limit = q.limit.clamp(1, 200) as usize;
    let offset = q.offset.max(0) as usize;
    let mut page: Vec<InventoryRow> = matching.drain(offset.min(matching.len())..).take(limit).collect();
    for r in page.iter_mut() {
        r.units = unit_prices(conn, &r.id)?;
    }
    Ok(InventoryPage { summary, rows: page, total, categories: categories(conn)? })
}

/// Products at or below their reorder level, and batches expiring soon.
pub fn low_stock(conn: &Connection, limit: usize) -> AppResult<Vec<InventoryRow>> {
    let mut rows: Vec<InventoryRow> = all_rows(conn)?.into_iter().filter(|r| r.active && r.low).collect();
    rows.sort_by(|a, b| {
        (a.stock_base * 1000 / a.reorder_level.max(1)).cmp(&(b.stock_base * 1000 / b.reorder_level.max(1)))
    });
    rows.truncate(limit);
    for r in rows.iter_mut() {
        r.units = unit_prices(conn, &r.id)?;
    }
    Ok(rows)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpiringBatch {
    pub product_id: String,
    pub product_name: String,
    pub batch_no: String,
    pub expiry: String,
    pub qty_base: i64,
    pub base_unit: String,
    pub expired: bool,
}

/// Batches with stock that expire before the end of the window, soonest first.
pub fn expiring_batches(conn: &Connection, months: u32, limit: i64) -> AppResult<Vec<ExpiringBatch>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, b.batch_no, b.expiry, b.qty_on_hand, p.base_unit, b.expiry < ?2
         FROM batches b JOIN products p ON p.id = b.product_id
         WHERE p.active = 1 AND b.qty_on_hand > 0 AND b.expiry IS NOT NULL AND b.expiry <= ?1
         ORDER BY b.expiry, p.name LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![month_after(months.saturating_sub(1)), db::current_month(), limit], |r| {
            Ok(ExpiringBatch {
                product_id: r.get(0)?,
                product_name: r.get(1)?,
                batch_no: r.get(2)?,
                expiry: r.get(3)?,
                qty_base: r.get(4)?,
                base_unit: r.get(5)?,
                expired: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitDetail {
    pub id: String,
    pub name: String,
    pub name_ur: Option<String>,
    pub base_qty: i64,
    /// Default price, used for batches without their own printed price.
    pub price: i64,
    pub sellable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchDetail {
    pub id: String,
    pub batch_no: String,
    pub expiry: Option<String>,
    pub cost_per_base: i64,
    pub qty_on_hand: i64,
    pub received_at: String,
    pub expired: bool,
    /// Prices printed on this batch, by unit id.
    pub prices: Vec<(String, i64)>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarcodeDetail {
    pub code: String,
    pub unit_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveDetail {
    pub created_at: String,
    pub batch_no: Option<String>,
    pub qty_delta: i64,
    pub reason: String,
    pub user_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductDetail {
    #[serde(flatten)]
    pub row: InventoryRow,
    pub tax_rate_bp: i64,
    pub rx_required: bool,
    pub controlled: bool,
    pub cold_chain: bool,
    pub all_units: Vec<UnitDetail>,
    pub barcodes: Vec<BarcodeDetail>,
    pub batches: Vec<BatchDetail>,
    pub last_buy: Option<LastBuy>,
    pub moves: Vec<MoveDetail>,
}

pub fn detail(conn: &Connection, id: &str) -> AppResult<ProductDetail> {
    let warn_until = month_after(EXPIRY_WARN_MONTHS - 1);
    let mut row = conn
        .query_row(&format!("SELECT * FROM ({STOCK_SQL}) WHERE id = ?2"), params![db::current_month(), id], |r| {
            stock_row(r, &warn_until)
        })
        .optional()?
        .ok_or_else(|| AppError::NotFound("Product not found.".into()))?;
    row.units = unit_prices(conn, id)?;
    let (tax_rate_bp, rx, controlled, cold): (i64, bool, bool, bool) = conn.query_row(
        "SELECT tax_rate_bp, rx_required, controlled, cold_chain FROM products WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let all_units = conn
        .prepare(
            "SELECT id, name, name_ur, base_qty, price, sellable FROM product_units WHERE product_id = ?1
             ORDER BY sort, base_qty",
        )?
        .query_map([id], |r| {
            Ok(UnitDetail {
                id: r.get(0)?,
                name: r.get(1)?,
                name_ur: r.get(2)?,
                base_qty: r.get(3)?,
                price: r.get(4)?,
                sellable: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let barcodes = conn
        .prepare(
            "SELECT b.code, u.name FROM barcodes b LEFT JOIN product_units u ON u.id = b.unit_id
             WHERE b.product_id = ?1 ORDER BY u.base_qty DESC",
        )?
        .query_map([id], |r| Ok(BarcodeDetail { code: r.get(0)?, unit_name: r.get(1)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    let month = db::current_month();
    let mut batches = conn
        .prepare(
            "SELECT id, batch_no, expiry, cost_per_base, qty_on_hand, received_at, expiry < ?2 FROM batches
             WHERE product_id = ?1 AND qty_on_hand <> 0 ORDER BY expiry IS NULL, expiry, received_at",
        )?
        .query_map(params![id, month], |r| {
            Ok(BatchDetail {
                id: r.get(0)?,
                batch_no: r.get(1)?,
                expiry: r.get(2)?,
                cost_per_base: r.get(3)?,
                qty_on_hand: r.get(4)?,
                received_at: r.get(5)?,
                expired: r.get::<_, Option<bool>>(6)?.unwrap_or(false),
                prices: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for b in batches.iter_mut() {
        b.prices = conn
            .prepare("SELECT unit_id, price FROM batch_unit_prices WHERE batch_id = ?1")?
            .query_map([&b.id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
    }
    let moves = conn
        .prepare(
            "SELECT m.created_at, b.batch_no, m.qty_delta, m.reason, u.name FROM stock_moves m
             LEFT JOIN batches b ON b.id = m.batch_id LEFT JOIN users u ON u.id = m.user_id
             WHERE m.product_id = ?1 AND m.reason <> 'sale' ORDER BY m.created_at DESC, m.rowid DESC LIMIT 15",
        )?
        .query_map([id], |r| {
            Ok(MoveDetail {
                created_at: r.get(0)?,
                batch_no: r.get(1)?,
                qty_delta: r.get(2)?,
                reason: r.get(3)?,
                user_name: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ProductDetail {
        row,
        tax_rate_bp,
        rx_required: rx,
        controlled,
        cold_chain: cold,
        all_units,
        barcodes,
        batches,
        last_buy: purchases::last_buy(conn, id)?,
        moves,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitEdit {
    /// Existing pack level; leave out to add a new one.
    pub id: Option<String>,
    #[serde(flatten)]
    pub unit: NewUnit,
    #[serde(default = "yes")]
    pub sellable: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductUpdate {
    pub name: String,
    pub generic_name: Option<String>,
    pub company: Option<String>,
    pub category: Option<String>,
    pub kind: String,
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
    pub units: Vec<UnitEdit>,
    #[serde(default)]
    pub barcodes: Vec<NewBarcode>,
    #[serde(default = "yes")]
    pub active: bool,
}

fn trimmed(s: &Option<String>) -> Option<String> {
    s.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// Saves changes to a product. How it is sold (packs or weight) can't change
/// once created; pack levels that are removed stop being sellable but stay
/// for old bills.
pub fn update(conn: &mut Connection, user: &User, id: &str, input: &ProductUpdate) -> AppResult<ProductDetail> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("Product name is required.".into()));
    }
    if !matches!(input.kind.as_str(), "medicine" | "general") {
        return Err(AppError::BadRequest("Type must be medicine or general.".into()));
    }
    if !(0..=10_000).contains(&input.tax_rate_bp) || input.reorder_level < 0 {
        return Err(AppError::BadRequest("Check the sales tax and reorder level.".into()));
    }
    let tx = conn.transaction()?;
    let sell_mode: String = tx
        .query_row("SELECT sell_mode FROM products WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::NotFound("Product not found.".into()))?;
    if sell_mode == "weight" {
        if input.price_per_kg.unwrap_or(0) <= 0 {
            return Err(AppError::BadRequest("Set a price per kg.".into()));
        }
    } else {
        if !input.units.iter().any(|u| u.sellable) {
            return Err(AppError::BadRequest("Keep at least one pack size the till can sell.".into()));
        }
        let mut names: Vec<String> = Vec::new();
        for u in &input.units {
            let n = u.unit.name.trim().to_lowercase();
            if n.is_empty()
                || u.unit.base_qty <= 0
                || u.unit.price < 0
                || u.unit.base_qty > crate::pricing::MAX_QTY
                || u.unit.price > crate::pricing::MAX_AMOUNT
            {
                return Err(AppError::BadRequest(format!(
                    "Pack size \"{}\" needs a name, a quantity above 0 and a price.",
                    u.unit.name
                )));
            }
            if names.contains(&n) {
                return Err(AppError::BadRequest(format!("Pack size \"{}\" is listed twice.", u.unit.name.trim())));
            }
            names.push(n);
        }
    }
    let base_unit = if sell_mode == "weight" { "g".to_string() } else { input.base_unit.trim().to_string() };
    if base_unit.is_empty() {
        return Err(AppError::BadRequest("Name the smallest unit (Tab, Cap, Pc…).".into()));
    }
    tx.execute(
        "UPDATE products SET name = ?1, generic_name = ?2, company = ?3, category = ?4, kind = ?5, base_unit = ?6,
            price_per_kg = ?7, tax_rate_bp = ?8, rx_required = ?9, controlled = ?10, cold_chain = ?11, rack = ?12,
            reorder_level = ?13, active = ?14, updated_at = ?15
         WHERE id = ?16",
        params![
            name,
            trimmed(&input.generic_name),
            trimmed(&input.company),
            trimmed(&input.category),
            input.kind,
            base_unit,
            if sell_mode == "weight" { input.price_per_kg } else { None },
            input.tax_rate_bp,
            input.rx_required,
            input.controlled,
            input.cold_chain,
            trimmed(&input.rack),
            input.reorder_level,
            input.active,
            db::now(),
            id
        ],
    )?;

    if sell_mode == "unit" {
        let existing = tx
            .prepare("SELECT id, name, base_qty FROM product_units WHERE product_id = ?1")?
            .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        // Rows that name their pack by id keep it, even if they rename it.
        let claimed: Vec<&str> = input
            .units
            .iter()
            .filter_map(|u| u.id.as_deref())
            .filter(|uid| existing.iter().any(|(e, _, _)| e == uid))
            .collect();
        let mut kept: Vec<String> = Vec::new();
        for (sort, u) in input.units.iter().enumerate() {
            // A new row with the name of a removed level brings that level back.
            let unit_id = u.id.clone().filter(|uid| claimed.contains(&uid.as_str())).or_else(|| {
                existing
                    .iter()
                    .filter(|(e, _, _)| !claimed.contains(&e.as_str()) && !kept.contains(e))
                    .find(|(_, n, _)| n.eq_ignore_ascii_case(u.unit.name.trim()))
                    .map(|(e, _, _)| e.clone())
            });
            match unit_id {
                Some(uid) => {
                    // Prices printed on batches were per the old pack size; they no longer apply.
                    if existing.iter().any(|(e, _, q)| *e == uid && *q != u.unit.base_qty) {
                        tx.execute("DELETE FROM batch_unit_prices WHERE unit_id = ?1", [&uid])?;
                    }
                    tx.execute(
                        "UPDATE product_units SET name = ?1, name_ur = ?2, base_qty = ?3, price = ?4, sellable = ?5,
                            sort = ?6 WHERE id = ?7",
                        params![
                            u.unit.name.trim(),
                            trimmed(&u.unit.name_ur),
                            u.unit.base_qty,
                            u.unit.price,
                            u.sellable,
                            sort as i64,
                            uid
                        ],
                    )
                    .map_err(|e| name_taken(e, &u.unit.name))?;
                    kept.push(uid);
                }
                None => {
                    let uid = db::new_id();
                    tx.execute(
                        "INSERT INTO product_units (id, product_id, name, name_ur, base_qty, price, sellable, sort)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            uid,
                            id,
                            u.unit.name.trim(),
                            trimmed(&u.unit.name_ur),
                            u.unit.base_qty,
                            u.unit.price,
                            u.sellable,
                            sort as i64
                        ],
                    )
                    .map_err(|e| name_taken(e, &u.unit.name))?;
                    kept.push(uid);
                }
            }
        }
        for (uid, _, _) in existing.iter().filter(|(e, _, _)| !kept.contains(e)) {
            tx.execute("UPDATE product_units SET sellable = 0 WHERE id = ?1", [uid])?;
        }
    }

    tx.execute("DELETE FROM barcodes WHERE product_id = ?1", [id])?;
    for b in &input.barcodes {
        let code = b.code.trim();
        if code.is_empty() {
            continue;
        }
        let taken: Option<String> = tx
            .query_row(
                "SELECT p.name FROM barcodes b JOIN products p ON p.id = b.product_id WHERE b.code = ?1",
                [code],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(other) = taken {
            return Err(AppError::Conflict(format!("Barcode {code} is already used by {other}."))
                .because("barcode.taken", json!({ "code": code, "product": other })));
        }
        let unit_id: Option<String> = match &b.unit_name {
            Some(n) => tx
                .query_row(
                    "SELECT id FROM product_units WHERE product_id = ?1 AND name = ?2 COLLATE NOCASE",
                    params![id, n.trim()],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };
        tx.execute("INSERT INTO barcodes (code, product_id, unit_id) VALUES (?1, ?2, ?3)", params![code, id, unit_id])?;
    }
    db::audit(&tx, Some(&user.id), None, "product.update", &json!({ "name": name, "active": input.active }), None)?;
    db::outbox(&tx, "product", id, "upsert", &json!({ "id": id, "name": name }))?;
    tx.commit()?;
    detail(conn, id)
}

pub const ADJUST_REASONS: &[&str] = &["damaged", "expired", "lost", "count", "other"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdjustInput {
    pub batch_id: String,
    /// What is actually on the shelf now, in base units.
    pub new_qty: i64,
    pub reason: String,
    pub note: Option<String>,
}

/// Corrects a batch's stock (breakage, expired stock thrown away, a recount).
pub fn adjust(conn: &mut Connection, user: &User, input: &AdjustInput) -> AppResult<ProductDetail> {
    if input.new_qty < 0 || input.new_qty > crate::pricing::MAX_QTY {
        return Err(AppError::BadRequest("Stock can't be below 0.".into()));
    }
    if !ADJUST_REASONS.contains(&input.reason.as_str()) {
        return Err(AppError::BadRequest("Choose why the stock is changing.".into()));
    }
    let tx = conn.transaction()?;
    let (product_id, batch_no, old, cost, name): (String, String, i64, i64, String) = tx
        .query_row(
            "SELECT b.product_id, b.batch_no, b.qty_on_hand, b.cost_per_base, p.name
             FROM batches b JOIN products p ON p.id = b.product_id WHERE b.id = ?1",
            [&input.batch_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("Batch not found.".into()))?;
    let delta = input.new_qty - old;
    if delta == 0 {
        return Err(AppError::BadRequest("That is the same as the stock now.".into()));
    }
    tx.execute("UPDATE batches SET qty_on_hand = ?1 WHERE id = ?2", params![input.new_qty, input.batch_id])?;
    let note = trimmed(&input.note);
    tx.execute(
        "INSERT INTO stock_moves (id, product_id, batch_id, qty_delta, reason, ref_id, user_id, created_at)
         VALUES (?1, ?2, ?3, ?4, 'adjust', ?5, ?6, ?7)",
        params![db::new_id(), product_id, input.batch_id, delta, input.reason, user.id, db::now()],
    )?;
    db::audit(
        &tx,
        Some(&user.id),
        None,
        "stock.adjust",
        &json!({
            "product": name,
            "batch": batch_no,
            "from": old,
            "to": input.new_qty,
            "reason": input.reason,
            "note": note,
            "costChange": delta * cost,
        }),
        None,
    )?;
    db::outbox(&tx, "stock", &input.batch_id, "adjust", &json!({ "productId": product_id, "delta": delta }))?;
    tx.commit()?;
    detail(conn, &product_id)
}

/// A pack name clash (with a pack no longer sold, say) as a message, not a database error.
fn name_taken(e: rusqlite::Error, name: &str) -> AppError {
    match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => {
            AppError::BadRequest(format!(
                "A pack called \"{}\" already exists for this product (perhaps one no longer sold).",
                name.trim()
            ))
        }
        e => e.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_roll_over_the_year() {
        let m = month_after(0);
        assert_eq!(m, db::current_month());
        let in_12 = month_after(12);
        assert_eq!(&in_12[5..], &m[5..]);
        assert_eq!(in_12[..4].parse::<i32>().unwrap(), m[..4].parse::<i32>().unwrap() + 1);
    }
}
