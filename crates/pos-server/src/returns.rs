//! Customer returns and refunds against a receipt.

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::{Approval, Auth, User},
    db,
    error::{AppError, AppResult},
    pricing, rules,
    sales::{self, StoreHeader},
};

pub const REFUND_METHODS: &[&str] = &["cash", "khata", "card", "jazzcash", "easypaisa"];
pub const CONDITIONS: &[&str] = &["restock", "damaged", "expired"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnableLine {
    pub sale_line_id: String,
    pub product_name: String,
    pub unit_name: String,
    pub sell_mode: String,
    /// Sold quantity, in the line's units (grams for weight lines).
    pub qty: i64,
    pub returned_qty: i64,
    pub unit_price: i64,
    pub line_total: i64,
    pub batch_no: Option<String>,
    pub batch_expiry: Option<String>,
    /// `None` when it can be returned; otherwise why not (e.g. `cold_chain`).
    pub blocked: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleForReturn {
    pub id: String,
    pub receipt_no: String,
    pub created_at: String,
    pub cashier_name: String,
    pub customer_id: Option<String>,
    pub customer_name: Option<String>,
    pub subtotal: i64,
    pub total: i64,
    pub refunded_so_far: i64,
    pub payment_methods: Vec<String>,
    pub lines: Vec<ReturnableLine>,
}

fn returned_qty(conn: &Connection, sale_line_id: &str) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(qty), 0) FROM return_lines WHERE sale_line_id = ?1",
        [sale_line_id],
        |r| r.get(0),
    )?)
}

fn refunded_so_far(conn: &Connection, sale_id: &str) -> AppResult<i64> {
    Ok(conn
        .query_row("SELECT COALESCE(SUM(refund_total), 0) FROM returns WHERE sale_id = ?1", [sale_id], |r| r.get(0))?)
}

/// Finds a sale by the receipt number printed on it (also its barcode).
pub fn sale_for_return(conn: &Connection, receipt_no: &str) -> AppResult<SaleForReturn> {
    let head = conn
        .query_row(
            "SELECT s.id, s.receipt_no, s.created_at, u.name, s.customer_id, c.name, s.subtotal, s.total
             FROM sales s JOIN users u ON u.id = s.user_id LEFT JOIN customers c ON c.id = s.customer_id
             WHERE s.status = 'completed' AND (s.receipt_no = ?1
                OR s.id = (SELECT sale_id FROM receipt_aliases WHERE receipt_no = ?1))",
            [receipt_no.trim().to_uppercase()],
            |r| {
                Ok(SaleForReturn {
                    id: r.get(0)?,
                    receipt_no: r.get(1)?,
                    created_at: r.get(2)?,
                    cashier_name: r.get(3)?,
                    customer_id: r.get(4)?,
                    customer_name: r.get(5)?,
                    subtotal: r.get(6)?,
                    total: r.get(7)?,
                    refunded_so_far: 0,
                    payment_methods: Vec::new(),
                    lines: Vec::new(),
                })
            },
        )
        .optional()?;
    let Some(mut sale) = head else {
        return Err(AppError::NotFound(format!("No sale with receipt number {receipt_no}."))
            .because("return.no_sale", json!({ "receiptNo": receipt_no })));
    };
    sale.refunded_so_far = refunded_so_far(conn, &sale.id)?;
    let mut stmt = conn.prepare("SELECT DISTINCT method FROM payments WHERE sale_id = ?1")?;
    sale.payment_methods = stmt.query_map([&sale.id], |r| r.get(0))?.collect::<Result<Vec<_>, _>>()?;

    let mut stmt = conn.prepare(
        "SELECT l.id, p.name, l.unit_name, p.sell_mode, l.qty, l.unit_price, l.line_total, p.cold_chain
         FROM sale_lines l JOIN products p ON p.id = l.product_id
         WHERE l.sale_id = ?1 ORDER BY l.line_no",
    )?;
    let rows = stmt
        .query_map([&sale.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)? != 0,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, product_name, unit_name, sell_mode, qty, unit_price, line_total, cold) in rows {
        let batch: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT b.batch_no, b.expiry FROM sale_line_batches lb JOIN batches b ON b.id = lb.batch_id
                 WHERE lb.sale_line_id = ?1 ORDER BY b.expiry IS NULL, b.expiry LIMIT 1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        sale.lines.push(ReturnableLine {
            returned_qty: returned_qty(conn, &id)?,
            sale_line_id: id,
            product_name,
            unit_name,
            sell_mode,
            qty,
            unit_price,
            line_total,
            batch_no: batch.as_ref().map(|b| b.0.clone()),
            batch_expiry: batch.and_then(|b| b.1),
            blocked: cold.then(|| "cold_chain".to_string()),
        });
    }
    Ok(sale)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnLineInput {
    pub sale_line_id: String,
    pub qty: i64,
    pub condition: String,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnInput {
    pub sale_id: String,
    pub till_code: String,
    pub lines: Vec<ReturnLineInput>,
    pub refund_method: String,
    pub client_ref: Option<String>,
    pub approval: Option<Approval>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnSlipLine {
    pub product_name: String,
    pub unit_name: String,
    pub sell_mode: String,
    pub qty: i64,
    pub refund: i64,
    pub condition: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnSlip {
    pub id: String,
    pub return_no: String,
    pub sale_receipt_no: String,
    pub created_at: String,
    pub till_code: String,
    pub cashier_name: String,
    pub customer_name: Option<String>,
    pub lines: Vec<ReturnSlipLine>,
    pub refund_total: i64,
    pub refund_method: String,
    pub approved_by_name: Option<String>,
    pub khata_balance: Option<i64>,
    pub store: StoreHeader,
    pub fbr_invoice_no: Option<String>,
}

struct SaleLineRow {
    sale_id: String,
    product_id: String,
    qty: i64,
    base_qty: i64,
    line_total: i64,
    cold_chain: bool,
    product_name: String,
}

fn next_return_no(tx: &Transaction, store_code: &str, till_code: &str) -> AppResult<String> {
    let key = format!("R:{till_code}");
    tx.execute(
        "INSERT INTO receipt_counters (till_code, last_no) VALUES (?1, 1)
         ON CONFLICT (till_code) DO UPDATE SET last_no = last_no + 1",
        [&key],
    )?;
    let n: i64 = tx.query_row("SELECT last_no FROM receipt_counters WHERE till_code = ?1", [&key], |r| r.get(0))?;
    Ok(format!("{store_code}-{till_code}-R{n:05}"))
}

/// Puts `base_qty` back into the batches this line was sold from, in the same
/// order they were taken, skipping what earlier returns already put back.
fn restock(
    tx: &Transaction,
    sale_line_id: &str,
    product_id: &str,
    base_qty: i64,
    return_id: &str,
    user_id: &str,
) -> AppResult<()> {
    let already: i64 = tx.query_row(
        "SELECT COALESCE(SUM(base_qty), 0) FROM return_lines WHERE sale_line_id = ?1 AND condition = 'restock'",
        [sale_line_id],
        |r| r.get(0),
    )?;
    let mut stmt = tx.prepare(
        "SELECT lb.batch_id, lb.base_qty FROM sale_line_batches lb JOIN batches b ON b.id = lb.batch_id
         WHERE lb.sale_line_id = ?1 ORDER BY b.expiry IS NULL, b.expiry, b.received_at",
    )?;
    let allocations = stmt
        .query_map([sale_line_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut skip = already;
    let mut left = base_qty;
    let now = db::now();
    for (batch_id, taken) in allocations {
        if left == 0 {
            break;
        }
        let used = skip.min(taken);
        skip -= used;
        let room = taken - used;
        if room == 0 {
            continue;
        }
        let put = room.min(left);
        left -= put;
        tx.execute("UPDATE batches SET qty_on_hand = qty_on_hand + ?1 WHERE id = ?2", params![put, batch_id])?;
        tx.execute(
            "INSERT INTO stock_moves (id, product_id, batch_id, qty_delta, reason, ref_id, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, 'return', ?5, ?6, ?7)",
            params![db::new_id(), product_id, batch_id, put, return_id, user_id, now],
        )?;
    }
    Ok(())
}

pub fn create_return(conn: &mut Connection, auth: &Auth, user: &User, input: &ReturnInput) -> AppResult<ReturnSlip> {
    if input.lines.is_empty() {
        return Err(AppError::BadRequest("Choose what is being returned.".into()).because("return.empty", json!({})));
    }
    let mut seen = std::collections::HashSet::new();
    if !input.lines.iter().all(|l| seen.insert(l.sale_line_id.as_str())) {
        return Err(AppError::BadRequest("Each item can only be listed once in a return.".into()));
    }
    if !REFUND_METHODS.contains(&input.refund_method.as_str()) {
        return Err(AppError::BadRequest(format!("Unknown refund method {}.", input.refund_method)));
    }
    if let Some(r) = &input.client_ref {
        let existing: Option<String> =
            conn.query_row("SELECT id FROM returns WHERE client_ref = ?1", [r], |row| row.get(0)).optional()?;
        if let Some(id) = existing {
            return return_slip(conn, &id)?.ok_or_else(|| AppError::Internal("return vanished".into()));
        }
    }
    let approver = auth.approver(conn, user, input.approval.as_ref())?;
    let approved = user.is_admin() || approver.is_some();
    let rules = rules::load(conn)?;
    let store_code = sales::store_code(conn)?;

    let (sale_total, sale_subtotal, customer_id, receipt_no): (i64, i64, Option<String>, String) = conn
        .query_row(
            "SELECT total, subtotal, customer_id, receipt_no FROM sales WHERE id = ?1 AND status = 'completed'",
            [&input.sale_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("Sale not found.".into()))?;
    if input.refund_method == "khata" && customer_id.is_none() {
        return Err(AppError::BadRequest("This sale had no khata customer. Refund in cash instead.".into())
            .because("return.no_khata_customer", json!({})));
    }
    // A bill put on khata is refunded to khata, so no cash leaves the drawer for money never paid.
    let on_khata: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM payments WHERE sale_id = ?1 AND method = 'khata')",
        [&input.sale_id],
        |r| r.get(0),
    )?;
    if on_khata && input.refund_method != "khata" {
        return Err(AppError::BadRequest("This sale was put on khata, so the refund goes back to khata.".into())
            .because("return.khata_sale", json!({})));
    }

    let tx = conn.transaction()?;
    let mut planned = Vec::with_capacity(input.lines.len());
    let mut refund_total = 0;
    for l in &input.lines {
        if !CONDITIONS.contains(&l.condition.as_str()) {
            return Err(AppError::BadRequest(format!("Unknown condition {}.", l.condition)));
        }
        let row = tx
            .query_row(
                "SELECT l.sale_id, l.product_id, l.qty, l.base_qty, l.line_total, p.cold_chain, p.name
                 FROM sale_lines l JOIN products p ON p.id = l.product_id WHERE l.id = ?1",
                [&l.sale_line_id],
                |r| {
                    Ok(SaleLineRow {
                        sale_id: r.get(0)?,
                        product_id: r.get(1)?,
                        qty: r.get(2)?,
                        base_qty: r.get(3)?,
                        line_total: r.get(4)?,
                        cold_chain: r.get::<_, i64>(5)? != 0,
                        product_name: r.get(6)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("A line on this return isn't on the sale.".into()))?;
        if row.sale_id != input.sale_id {
            return Err(AppError::BadRequest("A line on this return belongs to a different sale.".into()));
        }
        if row.cold_chain {
            return Err(AppError::Forbidden(format!("{} is a fridge item and can't be returned.", row.product_name))
                .because("return.cold_chain", json!({ "product": row.product_name })));
        }
        let left = row.qty - returned_qty(&tx, &l.sale_line_id)?;
        if l.qty <= 0 || l.qty > left {
            return Err(AppError::BadRequest(format!(
                "Only {left} of {} can still be returned from this sale.",
                row.product_name
            ))
            .because("return.too_many", json!({ "product": row.product_name, "left": left })));
        }
        // The share of the line being returned, after the bill's discount.
        let gross = pricing::round_div(row.line_total * l.qty, row.qty);
        let refund = if sale_subtotal > 0 { pricing::round_div(gross * sale_total, sale_subtotal) } else { 0 };
        let base_qty = pricing::round_div(row.base_qty * l.qty, row.qty);
        refund_total += refund;
        planned.push((l, row, refund, base_qty));
    }
    // Rounding must never refund more than the customer paid, and the last
    // item back settles exactly what is left of the bill.
    let already = refunded_so_far(&tx, &input.sale_id)?;
    let returning: i64 = planned.iter().map(|p| p.0.qty).sum();
    let qty_left: i64 = tx.query_row(
        "SELECT COALESCE(SUM(l.qty), 0) - COALESCE((SELECT SUM(rl.qty) FROM return_lines rl
                JOIN sale_lines x ON x.id = rl.sale_line_id WHERE x.sale_id = ?1), 0)
         FROM sale_lines l WHERE l.sale_id = ?1",
        [&input.sale_id],
        |r| r.get(0),
    )?;
    let target = if qty_left == returning { Some(sale_total - already) } else { None };
    let fix = match target {
        Some(t) => t - refund_total,
        None => (sale_total - already - refund_total).min(0),
    };
    if fix != 0 {
        refund_total += fix;
        if let Some(last) = planned.last_mut() {
            last.2 = (last.2 + fix).max(0);
        }
    }

    // Splitting a refund into small returns doesn't get round the manager PIN.
    if already + refund_total > rules.refund_approval_above && refund_total > 0 && !approved {
        return Err(AppError::ApprovalRequired(format!(
            "Refunds above Rs {} need a manager PIN.",
            sales::fmt_rs(rules.refund_approval_above)
        ))
        .because("approval.refund", json!({ "limit": rules.refund_approval_above })));
    }

    let shift_id: Option<String> = tx
        .query_row("SELECT id FROM shifts WHERE till_code = ?1 AND status = 'open'", [&input.till_code], |r| r.get(0))
        .optional()?;
    if shift_id.is_none() {
        return Err(AppError::Conflict(format!("Till {} has no open shift. Start a shift first.", input.till_code))
            .because("shift.none", json!({ "till": input.till_code })));
    }
    let return_id = db::new_id();
    let return_no = next_return_no(&tx, &store_code, &input.till_code)?;
    let now = db::now();
    tx.execute(
        "INSERT INTO returns (id, return_no, client_ref, sale_id, shift_id, till_code, user_id, created_at,
            refund_total, refund_method, approved_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            return_id,
            return_no,
            input.client_ref,
            input.sale_id,
            shift_id,
            input.till_code,
            user.id,
            now,
            refund_total,
            input.refund_method,
            approver.as_ref().map(|a| a.id.as_str())
        ],
    )?;
    for (l, row, refund, base_qty) in &planned {
        // Restock before inserting this line so `restock` only counts earlier returns.
        if l.condition == "restock" {
            restock(&tx, &l.sale_line_id, &row.product_id, *base_qty, &return_id, &user.id)?;
        }
        tx.execute(
            "INSERT INTO return_lines (id, return_id, sale_line_id, qty, base_qty, refund, condition, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![db::new_id(), return_id, l.sale_line_id, l.qty, base_qty, refund, l.condition, l.reason],
        )?;
    }
    if input.refund_method == "khata" {
        tx.execute(
            "INSERT INTO khata_entries (id, customer_id, sale_id, amount, method, note, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, 'return', ?5, ?6, ?7)",
            params![db::new_id(), customer_id, input.sale_id, -refund_total, return_no, user.id, now],
        )?;
    }
    db::audit(
        &tx,
        Some(&user.id),
        Some(&input.till_code),
        "return.create",
        &json!({ "returnNo": return_no, "receiptNo": receipt_no, "refund": refund_total, "method": input.refund_method }),
        approver.as_ref().map(|a| a.id.as_str()),
    )?;
    crate::fbr::queue(&tx, "return", &return_id)?;
    tx.commit()?;
    let slip = return_slip(conn, &return_id)?.ok_or_else(|| AppError::Internal("return vanished".into()))?;
    db::outbox(conn, "return", &return_id, "create", &serde_json::to_value(&slip).unwrap_or_default())?;
    Ok(slip)
}

pub fn return_slip(conn: &Connection, return_id: &str) -> AppResult<Option<ReturnSlip>> {
    let head = conn
        .query_row(
            "SELECT r.id, r.return_no, s.receipt_no, r.created_at, r.till_code, u.name, c.name, r.refund_total,
                    r.refund_method, ap.name, s.customer_id
             FROM returns r JOIN sales s ON s.id = r.sale_id JOIN users u ON u.id = r.user_id
             LEFT JOIN customers c ON c.id = s.customer_id LEFT JOIN users ap ON ap.id = r.approved_by
             WHERE r.id = ?1",
            [return_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, Option<String>>(9)?,
                    r.get::<_, Option<String>>(10)?,
                ))
            },
        )
        .optional()?;
    let Some((
        id,
        return_no,
        sale_receipt_no,
        created_at,
        till_code,
        cashier_name,
        customer_name,
        refund_total,
        refund_method,
        approved_by_name,
        customer_id,
    )) = head
    else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        "SELECT p.name, l.unit_name, p.sell_mode, rl.qty, rl.refund, rl.condition
         FROM return_lines rl JOIN sale_lines l ON l.id = rl.sale_line_id JOIN products p ON p.id = l.product_id
         WHERE rl.return_id = ?1 ORDER BY l.line_no",
    )?;
    let lines = stmt
        .query_map([&id], |r| {
            Ok(ReturnSlipLine {
                product_name: r.get(0)?,
                unit_name: r.get(1)?,
                sell_mode: r.get(2)?,
                qty: r.get(3)?,
                refund: r.get(4)?,
                condition: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let khata_balance = match &customer_id {
        Some(cid) if refund_method == "khata" => Some(sales::khata_balance(conn, cid)?),
        _ => None,
    };
    let fbr_invoice_no = crate::fbr::invoice_no(conn, "return", &id)?;
    Ok(Some(ReturnSlip {
        id,
        return_no,
        sale_receipt_no,
        created_at,
        till_code,
        cashier_name,
        customer_name,
        lines,
        refund_total,
        refund_method,
        approved_by_name,
        khata_balance,
        store: sales::store_header(conn)?,
        fbr_invoice_no,
    }))
}
