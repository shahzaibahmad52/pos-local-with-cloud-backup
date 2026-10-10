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
use serde::Serialize;

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

pub fn verify_secret(secret: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(secret.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
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
        {
            let attempts = self.attempts.lock().map_err(poisoned)?;
            if let Some(Attempts { locked_until: Some(until), .. }) = attempts.get(user_id) {
                if *until > Instant::now() {
                    let mins = (*until - Instant::now()).as_secs() / 60 + 1;
                    return Err(AppError::Locked(format!("Too many wrong tries. Try again in {mins} min.")));
                }
            }
        }

        let hash = load_secret_hash(conn, user_id)?;
        let ok = hash.as_deref().is_some_and(|h| verify_secret(secret, h));

        let mut attempts = self.attempts.lock().map_err(poisoned)?;
        if ok {
            attempts.remove(user_id);
            return load_user(conn, user_id)?.ok_or(AppError::Unauthorized);
        }
        let entry = attempts.entry(user_id.to_string()).or_default();
        entry.failures += 1;
        if entry.failures >= MAX_FAILED_ATTEMPTS {
            entry.failures = 0;
            entry.locked_until = Some(Instant::now() + LOCKOUT);
            return Err(AppError::Locked("Too many wrong tries. Locked for 5 minutes.".into()));
        }
        Err(AppError::Forbidden("Wrong PIN or password.".into()))
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
            assert!(matches!(auth.check_secret(&conn, "u1", "0000"), Err(AppError::Forbidden(_))));
        }
        assert!(matches!(auth.check_secret(&conn, "u1", "0000"), Err(AppError::Locked(_))));
        // Even the right PIN is refused while locked.
        assert!(matches!(auth.check_secret(&conn, "u1", "1234"), Err(AppError::Locked(_))));
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
