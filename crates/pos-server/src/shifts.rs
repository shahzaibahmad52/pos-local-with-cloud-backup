//! Cash in/out during a shift, and closing it with a blind cash count.

use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::{Approval, Auth, User},
    db,
    error::{AppError, AppResult},
    rules,
    sales::{self, StoreHeader},
};

/// Notes and coins in a Pakistani cash drawer, in rupees.
pub const DENOMINATIONS: &[i64] = &[5000, 1000, 500, 100, 50, 20, 10, 5, 2, 1];
/// A difference bigger than this (paisa) must be explained in a note.
pub const NOTE_REQUIRED_ABOVE: i64 = 10_000;
pub const MAX_COUNTS: i64 = 2;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShiftSummary {
    pub id: String,
    pub till_code: String,
    pub cashier_name: String,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub status: String,
    pub opening_cash: i64,
    pub bill_count: i64,
    pub sales_total: i64,
    pub cash_sales: i64,
    pub cash_refunds: i64,
    pub refunds_total: i64,
    pub returns_count: i64,
    pub cash_in: i64,
    pub cash_out: i64,
    /// Card, wallets and khata: shown for checking, not in the drawer.
    pub other_payments: BTreeMap<String, i64>,
    /// Hidden from the cashier until they have counted (blind count).
    pub expected_cash: Option<i64>,
    pub counted_cash: Option<i64>,
    pub difference: Option<i64>,
    pub count_attempts: i64,
    pub note: Option<String>,
    pub store: StoreHeader,
}

fn sum(conn: &Connection, sql: &str, shift_id: &str) -> AppResult<i64> {
    Ok(conn.query_row(sql, [shift_id], |r| r.get::<_, Option<i64>>(0))?.unwrap_or(0))
}

pub fn expected_cash(conn: &Connection, shift_id: &str) -> AppResult<i64> {
    let s = summary(conn, shift_id, true)?;
    Ok(s.expected_cash.unwrap_or(0))
}

pub fn summary(conn: &Connection, shift_id: &str, reveal_expected: bool) -> AppResult<ShiftSummary> {
    let head = conn
        .query_row(
            "SELECT s.id, s.till_code, u.name, s.opened_at, s.closed_at, s.status, s.opening_cash,
                    s.counted_cash, s.count_attempts, s.note
             FROM shifts s JOIN users u ON u.id = s.user_id WHERE s.id = ?1",
            [shift_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((
        id,
        till_code,
        cashier_name,
        opened_at,
        closed_at,
        status,
        opening_cash,
        counted_cash,
        count_attempts,
        note,
    )) = head
    else {
        return Err(AppError::NotFound("Shift not found.".into()));
    };

    let bill_count = sum(conn, "SELECT COUNT(*) FROM sales WHERE shift_id = ?1 AND status = 'completed'", &id)?;
    let sales_total = sum(conn, "SELECT SUM(total) FROM sales WHERE shift_id = ?1 AND status = 'completed'", &id)?;
    let cash_sales = sum(
        conn,
        "SELECT SUM(p.amount) FROM payments p JOIN sales s ON s.id = p.sale_id
         WHERE s.shift_id = ?1 AND s.status = 'completed' AND p.method = 'cash'",
        &id,
    )?;
    let cash_refunds =
        sum(conn, "SELECT SUM(refund_total) FROM returns WHERE shift_id = ?1 AND refund_method = 'cash'", &id)?;
    let refunds_total = sum(conn, "SELECT SUM(refund_total) FROM returns WHERE shift_id = ?1", &id)?;
    let returns_count = sum(conn, "SELECT COUNT(*) FROM returns WHERE shift_id = ?1", &id)?;
    let cash_in = sum(conn, "SELECT SUM(amount) FROM cash_moves WHERE shift_id = ?1 AND kind = 'in'", &id)?;
    let cash_out = sum(conn, "SELECT SUM(amount) FROM cash_moves WHERE shift_id = ?1 AND kind = 'out'", &id)?;

    let mut other_payments = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT p.method, SUM(p.amount) FROM payments p JOIN sales s ON s.id = p.sale_id
         WHERE s.shift_id = ?1 AND s.status = 'completed' AND p.method <> 'cash' GROUP BY p.method",
    )?;
    for row in stmt.query_map([&id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (method, amount) = row?;
        other_payments.insert(method, amount);
    }

    let expected = opening_cash + cash_sales + cash_in - cash_out - cash_refunds;
    let reveal = reveal_expected || count_attempts > 0;
    // Until the drawer is counted, nothing it could be worked out from is shown either.
    let (cash_sales, cash_refunds, other_payments) =
        if reveal { (cash_sales, cash_refunds, other_payments) } else { (0, 0, Default::default()) };
    Ok(ShiftSummary {
        id,
        till_code,
        cashier_name,
        opened_at,
        closed_at,
        status,
        opening_cash,
        bill_count,
        sales_total,
        cash_sales,
        cash_refunds,
        refunds_total,
        returns_count,
        cash_in,
        cash_out,
        other_payments,
        expected_cash: reveal.then_some(expected),
        counted_cash: if reveal { counted_cash } else { None },
        difference: if reveal { counted_cash.map(|c| c - expected) } else { None },
        count_attempts,
        note,
        store: sales::store_header(conn)?,
    })
}

fn open_shift_row(conn: &Connection, shift_id: &str) -> AppResult<(String, String)> {
    let row: Option<(String, String)> = conn
        .query_row("SELECT user_id, status FROM shifts WHERE id = ?1", [shift_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let (owner, status) = row.ok_or_else(|| AppError::NotFound("Shift not found.".into()))?;
    if status != "open" {
        return Err(AppError::Conflict("This shift is already closed.".into()).because("shift.closed", json!({})));
    }
    Ok((owner, status))
}

fn ensure_may_close(user: &User, shift_owner: &str) -> AppResult<()> {
    if user.is_admin() || user.id == shift_owner {
        Ok(())
    } else {
        Err(AppError::Forbidden("Only the cashier on this shift or the owner can close it.".into()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountInput {
    /// Count of each note/coin, keyed by its rupee value ("5000", "1000"…).
    pub denominations: BTreeMap<String, i64>,
}

/// Saves the cashier's count and reveals the expected cash. Allowed twice
/// (one recount); both counts are in the activity log.
pub fn count(conn: &Connection, user: &User, shift_id: &str, input: &CountInput) -> AppResult<ShiftSummary> {
    let (owner, _) = open_shift_row(conn, shift_id)?;
    ensure_may_close(user, &owner)?;
    let attempts: i64 = conn.query_row("SELECT count_attempts FROM shifts WHERE id = ?1", [shift_id], |r| r.get(0))?;
    if attempts >= MAX_COUNTS {
        return Err(AppError::Conflict("The cash has already been counted twice.".into())
            .because("shift.recount_limit", json!({})));
    }
    let mut counted = 0;
    for (value, n) in &input.denominations {
        let rupees: i64 = value.parse().map_err(|_| AppError::BadRequest(format!("Unknown note {value}.")))?;
        if !DENOMINATIONS.contains(&rupees) {
            return Err(AppError::BadRequest(format!("Unknown note {value}.")));
        }
        if *n < 0 || *n > 100_000 {
            return Err(AppError::BadRequest("Note counts must be 0 or more.".into()));
        }
        counted += rupees * 100 * n;
    }
    conn.execute(
        "UPDATE shifts SET counted_cash = ?1, count_attempts = count_attempts + 1, denominations = ?2 WHERE id = ?3",
        params![counted, serde_json::to_string(&input.denominations).unwrap_or_default(), shift_id],
    )?;
    let s = summary(conn, shift_id, true)?;
    db::audit(
        conn,
        Some(&user.id),
        Some(&s.till_code),
        "shift.count",
        &json!({ "shiftId": shift_id, "counted": counted, "expected": s.expected_cash, "attempt": s.count_attempts }),
        None,
    )?;
    Ok(s)
}

#[derive(Debug, Deserialize)]
pub struct CloseInput {
    pub note: Option<String>,
}

pub fn close(conn: &Connection, user: &User, shift_id: &str, input: &CloseInput) -> AppResult<ShiftSummary> {
    let (owner, _) = open_shift_row(conn, shift_id)?;
    ensure_may_close(user, &owner)?;
    let s = summary(conn, shift_id, true)?;
    let (Some(counted), Some(expected)) = (s.counted_cash, s.expected_cash) else {
        return Err(
            AppError::BadRequest("Count the cash before closing.".into()).because("shift.not_counted", json!({}))
        );
    };
    let diff = counted - expected;
    let note = input.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
    if diff.abs() > NOTE_REQUIRED_ABOVE && note.is_none() {
        return Err(AppError::BadRequest("Write a note explaining the difference.".into())
            .because("shift.note_required", json!({ "difference": diff })));
    }
    conn.execute(
        "UPDATE shifts SET status = 'closed', closed_at = ?1, expected_cash = ?2, note = ?3, closed_by = ?4 WHERE id = ?5",
        params![db::now(), expected, note, user.id, shift_id],
    )?;
    db::audit(
        conn,
        Some(&user.id),
        Some(&s.till_code),
        "shift.close",
        &json!({ "shiftId": shift_id, "counted": counted, "expected": expected, "difference": diff, "note": note }),
        None,
    )?;
    let closed = summary(conn, shift_id, true)?;
    db::outbox(conn, "shift", shift_id, "close", &serde_json::to_value(&closed).unwrap_or_default())?;
    Ok(closed)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CashMoveInput {
    pub till_code: String,
    pub kind: String,
    pub amount: i64,
    pub reason: String,
    pub approval: Option<Approval>,
}

/// The open shift on a till; money paid from or into its drawer is recorded there.
pub fn open_shift_id(conn: &Connection, till_code: &str) -> AppResult<String> {
    conn.query_row("SELECT id FROM shifts WHERE till_code = ?1 AND status = 'open'", [till_code], |r| r.get(0))
        .optional()?
        .ok_or_else(|| {
            AppError::Conflict(format!("Till {till_code} has no open shift, so its drawer can't be used."))
                .because("drawer.no_shift", json!({ "till": till_code }))
        })
}

/// Records cash going into or out of a drawer, so the shift's expected cash
/// includes it. Returns the cash move's id.
pub fn insert_cash_move(
    conn: &Connection,
    shift_id: &str,
    user_id: &str,
    kind: &str,
    amount: i64,
    reason: &str,
    approved_by: Option<&str>,
) -> AppResult<String> {
    let id = db::new_id();
    conn.execute(
        "INSERT INTO cash_moves (id, shift_id, user_id, kind, amount, reason, approved_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, shift_id, user_id, kind, amount, reason, approved_by, db::now()],
    )?;
    db::outbox(conn, "cash_move", &id, "create", &json!({ "shiftId": shift_id, "kind": kind, "amount": amount }))?;
    Ok(id)
}

/// Where money for a purchase, expense or khata payment came from or went to.
pub const MONEY_SOURCES: &[&str] = &["drawer", "owner", "bank", "wallet"];

/// Moves cash in or out of a till's drawer when `source` is the drawer, so
/// the shift's expected cash stays right. Other sources leave the drawer alone.
pub fn move_money(
    conn: &Connection,
    source: &str,
    till_code: Option<&str>,
    user_id: &str,
    kind: &str,
    amount: i64,
    reason: &str,
) -> AppResult<Option<String>> {
    if !MONEY_SOURCES.contains(&source) {
        return Err(AppError::BadRequest("Choose where the money is paid from: drawer, owner, bank or wallet.".into()));
    }
    if source != "drawer" || amount == 0 {
        return Ok(None);
    }
    let till = till_code.filter(|t| !t.trim().is_empty()).ok_or_else(|| {
        AppError::BadRequest("Choose which till's drawer the cash is from.".into()).because("drawer.no_till", json!({}))
    })?;
    let shift_id = open_shift_id(conn, till)?;
    if kind == "out" {
        ensure_drawer_has(conn, &shift_id, amount)?;
    }
    Ok(Some(insert_cash_move(conn, &shift_id, user_id, kind, amount, reason, None)?))
}

/// Cash can't be taken out of a drawer that doesn't hold it. The amount in
/// the drawer isn't named, so the blind count stays blind.
fn ensure_drawer_has(conn: &Connection, shift_id: &str, amount: i64) -> AppResult<()> {
    let in_drawer = summary(conn, shift_id, true)?.expected_cash.unwrap_or(0);
    if amount > in_drawer {
        return Err(
            AppError::BadRequest("There isn't that much cash in the drawer.".into()).because("drawer.short", json!({}))
        );
    }
    Ok(())
}

pub fn cash_move(conn: &Connection, auth: &Auth, user: &User, input: &CashMoveInput) -> AppResult<ShiftSummary> {
    if !matches!(input.kind.as_str(), "in" | "out") {
        return Err(AppError::BadRequest("Kind must be in or out.".into()));
    }
    if input.amount <= 0 || input.amount > crate::pricing::MAX_AMOUNT {
        return Err(AppError::BadRequest("Amount must be above 0.".into()));
    }
    if input.reason.trim().is_empty() {
        return Err(
            AppError::BadRequest("Write what the cash is for.".into()).because("cash.reason_required", json!({}))
        );
    }
    let shift_id = open_shift_id(conn, &input.till_code)?;
    let approver = auth.approver(conn, input.approval.as_ref())?;
    if input.kind == "out" && rules::load(conn)?.cash_out && approver.is_none() && !user.is_admin() {
        return Err(AppError::ApprovalRequired("Taking cash out needs a manager PIN.".into())
            .because("approval.cash_out", json!({})));
    }
    if input.kind == "out" {
        ensure_drawer_has(conn, &shift_id, input.amount)?;
    }
    insert_cash_move(
        conn,
        &shift_id,
        &user.id,
        &input.kind,
        input.amount,
        input.reason.trim(),
        approver.as_ref().map(|a| a.id.as_str()),
    )?;
    db::audit(
        conn,
        Some(&user.id),
        Some(&input.till_code),
        &format!("cash.{}", input.kind),
        &json!({ "amount": input.amount, "reason": input.reason.trim() }),
        approver.as_ref().map(|a| a.id.as_str()),
    )?;
    summary(conn, &shift_id, user.is_admin())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTill {
    pub till_code: String,
    pub cashier_name: String,
}

/// Tills with an open shift: the drawers money can be paid from right now.
pub fn open_tills(conn: &Connection) -> AppResult<Vec<OpenTill>> {
    let mut stmt = conn.prepare(
        "SELECT s.till_code, u.name FROM shifts s JOIN users u ON u.id = s.user_id
         WHERE s.status = 'open' ORDER BY s.till_code",
    )?;
    let rows = stmt
        .query_map([], |r| Ok(OpenTill { till_code: r.get(0)?, cashier_name: r.get(1)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
