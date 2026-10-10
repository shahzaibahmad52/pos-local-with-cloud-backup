use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{AppError, AppResult};

const MAX_FAILED_ATTEMPTS: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(5 * 60);
/// Tills stay signed in through a whole shift; this is the hard upper limit.
const SESSION_TTL: Duration = Duration::from_secs(16 * 60 * 60);

pub fn hash_secret(secret: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .expect("argon2 hashing with a generated salt cannot fail")
        .to_string()
}

/// A cashier PIN is exactly 4 digits; an admin password at least 8 characters.
pub fn check_secret_rules(role: &str, secret: &str) -> AppResult<()> {
    match role {
        "cashier" if secret.len() != 4 || !secret.chars().all(|c| c.is_ascii_digit()) => {
            Err(AppError::BadRequest("A cashier PIN must be exactly 4 digits.".into()))
        }
        "admin" if secret.chars().count() < 8 => {
            Err(AppError::BadRequest("An admin password must be at least 8 characters.".into()))
        }
        "cashier" | "admin" => Ok(()),
        _ => Err(AppError::BadRequest("Role must be admin or cashier.".into())),
    }
}

pub fn verify_secret(secret: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(secret.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

/// A manager's PIN or password, typed on the till to allow something.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub user_id: String,
    pub secret: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub name: String,
    pub role: String,
    pub can_approve: bool,
    pub language: String,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }

    pub fn may_approve(&self) -> bool {
        self.is_admin() || self.can_approve
    }
}

pub fn load_user(conn: &Connection, id: &str) -> AppResult<Option<User>> {
    Ok(conn
        .query_row("SELECT id, name, role, can_approve, language FROM users WHERE id = ?1 AND active = 1", [id], |r| {
            Ok(User {
                id: r.get(0)?,
                name: r.get(1)?,
                role: r.get(2)?,
                can_approve: r.get::<_, i64>(3)? != 0,
                language: r.get(4)?,
            })
        })
        .optional()?)
}

fn load_secret_hash(conn: &Connection, id: &str) -> AppResult<Option<String>> {
    Ok(conn.query_row("SELECT secret_hash FROM users WHERE id = ?1 AND active = 1", [id], |r| r.get(0)).optional()?)
}

#[derive(Default)]
struct Attempts {
    failures: u32,
    locked_until: Option<Instant>,
}

struct Session {
    user_id: String,
    created: Instant,
}

/// Signed-in tills and failed PIN attempts. Kept in memory on the main PC:
/// restarting it signs everyone out, which is acceptable for a shop.
#[derive(Default)]
pub struct Auth {
    sessions: Mutex<HashMap<String, Session>>,
    attempts: Mutex<HashMap<String, Attempts>>,
}

impl Auth {
    /// Checks a PIN or password, with a lockout after repeated failures so a
    /// 4-digit PIN can't be guessed by trying them all.
    pub fn check_secret(&self, conn: &Connection, user_id: &str, secret: &str) -> AppResult<User> {
        let hash = load_secret_hash(conn, user_id)?;
        self.check_code(conn, user_id, hash.as_deref(), secret)?;
        load_user(conn, user_id)?.ok_or(AppError::Unauthorized)
    }

    /// Checks `secret` against `hash` under the lockout for `key` (a user id,
    /// or a name for codes that belong to no user).
    pub fn check_code(&self, conn: &Connection, key: &str, hash: Option<&str>, secret: &str) -> AppResult<()> {
        {
            let attempts = self.attempts.lock().map_err(poisoned)?;
            if let Some(Attempts { locked_until: Some(until), .. }) = attempts.get(key) {
                if *until > Instant::now() {
                    let mins = (*until - Instant::now()).as_secs() / 60 + 1;
                    return Err(AppError::Locked(format!("Too many wrong tries. Try again in {mins} min."))
                        .because("auth.locked", json!({ "minutes": mins })));
                }
            }
        }

        let ok = hash.is_some_and(|h| verify_secret(secret, h));

        let mut attempts = self.attempts.lock().map_err(poisoned)?;
        if ok {
            attempts.remove(key);
            return Ok(());
        }
        let entry = attempts.entry(key.to_string()).or_default();
        entry.failures += 1;
        if entry.failures >= MAX_FAILED_ATTEMPTS {
            entry.failures = 0;
            entry.locked_until = Some(Instant::now() + LOCKOUT);
            let user = load_user(conn, key)?.map(|u| u.id);
            let details = match user {
                Some(_) => json!({ "minutes": 5 }),
                None => json!({ "minutes": 5, "what": key }),
            };
            crate::db::audit(conn, user.as_deref(), None, "auth.locked", &details, None)?;
            return Err(AppError::Locked("Too many wrong tries. Locked for 5 minutes.".into())
                .because("auth.locked", json!({ "minutes": 5 })));
        }
        Err(AppError::Forbidden("Wrong PIN or password.".into()).because("auth.wrong", json!({})))
    }

    /// Checks a manager approval, if one was given. Returns the approver.
    pub fn approver(&self, conn: &Connection, approval: Option<&Approval>) -> AppResult<Option<User>> {
        let Some(a) = approval else { return Ok(None) };
        let approver = self.check_secret(conn, &a.user_id, &a.secret)?;
        if !approver.may_approve() {
            return Err(AppError::Forbidden(format!("{} can't approve this.", approver.name))
                .because("approval.not_allowed", json!({ "name": approver.name })));
        }
        Ok(Some(approver))
    }

    pub fn start_session(&self, user_id: &str) -> AppResult<String> {
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        self.sessions
            .lock()
            .map_err(poisoned)?
            .insert(token.clone(), Session { user_id: user_id.to_string(), created: Instant::now() });
        Ok(token)
    }

    pub fn session_user_id(&self, token: &str) -> AppResult<Option<String>> {
        let mut sessions = self.sessions.lock().map_err(poisoned)?;
        match sessions.get(token) {
            Some(s) if s.created.elapsed() < SESSION_TTL => Ok(Some(s.user_id.clone())),
            Some(_) => {
                sessions.remove(token);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    pub fn end_session(&self, token: &str) -> AppResult<()> {
        self.sessions.lock().map_err(poisoned)?.remove(token);
        Ok(())
    }
}

fn poisoned<T>(_: T) -> AppError {
    AppError::Internal("session store lock poisoned".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn user(conn: &Connection, id: &str, pin: &str) {
        conn.execute(
            "INSERT INTO users (id, name, role, secret_hash, created_at) VALUES (?1, 'Ali', 'cashier', ?2, ?3)",
            rusqlite::params![id, hash_secret(pin), db::now()],
        )
        .unwrap();
    }

    #[test]
    fn hashes_verify() {
        let h = hash_secret("1234");
        assert!(verify_secret("1234", &h));
        assert!(!verify_secret("1235", &h));
        assert!(!verify_secret("1234", "not-a-hash"));
    }

    #[test]
    fn locks_after_five_wrong_pins() {
        let conn = db::open_in_memory().unwrap();
        user(&conn, "u1", "1234");
        let auth = Auth::default();
        for _ in 0..4 {
            assert!(auth.check_secret(&conn, "u1", "0000").unwrap_err().code() == "forbidden");
        }
        assert!(auth.check_secret(&conn, "u1", "0000").unwrap_err().code() == "locked");
        // Even the right PIN is refused while locked.
        assert!(auth.check_secret(&conn, "u1", "1234").unwrap_err().code() == "locked");
    }

    #[test]
    fn sessions_round_trip() {
        let auth = Auth::default();
        let token = auth.start_session("u1").unwrap();
        assert_eq!(auth.session_user_id(&token).unwrap().as_deref(), Some("u1"));
        auth.end_session(&token).unwrap();
        assert_eq!(auth.session_user_id(&token).unwrap(), None);
    }
}
