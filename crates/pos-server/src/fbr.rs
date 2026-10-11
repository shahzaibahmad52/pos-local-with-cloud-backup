//! Optional reporting of sales and refunds to FBR (PRAL's Integrated
//! Invoicing System, "POS integration").
//!
//! Switched off by default. When on, every sale and refund is queued in the
//! same transaction that records it, then posted to PRAL in the background.
//! FBR's invoice number comes back and is printed on the receipt with a QR
//! code. A sale never waits on FBR: if PRAL can't be reached the queue is
//! retried until it is.
//!
//! The request follows PRAL's published POS integration format. It has not
//! been checked against the real service from here, so test with the
//! sandbox POS ID FBR issues before going live.

use std::sync::Mutex;

use reqwest::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    auth::User,
    db,
    error::{AppError, AppResult},
    pricing,
};

pub const SANDBOX_URL: &str = "https://esp.fbr.gov.pk:8244/FBR/v1/api/Live/PostData";
pub const LIVE_URL: &str = "https://gw.fbr.gov.pk/imsp/v1/api/Live/PostData";

/// PCT heading used when a product has none: 3004.9099, other medicaments.
pub const DEFAULT_PCT: &str = "30049099";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FbrSettings {
    pub enabled: bool,
    /// "sandbox" or "live".
    pub mode: String,
    pub pos_id: Option<String>,
    /// The token itself is never sent back to the screen.
    pub has_token: bool,
    pub default_pct: String,
    /// Overrides the PRAL address if FBR moves it.
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FbrInput {
    pub enabled: bool,
    pub mode: String,
    pub pos_id: Option<String>,
    /// Leave empty to keep the saved token.
    pub token: Option<String>,
    pub default_pct: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FbrStatus {
    #[serde(flatten)]
    pub settings: FbrSettings,
    pub waiting: i64,
    /// Waiting items that have failed at least once.
    pub failing: i64,
    pub last_error: Option<String>,
    pub last_sent_at: Option<String>,
}

fn clean(s: &Option<String>) -> Option<String> {
    s.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

pub fn enabled(conn: &Connection) -> AppResult<bool> {
    Ok(db::get_setting(conn, "fbr_enabled")?.as_deref() == Some("1"))
}

pub fn settings(conn: &Connection) -> AppResult<FbrSettings> {
    Ok(FbrSettings {
        enabled: enabled(conn)?,
        mode: db::get_setting(conn, "fbr_mode")?.unwrap_or_else(|| "sandbox".into()),
        pos_id: db::get_setting(conn, "fbr_pos_id")?,
        has_token: db::get_setting(conn, "fbr_token")?.is_some_and(|t| !t.is_empty()),
        default_pct: db::get_setting(conn, "fbr_default_pct")?.unwrap_or_else(|| DEFAULT_PCT.into()),
        url: db::get_setting(conn, "fbr_url")?,
    })
}

pub fn status(conn: &Connection) -> AppResult<FbrStatus> {
    let (waiting, failing): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(attempts > 0), 0) FROM fbr_queue WHERE sent_at IS NULL",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let last_error = conn
        .query_row(
            "SELECT last_error FROM fbr_queue WHERE sent_at IS NULL AND last_error IS NOT NULL
             ORDER BY created_at DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let last_sent_at = conn.query_row("SELECT MAX(sent_at) FROM fbr_queue", [], |r| r.get(0))?;
    Ok(FbrStatus { settings: settings(conn)?, waiting, failing, last_error, last_sent_at })
}

pub fn save(conn: &mut Connection, user: &User, input: &FbrInput) -> AppResult<FbrStatus> {
    if !user.is_admin() {
        return Err(AppError::Forbidden("Only the owner can change FBR settings.".into()));
    }
    if !matches!(input.mode.as_str(), "sandbox" | "live") {
        return Err(AppError::BadRequest("Choose sandbox or live.".into()));
    }
    let pos_id = clean(&input.pos_id);
    if pos_id.as_deref().is_some_and(|p| !p.chars().all(|c| c.is_ascii_digit())) {
        return Err(AppError::BadRequest("The FBR POS ID is a number.".into()).because("fbr.pos_id", json!({})));
    }
    let pct = clean(&input.default_pct).unwrap_or_else(|| DEFAULT_PCT.into());
    if !pct.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(AppError::BadRequest("The PCT code is digits, e.g. 3004.9099.".into()));
    }
    let url = clean(&input.url);
    if url.as_deref().is_some_and(|u| !u.starts_with("https://") && !u.starts_with("http://")) {
        return Err(AppError::BadRequest("The FBR address must start with https://".into()));
    }
    let tx = conn.transaction()?;
    let had_token = db::get_setting(&tx, "fbr_token")?.is_some_and(|t| !t.is_empty());
    let token = clean(&input.token);
    if input.enabled && (pos_id.is_none() || (token.is_none() && !had_token)) {
        return Err(AppError::BadRequest("Enter the POS ID and token FBR gave you before switching this on.".into())
            .because("fbr.incomplete", json!({})));
    }
    db::set_setting(&tx, "fbr_enabled", if input.enabled { "1" } else { "0" })?;
    db::set_setting(&tx, "fbr_mode", &input.mode)?;
    db::set_setting(&tx, "fbr_default_pct", &pct)?;
    for (key, value) in [("fbr_pos_id", &pos_id), ("fbr_url", &url)] {
        match value {
            Some(v) => db::set_setting(&tx, key, v)?,
            None => {
                tx.execute("DELETE FROM settings WHERE key = ?1", [key])?;
            }
        }
    }
    if let Some(t) = &token {
        db::set_setting(&tx, "fbr_token", t)?;
    }
    db::audit(
        &tx,
        Some(&user.id),
        None,
        "fbr.settings",
        &json!({ "enabled": input.enabled, "mode": input.mode, "posId": pos_id, "tokenChanged": token.is_some() }),
        None,
    )?;
    tx.commit()?;
    status(conn)
}

/// Called in the same transaction that records a sale or refund.
pub fn queue(conn: &Connection, kind: &str, doc_id: &str) -> AppResult<()> {
    if enabled(conn)? {
        conn.execute(
            "INSERT OR IGNORE INTO fbr_queue (doc_id, kind, created_at) VALUES (?1, ?2, ?3)",
            params![doc_id, kind, db::now()],
        )?;
    }
    Ok(())
}

pub fn invoice_no(conn: &Connection, kind: &str, doc_id: &str) -> AppResult<Option<String>> {
    let table = if kind == "return" { "returns" } else { "sales" };
    Ok(conn
        .query_row(&format!("SELECT fbr_invoice_no FROM {table} WHERE id = ?1"), [doc_id], |r| r.get(0))
        .optional()?
        .flatten())
}

fn rupees(paisa: i64) -> f64 {
    paisa as f64 / 100.0
}

/// PRAL wants "yyyy-MM-dd HH:mm:ss" in local time.
fn fbr_time(stamp: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(stamp)
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|_| stamp.replace('T', " ").chars().take(19).collect())
}

/// 1 cash, 2 card, 5 mixed (PRAL's payment modes). Wallets are paid by card or account.
fn payment_mode(methods: &[String]) -> i64 {
    let modes: std::collections::BTreeSet<i64> =
        methods.iter().map(|m| if m == "cash" || m == "khata" { 1 } else { 2 }).collect();
    match modes.len() {
        0 => 1,
        1 => *modes.iter().next().unwrap_or(&1),
        _ => 5,
    }
}

struct Item {
    /// The barcode of the pack sold, else any of the product's, else its id.
    code: String,
    name: String,
    qty: f64,
    pct: String,
    tax_bp: i64,
    /// What the customer paid for this line, after its share of the bill discount.
    amount: i64,
    discount: i64,
}

fn item_json(i: &Item, invoice_type: i64, ref_usin: Option<&str>) -> Value {
    let tax = pricing::included_tax(i.amount, i.tax_bp);
    json!({
        "ItemCode": i.code,
        "ItemName": i.name,
        "Quantity": i.qty,
        "PCTCode": i.pct,
        "TaxRate": i.tax_bp as f64 / 100.0,
        "SaleValue": rupees(i.amount - tax),
        "TotalAmount": rupees(i.amount),
        "TaxCharged": rupees(tax),
        "Discount": rupees(i.discount),
        "FurtherTax": 0.0,
        "InvoiceType": invoice_type,
        "RefUSIN": ref_usin,
    })
}

/// Shares a bill discount over its lines in proportion; the last line takes the rounding.
fn share_discount(totals: &[i64], discount: i64) -> Vec<i64> {
    let subtotal: i64 = totals.iter().sum();
    if discount <= 0 || subtotal <= 0 {
        return vec![0; totals.len()];
    }
    let mut out: Vec<i64> = totals.iter().map(|t| pricing::round_div(t * discount, subtotal)).collect();
    let diff = discount - out.iter().sum::<i64>();
    if let Some(last) = out.last_mut() {
        *last += diff;
    }
    out
}

fn line_qty(qty: i64, sell_mode: &str) -> f64 {
    // Weight lines are kept in grams; FBR gets kilograms.
    if sell_mode == "weight" {
        qty as f64 / 1000.0
    } else {
        qty as f64
    }
}

/// The invoice PRAL expects for one sale or refund.
pub fn build_invoice(conn: &Connection, kind: &str, doc_id: &str) -> AppResult<Value> {
    let s = settings(conn)?;
    let pos_id: i64 = s.pos_id.as_deref().and_then(|p| p.parse().ok()).unwrap_or(0);
    if kind == "return" {
        return build_refund(conn, &s, pos_id, doc_id);
    }
    let (receipt_no, created_at, discount, total, buyer, phone): (
        String,
        String,
        i64,
        i64,
        Option<String>,
        Option<String>,
    ) = conn.query_row(
        "SELECT s.receipt_no, s.created_at, s.discount, s.total, c.name, c.phone
             FROM sales s LEFT JOIN customers c ON c.id = s.customer_id WHERE s.id = ?1",
        [doc_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )?;
    let rows: Vec<(String, String, i64, String, i64, i64)> = conn
        .prepare(
            "SELECT COALESCE((SELECT MIN(code) FROM barcodes b WHERE b.product_id = l.product_id AND b.unit_id = l.unit_id),
                             (SELECT MIN(code) FROM barcodes b WHERE b.product_id = l.product_id), l.product_id),
                    p.name, l.qty, p.sell_mode, p.tax_rate_bp, l.line_total
             FROM sale_lines l JOIN products p ON p.id = l.product_id WHERE l.sale_id = ?1 ORDER BY l.line_no",
        )?
        .query_map([doc_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
        .collect::<Result<_, _>>()?;
    let shares = share_discount(&rows.iter().map(|r| r.5).collect::<Vec<_>>(), discount);
    let items: Vec<Item> = rows
        .iter()
        .zip(&shares)
        .map(|(r, d)| Item {
            code: r.0.chars().take(20).collect(),
            name: r.1.clone(),
            qty: line_qty(r.2, &r.3),
            pct: s.default_pct.clone(),
            tax_bp: r.4,
            amount: r.5 - d,
            discount: *d,
        })
        .collect();
    let methods: Vec<String> = conn
        .prepare("SELECT method FROM payments WHERE sale_id = ?1")?
        .query_map([doc_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(invoice_json(
        pos_id,
        &receipt_no,
        &created_at,
        buyer,
        phone,
        total,
        discount,
        payment_mode(&methods),
        1,
        None,
        &items,
    ))
}

fn build_refund(conn: &Connection, s: &FbrSettings, pos_id: i64, doc_id: &str) -> AppResult<Value> {
    let (return_no, created_at, refund_total, method, sale_receipt, buyer, phone): (
        String,
        String,
        i64,
        String,
        String,
        Option<String>,
        Option<String>,
    ) = conn.query_row(
        "SELECT r.return_no, r.created_at, r.refund_total, r.refund_method, s.receipt_no, c.name, c.phone
         FROM returns r JOIN sales s ON s.id = r.sale_id LEFT JOIN customers c ON c.id = s.customer_id
         WHERE r.id = ?1",
        [doc_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
    )?;
    let items: Vec<Item> = conn
        .prepare(
            "SELECT COALESCE((SELECT MIN(code) FROM barcodes b WHERE b.product_id = l.product_id AND b.unit_id = l.unit_id),
                             (SELECT MIN(code) FROM barcodes b WHERE b.product_id = l.product_id), l.product_id),
                    p.name, rl.qty, p.sell_mode, p.tax_rate_bp, rl.refund
             FROM return_lines rl JOIN sale_lines l ON l.id = rl.sale_line_id JOIN products p ON p.id = l.product_id
             WHERE rl.return_id = ?1 ORDER BY l.line_no",
        )?
        .query_map([doc_id], |r| {
            Ok(Item {
                code: r.get::<_, String>(0)?.chars().take(20).collect(),
                name: r.get(1)?,
                qty: line_qty(r.get(2)?, &r.get::<_, String>(3)?),
                pct: s.default_pct.clone(),
                tax_bp: r.get(4)?,
                amount: r.get(5)?,
                discount: 0,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(invoice_json(
        pos_id,
        &return_no,
        &created_at,
        buyer,
        phone,
        refund_total,
        0,
        payment_mode(&[method]),
        3,
        Some(&sale_receipt),
        &items,
    ))
}

#[allow(clippy::too_many_arguments)]
fn invoice_json(
    pos_id: i64,
    usin: &str,
    created_at: &str,
    buyer: Option<String>,
    phone: Option<String>,
    total: i64,
    discount: i64,
    payment_mode: i64,
    invoice_type: i64,
    ref_usin: Option<&str>,
    items: &[Item],
) -> Value {
    let taxes: Vec<i64> = items.iter().map(|i| pricing::included_tax(i.amount, i.tax_bp)).collect();
    let tax: i64 = taxes.iter().sum();
    let sale_value: i64 = items.iter().map(|i| i.amount).sum::<i64>() - tax;
    json!({
        "InvoiceNumber": "",
        "POSID": pos_id,
        "USIN": usin,
        "DateTime": fbr_time(created_at),
        "BuyerNTN": "",
        "BuyerCNIC": "",
        "BuyerName": buyer.unwrap_or_default(),
        "BuyerPhoneNumber": phone.unwrap_or_default(),
        "TotalBillAmount": rupees(total),
        "TotalQuantity": items.iter().map(|i| i.qty).sum::<f64>(),
        "TotalSaleValue": rupees(sale_value),
        "TotalTaxCharged": rupees(tax),
        "Discount": rupees(discount),
        "FurtherTax": 0.0,
        "PaymentMode": payment_mode,
        "RefUSIN": ref_usin,
        "InvoiceType": invoice_type,
        "Items": items.iter().map(|i| item_json(i, invoice_type, ref_usin)).collect::<Vec<_>>(),
    })
}

/// PRAL's reply. `Code` is "100" when the invoice was accepted.
#[derive(Debug, Deserialize)]
struct Reply {
    #[serde(rename = "InvoiceNumber", default)]
    invoice_number: Option<String>,
    #[serde(rename = "Code", default)]
    code: Option<Value>,
    #[serde(rename = "Response", default)]
    response: Option<String>,
    #[serde(rename = "Errors", default)]
    errors: Option<Value>,
}

fn lock(db: &Mutex<Connection>) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
    db.lock().map_err(|_| AppError::Internal("database lock poisoned".into()))
}

/// Minutes to wait before trying a failed invoice again: 1, 2, 4… up to an hour.
fn backoff_minutes(attempts: i64) -> i64 {
    1i64 << attempts.clamp(0, 6)
}

struct Job {
    doc_id: String,
    kind: String,
    body: Value,
    url: String,
    token: String,
}

fn next_jobs(conn: &Connection, only: Option<&str>, limit: i64) -> AppResult<Vec<Job>> {
    if !enabled(conn)? {
        return Ok(Vec::new());
    }
    let s = settings(conn)?;
    let token = db::get_setting(conn, "fbr_token")?.unwrap_or_default();
    let url = s.url.clone().unwrap_or_else(|| if s.mode == "live" { LIVE_URL } else { SANDBOX_URL }.to_string());
    let now = db::now();
    let waiting: Vec<(String, String)> = conn
        .prepare(
            "SELECT doc_id, kind FROM fbr_queue
             WHERE sent_at IS NULL AND (?1 IS NOT NULL AND doc_id = ?1 OR ?1 IS NULL AND (next_try_at IS NULL OR next_try_at <= ?2))
             ORDER BY created_at LIMIT ?3",
        )?
        .query_map(params![only, now, limit], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut jobs = Vec::new();
    for (doc_id, kind) in waiting {
        let body = build_invoice(conn, &kind, &doc_id)?;
        jobs.push(Job { doc_id, kind, body, url: url.clone(), token: token.clone() });
    }
    Ok(jobs)
}

async fn post(client: &Client, job: &Job) -> Result<String, String> {
    let res = client
        .post(&job.url)
        .bearer_auth(&job.token)
        .timeout(std::time::Duration::from_secs(15))
        .json(&job.body)
        .send()
        .await
        .map_err(|e| format!("Can't reach FBR: {e}"))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| format!("FBR reply unreadable: {e}"))?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("FBR refused the token (401). Check the token in Settings.".into());
    }
    let reply: Reply = serde_json::from_str(&text)
        .map_err(|_| format!("FBR replied {status}: {}", text.chars().take(200).collect::<String>()))?;
    let code = reply.code.as_ref().map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| c.to_string()));
    match (code.as_deref(), reply.invoice_number) {
        (Some("100"), Some(no)) if !no.is_empty() => Ok(no),
        _ => Err(format!(
            "FBR code {}: {}{}",
            code.unwrap_or_else(|| status.as_u16().to_string()),
            reply.response.unwrap_or_default(),
            reply.errors.filter(|e| !e.is_null()).map(|e| format!(" ({e})")).unwrap_or_default()
        )),
    }
}

fn record(conn: &Connection, job: &Job, result: &Result<String, String>) -> AppResult<()> {
    match result {
        Ok(no) => {
            let table = if job.kind == "return" { "returns" } else { "sales" };
            conn.execute(&format!("UPDATE {table} SET fbr_invoice_no = ?1 WHERE id = ?2"), params![no, job.doc_id])?;
            conn.execute(
                "UPDATE fbr_queue SET sent_at = ?1, last_error = NULL, attempts = attempts + 1 WHERE doc_id = ?2",
                params![db::now(), job.doc_id],
            )?;
        }
        Err(e) => {
            let attempts: i64 =
                conn.query_row("SELECT attempts FROM fbr_queue WHERE doc_id = ?1", [&job.doc_id], |r| r.get(0))?;
            let next = (chrono::Local::now() + chrono::Duration::minutes(backoff_minutes(attempts)))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
            conn.execute(
                "UPDATE fbr_queue SET attempts = attempts + 1, last_error = ?1, next_try_at = ?2 WHERE doc_id = ?3",
                params![e, next, job.doc_id],
            )?;
        }
    }
    Ok(())
}

/// Sends what is due. Returns how many FBR accepted.
pub async fn send_due(db: &Mutex<Connection>, client: &Client) -> AppResult<i64> {
    let jobs = next_jobs(&*lock(db)?, None, 20)?;
    let mut sent = 0;
    for job in jobs {
        let result = post(client, &job).await;
        sent += i64::from(result.is_ok());
        let failed_to_reach = matches!(&result, Err(e) if e.starts_with("Can't reach"));
        record(&*lock(db)?, &job, &result)?;
        // No point trying the rest while FBR is unreachable.
        if failed_to_reach {
            break;
        }
    }
    Ok(sent)
}

/// Sends one sale or refund straight away, so its receipt can carry FBR's number.
pub async fn send_now(db: &Mutex<Connection>, client: &Client, doc_id: &str) -> AppResult<Option<String>> {
    let jobs = next_jobs(&*lock(db)?, Some(doc_id), 1)?;
    let Some(job) = jobs.into_iter().next() else { return Ok(None) };
    let result = post(client, &job).await;
    record(&*lock(db)?, &job, &result)?;
    Ok(result.ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bill_discount_is_shared_over_the_lines() {
        assert_eq!(share_discount(&[3_000, 7_000], 1_000), vec![300, 700]);
        assert_eq!(share_discount(&[100, 100, 100], 100), vec![33, 33, 34]);
        assert_eq!(share_discount(&[500], 0), vec![0]);
    }

    #[test]
    fn payment_modes() {
        assert_eq!(payment_mode(&["cash".into()]), 1);
        assert_eq!(payment_mode(&["easypaisa".into()]), 2);
        assert_eq!(payment_mode(&["cash".into(), "card".into()]), 5);
    }

    #[test]
    fn times_are_local_without_offset() {
        assert_eq!(fbr_time("2026-10-11T10:15:00+05:00"), "2026-10-11 10:15:00");
    }
}
