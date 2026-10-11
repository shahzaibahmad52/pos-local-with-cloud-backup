//! Cloud backup against a stand-in for Turso: a small server speaking the
//! same HTTP protocol (Hrana `/v2/pipeline`), backed by an SQLite database.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use pos_server::{
    auth::{self, Auth, User},
    cloud::{
        hq::{self, AddStore},
        hrana::{from_json, to_json},
        sync::{self, Cloud},
    },
    dashboard, db,
    expenses::{self, ExpenseInput},
    products, sales, seed,
    store::{self, SetupInput},
};
use rusqlite::{types::Value, Connection};
use serde_json::{json, Value as J};

const TOKEN: &str = "good-token";

#[derive(Clone)]
struct Fake {
    db: Arc<Mutex<Connection>>,
    /// While false, every request fails as if the internet were down.
    online: Arc<Mutex<bool>>,
}

fn exec(conn: &Connection, stmt: &J) -> Result<J, String> {
    let sql = stmt["sql"].as_str().unwrap_or("");
    let args: Vec<Value> =
        stmt["args"].as_array().map(|a| a.iter().map(|v| from_json(v).unwrap()).collect()).unwrap_or_default();
    let mut st = conn.prepare(sql).map_err(|e| e.to_string())?;
    let cols: Vec<J> = st.column_names().iter().map(|c| json!({ "name": c })).collect();
    let n = st.column_count();
    let mut rows = Vec::new();
    if n > 0 {
        let mut q = st.query(rusqlite::params_from_iter(args.iter())).map_err(|e| e.to_string())?;
        while let Some(r) = q.next().map_err(|e| e.to_string())? {
            rows.push(J::Array((0..n).map(|i| to_json(&r.get::<_, Value>(i).unwrap())).collect()));
        }
    } else {
        st.execute(rusqlite::params_from_iter(args.iter())).map_err(|e| e.to_string())?;
    }
    Ok(json!({ "cols": cols, "rows": rows, "affected_row_count": conn.changes() }))
}

fn cond(c: &J, results: &[Option<J>], errors: &[Option<J>]) -> bool {
    match c["type"].as_str() {
        Some("ok") => results.get(c["step"].as_u64().unwrap() as usize).is_some_and(Option::is_some),
        Some("error") => errors.get(c["step"].as_u64().unwrap() as usize).is_some_and(Option::is_some),
        Some("not") => !cond(&c["cond"], results, errors),
        Some("and") => c["conds"].as_array().unwrap().iter().all(|x| cond(x, results, errors)),
        _ => true,
    }
}

async fn pipeline(State(f): State<Fake>, headers: HeaderMap, Json(body): Json<J>) -> (axum::http::StatusCode, Json<J>) {
    if !*f.online.lock().unwrap() {
        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "message": "offline" })));
    }
    if headers.get("authorization").and_then(|h| h.to_str().ok()) != Some(&format!("Bearer {TOKEN}")) {
        return (axum::http::StatusCode::UNAUTHORIZED, Json(json!({ "message": "bad token" })));
    }
    let conn = f.db.lock().unwrap();
    let mut out = Vec::new();
    for req in body["requests"].as_array().unwrap() {
        let r = match req["type"].as_str().unwrap() {
            "execute" => match exec(&conn, &req["stmt"]) {
                Ok(result) => json!({ "type": "ok", "response": { "type": "execute", "result": result } }),
                Err(m) => json!({ "type": "error", "error": { "message": m } }),
            },
            "sequence" => match conn.execute_batch(req["sql"].as_str().unwrap()) {
                Ok(()) => json!({ "type": "ok", "response": { "type": "sequence" } }),
                Err(e) => json!({ "type": "error", "error": { "message": e.to_string() } }),
            },
            "batch" => {
                let mut results: Vec<Option<J>> = Vec::new();
                let mut errors: Vec<Option<J>> = Vec::new();
                for step in req["batch"]["steps"].as_array().unwrap() {
                    if step.get("condition").is_some_and(|c| !c.is_null())
                        && !cond(&step["condition"], &results, &errors)
                    {
                        results.push(None);
                        errors.push(None);
                        continue;
                    }
                    match exec(&conn, &step["stmt"]) {
                        Ok(r) => {
                            results.push(Some(r));
                            errors.push(None);
                        }
                        Err(m) => {
                            results.push(None);
                            errors.push(Some(json!({ "message": m })));
                        }
                    }
                }
                json!({ "type": "ok", "response": { "type": "batch", "result": { "step_results": results, "step_errors": errors } } })
            }
            _ => json!({ "type": "ok", "response": { "type": "close" } }),
        };
        out.push(r);
    }
    (axum::http::StatusCode::OK, Json(json!({ "baton": null, "results": out })))
}

/// Starts a stand-in cloud database; returns its URL and handles.
async fn fake_cloud() -> (String, Fake) {
    let fake =
        Fake { db: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())), online: Arc::new(Mutex::new(true)) };
    let app = Router::new().route("/v2/pipeline", post(pipeline)).with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://127.0.0.1:{}", addr.port()), fake)
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn user(conn: &Connection, name: &str) -> User {
    let id: String = conn.query_row("SELECT id FROM users WHERE name = ?1", [name], |r| r.get(0)).unwrap();
    auth::load_user(conn, &id).unwrap().unwrap()
}

fn demo() -> Arc<Mutex<Connection>> {
    let mut conn = db::open_in_memory().unwrap();
    seed::demo(&mut conn).unwrap();
    Arc::new(Mutex::new(conn))
}

fn sell_panadol(conn: &mut Connection) -> sales::Receipt {
    let owner = user(conn, "Owner");
    let p = products::search(conn, "Panadol", 1).unwrap().remove(0);
    let strip = p.units.iter().find(|u| u.name == "Strip").unwrap().id.clone();
    sales::create_sale(
        conn,
        &Auth::default(),
        &owner,
        &sales::SaleInput {
            till_code: "T01".into(),
            customer_id: None,
            lines: vec![sales::SaleLineInput {
                product_id: p.id,
                unit_id: Some(strip),
                qty: Some(2),
                grams: None,
                amount: None,
            }],
            discount_bp: 0,
            payments: vec![sales::PaymentInput { method: "cash".into(), amount: 6_000 }],
            client_ref: None,
            approval: None,
        },
    )
    .unwrap()
}

/// Tables that must match row for row between the store and its cloud copy.
const COMPARE: &[&str] = &[
    "stores",
    "users",
    "products",
    "product_units",
    "barcodes",
    "batches",
    "batch_unit_prices",
    "stock_moves",
    "customers",
    "sales",
    "sale_lines",
    "sale_line_batches",
    "payments",
    "khata_entries",
    "audit_log",
    "suppliers",
    "purchases",
    "purchase_lines",
    "supplier_entries",
    "expenses",
];

fn same_rows(local: &Connection, cloud: &Connection) {
    for t in COMPARE {
        // The restore itself is logged on the new PC only.
        let filter = if *t == "audit_log" { "WHERE action <> 'cloud.restore'" } else { "" };
        let sql = format!("SELECT * FROM {t} {filter} ORDER BY 1");
        let dump = |c: &Connection| -> Vec<Vec<Value>> {
            let mut st = c.prepare(&sql).unwrap();
            let n = st.column_count();
            st.query_map([], |r| (0..n).map(|i| r.get::<_, Value>(i)).collect())
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(dump(local), dump(cloud), "table {t} differs");
    }
}

#[tokio::test]
async fn store_is_copied_to_the_cloud_and_kept_in_step() {
    let (url, fake) = fake_cloud().await;
    let store = demo();
    let cloud = Cloud::default();

    sync::enable(&store, &cloud, &url, TOKEN).await.unwrap();
    assert!(sync::pending(&store.lock().unwrap()).unwrap() > 50, "everything is queued for the first upload");
    let sent = sync::sync_once(&store, &cloud).await.unwrap();
    assert!(sent > 50);
    assert_eq!(sync::pending(&store.lock().unwrap()).unwrap(), 0);
    same_rows(&store.lock().unwrap(), &fake.db.lock().unwrap());
    {
        let c = fake.db.lock().unwrap();
        assert_eq!(
            count(&c, "SELECT COUNT(*) FROM settings WHERE key LIKE 'cloud%'"),
            0,
            "the token never leaves the PC"
        );
        let code: String =
            c.query_row("SELECT value FROM _sync_meta WHERE key = 'store_code'", [], |r| r.get(0)).unwrap();
        assert_eq!(code, "B1");
    }

    // A sale and a deleted expense while the internet is down.
    *fake.online.lock().unwrap() = false;
    let expense_id = {
        let mut conn = store.lock().unwrap();
        sell_panadol(&mut conn);
        let owner = user(&conn, "Owner");
        let e = expenses::add(
            &mut conn,
            &owner,
            &ExpenseInput {
                category: "Tea & food".into(),
                amount: 30_000,
                date: pos_server::ledger::today(),
                paid_from: "owner".into(),
                till_code: None,
                note: None,
            },
        )
        .unwrap();
        e.id
    };
    assert!(sync::sync_once(&store, &cloud).await.is_err());
    assert!(cloud.status().last_error.is_some());
    let waiting = sync::pending(&store.lock().unwrap()).unwrap();
    assert!(waiting > 0, "changes wait for the internet");
    {
        let mut conn = store.lock().unwrap();
        let owner = user(&conn, "Owner");
        expenses::delete(&mut conn, &owner, &expense_id).unwrap();
    }

    *fake.online.lock().unwrap() = true;
    sync::sync_once(&store, &cloud).await.unwrap();
    assert!(cloud.status().last_error.is_none());
    same_rows(&store.lock().unwrap(), &fake.db.lock().unwrap());
    assert_eq!(count(&fake.db.lock().unwrap(), "SELECT COUNT(*) FROM sales"), 1);

    // The dashboard sums are the same in the cloud.
    let today = pos_server::ledger::today();
    let local = dashboard::figures(&store.lock().unwrap(), &today, &today).unwrap();
    let remote = dashboard::figures(&fake.db.lock().unwrap(), &today, &today).unwrap();
    assert_eq!(local, remote);
}

#[tokio::test]
async fn wrong_token_and_other_stores_database_are_refused() {
    let (url, fake) = fake_cloud().await;
    let store = demo();
    let cloud = Cloud::default();
    let err = sync::enable(&store, &cloud, &url, "wrong").await.unwrap_err();
    assert_eq!(err.reason(), Some("cloud.auth"));
    assert!(sync::view(&store.lock().unwrap(), &cloud).unwrap().enabled.eq(&false));

    // The database already holds another store's backup.
    fake.db
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TABLE _sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO _sync_meta VALUES ('store_code', 'B2');",
        )
        .unwrap();
    let err = sync::enable(&store, &cloud, &url, TOKEN).await.unwrap_err();
    assert_eq!(err.reason(), Some("cloud.other_store"));
}

#[tokio::test]
async fn a_new_main_pc_is_rebuilt_from_the_cloud() {
    let (url, fake) = fake_cloud().await;
    let store = demo();
    let cloud = Cloud::default();
    sell_panadol(&mut store.lock().unwrap());
    sync::enable(&store, &cloud, &url, TOKEN).await.unwrap();
    sync::sync_once(&store, &cloud).await.unwrap();

    let new_pc = Arc::new(Mutex::new(db::open_in_memory().unwrap()));
    let err = sync::restore(&new_pc, &cloud, &url, "wrong").await.unwrap_err();
    assert_eq!(err.reason(), Some("cloud.auth"));
    let name = sync::restore(&new_pc, &cloud, &url, TOKEN).await.unwrap();
    assert_eq!(name, "Branch 1 — Gulberg");

    same_rows(&store.lock().unwrap(), &new_pc.lock().unwrap());
    {
        let conn = new_pc.lock().unwrap();
        // The owner signs in with the same password, and the backup carries on.
        let owner = user(&conn, "Owner");
        assert!(Auth::default().check_secret(&conn, &owner.id, seed::DEMO_OWNER_PASSWORD).is_ok());
        assert!(sync::view(&conn, &cloud).unwrap().enabled);
        assert_eq!(sync::pending(&conn).unwrap(), 0);
        assert_eq!(products::search(&conn, "Panadol", 1).unwrap()[0].stock_base, 1_220);
    }
    // New sales on the new PC go up to the same cloud database.
    sell_panadol(&mut new_pc.lock().unwrap());
    sync::sync_once(&new_pc, &cloud).await.unwrap();
    assert_eq!(count(&fake.db.lock().unwrap(), "SELECT COUNT(*) FROM sales"), 2);

    // Restoring over a store that is already set up is refused.
    assert!(sync::restore(&new_pc, &cloud, &url, TOKEN).await.is_err());
}

#[tokio::test]
async fn head_office_sees_every_store() {
    let cloud = Cloud::default();
    // Branch 1, with a sale today.
    let (url1, _f1) = fake_cloud().await;
    let b1 = demo();
    sell_panadol(&mut b1.lock().unwrap());
    sync::enable(&b1, &cloud, &url1, TOKEN).await.unwrap();
    sync::sync_once(&b1, &cloud).await.unwrap();

    // Branch 2, an empty new store with its own cloud database.
    let (url2, _f2) = fake_cloud().await;
    let b2 = Arc::new(Mutex::new(db::open_in_memory().unwrap()));
    store::setup(
        &mut b2.lock().unwrap(),
        &SetupInput {
            store_code: "B2".into(),
            store_name: "Branch 2 — Model Town".into(),
            address: None,
            phone: None,
            owner_name: "Owner".into(),
            owner_password: "owner1234".into(),
        },
    )
    .unwrap();
    sync::enable(&b2, &cloud, &url2, TOKEN).await.unwrap();
    sync::sync_once(&b2, &cloud).await.unwrap();

    // The owner, on Branch 2's PC, adds Branch 1.
    hq::add(&b2, &cloud, &AddStore { url: url1.clone(), token: TOKEN.into(), name: None }).await.unwrap();
    let dup = hq::add(&b2, &cloud, &AddStore { url: url1, token: TOKEN.into(), name: None }).await.unwrap_err();
    assert_eq!(dup.reason(), Some("hq.duplicate"));

    let today = pos_server::ledger::today();
    let rows = hq::overview(&b2, &cloud, &today, &today).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].this_store && rows[0].code == "B2");
    let branch1 = &rows[1];
    assert_eq!(branch1.name, "Branch 1 — Gulberg");
    assert_eq!(branch1.figures.as_ref().unwrap().sales, 6_000);
    assert_eq!(branch1.khata_due, 420_000);
    assert!(branch1.last_sync_at.is_some());
    assert!(branch1.error.is_none());
    // Branch 2's own token isn't in the list sent to the screen.
    assert!(!serde_json::to_string(&rows).unwrap().contains(TOKEN));
}
