//! Customers & khata (udhaar) in the backoffice: balances, ageing, ledger,
//! payments received and old balances carried over from the paper register.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::User,
    db,
    error::{AppError, AppResult},
    ledger::{self, Entry},
    sales::{self, fmt_rs, StoreHeader},
    shifts,
};

/// A balance unpaid for this many days counts as overdue.
pub const OVERDUE_DAYS: i64 = 30;
pub const PAYMENT_METHODS: &[&str] = &["cash", "jazzcash", "easypaisa", "bank"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub credit_limit: i64,
    pub receipt_language: Option<String>,
    pub active: bool,
    pub balance: i64,
    pub last_paid: Option<String>,
    /// Days since the oldest unpaid bill.
    pub age_days: i64,
    pub overdue: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KhataSummary {
    pub total_due: i64,
    pub owing_count: i64,
    pub overdue_total: i64,
    pub overdue_count: i64,
    pub collected_this_month: i64,
    pub near_limit_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountList {
    pub summary: KhataSummary,
    pub accounts: Vec<Account>,
}

pub fn entries(conn: &Connection, customer_id: &str) -> AppResult<Vec<Entry>> {
    let mut stmt = conn.prepare(
        "SELECT amount, substr(created_at, 1, 10) FROM khata_entries WHERE customer_id = ?1
         ORDER BY created_at, rowid",
    )?;
    let rows = stmt
        .query_map([customer_id], |r| Ok(Entry { amount: r.get(0)?, date: r.get(1)?, due: None }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn account(conn: &Connection, id: &str) -> AppResult<Option<Account>> {
    let row = conn
        .query_row(
            "SELECT id, name, phone, notes, credit_limit, receipt_language, active FROM customers WHERE id = ?1",
            [id],
            |r| {
                Ok(Account {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    phone: r.get(2)?,
                    notes: r.get(3)?,
                    credit_limit: r.get(4)?,
                    receipt_language: r.get(5)?,
                    active: r.get(6)?,
                    balance: 0,
                    last_paid: None,
                    age_days: 0,
                    overdue: false,
                })
            },
        )
        .optional()?;
    let Some(mut a) = row else { return Ok(None) };
    let list = entries(conn, &a.id)?;
    a.balance = list.iter().map(|e| e.amount).sum();
    a.last_paid = conn
        .query_row(
            "SELECT MAX(substr(created_at, 1, 10)) FROM khata_entries
             WHERE customer_id = ?1 AND method IN ('cash', 'jazzcash', 'easypaisa', 'bank')",
            [&a.id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let today = ledger::today();
    a.age_days = ledger::open_charges(&list).first().map(|o| ledger::days_between(&o.date, &today)).unwrap_or(0);
    a.overdue = a.balance > 0 && a.age_days >= OVERDUE_DAYS;
    Ok(Some(a))
}

/// `tab`: `all`, `balance` (owes money) or `overdue`.
pub fn list(conn: &Connection, query: &str, tab: &str) -> AppResult<AccountList> {
    let ids = conn
        .prepare("SELECT id FROM customers ORDER BY name COLLATE NOCASE")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let q = query.trim().to_lowercase();
    let mut all = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(a) = account(conn, &id)? {
            all.push(a);
        }
    }
    let month = db::current_month();
    let collected: i64 = conn.query_row(
        "SELECT COALESCE(-SUM(amount), 0) FROM khata_entries
         WHERE substr(created_at, 1, 7) = ?1 AND method IN ('cash', 'jazzcash', 'easypaisa', 'bank')",
        [&month],
        |r| r.get(0),
    )?;
    let owing: Vec<&Account> = all.iter().filter(|a| a.balance > 0).collect();
    let summary = KhataSummary {
        total_due: owing.iter().map(|a| a.balance).sum(),
        owing_count: owing.len() as i64,
        overdue_total: owing.iter().filter(|a| a.overdue).map(|a| a.balance).sum(),
        overdue_count: owing.iter().filter(|a| a.overdue).count() as i64,
        collected_this_month: collected,
        near_limit_count: owing.iter().filter(|a| a.credit_limit > 0 && a.balance * 10 >= a.credit_limit * 8).count()
            as i64,
    };
    let accounts = all
        .into_iter()
        .filter(|a| match tab {
            "balance" => a.balance > 0,
            "overdue" => a.overdue,
            _ => true,
        })
        .filter(|a| {
            q.is_empty() || a.name.to_lowercase().contains(&q) || a.phone.as_deref().is_some_and(|p| p.contains(&q))
        })
        .collect();
    Ok(AccountList { summary, accounts })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerLine {
    pub created_at: String,
    /// `sale`, `return`, `opening`, or how a payment was made.
    pub kind: String,
    pub reference: Option<String>,
    pub note: Option<String>,
    /// > 0: udhaar given; < 0: paid or refunded.
    pub amount: i64,
    pub balance: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Statement {
    pub account: Account,
    pub lines: Vec<LedgerLine>,
    pub store: StoreHeader,
}

pub fn statement(conn: &Connection, customer_id: &str) -> AppResult<Statement> {
    let account = account(conn, customer_id)?.ok_or_else(|| AppError::NotFound("Customer not found.".into()))?;
    let mut stmt = conn.prepare(
        "SELECT k.created_at, k.method, s.receipt_no, k.note, k.amount
         FROM khata_entries k LEFT JOIN sales s ON s.id = k.sale_id
         WHERE k.customer_id = ?1 ORDER BY k.created_at, k.rowid",
    )?;
    let mut balance = 0;
    let lines = stmt
        .query_map([customer_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(created_at, method, receipt_no, note, amount)| {
            balance += amount;
            let kind = method.unwrap_or_else(|| "sale".into());
            // A refund's note is its return number; show it as the reference.
            let (reference, note) = if kind == "return" { (note, receipt_no) } else { (receipt_no, note) };
            LedgerLine { created_at, kind, reference, note, amount, balance }
        })
        .collect();
    Ok(Statement { account, lines, store: sales::store_header(conn)? })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentInput {
    pub amount: i64,
    pub method: String,
    /// Cash goes into this till's drawer when given; otherwise to the owner.
    pub till_code: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentSlip {
    pub customer_name: String,
    pub amount: i64,
    pub method: String,
    pub balance_before: i64,
    pub balance_after: i64,
    pub created_at: String,
    pub received_by: String,
    pub store: StoreHeader,
}

pub fn receive_payment(
    conn: &mut Connection,
    user: &User,
    customer_id: &str,
    input: &PaymentInput,
) -> AppResult<PaymentSlip> {
    if input.amount <= 0 {
        return Err(AppError::BadRequest("Amount must be above 0.".into()));
    }
    if !PAYMENT_METHODS.contains(&input.method.as_str()) {
        return Err(AppError::BadRequest("Paid by must be cash, JazzCash, Easypaisa or bank.".into()));
    }
    let tx = conn.transaction()?;
    let name: String = tx
        .query_row("SELECT name FROM customers WHERE id = ?1", [customer_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::NotFound("Customer not found.".into()))?;
    let before = sales::khata_balance(&tx, customer_id)?;
    if input.amount > before {
        return Err(AppError::BadRequest(format!("{name} owes only Rs {}.", fmt_rs(before.max(0))))
            .because("khata.overpay", json!({ "name": name, "balance": before.max(0) })));
    }
    let till = input.till_code.as_deref().filter(|t| !t.trim().is_empty());
    let into_drawer = input.method == "cash" && till.is_some();
    if into_drawer {
        shifts::move_money(&tx, "drawer", till, &user.id, "in", input.amount, &format!("Khata payment · {name}"))?;
    }
    let now = db::now();
    let note = input.note.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let id = db::new_id();
    tx.execute(
        "INSERT INTO khata_entries (id, customer_id, amount, method, note, user_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, customer_id, -input.amount, input.method, note, user.id, now],
    )?;
    db::audit(
        &tx,
        Some(&user.id),
        till.filter(|_| into_drawer),
        "khata.payment",
        &json!({ "customer": name, "amount": input.amount, "method": input.method }),
        None,
    )?;
    db::outbox(&tx, "khata", &id, "create", &json!({ "customerId": customer_id, "amount": -input.amount }))?;
    tx.commit()?;
    Ok(PaymentSlip {
        customer_name: name,
        amount: input.amount,
        method: input.method.clone(),
        balance_before: before,
        balance_after: before - input.amount,
        created_at: now,
        received_by: user.name.clone(),
        store: sales::store_header(conn)?,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpeningInput {
    pub amount: i64,
    pub note: Option<String>,
}

/// Carries over what a customer already owed in the paper register.
pub fn add_opening(conn: &Connection, user: &User, customer_id: &str, input: &OpeningInput) -> AppResult<Account> {
    if input.amount <= 0 {
        return Err(AppError::BadRequest("Amount must be above 0.".into()));
    }
    let name: String = conn
        .query_row("SELECT name FROM customers WHERE id = ?1", [customer_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::NotFound("Customer not found.".into()))?;
    let note = input.note.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("Old balance (register)");
    let id = db::new_id();
    conn.execute(
        "INSERT INTO khata_entries (id, customer_id, amount, method, note, user_id, created_at)
         VALUES (?1, ?2, ?3, 'opening', ?4, ?5, ?6)",
        params![id, customer_id, input.amount, note, user.id, db::now()],
    )?;
    db::audit(conn, Some(&user.id), None, "khata.opening", &json!({ "customer": name, "amount": input.amount }), None)?;
    db::outbox(conn, "khata", &id, "create", &json!({ "customerId": customer_id, "amount": input.amount }))?;
    account(conn, customer_id)?.ok_or_else(|| AppError::Internal("customer missing".into()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerUpdate {
    pub name: String,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub credit_limit: i64,
    pub receipt_language: Option<String>,
    #[serde(default = "yes")]
    pub active: bool,
}

fn yes() -> bool {
    true
}

pub fn update(conn: &Connection, user: &User, id: &str, input: &CustomerUpdate) -> AppResult<Account> {
    if input.name.trim().is_empty() {
        return Err(AppError::BadRequest("Customer name is required.".into()));
    }
    if input.credit_limit < 0 {
        return Err(AppError::BadRequest("Credit limit can't be negative.".into()));
    }
    if let Some(l) = &input.receipt_language {
        if !matches!(l.as_str(), "en" | "ur" | "both") {
            return Err(AppError::BadRequest("Receipt language must be en, ur or both.".into()));
        }
    }
    let blank = |s: &Option<String>| s.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let n = conn.execute(
        "UPDATE customers SET name = ?1, phone = ?2, notes = ?3, credit_limit = ?4, receipt_language = ?5, active = ?6
         WHERE id = ?7",
        params![
            input.name.trim(),
            blank(&input.phone),
            blank(&input.notes),
            input.credit_limit,
            input.receipt_language,
            input.active,
            id
        ],
    )?;
    if n == 0 {
        return Err(AppError::NotFound("Customer not found.".into()));
    }
    db::audit(
        conn,
        Some(&user.id),
        None,
        "customer.update",
        &json!({ "name": input.name.trim(), "creditLimit": input.credit_limit, "active": input.active }),
        None,
    )?;
    db::outbox(conn, "customer", id, "upsert", &json!({ "id": id, "name": input.name.trim() }))?;
    account(conn, id)?.ok_or_else(|| AppError::Internal("customer missing".into()))
}
