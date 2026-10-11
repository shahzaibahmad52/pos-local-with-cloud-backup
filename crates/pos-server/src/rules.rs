//! Manager-approval rules and the activity log.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    auth::{Approval, Auth, User},
    db,
    error::{AppError, AppResult},
};

/// Which till actions need a manager PIN. Same for every till in the store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRules {
    /// Cashiers may give up to this discount without a PIN (500 = 5%).
    pub cashier_max_discount_bp: i64,
    /// Refunds above this amount (paisa) need a PIN.
    pub refund_approval_above: i64,
    pub remove_item: bool,
    pub cancel_bill: bool,
    pub cash_out: bool,
    pub no_sale_drawer: bool,
}

impl Default for ApprovalRules {
    fn default() -> Self {
        Self {
            cashier_max_discount_bp: 500,
            refund_approval_above: 50_000,
            remove_item: true,
            cancel_bill: true,
            cash_out: false,
            no_sale_drawer: true,
        }
    }
}

fn flag(conn: &Connection, key: &str, default: bool) -> AppResult<bool> {
    Ok(db::get_setting(conn, key)?.map(|v| v == "1").unwrap_or(default))
}

pub fn load(conn: &Connection) -> AppResult<ApprovalRules> {
    let d = ApprovalRules::default();
    Ok(ApprovalRules {
        cashier_max_discount_bp: db::setting_i64(conn, "cashier_max_discount_bp", d.cashier_max_discount_bp)?,
        refund_approval_above: db::setting_i64(conn, "refund_approval_above", d.refund_approval_above)?,
        remove_item: flag(conn, "rule_remove_item", d.remove_item)?,
        cancel_bill: flag(conn, "rule_cancel_bill", d.cancel_bill)?,
        cash_out: flag(conn, "rule_cash_out", d.cash_out)?,
        no_sale_drawer: flag(conn, "rule_no_sale_drawer", d.no_sale_drawer)?,
    })
}

pub fn save(conn: &Connection, rules: &ApprovalRules, by: &User) -> AppResult<ApprovalRules> {
    if !(0..=10_000).contains(&rules.cashier_max_discount_bp) {
        return Err(AppError::BadRequest("Discount limit must be between 0% and 100%.".into()));
    }
    if rules.refund_approval_above < 0 {
        return Err(AppError::BadRequest("Refund limit can't be negative.".into()));
    }
    let b = |v: bool| if v { "1" } else { "0" };
    db::set_setting(conn, "cashier_max_discount_bp", &rules.cashier_max_discount_bp.to_string())?;
    db::set_setting(conn, "refund_approval_above", &rules.refund_approval_above.to_string())?;
    db::set_setting(conn, "rule_remove_item", b(rules.remove_item))?;
    db::set_setting(conn, "rule_cancel_bill", b(rules.cancel_bill))?;
    db::set_setting(conn, "rule_cash_out", b(rules.cash_out))?;
    db::set_setting(conn, "rule_no_sale_drawer", b(rules.no_sale_drawer))?;
    db::audit(conn, Some(&by.id), None, "rules.update", &serde_json::to_value(rules).unwrap_or_default(), None)?;
    load(conn)
}

/// Till actions that happen before a sale exists, so the till reports them here.
pub const TILL_ACTIONS: &[&str] = &["remove_item", "cancel_bill", "no_sale_drawer"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionInput {
    pub action: String,
    pub till_code: String,
    #[serde(default)]
    pub details: Value,
    pub approval: Option<Approval>,
}

/// Records a till action, first checking the manager PIN when the rules ask for one.
pub fn record_action(conn: &Connection, auth: &Auth, user: &User, input: &ActionInput) -> AppResult<Value> {
    if !TILL_ACTIONS.contains(&input.action.as_str()) {
        return Err(AppError::BadRequest(format!("Unknown action {}.", input.action)));
    }
    let rules = load(conn)?;
    let needs_pin = match input.action.as_str() {
        "remove_item" => rules.remove_item,
        "cancel_bill" => rules.cancel_bill,
        "no_sale_drawer" => rules.no_sale_drawer,
        _ => false,
    };
    let approver = auth.approver(conn, input.approval.as_ref())?;
    if needs_pin && approver.is_none() && !user.is_admin() {
        return Err(AppError::ApprovalRequired("A manager PIN is needed for this.".into())
            .because("approval.action", json!({ "action": input.action })));
    }
    db::audit(
        conn,
        Some(&user.id),
        Some(&input.till_code),
        &format!("till.{}", input.action),
        &input.details,
        approver.as_ref().map(|a| a.id.as_str()),
    )?;
    Ok(json!({ "ok": true, "approvedBy": approver.map(|a| a.name) }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: String,
    pub created_at: String,
    pub till_code: Option<String>,
    pub user_name: Option<String>,
    pub action: String,
    pub details: Value,
    pub approved_by_name: Option<String>,
    /// Worth the owner's attention: cash short/over, cancelled bills, drawer
    /// opened without a sale, lockouts.
    pub flagged: bool,
}

fn is_flagged(action: &str, details: &Value) -> bool {
    match action {
        "shift.close" => details.get("difference").and_then(Value::as_i64).is_some_and(|d| d != 0),
        "till.cancel_bill" | "till.no_sale_drawer" | "auth.locked" => true,
        // Sold offline with less stock than recorded, a different price, or too big a discount.
        "sale.offline" => {
            details.get("oversold").and_then(Value::as_i64).is_some_and(|n| n > 0)
                || details.get("priceDiffers") == Some(&Value::Bool(true))
                || details.get("discountOverLimit") == Some(&Value::Bool(true))
        }
        _ => false,
    }
}

/// Activity for one day (`YYYY-MM-DD`), newest first.
pub fn activity(conn: &Connection, date: &str, filter: &str) -> AppResult<Vec<ActivityEntry>> {
    if date.len() != 10 || !date.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return Err(AppError::BadRequest("Date must look like 2026-10-10.".into()));
    }
    let mut stmt = conn.prepare(
        "SELECT a.id, a.created_at, a.till_code, u.name, a.action, a.details, ap.name
         FROM audit_log a
         LEFT JOIN users u ON u.id = a.user_id
         LEFT JOIN users ap ON ap.id = a.approved_by
         WHERE a.created_at LIKE ?1 || '%'
         ORDER BY a.created_at DESC, a.rowid DESC
         LIMIT 1000",
    )?;
    let rows = stmt
        .query_map(params![date], |r| {
            let details: String = r.get(5)?;
            let action: String = r.get(4)?;
            let details: Value = serde_json::from_str(&details).unwrap_or(Value::Null);
            Ok(ActivityEntry {
                id: r.get(0)?,
                created_at: r.get(1)?,
                till_code: r.get(2)?,
                user_name: r.get(3)?,
                flagged: is_flagged(&action, &details),
                action,
                details,
                approved_by_name: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter(|e| match filter {
            "attention" => e.flagged,
            "approvals" => e.approved_by_name.is_some(),
            "money" => ["sale.", "return.", "shift.", "cash."].iter().any(|p| e.action.starts_with(p)),
            "stock" => ["product.", "stock.", "count.", "transfer."].iter().any(|p| e.action.starts_with(p)),
            _ => true,
        })
        .collect())
}
