//! Store details, receipt options, users and the owner's recovery code.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::{self, Auth, User},
    db,
    error::{AppError, AppResult},
    sales::{self, StoreHeader},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreInput {
    pub name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub ntn: Option<String>,
    pub drug_license: Option<String>,
    pub receipt_footer: Option<String>,
    pub receipt_footer_ur: Option<String>,
    pub receipt_language: String,
}

fn clean(s: &Option<String>) -> Option<String> {
    s.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

fn set_or_clear(conn: &Connection, key: &str, value: &Option<String>) -> AppResult<()> {
    match clean(value) {
        Some(v) => db::set_setting(conn, key, &v)?,
        None => {
            conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        }
    }
    Ok(())
}

/// Saves the store's details. The store code can't change because every
/// receipt number starts with it.
pub fn update_store(conn: &mut Connection, user: &User, input: &StoreInput) -> AppResult<StoreHeader> {
    if input.name.trim().is_empty() {
        return Err(AppError::BadRequest("Store name is required.".into()));
    }
    if !matches!(input.receipt_language.as_str(), "en" | "ur" | "both") {
        return Err(AppError::BadRequest("Receipt language must be en, ur or both.".into()));
    }
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE stores SET name = ?1, address = ?2, phone = ?3, ntn = ?4, drug_license = ?5",
        params![
            input.name.trim(),
            clean(&input.address),
            clean(&input.phone),
            clean(&input.ntn),
            clean(&input.drug_license)
        ],
    )?;
    set_or_clear(&tx, "receipt_footer", &input.receipt_footer)?;
    set_or_clear(&tx, "receipt_footer_ur", &input.receipt_footer_ur)?;
    db::set_setting(&tx, "receipt_language", &input.receipt_language)?;
    db::audit(&tx, Some(&user.id), None, "store.update", &json!({ "name": input.name.trim() }), None)?;
    tx.commit()?;
    sales::store_header(conn)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRow {
    pub id: String,
    pub name: String,
    pub role: String,
    pub can_approve: bool,
    pub language: String,
    pub active: bool,
    pub created_at: String,
}

pub fn users(conn: &Connection) -> AppResult<Vec<UserRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, role, can_approve, language, active, created_at FROM users
         ORDER BY active DESC, role = 'admin' DESC, name COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(UserRow {
                id: r.get(0)?,
                name: r.get(1)?,
                role: r.get(2)?,
                can_approve: r.get(3)?,
                language: r.get(4)?,
                active: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdate {
    pub name: String,
    pub can_approve: bool,
    pub language: String,
    pub active: bool,
    /// New PIN (cashier) or password (admin); leave out to keep the old one.
    pub secret: Option<String>,
}

pub fn update_user(conn: &Connection, by: &User, id: &str, input: &UserUpdate) -> AppResult<UserRow> {
    let role: String = conn
        .query_row("SELECT role FROM users WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::NotFound("User not found.".into()))?;
    if input.name.trim().is_empty() {
        return Err(AppError::BadRequest("Name is required.".into()));
    }
    if !matches!(input.language.as_str(), "en" | "ur") {
        return Err(AppError::BadRequest("Language must be en or ur.".into()));
    }
    if !input.active && id == by.id {
        return Err(AppError::BadRequest("You can't switch off your own login.".into()));
    }
    if role == "admin" && !input.active {
        let others: i64 =
            conn.query_row("SELECT COUNT(*) FROM users WHERE role = 'admin' AND active = 1 AND id <> ?1", [id], |r| {
                r.get(0)
            })?;
        if others == 0 {
            return Err(AppError::BadRequest("Keep at least one admin login active.".into()));
        }
    }
    let secret = input.secret.as_deref().filter(|s| !s.is_empty());
    if let Some(s) = secret {
        auth::check_secret_rules(&role, s)?;
    }
    conn.execute(
        "UPDATE users SET name = ?1, can_approve = ?2, language = ?3, active = ?4 WHERE id = ?5",
        params![input.name.trim(), input.can_approve || role == "admin", input.language, input.active, id],
    )?;
    if let Some(s) = secret {
        conn.execute("UPDATE users SET secret_hash = ?1 WHERE id = ?2", params![auth::hash_secret(s), id])?;
    }
    db::audit(
        conn,
        Some(&by.id),
        None,
        "user.update",
        &json!({
            "name": input.name.trim(),
            "canApprove": input.can_approve,
            "active": input.active,
            "secretChanged": secret.is_some(),
        }),
        None,
    )?;
    users(conn)?.into_iter().find(|u| u.id == id).ok_or_else(|| AppError::Internal("user missing".into()))
}

const RECOVERY_KEY: &str = "recovery_code_hash";
/// Letters that can't be mistaken for each other when read off paper.
const CODE_CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub fn has_recovery_code(conn: &Connection) -> AppResult<bool> {
    Ok(db::get_setting(conn, RECOVERY_KEY)?.is_some())
}

/// Makes a new one-time recovery code for when the owner forgets the
/// password. Only its hash is kept; the code is shown once to print.
pub fn new_recovery_code(conn: &Connection, by: &User) -> AppResult<String> {
    let raw = uuid::Uuid::new_v4().into_bytes();
    let chars: String = raw.iter().take(12).map(|b| CODE_CHARS[*b as usize % CODE_CHARS.len()] as char).collect();
    let code = format!("{}-{}-{}", &chars[0..4], &chars[4..8], &chars[8..12]);
    db::set_setting(conn, RECOVERY_KEY, &auth::hash_secret(&code))?;
    db::audit(conn, Some(&by.id), None, "recovery.new", &json!({}), None)?;
    Ok(code)
}

fn normalise(code: &str) -> String {
    let c: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase();
    if c.len() == 12 {
        format!("{}-{}-{}", &c[0..4], &c[4..8], &c[8..12])
    } else {
        c
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverInput {
    pub user_id: String,
    pub code: String,
    pub new_password: String,
}

/// Sets a new admin password using the printed recovery code. The code then
/// stops working; the owner prints a new one from Settings.
pub fn recover(conn: &Connection, auth: &Auth, input: &RecoverInput) -> AppResult<User> {
    let role: Option<String> = conn
        .query_row("SELECT role FROM users WHERE id = ?1 AND active = 1", [&input.user_id], |r| r.get(0))
        .optional()?;
    if role.as_deref() != Some("admin") {
        return Err(AppError::BadRequest("Only an admin password can be reset with the recovery code.".into()));
    }
    auth::check_secret_rules("admin", &input.new_password)?;
    let hash = db::get_setting(conn, RECOVERY_KEY)?;
    auth.check_code(conn, "recovery-code", hash.as_deref(), &normalise(&input.code))?;
    conn.execute(
        "UPDATE users SET secret_hash = ?1 WHERE id = ?2",
        params![auth::hash_secret(&input.new_password), input.user_id],
    )?;
    conn.execute("DELETE FROM settings WHERE key = ?1", [RECOVERY_KEY])?;
    db::audit(conn, Some(&input.user_id), None, "recovery.used", &json!({}), None)?;
    auth::load_user(conn, &input.user_id)?.ok_or(AppError::Unauthorized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_forgiving_to_type() {
        assert_eq!(normalise("abcd efgh-jk23"), "ABCD-EFGH-JK23");
        assert_eq!(normalise("ABCD-EFGH-JK23"), "ABCD-EFGH-JK23");
    }
}
