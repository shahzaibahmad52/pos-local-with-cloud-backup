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

/// Every money figure for sales made and expenses paid between `?1` and
/// `?2` (`YYYY-MM-DD`, inclusive), in one row. The same SQL runs on this PC
/// and on other stores' cloud copies (head-office view).
pub const FIGURES_SQL: &str = "
SELECT
  (SELECT COALESCE(SUM(total), 0) FROM sales
    WHERE status = 'completed' AND substr(created_at, 1, 10) BETWEEN ?1 AND ?2),
  (SELECT COUNT(*) FROM sales
    WHERE status = 'completed' AND substr(created_at, 1, 10) BETWEEN ?1 AND ?2),
  (SELECT COALESCE(SUM(tax_included), 0) FROM sales
    WHERE status = 'completed' AND substr(created_at, 1, 10) BETWEEN ?1 AND ?2),
  (SELECT COALESCE(SUM(l.cost_total), 0) FROM sale_lines l JOIN sales s ON s.id = l.sale_id
    WHERE s.status = 'completed' AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2),
  (SELECT COALESCE(SUM(refund_total), 0) FROM returns WHERE substr(created_at, 1, 10) BETWEEN ?1 AND ?2),
  (SELECT COALESCE(SUM(rl.refund * l.tax_included / l.line_total), 0) FROM return_lines rl
    JOIN returns r ON r.id = rl.return_id JOIN sale_lines l ON l.id = rl.sale_line_id
    WHERE substr(r.created_at, 1, 10) BETWEEN ?1 AND ?2 AND l.line_total > 0),
  (SELECT COALESCE(SUM(l.cost_total * rl.qty / l.qty), 0) FROM return_lines rl
    JOIN returns r ON r.id = rl.return_id JOIN sale_lines l ON l.id = rl.sale_line_id
    WHERE substr(r.created_at, 1, 10) BETWEEN ?1 AND ?2 AND rl.condition = 'restock' AND l.qty > 0),
  (SELECT COALESCE(SUM(amount), 0) FROM expenses WHERE expense_date BETWEEN ?1 AND ?2)";

/// Total khata owed and how many customers owe it.
pub const KHATA_DUE_SQL: &str = "
SELECT COALESCE(SUM(b), 0), COUNT(*) FROM (
  SELECT SUM(amount) AS b FROM khata_entries GROUP BY customer_id) WHERE b > 0";

/// Turns the eight numbers from [`FIGURES_SQL`] into the dashboard figures.
pub fn figures_from(v: [i64; 8]) -> Figures {
    let [sales, bills, tax, cost, refunds, tax_refunded, cost_restocked, expenses] = v;
    let net_sales = sales - refunds;
    let sales_tax = tax - tax_refunded;
    let cost_of_sales = cost - cost_restocked;
    let gross_profit = net_sales - sales_tax - cost_of_sales;
    Figures {
        sales,
        bills,
        refunds,
        net_sales,
        sales_tax,
        cost_of_sales,
        gross_profit,
        expenses,
        net_profit: gross_profit - expenses,
    }
}

/// Money figures for sales made and expenses paid between two dates.
pub fn figures(conn: &Connection, from: &str, to: &str) -> AppResult<Figures> {
    let v = conn.query_row(FIGURES_SQL, params![from, to], |r| {
        let mut v = [0i64; 8];
        for (i, slot) in v.iter_mut().enumerate() {
            *slot = r.get(i)?;
        }
        Ok(v)
    })?;
    Ok(figures_from(v))
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
