//! First-run setup, users, shifts and customers.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::{self, User},
    db,
    error::{AppError, AppResult},
    sales,
};

pub fn is_set_up(conn: &Connection) -> AppResult<bool> {
    Ok(conn.query_row("SELECT EXISTS (SELECT 1 FROM stores)", [], |r| r.get(0))?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupInput {
    pub store_code: String,
    pub store_name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub owner_name: String,
    pub owner_password: String,
}

/// Creates the store and its owner account. Only works on an empty database.
pub fn setup(conn: &mut Connection, input: &SetupInput) -> AppResult<User> {
    if is_set_up(conn)? {
        return Err(AppError::Conflict("This store is already set up.".into()));
    }
    let code = input.store_code.trim().to_uppercase();
    if code.is_empty() || code.len() > 6 || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(AppError::BadRequest("Store code must be 1–6 letters or digits, e.g. B1.".into()));
    }
    if input.store_name.trim().is_empty() || input.owner_name.trim().is_empty() {
        return Err(AppError::BadRequest("Store name and owner name are required.".into()));
    }
    if input.owner_password.chars().count() < 8 {
        return Err(AppError::BadRequest("The owner password must be at least 8 characters.".into()));
    }
    let tx = conn.transaction()?;
    let now = db::now();
    tx.execute(
        "INSERT INTO stores (id, code, name, address, phone, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![db::new_id(), code, input.store_name.trim(), input.address, input.phone, now],
    )?;
    let owner_id = db::new_id();
    tx.execute(
        "INSERT INTO users (id, name, role, secret_hash, can_approve, created_at)
         VALUES (?1, ?2, 'admin', ?3, 1, ?4)",
        params![owner_id, input.owner_name.trim(), auth::hash_secret(&input.owner_password), now],
    )?;
    db::audit(&tx, Some(&owner_id), None, "store.setup", &json!({ "code": code }), None)?;
    tx.commit()?;
    auth::load_user(conn, &owner_id)?.ok_or_else(|| AppError::Internal("owner missing".into()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginTile {
    pub id: String,
    pub name: String,
    pub role: String,
    pub language: String,
    /// Can approve with their PIN (shown in the manager-approval dialog).
    pub can_approve: bool,
}

pub fn login_tiles(conn: &Connection) -> AppResult<Vec<LoginTile>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, role, language, role = 'admin' OR can_approve = 1 FROM users WHERE active = 1
         ORDER BY role = 'admin', name COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(LoginTile {
                id: r.get(0)?,
                name: r.get(1)?,
                role: r.get(2)?,
                language: r.get(3)?,
                can_approve: r.get::<_, i64>(4)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewUser {
    pub name: String,
    pub role: String,
    pub secret: String,
    #[serde(default)]
    pub can_approve: bool,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String {
    "en".into()
}

pub fn create_user(conn: &Connection, input: &NewUser, by: &User) -> AppResult<User> {
    match input.role.as_str() {
        "cashier" => {
            if input.secret.len() != 4 || !input.secret.chars().all(|c| c.is_ascii_digit()) {
                return Err(AppError::BadRequest("A cashier PIN must be exactly 4 digits.".into()));
            }
        }
        "admin" => {
            if input.secret.chars().count() < 8 {
                return Err(AppError::BadRequest("An admin password must be at least 8 characters.".into()));
            }
        }
        _ => return Err(AppError::BadRequest("Role must be admin or cashier.".into())),
    }
    if !matches!(input.language.as_str(), "en" | "ur") {
        return Err(AppError::BadRequest("Language must be en or ur.".into()));
    }
    if input.name.trim().is_empty() {
        return Err(AppError::BadRequest("Name is required.".into()));
    }
    let id = db::new_id();
    conn.execute(
        "INSERT INTO users (id, name, role, secret_hash, can_approve, language, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            input.name.trim(),
            input.role,
            auth::hash_secret(&input.secret),
            input.can_approve as i64,
            input.language,
            db::now()
        ],
    )?;
    db::audit(conn, Some(&by.id), None, "user.create", &json!({ "userId": id, "role": input.role }), None)?;
    auth::load_user(conn, &id)?.ok_or_else(|| AppError::Internal("user missing".into()))
}

pub fn set_language(conn: &Connection, user_id: &str, language: &str) -> AppResult<()> {
    if !matches!(language, "en" | "ur") {
        return Err(AppError::BadRequest("Language must be en or ur.".into()));
    }
    conn.execute("UPDATE users SET language = ?1 WHERE id = ?2", params![language, user_id])?;
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shift {
    pub id: String,
    pub till_code: String,
    pub user_id: String,
    pub user_name: String,
    pub opened_at: String,
    pub opening_cash: i64,
}

pub fn open_shift_for(conn: &Connection, till_code: &str) -> AppResult<Option<Shift>> {
    Ok(conn
        .query_row(
            "SELECT s.id, s.till_code, s.user_id, u.name, s.opened_at, s.opening_cash
             FROM shifts s JOIN users u ON u.id = s.user_id
             WHERE s.till_code = ?1 AND s.status = 'open'",
            [till_code],
            |r| {
                Ok(Shift {
                    id: r.get(0)?,
                    till_code: r.get(1)?,
                    user_id: r.get(2)?,
                    user_name: r.get(3)?,
                    opened_at: r.get(4)?,
                    opening_cash: r.get(5)?,
                })
            },
        )
        .optional()?)
}

pub fn open_shift(conn: &Connection, user: &User, till_code: &str, opening_cash: i64) -> AppResult<Shift> {
    if let Some(existing) = open_shift_for(conn, till_code)? {
        return Ok(existing);
    }
    if opening_cash < 0 {
        return Err(AppError::BadRequest("Opening cash can't be negative.".into()));
    }
    let id = db::new_id();
    conn.execute(
        "INSERT INTO shifts (id, till_code, user_id, opened_at, opening_cash) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, till_code, user.id, db::now(), opening_cash],
    )?;
    db::audit(
        conn,
        Some(&user.id),
        Some(till_code),
        "shift.open",
        &json!({ "shiftId": id, "openingCash": opening_cash }),
        None,
    )?;
    open_shift_for(conn, till_code)?.ok_or_else(|| AppError::Internal("shift missing".into()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerView {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub credit_limit: i64,
    pub balance: i64,
    pub receipt_language: Option<String>,
}

pub fn customers(conn: &Connection, query: &str) -> AppResult<Vec<CustomerView>> {
    let like = format!("%{}%", query.trim());
    let mut stmt = conn.prepare(
        "SELECT id, name, phone, credit_limit, receipt_language FROM customers
         WHERE name LIKE ?1 OR phone LIKE ?1 ORDER BY name COLLATE NOCASE LIMIT 50",
    )?;
    let rows = stmt
        .query_map([like], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, name, phone, credit_limit, receipt_language)| {
            Ok(CustomerView {
                balance: sales::khata_balance(conn, &id)?,
                id,
                name,
                phone,
                credit_limit,
                receipt_language,
            })
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCustomer {
    pub name: String,
    pub phone: Option<String>,
    #[serde(default)]
    pub credit_limit: i64,
    pub receipt_language: Option<String>,
}

pub fn create_customer(conn: &Connection, input: &NewCustomer, by: &User) -> AppResult<CustomerView> {
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
    // Only an admin decides how much udhaar a customer may take.
    let limit = if by.is_admin() { input.credit_limit } else { 0 };
    let id = db::new_id();
    conn.execute(
        "INSERT INTO customers (id, name, phone, credit_limit, receipt_language, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, input.name.trim(), input.phone, limit, input.receipt_language, db::now()],
    )?;
    db::outbox(conn, "customer", &id, "upsert", &json!({ "id": id, "name": input.name.trim() }))?;
    Ok(CustomerView {
        id,
        name: input.name.trim().to_string(),
        phone: input.phone.clone(),
        credit_limit: limit,
        balance: 0,
        receipt_language: input.receipt_language.clone(),
    })
}
