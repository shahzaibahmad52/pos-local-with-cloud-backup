//! Shop expenses: electricity, generator fuel, rent, salaries...

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::User,
    db,
    error::{AppError, AppResult},
    ledger, shifts,
};

/// Categories offered as buttons. Any other text is accepted too.
pub const CATEGORIES: &[&str] =
    &["Electricity", "Generator fuel", "Rent", "Salaries", "Internet & phone", "Tea & food", "Repairs", "Other"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseInput {
    pub category: String,
    pub amount: i64,
    /// `YYYY-MM-DD`
    pub date: String,
    pub paid_from: String,
    /// Till whose drawer paid, when `paid_from` is `drawer`.
    pub till_code: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseView {
    pub id: String,
    pub category: String,
    pub amount: i64,
    pub date: String,
    pub paid_from: String,
    pub till_code: Option<String>,
    pub note: Option<String>,
    pub user_name: String,
    pub created_at: String,
    /// Paid from a drawer whose shift is already closed: can't be deleted.
    pub locked: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthExpenses {
    pub month: String,
    pub total: i64,
    pub by_category: Vec<(String, i64)>,
    pub entries: Vec<ExpenseView>,
}

pub fn add(conn: &mut Connection, user: &User, input: &ExpenseInput) -> AppResult<ExpenseView> {
    let category = input.category.trim();
    if category.is_empty() {
        return Err(AppError::BadRequest("Choose a category.".into()));
    }
    if input.amount <= 0 {
        return Err(AppError::BadRequest("Amount must be above 0.".into()));
    }
    if !ledger::valid_date(&input.date) {
        return Err(AppError::BadRequest("Date must look like 2026-10-10.".into()));
    }
    if input.date > ledger::today() {
        return Err(AppError::BadRequest("The date can't be in the future.".into()));
    }
    let note = input.note.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let tx = conn.transaction()?;
    let reason = match note {
        Some(n) => format!("Expense: {category} · {n}"),
        None => format!("Expense: {category}"),
    };
    let cash_move =
        shifts::move_money(&tx, &input.paid_from, input.till_code.as_deref(), &user.id, "out", input.amount, &reason)?;
    let id = db::new_id();
    tx.execute(
        "INSERT INTO expenses (id, category, amount, expense_date, paid_from, cash_move_id, note, user_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![id, category, input.amount, input.date, input.paid_from, cash_move, note, user.id, db::now()],
    )?;
    db::audit(
        &tx,
        Some(&user.id),
        input.till_code.as_deref().filter(|_| cash_move.is_some()),
        "expense.add",
        &json!({ "category": category, "amount": input.amount, "paidFrom": input.paid_from, "note": note }),
        None,
    )?;
    db::outbox(&tx, "expense", &id, "create", &json!({ "category": category, "amount": input.amount }))?;
    tx.commit()?;
    one(conn, &id)?.ok_or_else(|| AppError::Internal("expense missing".into()))
}

const SELECT: &str = "SELECT e.id, e.category, e.amount, e.expense_date, e.paid_from, s.till_code, e.note, u.name,
        e.created_at, s.status = 'closed'
     FROM expenses e JOIN users u ON u.id = e.user_id
     LEFT JOIN cash_moves m ON m.id = e.cash_move_id LEFT JOIN shifts s ON s.id = m.shift_id";

fn row(r: &rusqlite::Row) -> rusqlite::Result<ExpenseView> {
    Ok(ExpenseView {
        id: r.get(0)?,
        category: r.get(1)?,
        amount: r.get(2)?,
        date: r.get(3)?,
        paid_from: r.get(4)?,
        till_code: r.get(5)?,
        note: r.get(6)?,
        user_name: r.get(7)?,
        created_at: r.get(8)?,
        locked: r.get::<_, Option<bool>>(9)?.unwrap_or(false),
    })
}

fn one(conn: &Connection, id: &str) -> AppResult<Option<ExpenseView>> {
    Ok(conn.query_row(&format!("{SELECT} WHERE e.id = ?1"), [id], row).optional()?)
}

/// One month's expenses (`YYYY-MM`), newest first, with totals per category.
pub fn month(conn: &Connection, month: &str) -> AppResult<MonthExpenses> {
    if !crate::products::valid_month(month) {
        return Err(AppError::BadRequest("Month must look like 2026-10.".into()));
    }
    let entries = conn
        .prepare(&format!(
            "{SELECT} WHERE substr(e.expense_date, 1, 7) = ?1 ORDER BY e.expense_date DESC, e.created_at DESC"
        ))?
        .query_map([month], row)?
        .collect::<Result<Vec<_>, _>>()?;
    let by_category = conn
        .prepare(
            "SELECT category, SUM(amount) FROM expenses WHERE substr(expense_date, 1, 7) = ?1
             GROUP BY category ORDER BY SUM(amount) DESC",
        )?
        .query_map([month], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(MonthExpenses { month: month.into(), total: entries.iter().map(|e| e.amount).sum(), by_category, entries })
}

/// Deletes a mistaken entry. One paid from a drawer whose shift is closed
/// stays, because that shift's count already included it.
pub fn delete(conn: &mut Connection, user: &User, id: &str) -> AppResult<()> {
    let e = one(conn, id)?.ok_or_else(|| AppError::NotFound("Expense not found.".into()))?;
    if e.locked {
        return Err(AppError::Conflict(
            "This was paid from a drawer whose shift is already closed, so it can't be deleted.".into(),
        )
        .because("expense.locked", json!({})));
    }
    let tx = conn.transaction()?;
    let cash_move: Option<String> =
        tx.query_row("SELECT cash_move_id FROM expenses WHERE id = ?1", [id], |r| r.get(0))?;
    tx.execute("DELETE FROM expenses WHERE id = ?1", [id])?;
    if let Some(m) = cash_move {
        tx.execute("DELETE FROM cash_moves WHERE id = ?1", [&m])?;
    }
    db::audit(
        &tx,
        Some(&user.id),
        None,
        "expense.delete",
        &json!({ "category": e.category, "amount": e.amount, "date": e.date }),
        None,
    )?;
    db::outbox(&tx, "expense", id, "delete", &json!({}))?;
    tx.commit()?;
    Ok(())
}
