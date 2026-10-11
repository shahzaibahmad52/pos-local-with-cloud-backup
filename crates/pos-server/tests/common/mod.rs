//! A stand-in for Turso for tests: a small server speaking the same HTTP
//! protocol (Hrana `/v2/pipeline`), backed by an SQLite database.
#![allow(dead_code)]

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use pos_server::cloud::hrana::{from_json, to_json};
use rusqlite::{types::Value, Connection};
use serde_json::{json, Value as J};

pub const TOKEN: &str = "good-token";

#[derive(Clone)]
pub struct Fake {
    pub db: Arc<Mutex<Connection>>,
    /// While false, every request fails as if the internet were down.
    pub online: Arc<Mutex<bool>>,
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
pub async fn fake_cloud() -> (String, Fake) {
    let fake =
        Fake { db: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())), online: Arc::new(Mutex::new(true)) };
    let app = Router::new().route("/v2/pipeline", post(pipeline)).with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://127.0.0.1:{}", addr.port()), fake)
}
