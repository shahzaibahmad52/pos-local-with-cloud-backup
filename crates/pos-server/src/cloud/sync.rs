//! Copying the store database to its cloud database (Turso), and back.
//!
//! The cloud database is a full copy of the store's: same tables, same rows.
//! - Triggers on every table queue the primary key of each changed row in
//!   `sync_queue`, but only while cloud backup is switched on.
//! - The worker reads the current version of each queued row and upserts it
//!   in the cloud (or deletes it there), in one transaction per batch.
//! - Restoring reads every table back into a new main PC.
//!
//! The shop never waits for the internet: sales are saved locally and the
//! queue simply grows until the connection is back.

use std::sync::Mutex;

use rusqlite::{types::Value, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value as Json;

use super::hrana::{normalise_url, Client, CloudError, CloudResult, Remote, Stmt};
use crate::{
    db,
    error::{AppError, AppResult},
};

/// Settings that stay on this PC: the cloud token and other stores' tokens.
const PRIVATE_SETTINGS: &[&str] = &["cloud\\_%", "hq\\_%"];
/// Tables that exist only on this PC.
const LOCAL_TABLES: &[&str] = &["sync_queue", "sqlite_sequence"];
/// Rows per upload. Each becomes one statement in a single transaction.
pub const BATCH: i64 = 400;

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Tables copied to the cloud, with their primary key columns.
pub fn tracked_tables(conn: &Connection) -> rusqlite::Result<Vec<(String, Vec<String>)>> {
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY rowid")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for name in names.into_iter().filter(|n| !LOCAL_TABLES.contains(&n.as_str()) && !n.starts_with('_')) {
        let mut pk: Vec<(i64, String)> = conn
            .prepare(&format!("PRAGMA table_info({})", quote(&name)))?
            .query_map([], |r| Ok((r.get::<_, i64>(5)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|(k, _)| *k > 0)
            .collect();
        pk.sort();
        if !pk.is_empty() {
            out.push((name, pk.into_iter().map(|(_, c)| c).collect()));
        }
    }
    Ok(out)
}

fn private_filter(row: &str) -> String {
    PRIVATE_SETTINGS.iter().map(|p| format!(" AND {row}.key NOT LIKE '{p}' ESCAPE '\\'")).collect()
}

/// Creates the change-tracking triggers for every table (once per table).
pub fn ensure_tracking(conn: &Connection) -> rusqlite::Result<()> {
    let enabled = "(SELECT value FROM settings WHERE key = 'cloud_enabled') = '1'";
    for (table, pk) in tracked_tables(conn)? {
        for (event, row) in [("INSERT", "NEW"), ("UPDATE", "NEW"), ("DELETE", "OLD")] {
            let keys = pk.iter().map(|c| format!("{row}.{}", quote(c))).collect::<Vec<_>>().join(", ");
            let extra = if table == "settings" { private_filter(row) } else { String::new() };
            conn.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS {trigger} AFTER {event} ON {t} WHEN {enabled}{extra}
                 BEGIN INSERT INTO sync_queue (tbl, pk) VALUES ('{table}', json_array({keys})); END;",
                trigger = quote(&format!("_sync_{}_{}", table, event.to_lowercase())),
                t = quote(&table),
            ))?;
        }
    }
    Ok(())
}

/// Queues every row of every table, for the first upload (or after the
/// backup was switched off for a while).
pub fn enqueue_everything(conn: &Connection) -> rusqlite::Result<i64> {
    let mut total = 0;
    for (table, pk) in tracked_tables(conn)? {
        let keys = pk.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
        let filter = if table == "settings" { format!("WHERE 1{}", private_filter("settings")) } else { String::new() };
        total += conn.execute(
            &format!(
                "INSERT INTO sync_queue (tbl, pk) SELECT '{table}', json_array({keys}) FROM {} {filter}",
                quote(&table)
            ),
            [],
        )? as i64;
    }
    Ok(total)
}

pub fn pending(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM sync_queue", [], |r| r.get(0))
}

fn pk_values(pk: &str) -> Option<Vec<Value>> {
    let list: Vec<Json> = serde_json::from_str(pk).ok()?;
    list.into_iter()
        .map(|v| match v {
            Json::Null => Some(Value::Null),
            Json::String(s) => Some(Value::Text(s)),
            Json::Number(n) => n.as_i64().map(Value::Integer).or_else(|| n.as_f64().map(Value::Real)),
            _ => None,
        })
        .collect()
}

/// The statement that makes the cloud's copy of one row match this PC's.
fn row_statement(conn: &Connection, table: &str, pk_cols: &[String], pk: &[Value]) -> rusqlite::Result<Option<Stmt>> {
    let wheres = pk_cols.iter().enumerate().map(|(i, c)| format!("{} = ?{}", quote(c), i + 1)).collect::<Vec<_>>();
    let mut stmt = conn.prepare(&format!("SELECT * FROM {} WHERE {}", quote(table), wheres.join(" AND ")))?;
    let cols: Vec<String> = stmt.column_names().into_iter().map(str::to_string).collect();
    let row: Option<Vec<Value>> = stmt
        .query_row(rusqlite::params_from_iter(pk.iter()), |r| (0..cols.len()).map(|i| r.get::<_, Value>(i)).collect())
        .optional()?;
    let Some(values) = row else {
        // Gone here, so gone in the cloud too. The activity log is never deleted.
        if table == "audit_log" {
            return Ok(None);
        }
        return Ok(Some(Stmt::new(
            format!("DELETE FROM {} WHERE {}", quote(table), wheres.join(" AND ")),
            pk.to_vec(),
        )));
    };
    let names = cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
    let marks = (1..=cols.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    let sets: Vec<String> =
        cols.iter().filter(|c| !pk_cols.contains(c)).map(|c| format!("{0} = excluded.{0}", quote(c))).collect();
    let sql = if table == "audit_log" || sets.is_empty() {
        format!("INSERT OR IGNORE INTO {} ({names}) VALUES ({marks})", quote(table))
    } else {
        let conflict = pk_cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
        format!(
            "INSERT INTO {} ({names}) VALUES ({marks}) ON CONFLICT ({conflict}) DO UPDATE SET {}",
            quote(table),
            sets.join(", ")
        )
    };
    Ok(Some(Stmt::new(sql, values)))
}

/// What the backoffice shows about the cloud backup.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub syncing: bool,
}

/// Shared by the API and the background worker.
#[derive(Default)]
pub struct Cloud {
    pub client: Client,
    pub status: Mutex<Status>,
    pub wake: tokio::sync::Notify,
    /// The URL whose schema has been checked since this program started.
    checked: Mutex<Option<String>>,
}

impl Cloud {
    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn set_status(&self, f: impl FnOnce(&mut Status)) {
        if let Ok(mut s) = self.status.lock() {
            f(&mut s);
        }
    }
}

pub fn remote_from_settings(conn: &Connection) -> rusqlite::Result<Option<Remote>> {
    if db::get_setting(conn, "cloud_enabled")?.as_deref() != Some("1") {
        return Ok(None);
    }
    let (Some(url), Some(token)) = (db::get_setting(conn, "cloud_url")?, db::get_setting(conn, "cloud_token")?) else {
        return Ok(None);
    };
    Ok(normalise_url(&url).map(|base| Remote { base, token }))
}

/// What the cloud database says about itself.
#[derive(Debug, Default)]
pub struct CloudInfo {
    pub store_code: Option<String>,
    pub store_name: Option<String>,
    pub schema_version: i64,
    pub last_sync_at: Option<String>,
}

pub async fn cloud_info(client: &Client, remote: &Remote) -> CloudResult<CloudInfo> {
    let meta = match client.query(remote, Stmt::new("SELECT key, value FROM _sync_meta", vec![])).await {
        Ok(rows) => rows,
        Err(CloudError::Sql(m)) if m.contains("no such table") => return Ok(CloudInfo::default()),
        Err(e) => return Err(e),
    };
    let mut info = CloudInfo::default();
    for row in meta.rows {
        if let [Value::Text(k), Value::Text(v)] = row.as_slice() {
            match k.as_str() {
                "store_code" => info.store_code = Some(v.clone()),
                "store_name" => info.store_name = Some(v.clone()),
                "schema_version" => info.schema_version = v.parse().unwrap_or(0),
                "last_sync_at" => info.last_sync_at = Some(v.clone()),
                _ => {}
            }
        }
    }
    Ok(info)
}

fn meta(key: &str, value: &str) -> Stmt {
    Stmt::new(
        "INSERT INTO _sync_meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        vec![Value::Text(key.into()), Value::Text(value.into())],
    )
}

/// Makes sure the cloud database has this app's tables and belongs to this store.
async fn prepare_cloud(client: &Client, remote: &Remote, store_code: &str, store_name: &str) -> AppResult<()> {
    let info = cloud_info(client, remote).await.map_err(cloud_err)?;
    if let Some(other) = info.store_code.as_deref().filter(|c| *c != store_code) {
        return Err(AppError::Conflict(format!(
            "That cloud database holds store {other}'s backup. Each store needs its own database."
        ))
        .because("cloud.other_store", serde_json::json!({ "store": other })));
    }
    if info.schema_version > db::schema_version() {
        return Err(AppError::Conflict(
            "The cloud backup was made by a newer version of the app. Update this PC first.".into(),
        )
        .because("cloud.newer", serde_json::json!({})));
    }
    client
        .script(remote, "CREATE TABLE IF NOT EXISTS _sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        .await
        .map_err(cloud_err)?;
    for (i, sql) in db::migrations().iter().enumerate().skip(info.schema_version as usize) {
        client.script(remote, sql).await.map_err(cloud_err)?;
        client.batch(remote, &[meta("schema_version", &(i + 1).to_string())]).await.map_err(cloud_err)?;
    }
    client.batch(remote, &[meta("store_code", store_code), meta("store_name", store_name)]).await.map_err(cloud_err)?;
    Ok(())
}

pub fn cloud_err(e: CloudError) -> AppError {
    let reason = match e {
        CloudError::Network(_) => "cloud.offline",
        CloudError::Auth => "cloud.auth",
        _ => "cloud.error",
    };
    AppError::Conflict(e.to_string()).because(reason, serde_json::json!({}))
}

fn store_identity(conn: &Connection) -> AppResult<(String, String)> {
    conn.query_row("SELECT code, name FROM stores LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| AppError::Conflict("Set up the store first.".into()))
}

fn lock(db: &Mutex<Connection>) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
    db.lock().map_err(|_| AppError::Internal("database lock poisoned".into()))
}

/// Uploads everything waiting. Returns how many changes went up.
pub async fn sync_once(db: &Mutex<Connection>, cloud: &Cloud) -> AppResult<i64> {
    let (remote, code, name) = {
        let conn = lock(db)?;
        match remote_from_settings(&conn)? {
            Some(r) => {
                let (code, name) = store_identity(&conn)?;
                (r, code, name)
            }
            None => return Ok(0),
        }
    };
    cloud.set_status(|s| s.syncing = true);
    let result = upload(db, cloud, &remote, &code, &name).await;
    cloud.set_status(|s| {
        s.syncing = false;
        match &result {
            Ok(_) => {
                s.last_sync_at = Some(db::now());
                s.last_error = None;
            }
            Err(e) => s.last_error = Some(e.to_string()),
        }
    });
    if result.is_ok() {
        if let Ok(conn) = lock(db) {
            let _ = conn.execute(
                "INSERT INTO settings (key, value) VALUES ('cloud_last_sync', ?1)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                [db::now()],
            );
        }
    }
    result
}

async fn upload(db: &Mutex<Connection>, cloud: &Cloud, remote: &Remote, code: &str, name: &str) -> AppResult<i64> {
    let checked = cloud.checked.lock().ok().and_then(|c| c.clone());
    if checked.as_deref() != Some(remote.base.as_str()) {
        prepare_cloud(&cloud.client, remote, code, name).await?;
        if let Ok(mut c) = cloud.checked.lock() {
            *c = Some(remote.base.clone());
        }
    }
    let mut total = 0;
    loop {
        let (stmts, max_seq, count) = {
            let conn = lock(db)?;
            let tables = tracked_tables(&conn)?;
            let queue: Vec<(i64, String, String)> = conn
                .prepare("SELECT seq, tbl, pk FROM sync_queue ORDER BY seq LIMIT ?1")?
                .query_map([BATCH], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<_, _>>()?;
            let Some(max_seq) = queue.last().map(|q| q.0) else { break };
            // A row changed several times is sent once, as it is now.
            let mut seen = std::collections::HashSet::new();
            let mut stmts = Vec::new();
            for (_, tbl, pk) in &queue {
                if !seen.insert((tbl.clone(), pk.clone())) {
                    continue;
                }
                let (Some((_, cols)), Some(values)) = (tables.iter().find(|(t, _)| t == tbl), pk_values(pk)) else {
                    continue;
                };
                if let Some(s) = row_statement(&conn, tbl, cols, &values)? {
                    stmts.push(s);
                }
            }
            (stmts, max_seq, queue.len() as i64)
        };
        let mut all = stmts;
        all.push(meta("last_sync_at", &db::now()));
        cloud.client.batch(remote, &all).await.map_err(cloud_err)?;
        lock(db)?.execute("DELETE FROM sync_queue WHERE seq <= ?1", [max_seq])?;
        total += count;
        if count < BATCH {
            break;
        }
    }
    Ok(total)
}

/// Turns cloud backup on: checks the database, then queues everything for
/// the first upload.
pub async fn enable(db: &Mutex<Connection>, cloud: &Cloud, url: &str, token: &str) -> AppResult<()> {
    let base = normalise_url(url).ok_or_else(|| {
        AppError::BadRequest("Paste the database URL from Turso, like libsql://store-b1-yourname.turso.io".into())
            .because("cloud.bad_url", serde_json::json!({}))
    })?;
    if token.trim().is_empty() {
        return Err(AppError::BadRequest("Paste the database token from Turso.".into()));
    }
    let remote = Remote { base: base.clone(), token: token.trim().to_string() };
    let (code, name) = store_identity(&*lock(db)?)?;
    prepare_cloud(&cloud.client, &remote, &code, &name).await?;
    if let Ok(mut c) = cloud.checked.lock() {
        *c = Some(base);
    }
    let mut conn = lock(db)?;
    let tx = conn.transaction()?;
    db::set_setting(&tx, "cloud_url", url.trim())?;
    db::set_setting(&tx, "cloud_token", token.trim())?;
    db::set_setting(&tx, "cloud_enabled", "1")?;
    tx.execute("DELETE FROM sync_queue", [])?;
    enqueue_everything(&tx)?;
    db::audit(&tx, None, None, "cloud.enable", &serde_json::json!({ "url": url.trim() }), None)?;
    tx.commit()?;
    drop(conn);
    cloud.wake.notify_one();
    Ok(())
}

pub fn disable(conn: &Connection) -> AppResult<()> {
    db::set_setting(conn, "cloud_enabled", "0")?;
    conn.execute("DELETE FROM sync_queue", [])?;
    db::audit(conn, None, None, "cloud.disable", &serde_json::json!({}), None)?;
    Ok(())
}

/// Rebuilds this (empty) store database from its cloud copy.
pub async fn restore(db: &Mutex<Connection>, cloud: &Cloud, url: &str, token: &str) -> AppResult<String> {
    if crate::store::is_set_up(&*lock(db)?)? {
        return Err(AppError::Conflict("This PC already has a store. Restore only works on a new PC.".into()));
    }
    let base = normalise_url(url).ok_or_else(|| {
        AppError::BadRequest("Paste the database URL from Turso.".into())
            .because("cloud.bad_url", serde_json::json!({}))
    })?;
    let remote = Remote { base, token: token.trim().to_string() };
    let info = cloud_info(&cloud.client, &remote).await.map_err(cloud_err)?;
    let Some(code) = info.store_code.clone() else {
        return Err(AppError::NotFound("That cloud database has no store backup in it.".into())
            .because("cloud.empty", serde_json::json!({})));
    };
    if info.schema_version > db::schema_version() {
        return Err(AppError::Conflict(
            "The cloud backup was made by a newer version of the app. Update this PC first.".into(),
        )
        .because("cloud.newer", serde_json::json!({})));
    }

    // Fill a fresh database in memory first, so a dropped connection halfway
    // leaves this PC untouched.
    let mut fresh = db::open_in_memory()?;
    fresh.execute_batch("PRAGMA foreign_keys = OFF;")?;
    let tables = tracked_tables(&fresh)?;
    for (table, _) in &tables {
        let mut offset = 0;
        loop {
            let page = cloud
                .client
                .query(
                    &remote,
                    Stmt::new(
                        format!("SELECT * FROM {} ORDER BY rowid LIMIT 1000 OFFSET ?1", quote(table)),
                        vec![Value::Integer(offset)],
                    ),
                )
                .await
                .map_err(cloud_err)?;
            if page.rows.is_empty() {
                break;
            }
            let tx = fresh.transaction()?;
            {
                let names = page.cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
                let marks = (1..=page.cols.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
                let mut ins =
                    tx.prepare(&format!("INSERT OR REPLACE INTO {} ({names}) VALUES ({marks})", quote(table)))?;
                for row in &page.rows {
                    ins.execute(rusqlite::params_from_iter(row.iter()))?;
                }
            }
            tx.commit()?;
            offset += page.rows.len() as i64;
            if page.rows.len() < 1000 {
                break;
            }
        }
    }
    fresh.execute_batch("PRAGMA foreign_keys = ON;")?;
    db::audit(&fresh, None, None, "cloud.restore", &serde_json::json!({ "store": code }), None)?;
    db::set_setting(&fresh, "cloud_url", url.trim())?;
    db::set_setting(&fresh, "cloud_token", token.trim())?;
    db::set_setting(&fresh, "cloud_enabled", "1")?;
    fresh.execute("DELETE FROM sync_queue", [])?;
    // Shifts left open on the old PC can't be counted here; they stay open
    // so the owner sees them, and are closed from the till as usual.

    let mut conn = lock(db)?;
    let backup = rusqlite::backup::Backup::new(&fresh, &mut conn)?;
    backup.run_to_completion(256, std::time::Duration::ZERO, None)?;
    drop(backup);
    drop(conn);
    cloud.wake.notify_one();
    Ok(info.store_name.unwrap_or(code))
}

/// Cloud backup settings for the backoffice. The token is never sent back.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudView {
    pub enabled: bool,
    pub url: Option<String>,
    pub pending: i64,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub syncing: bool,
}

pub fn view(conn: &Connection, cloud: &Cloud) -> AppResult<CloudView> {
    let status = cloud.status();
    Ok(CloudView {
        enabled: db::get_setting(conn, "cloud_enabled")?.as_deref() == Some("1"),
        url: db::get_setting(conn, "cloud_url")?,
        pending: pending(conn)?,
        last_sync_at: status.last_sync_at.or(db::get_setting(conn, "cloud_last_sync")?),
        last_error: status.last_error,
        syncing: status.syncing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_are_queued_only_while_backup_is_on() {
        let conn = db::open_in_memory().unwrap();
        db::set_setting(&conn, "receipt_footer", "x").unwrap();
        assert_eq!(pending(&conn).unwrap(), 0);
        db::set_setting(&conn, "cloud_enabled", "1").unwrap();
        db::set_setting(&conn, "receipt_footer", "y").unwrap();
        db::set_setting(&conn, "cloud_token", "secret").unwrap();
        let queued: Vec<(String, String)> = conn
            .prepare("SELECT tbl, pk FROM sync_queue")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(queued, vec![("settings".to_string(), "[\"receipt_footer\"]".to_string())], "the token stays local");
    }

    #[test]
    fn every_table_but_the_queue_is_tracked() {
        let conn = db::open_in_memory().unwrap();
        let tables: Vec<String> = tracked_tables(&conn).unwrap().into_iter().map(|t| t.0).collect();
        assert!(tables.contains(&"sales".to_string()));
        assert!(tables.contains(&"batch_unit_prices".to_string()));
        assert!(!tables.contains(&"sync_queue".to_string()));
        let pk = tracked_tables(&conn).unwrap().into_iter().find(|t| t.0 == "sale_line_batches").unwrap().1;
        assert_eq!(pk, vec!["sale_line_id", "batch_id"]);
    }
}
