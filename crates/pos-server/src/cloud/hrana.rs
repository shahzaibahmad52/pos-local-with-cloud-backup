//! A small client for Turso / libSQL's HTTP protocol ("Hrana over HTTP",
//! `POST /v2/pipeline`). Only what the backup needs: run statements, run a
//! batch atomically, run a script, and read rows.

use base64::Engine as _;
use rusqlite::types::Value;
use serde_json::{json, Value as Json};

/// Cloud database address and access token, as the owner pastes them from Turso.
#[derive(Debug, Clone)]
pub struct Remote {
    /// `https://…` base URL.
    pub base: String,
    pub token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("can't reach the cloud database ({0})")]
    Network(String),
    #[error("the cloud refused the access token")]
    Auth,
    #[error("cloud database error: {0}")]
    Sql(String),
    #[error("unexpected reply from the cloud: {0}")]
    Protocol(String),
}

pub type CloudResult<T> = Result<T, CloudError>;

/// Turns what the owner pasted (`libsql://name-org.turso.io`, or an
/// `https://` URL) into the HTTP base URL. Plain `http://` is accepted
/// only for this computer, for testing.
pub fn normalise_url(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/');
    let rest = s.strip_prefix("libsql://").or_else(|| s.strip_prefix("https://"));
    if let Some(host) = rest {
        let ok = !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || "-.:".contains(c));
        return ok.then(|| format!("https://{host}"));
    }
    let host = s.strip_prefix("http://")?;
    let name = host.split(':').next().unwrap_or("");
    matches!(name, "localhost" | "127.0.0.1").then(|| format!("http://{host}"))
}

pub fn to_json(v: &Value) -> Json {
    match v {
        Value::Null => json!({ "type": "null" }),
        Value::Integer(i) => json!({ "type": "integer", "value": i.to_string() }),
        Value::Real(f) => json!({ "type": "float", "value": f }),
        Value::Text(t) => json!({ "type": "text", "value": t }),
        Value::Blob(b) => {
            json!({ "type": "blob", "base64": base64::engine::general_purpose::STANDARD_NO_PAD.encode(b) })
        }
    }
}

pub fn from_json(v: &Json) -> CloudResult<Value> {
    let bad = || CloudError::Protocol(format!("value {v}"));
    Ok(match v["type"].as_str().ok_or_else(bad)? {
        "null" => Value::Null,
        "integer" => Value::Integer(v["value"].as_str().and_then(|s| s.parse().ok()).ok_or_else(bad)?),
        "float" => Value::Real(v["value"].as_f64().ok_or_else(bad)?),
        "text" => Value::Text(v["value"].as_str().ok_or_else(bad)?.to_string()),
        "blob" => {
            let b64 = v["base64"].as_str().ok_or_else(bad)?;
            let engine = base64::engine::general_purpose::STANDARD_NO_PAD;
            Value::Blob(engine.decode(b64.trim_end_matches('=')).map_err(|_| bad())?)
        }
        _ => return Err(bad()),
    })
}

/// One SQL statement with positional arguments.
#[derive(Debug, Clone)]
pub struct Stmt {
    pub sql: String,
    pub args: Vec<Value>,
}

impl Stmt {
    pub fn new(sql: impl Into<String>, args: Vec<Value>) -> Self {
        Self { sql: sql.into(), args }
    }

    fn to_json(&self) -> Json {
        json!({ "sql": self.sql, "args": self.args.iter().map(to_json).collect::<Vec<_>>(), "want_rows": true })
    }
}

/// Rows returned by a query, with column names.
#[derive(Debug, Clone, Default)]
pub struct Rows {
    pub cols: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

impl Rows {
    /// First column of the first row as a number (0 if missing or NULL).
    pub fn first_i64(&self) -> i64 {
        match self.rows.first().and_then(|r| r.first()) {
            Some(Value::Integer(i)) => *i,
            Some(Value::Real(f)) => *f as i64,
            _ => 0,
        }
    }

    pub fn first_text(&self) -> Option<String> {
        match self.rows.first().and_then(|r| r.first()) {
            Some(Value::Text(t)) => Some(t.clone()),
            _ => None,
        }
    }
}

fn parse_rows(result: &Json) -> CloudResult<Rows> {
    let cols = result["cols"]
        .as_array()
        .map(|c| c.iter().map(|c| c["name"].as_str().unwrap_or("").to_string()).collect())
        .unwrap_or_default();
    let mut rows = Vec::new();
    for r in result["rows"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let cells = r.as_array().ok_or_else(|| CloudError::Protocol("row is not a list".into()))?;
        rows.push(cells.iter().map(from_json).collect::<CloudResult<Vec<_>>>()?);
    }
    Ok(Rows { cols, rows })
}

pub struct Client {
    http: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    /// The underlying HTTP client, shared with other outside services (FBR).
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("HTTP client settings are valid");
        Self { http }
    }

    async fn pipeline(&self, remote: &Remote, requests: Vec<Json>) -> CloudResult<Vec<Json>> {
        let mut requests = requests;
        requests.push(json!({ "type": "close" }));
        let res = self
            .http
            .post(format!("{}/v2/pipeline", remote.base))
            .bearer_auth(&remote.token)
            .json(&json!({ "baton": null, "requests": requests }))
            .send()
            .await
            .map_err(|e| CloudError::Network(e.without_url().to_string()))?;
        let status = res.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(CloudError::Auth);
        }
        let body: Json = res.json().await.map_err(|e| CloudError::Network(e.without_url().to_string()))?;
        if !status.is_success() {
            let msg = body["error"]["message"].as_str().or(body["message"].as_str()).unwrap_or("").to_string();
            return Err(CloudError::Protocol(format!("HTTP {status} {msg}")));
        }
        let results = body["results"].as_array().ok_or_else(|| CloudError::Protocol("no results".into()))?.clone();
        // Every request but the final "close" must have worked.
        for r in &results[..results.len().saturating_sub(1)] {
            if r["type"] == "error" {
                return Err(CloudError::Sql(r["error"]["message"].as_str().unwrap_or("unknown").to_string()));
            }
        }
        Ok(results)
    }

    /// Runs one statement and returns its rows.
    pub async fn query(&self, remote: &Remote, stmt: Stmt) -> CloudResult<Rows> {
        let results = self.pipeline(remote, vec![json!({ "type": "execute", "stmt": stmt.to_json() })]).await?;
        parse_rows(&results[0]["response"]["result"])
    }

    /// Runs several statements in one transaction: all of them or none.
    pub async fn batch(&self, remote: &Remote, stmts: &[Stmt]) -> CloudResult<()> {
        if stmts.is_empty() {
            return Ok(());
        }
        let mut steps = vec![json!({ "stmt": { "sql": "BEGIN" } })];
        for (i, s) in stmts.iter().enumerate() {
            steps.push(json!({ "stmt": s.to_json(), "condition": { "type": "ok", "step": i } }));
        }
        let last = stmts.len();
        steps.push(json!({ "stmt": { "sql": "COMMIT" }, "condition": { "type": "ok", "step": last } }));
        steps.push(json!({ "stmt": { "sql": "ROLLBACK" }, "condition": { "type": "not", "cond": { "type": "ok", "step": last + 1 } } }));
        let results = self.pipeline(remote, vec![json!({ "type": "batch", "batch": { "steps": steps } })]).await?;
        let result = &results[0]["response"]["result"];
        if let Some(errors) = result["step_errors"].as_array() {
            if let Some(e) = errors.iter().find(|e| !e.is_null()) {
                return Err(CloudError::Sql(e["message"].as_str().unwrap_or("unknown").to_string()));
            }
        }
        // COMMIT must have run.
        if result["step_results"].get(last + 1).is_none_or(Json::is_null) {
            return Err(CloudError::Sql("the cloud did not save the changes".into()));
        }
        Ok(())
    }

    /// Runs a script of several statements (schema setup).
    pub async fn script(&self, remote: &Remote, sql: &str) -> CloudResult<()> {
        self.pipeline(remote, vec![json!({ "type": "sequence", "sql": sql })]).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(normalise_url("libsql://b1-shop.turso.io").as_deref(), Some("https://b1-shop.turso.io"));
        assert_eq!(normalise_url(" https://b1-shop.turso.io/ ").as_deref(), Some("https://b1-shop.turso.io"));
        assert_eq!(normalise_url("http://127.0.0.1:8080").as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(normalise_url("http://example.com"), None);
        assert_eq!(normalise_url("https://evil.com/x?y"), None);
    }

    #[test]
    fn values_round_trip() {
        for v in
            [Value::Null, Value::Integer(-42), Value::Real(1.5), Value::Text("سلام".into()), Value::Blob(vec![1, 2, 3])]
        {
            assert_eq!(from_json(&to_json(&v)).unwrap(), v);
        }
    }
}
