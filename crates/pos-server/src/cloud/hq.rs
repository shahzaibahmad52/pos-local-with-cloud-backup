//! Head-office view: today's (or any period's) figures for every store,
//! read from each store's cloud copy. The owner adds the other stores'
//! cloud database URL and a read-only token once, on their own main PC.

use std::sync::Mutex;

use rusqlite::{types::Value, Connection};
use serde::{Deserialize, Serialize};

use super::{
    hrana::{normalise_url, Remote, Stmt},
    sync::{self, cloud_err, Cloud},
};
use crate::{
    dashboard::{self, Figures, FIGURES_SQL, KHATA_DUE_SQL},
    db,
    error::{AppError, AppResult},
    ledger, sales,
};

const KEY: &str = "hq_stores";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HqStore {
    id: String,
    code: String,
    name: String,
    url: String,
    token: String,
}

fn load(conn: &Connection) -> AppResult<Vec<HqStore>> {
    Ok(db::get_setting(conn, KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
}

fn save(conn: &Connection, list: &[HqStore]) -> AppResult<()> {
    db::set_setting(conn, KEY, &serde_json::to_string(list).unwrap_or_else(|_| "[]".into()))?;
    Ok(())
}

fn lock(db: &Mutex<Connection>) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
    Ok(db.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreFigures {
    /// `None` for this store.
    pub id: Option<String>,
    pub code: String,
    pub name: String,
    pub this_store: bool,
    pub figures: Option<Figures>,
    pub khata_due: i64,
    /// When the store's cloud copy was last updated.
    pub last_sync_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddStore {
    pub url: String,
    pub token: String,
    pub name: Option<String>,
}

/// Adds another store's cloud database to the head-office view.
pub async fn add(db: &Mutex<Connection>, cloud: &Cloud, input: &AddStore) -> AppResult<()> {
    let base = normalise_url(&input.url).ok_or_else(|| {
        AppError::BadRequest("Paste the store's database URL from Turso.".into())
            .because("cloud.bad_url", serde_json::json!({}))
    })?;
    let remote = Remote { base, token: input.token.trim().to_string() };
    let info = sync::cloud_info(&cloud.client, &remote).await.map_err(cloud_err)?;
    let code = info.store_code.ok_or_else(|| {
        AppError::NotFound("That cloud database has no store backup in it yet.".into())
            .because("cloud.empty", serde_json::json!({}))
    })?;
    let conn = lock(db)?;
    let mut list = load(&conn)?;
    let own = sales::store_code(&conn)?;
    if code == own || list.iter().any(|s| s.code == code) {
        return Err(AppError::Conflict(format!("Store {code} is already in the list."))
            .because("hq.duplicate", serde_json::json!({ "store": code })));
    }
    let name = input.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    list.push(HqStore {
        id: db::new_id(),
        name: name.or(info.store_name).unwrap_or_else(|| code.clone()),
        code,
        url: input.url.trim().to_string(),
        token: input.token.trim().to_string(),
    });
    save(&conn, &list)
}

pub fn remove(conn: &Connection, id: &str) -> AppResult<()> {
    let list: Vec<HqStore> = load(conn)?.into_iter().filter(|s| s.id != id).collect();
    save(conn, &list)
}

fn ints(row: &[Value]) -> Vec<i64> {
    row.iter()
        .map(|v| match v {
            Value::Integer(i) => *i,
            Value::Real(f) => *f as i64,
            _ => 0,
        })
        .collect()
}

/// Figures for this store and every added store between two dates.
pub async fn overview(db: &Mutex<Connection>, cloud: &Cloud, from: &str, to: &str) -> AppResult<Vec<StoreFigures>> {
    if !ledger::valid_date(from) || !ledger::valid_date(to) || from > to {
        return Err(AppError::BadRequest("Choose a valid date range.".into()));
    }
    let (mut out, others) = {
        let conn = lock(db)?;
        let header = sales::store_header(&conn)?;
        let (khata_due, _): (i64, i64) = conn.query_row(KHATA_DUE_SQL, [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let view = sync::view(&conn, cloud)?;
        let here = StoreFigures {
            id: None,
            code: header.code,
            name: header.name,
            this_store: true,
            figures: Some(dashboard::figures(&conn, from, to)?),
            khata_due,
            last_sync_at: view.enabled.then_some(view.last_sync_at).flatten(),
            error: None,
        };
        (vec![here], load(&conn)?)
    };
    for s in others {
        let mut row = StoreFigures {
            id: Some(s.id.clone()),
            code: s.code.clone(),
            name: s.name.clone(),
            this_store: false,
            figures: None,
            khata_due: 0,
            last_sync_at: None,
            error: None,
        };
        let Some(base) = normalise_url(&s.url) else {
            row.error = Some("Bad database URL".into());
            out.push(row);
            continue;
        };
        let remote = Remote { base, token: s.token.clone() };
        let args = vec![Value::Text(from.into()), Value::Text(to.into())];
        let result = async {
            let f = cloud.client.query(&remote, Stmt::new(FIGURES_SQL, args)).await?;
            let k = cloud.client.query(&remote, Stmt::new(KHATA_DUE_SQL, vec![])).await?;
            let info = sync::cloud_info(&cloud.client, &remote).await?;
            Ok::<_, super::hrana::CloudError>((f, k, info))
        }
        .await;
        match result {
            Ok((f, k, info)) => {
                let v = f.rows.first().map(|r| ints(r)).unwrap_or_default();
                if let Ok(arr) = <[i64; 8]>::try_from(v) {
                    row.figures = Some(dashboard::figures_from(arr));
                }
                row.khata_due = k.first_i64();
                row.last_sync_at = info.last_sync_at;
            }
            Err(e) => row.error = Some(e.to_string()),
        }
        out.push(row);
    }
    Ok(out)
}

/// Other stores added to the All stores list (no tokens), e.g. to choose
/// where a transfer goes.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownStore {
    pub id: String,
    pub code: String,
    pub name: String,
}

pub fn known_stores(conn: &Connection) -> AppResult<Vec<KnownStore>> {
    Ok(load(conn)?.into_iter().map(|s| KnownStore { id: s.id, code: s.code, name: s.name }).collect())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Incoming {
    pub notes: Vec<crate::transfers::TransferNote>,
    /// Stores whose cloud copy couldn't be read, with why.
    pub errors: Vec<(String, String)>,
}

/// Transfers other stores have sent to this one and that haven't been
/// received here yet, read from their cloud copies.
pub async fn incoming_transfers(db: &Mutex<Connection>, cloud: &Cloud) -> AppResult<Incoming> {
    let (own, stores, done) = {
        let conn = lock(db)?;
        let done: Vec<String> = conn
            .prepare("SELECT source_id FROM transfers WHERE source_id IS NOT NULL")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        (sales::store_code(&conn)?, load(&conn)?, done)
    };
    let mut out = Incoming { notes: Vec::new(), errors: Vec::new() };
    for s in stores {
        let Some(base) = normalise_url(&s.url) else { continue };
        let remote = Remote { base, token: s.token.clone() };
        let stmt = Stmt::new(
            "SELECT payload FROM transfers WHERE direction = 'out' AND other_code = ?1 ORDER BY created_at",
            vec![Value::Text(own.clone())],
        );
        match cloud.client.query(&remote, stmt).await {
            Ok(rows) => {
                for row in rows.rows {
                    if let Some(Value::Text(p)) = row.first() {
                        if let Ok(note) = serde_json::from_str::<crate::transfers::TransferNote>(p) {
                            if !done.contains(&note.transfer_id) {
                                out.notes.push(note);
                            }
                        }
                    }
                }
            }
            // An older backup without transfers yet just has nothing to send.
            Err(super::hrana::CloudError::Sql(m)) if m.contains("no such table") => {}
            Err(e) => out.errors.push((s.name.clone(), e.to_string())),
        }
    }
    Ok(out)
}
