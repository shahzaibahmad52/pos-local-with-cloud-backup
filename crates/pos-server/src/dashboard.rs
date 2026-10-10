//! The owner's dashboard: sales, profit and expenses for a period, plus the
//! stock that needs attention.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult},
    inventory::{self, ExpiringBatch, InventoryRow},
    khata, ledger, purchases,
};

#[derive(Debug, Deserialize)]
pub struct Period {
    /// `YYYY-MM-DD`, inclusive.
    pub from: String,
    pub to: String,
}

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Figures {
    pub sales: i64,
    pub bills: i64,
    pub refunds: i64,
    /// Sales minus refunds.
    pub net_sales: i64,
    /// Sales tax included in prices (owed to the government), net of refunds.
    pub sales_tax: i64,
    /// Cost of what was sold, less stock that came back on the shelf.
    pub cost_of_sales: i64,
    pub gross_profit: i64,
    pub expenses: i64,
    pub net_profit: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayTotal {
    pub date: String,
    pub net_sales: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub from: String,
    pub to: String,
    pub figures: Figures,
    pub payments: Vec<(String, i64)>,
    pub khata_due: i64,
    pub khata_customers: i64,
    pub payables: i64,
    pub payables_overdue: i64,
    pub days: Vec<DayTotal>,
    pub low_stock: Vec<InventoryRow>,
    pub expiring: Vec<ExpiringBatch>,
}

fn one(conn: &Connection, sql: &str, from: &str, to: &str) -> AppResult<i64> {
    Ok(conn.query_row(sql, params![from, to], |r| r.get::<_, Option<i64>>(0))?.unwrap_or(0))
}

/// Money figures for sales made and expenses paid between two dates.
pub fn figures(conn: &Connection, from: &str, to: &str) -> AppResult<Figures> {
    const SALES_IN: &str = "s.status = 'completed' AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2";
    const RETURNS_IN: &str = "substr(r.created_at, 1, 10) BETWEEN ?1 AND ?2";
    let sales = one(conn, &format!("SELECT SUM(s.total) FROM sales s WHERE {SALES_IN}"), from, to)?;
    let bills = one(conn, &format!("SELECT COUNT(*) FROM sales s WHERE {SALES_IN}"), from, to)?;
    let tax = one(conn, &format!("SELECT SUM(s.tax_included) FROM sales s WHERE {SALES_IN}"), from, to)?;
    let cost = one(
        conn,
        &format!("SELECT SUM(l.cost_total) FROM sale_lines l JOIN sales s ON s.id = l.sale_id WHERE {SALES_IN}"),
        from,
        to,
    )?;
    let refunds = one(conn, &format!("SELECT SUM(r.refund_total) FROM returns r WHERE {RETURNS_IN}"), from, to)?;
    // The tax and cost share of each returned line, in proportion to what came back.
    let tax_refunded = one(
        conn,
        &format!(
            "SELECT SUM(rl.refund * l.tax_included / l.line_total) FROM return_lines rl
             JOIN returns r ON r.id = rl.return_id JOIN sale_lines l ON l.id = rl.sale_line_id
             WHERE {RETURNS_IN} AND l.line_total > 0"
        ),
        from,
        to,
    )?;
    let cost_restocked = one(
        conn,
        &format!(
            "SELECT SUM(l.cost_total * rl.qty / l.qty) FROM return_lines rl
             JOIN returns r ON r.id = rl.return_id JOIN sale_lines l ON l.id = rl.sale_line_id
             WHERE {RETURNS_IN} AND rl.condition = 'restock' AND l.qty > 0"
        ),
        from,
        to,
    )?;
    let expenses = one(conn, "SELECT SUM(amount) FROM expenses WHERE expense_date BETWEEN ?1 AND ?2", from, to)?;
    let net_sales = sales - refunds;
    let sales_tax = tax - tax_refunded;
    let cost_of_sales = cost - cost_restocked;
    let gross_profit = net_sales - sales_tax - cost_of_sales;
    Ok(Figures {
        sales,
        bills,
        refunds,
        net_sales,
        sales_tax,
        cost_of_sales,
        gross_profit,
        expenses,
        net_profit: gross_profit - expenses,
    })
}

pub fn dashboard(conn: &Connection, period: &Period) -> AppResult<Dashboard> {
    if !ledger::valid_date(&period.from) || !ledger::valid_date(&period.to) || period.from > period.to {
        return Err(AppError::BadRequest("Choose a valid date range.".into()));
    }
    let (from, to) = (period.from.as_str(), period.to.as_str());
    let figures = figures(conn, from, to)?;
    let payments = conn
        .prepare(
            "SELECT p.method, SUM(p.amount) FROM payments p JOIN sales s ON s.id = p.sale_id
             WHERE s.status = 'completed' AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2
             GROUP BY p.method ORDER BY SUM(p.amount) DESC",
        )?
        .query_map(params![from, to], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    let today = ledger::today();
    let mut days = Vec::with_capacity(14);
    for back in (0..14).rev() {
        let date = ledger::add_days(&today, -back).unwrap_or_default();
        let f = conn.query_row(
            "SELECT COALESCE((SELECT SUM(total) FROM sales WHERE status = 'completed' AND substr(created_at, 1, 10) = ?1), 0)
                  - COALESCE((SELECT SUM(refund_total) FROM returns WHERE substr(created_at, 1, 10) = ?1), 0)",
            [&date],
            |r| r.get(0),
        )?;
        days.push(DayTotal { date, net_sales: f });
    }

    let khata = khata::list(conn, "", "balance")?.summary;
    let suppliers = purchases::suppliers(conn)?;
    Ok(Dashboard {
        from: period.from.clone(),
        to: period.to.clone(),
        figures,
        payments,
        khata_due: khata.total_due,
        khata_customers: khata.owing_count,
        payables: suppliers.iter().map(|s| s.balance.max(0)).sum(),
        payables_overdue: suppliers.iter().filter(|s| s.overdue && s.balance > 0).map(|s| s.balance).sum(),
        days,
        low_stock: inventory::low_stock(conn, 8)?,
        expiring: inventory::expiring_batches(conn, 3, 8)?,
    })
}
