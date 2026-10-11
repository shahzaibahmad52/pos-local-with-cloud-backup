use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

/// Schema migrations, applied in order. `PRAGMA user_version` records how
/// many have run, so each runs exactly once per database.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_returns_shifts.sql"),
    include_str!("../migrations/003_backoffice.sql"),
    include_str!("../migrations/004_cloud_sync.sql"),
    include_str!("../migrations/005_counts_transfers.sql"),
    include_str!("../migrations/006_fbr.sql"),
    include_str!("../migrations/007_offline_price_lists.sql"),
    include_str!("../migrations/008_retry_safety.sql"),
];

/// How many migrations this version of the app has (the schema version).
pub fn schema_version() -> i64 {
    MIGRATIONS.len() as i64
}

/// The SQL of each migration, for setting up the same schema in the cloud.
pub fn migrations() -> &'static [&'static str] {
    MIGRATIONS
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    migrate(&conn)?;
    crate::cloud::sync::ensure_tracking(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(&conn)?;
    crate::cloud::sync::ensure_tracking(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", index + 1))?;
        tx.commit()?;
    }
    Ok(())
}

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Local time with offset, e.g. `2026-10-10T14:32:05+05:00`.
pub fn now() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// Current month as `YYYY-MM`, the format batch expiry dates are stored in.
pub fn current_month() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

pub fn setting_i64(conn: &Connection, key: &str, default: i64) -> rusqlite::Result<i64> {
    Ok(get_setting(conn, key)?.and_then(|v| v.parse().ok()).unwrap_or(default))
}

pub fn audit(
    conn: &Connection,
    user_id: Option<&str>,
    till_code: Option<&str>,
    action: &str,
    details: &serde_json::Value,
    approved_by: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO audit_log (id, created_at, user_id, till_code, action, details, approved_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![new_id(), now(), user_id, till_code, action, details.to_string(), approved_by],
    )?;
    Ok(())
}

pub fn outbox(
    conn: &Connection,
    entity: &str,
    entity_id: &str,
    op: &str,
    payload: &serde_json::Value,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO outbox (entity, entity_id, op, payload, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![entity, entity_id, op, payload.to_string(), now()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_run_once() {
        let conn = open_in_memory().unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        migrate(&conn).unwrap();
    }

    #[test]
    fn audit_log_is_append_only() {
        let conn = open_in_memory().unwrap();
        audit(&conn, None, None, "test", &serde_json::json!({}), None).unwrap();
        assert!(conn.execute("UPDATE audit_log SET action = 'x'", []).is_err());
        assert!(conn.execute("DELETE FROM audit_log", []).is_err());
    }
}
