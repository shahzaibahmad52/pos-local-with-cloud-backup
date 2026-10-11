//! Stock counts: count one rack (or the whole shop) batch by batch, then
//! correct the stock to what is really on the shelf. Selling carries on
//! during the count: each batch's system quantity is noted when it is
//! counted, and only the difference found then is applied.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::User,
    db,
    error::{AppError, AppResult},
    inventory::{self, UnitPrice},
};

pub const REASONS: &[&str] = &["damaged", "expired", "lost", "count", "other"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CountHead {
    pub id: String,
    pub count_no: String,
    pub section: Option<String>,
    pub blind: bool,
    pub status: String,
    pub started_by: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub lines: i64,
    pub counted: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CountLine {
    pub id: String,
    pub product_id: String,
    pub product_name: String,
    pub rack: Option<String>,
    pub sell_mode: String,
    pub base_unit: String,
    pub units: Vec<UnitPrice>,
    pub batch_no: String,
    pub expiry: Option<String>,
    /// Stock the system has now (hidden in a blind count until counted).
    pub system_qty: Option<i64>,
    pub counted_qty: Option<i64>,
    /// Counted minus the system quantity when it was counted.
    pub difference: Option<i64>,
    pub cost_per_base: i64,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CountView {
    #[serde(flatten)]
    pub head: CountHead,
    pub items: Vec<CountLine>,
    /// Value at cost of what is missing / extra among counted lines.
    pub short_value: i64,
    pub extra_value: i64,
    pub racks: Vec<RackProgress>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RackProgress {
    pub rack: Option<String>,
    pub lines: i64,
    pub counted: i64,
}

const HEAD_SQL: &str = "SELECT c.id, c.count_no, c.section, c.blind, c.status, u.name, c.started_at, c.finished_at,
        (SELECT COUNT(*) FROM stock_count_lines l WHERE l.count_id = c.id),
        (SELECT COUNT(*) FROM stock_count_lines l WHERE l.count_id = c.id AND l.counted_qty IS NOT NULL)
     FROM stock_counts c JOIN users u ON u.id = c.started_by";

fn head(r: &rusqlite::Row) -> rusqlite::Result<CountHead> {
    Ok(CountHead {
        id: r.get(0)?,
        count_no: r.get(1)?,
        section: r.get(2)?,
        blind: r.get(3)?,
        status: r.get(4)?,
        started_by: r.get(5)?,
        started_at: r.get(6)?,
        finished_at: r.get(7)?,
        lines: r.get(8)?,
        counted: r.get(9)?,
    })
}

pub fn list(conn: &Connection) -> AppResult<Vec<CountHead>> {
    let rows = conn
        .prepare(&format!("{HEAD_SQL} ORDER BY c.started_at DESC LIMIT 50"))?
        .query_map([], head)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Racks with stock, for choosing what to count.
pub fn racks(conn: &Connection) -> AppResult<Vec<RackProgress>> {
    let rows = conn
        .prepare(
            "SELECT p.rack, COUNT(*) FROM batches b JOIN products p ON p.id = b.product_id
             WHERE p.active = 1 AND b.qty_on_hand > 0 GROUP BY p.rack ORDER BY p.rack IS NULL, p.rack COLLATE NOCASE",
        )?
        .query_map([], |r| Ok(RackProgress { rack: r.get(0)?, lines: r.get(1)?, counted: 0 }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn add_batches(conn: &Connection, count_id: &str, filter: &str, arg: Option<&str>) -> AppResult<i64> {
    let sql = format!(
        "INSERT OR IGNORE INTO stock_count_lines (id, count_id, product_id, batch_id)
         SELECT lower(hex(randomblob(16))), ?1, b.product_id, b.id FROM batches b JOIN products p ON p.id = b.product_id
         WHERE p.active = 1 AND b.qty_on_hand > 0 {filter}"
    );
    Ok(conn.execute(&sql, params![count_id, arg])? as i64)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInput {
    /// Rack to count; empty / missing = the whole store.
    pub section: Option<String>,
    /// `true` to count the batches with no rack.
    #[serde(default)]
    pub no_rack: bool,
    #[serde(default = "yes")]
    pub blind: bool,
}

fn yes() -> bool {
    true
}

pub fn start(conn: &mut Connection, user: &User, input: &StartInput) -> AppResult<CountView> {
    let tx = conn.transaction()?;
    let n: i64 = tx.query_row("SELECT COUNT(*) FROM stock_counts", [], |r| r.get(0))?;
    let id = db::new_id();
    let count_no = format!("SC-{:04}", n + 1);
    let section = input.section.as_deref().map(str::trim).filter(|s| !s.is_empty());
    tx.execute(
        "INSERT INTO stock_counts (id, count_no, section, blind, started_by, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, count_no, section.or(input.no_rack.then_some("—")), input.blind, user.id, db::now()],
    )?;
    let added = match (section, input.no_rack) {
        (Some(rack), _) => add_batches(&tx, &id, "AND p.rack = ?2 COLLATE NOCASE", Some(rack))?,
        (None, true) => add_batches(&tx, &id, "AND (p.rack IS NULL OR p.rack = '') AND ?2 IS NULL", None)?,
        (None, false) => add_batches(&tx, &id, "AND ?2 IS NULL", None)?,
    };
    if added == 0 {
        return Err(AppError::BadRequest("Nothing with stock on that shelf.".into()).because("count.empty", json!({})));
    }
    db::audit(&tx, Some(&user.id), None, "count.start", &json!({ "countNo": count_no, "section": section }), None)?;
    tx.commit()?;
    view(conn, &id)
}

pub fn view(conn: &Connection, id: &str) -> AppResult<CountView> {
    let head = conn
        .query_row(&format!("{HEAD_SQL} WHERE c.id = ?1"), [id], head)
        .optional()?
        .ok_or_else(|| AppError::NotFound("Stock count not found.".into()))?;
    let mut stmt = conn.prepare(
        "SELECT l.id, l.product_id, p.name, p.rack, p.sell_mode, p.base_unit, b.batch_no, b.expiry, b.qty_on_hand,
                l.system_qty, l.counted_qty, b.cost_per_base, l.reason
         FROM stock_count_lines l JOIN products p ON p.id = l.product_id JOIN batches b ON b.id = l.batch_id
         WHERE l.count_id = ?1 ORDER BY p.rack IS NULL, p.rack COLLATE NOCASE, p.name COLLATE NOCASE, b.expiry",
    )?;
    let open = head.status == "open";
    let mut items = Vec::new();
    for r in stmt.query_map([id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, i64>(8)?,
            r.get::<_, Option<i64>>(9)?,
            r.get::<_, Option<i64>>(10)?,
            r.get::<_, i64>(11)?,
            r.get::<_, Option<String>>(12)?,
        ))
    })? {
        let (lid, pid, name, rack, sell_mode, base_unit, batch_no, expiry, now_qty, at_count, counted, cost, reason) =
            r?;
        let difference = counted.zip(at_count).map(|(c, s)| c - s);
        // A blind count shows the system figure only after the batch is counted.
        let system_qty = if open && head.blind && counted.is_none() { None } else { at_count.or(Some(now_qty)) };
        items.push(CountLine {
            id: lid,
            units: inventory::unit_prices(conn, &pid)?,
            product_id: pid,
            product_name: name,
            rack,
            sell_mode,
            base_unit,
            batch_no,
            expiry,
            system_qty,
            counted_qty: counted,
            difference,
            cost_per_base: cost,
            reason,
        });
    }
    let short_value = items.iter().filter_map(|l| l.difference.filter(|d| *d < 0).map(|d| -d * l.cost_per_base)).sum();
    let extra_value = items.iter().filter_map(|l| l.difference.filter(|d| *d > 0).map(|d| d * l.cost_per_base)).sum();
    let mut racks: Vec<RackProgress> = Vec::new();
    for l in &items {
        match racks.iter_mut().find(|r| r.rack == l.rack) {
            Some(r) => {
                r.lines += 1;
                r.counted += l.counted_qty.is_some() as i64;
            }
            None => {
                racks.push(RackProgress { rack: l.rack.clone(), lines: 1, counted: l.counted_qty.is_some() as i64 })
            }
        }
    }
    Ok(CountView { head, items, short_value, extra_value, racks })
}

fn open_count(conn: &Connection, id: &str) -> AppResult<()> {
    let status: String = conn
        .query_row("SELECT status FROM stock_counts WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::NotFound("Stock count not found.".into()))?;
    if status != "open" {
        return Err(AppError::Conflict("This count is already finished.".into()).because("count.closed", json!({})));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineInput {
    /// What is on the shelf, in base units; `None` clears the count.
    pub counted: Option<i64>,
    pub reason: Option<String>,
}

/// Saves one batch's count, noting what the system had at that moment.
pub fn set_line(conn: &Connection, count_id: &str, line_id: &str, input: &LineInput) -> AppResult<CountView> {
    open_count(conn, count_id)?;
    if input.counted.is_some_and(|c| c < 0) {
        return Err(AppError::BadRequest("A count can't be below 0.".into()));
    }
    if let Some(r) = input.reason.as_deref().filter(|r| !r.is_empty()) {
        if !REASONS.contains(&r) {
            return Err(AppError::BadRequest("Unknown reason.".into()));
        }
    }
    let n = conn.execute(
        // A new count is a new look at the shelf, so the system quantity is
        // taken again at that moment; changing only the reason keeps both.
        "UPDATE stock_count_lines SET
            system_qty = CASE WHEN ?1 IS NULL THEN NULL
                              WHEN counted_qty IS ?1 THEN system_qty
                              ELSE (SELECT qty_on_hand FROM batches WHERE id = batch_id) END,
            counted_at = CASE WHEN ?1 IS NULL THEN NULL WHEN counted_qty IS ?1 THEN counted_at ELSE ?2 END,
            counted_qty = ?1,
            reason = ?3
         WHERE id = ?4 AND count_id = ?5",
        params![input.counted, db::now(), input.reason.as_deref().filter(|r| !r.is_empty()), line_id, count_id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound("That line isn't in this count.".into()));
    }
    view(conn, count_id)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProduct {
    pub product_id: String,
}

/// Adds a product found on the shelf that wasn't in the list (every batch it has).
pub fn add_product(conn: &Connection, count_id: &str, input: &AddProduct) -> AppResult<CountView> {
    open_count(conn, count_id)?;
    let n = conn.execute(
        "INSERT OR IGNORE INTO stock_count_lines (id, count_id, product_id, batch_id)
         SELECT lower(hex(randomblob(16))), ?1, product_id, id FROM batches WHERE product_id = ?2
         ORDER BY qty_on_hand > 0 DESC, received_at DESC",
        params![count_id, input.product_id],
    )?;
    if n == 0 {
        let any: bool =
            conn.query_row("SELECT EXISTS (SELECT 1 FROM batches WHERE product_id = ?1)", [&input.product_id], |r| {
                r.get(0)
            })?;
        if !any {
            return Err(AppError::BadRequest("This product has no batches yet. Receive it in Purchases first.".into())
                .because("count.no_batches", json!({})));
        }
    }
    view(conn, count_id)
}

/// Applies the differences found and closes the count.
pub fn apply(conn: &mut Connection, user: &User, count_id: &str) -> AppResult<CountView> {
    open_count(conn, count_id)?;
    let tx = conn.transaction()?;
    let lines: Vec<(String, String, i64, i64, Option<String>)> = tx
        .prepare(
            "SELECT product_id, batch_id, counted_qty, system_qty, reason FROM stock_count_lines
             WHERE count_id = ?1 AND counted_qty IS NOT NULL AND counted_qty <> system_qty",
        )?
        .query_map([count_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    let now = db::now();
    let mut short = 0i64;
    let mut extra = 0i64;
    for (product_id, batch_id, counted, system, _) in &lines {
        let delta = counted - system;
        // Sales since the count already came off the stock; add only the difference.
        let (on_hand, cost): (i64, i64) =
            tx.query_row("SELECT qty_on_hand, cost_per_base FROM batches WHERE id = ?1", [batch_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let applied = (on_hand + delta).max(0) - on_hand;
        tx.execute("UPDATE batches SET qty_on_hand = qty_on_hand + ?1 WHERE id = ?2", params![applied, batch_id])?;
        tx.execute(
            "INSERT INTO stock_moves (id, product_id, batch_id, qty_delta, reason, ref_id, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, 'count', ?5, ?6, ?7)",
            params![db::new_id(), product_id, batch_id, applied, count_id, user.id, now],
        )?;
        if applied < 0 {
            short += -applied * cost;
        } else {
            extra += applied * cost;
        }
    }
    let count_no: String = tx.query_row("SELECT count_no FROM stock_counts WHERE id = ?1", [count_id], |r| r.get(0))?;
    tx.execute(
        "UPDATE stock_counts SET status = 'applied', finished_by = ?1, finished_at = ?2 WHERE id = ?3",
        params![user.id, now, count_id],
    )?;
    db::audit(
        &tx,
        Some(&user.id),
        None,
        "count.apply",
        &json!({ "countNo": count_no, "changed": lines.len(), "short": short, "extra": extra }),
        None,
    )?;
    tx.commit()?;
    view(conn, count_id)
}

pub fn cancel(conn: &Connection, user: &User, count_id: &str) -> AppResult<CountView> {
    open_count(conn, count_id)?;
    conn.execute(
        "UPDATE stock_counts SET status = 'cancelled', finished_by = ?1, finished_at = ?2 WHERE id = ?3",
        params![user.id, db::now(), count_id],
    )?;
    db::audit(conn, Some(&user.id), None, "count.cancel", &json!({}), None)?;
    view(conn, count_id)
}
