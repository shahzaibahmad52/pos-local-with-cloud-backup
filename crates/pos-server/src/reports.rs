//! Financial and stock reports. Every report has the same shape: columns
//! (a key and how to show the value), rows, and an optional total row, so
//! the backoffice shows, prints and exports them all the same way. The
//! profit & loss statement uses `lines` instead.
//!
//! Money is paisa; percentages are basis points (1780 = 17.8%). Text values
//! starting with `@` (like `@expired`) are words the till translates.

use std::collections::{BTreeMap, HashMap};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{
    dashboard,
    error::{AppError, AppResult},
    inventory::{self, UnitPrice},
    khata, ledger, purchases,
};

#[derive(Debug, Serialize)]
pub struct Column {
    pub key: &'static str,
    /// `text`, `money`, `int`, `pct`, `date`, `month` (`YYYY-MM`) or `datetime`
    pub kind: &'static str,
}

/// One line of a statement (profit & loss).
#[derive(Debug, Serialize)]
pub struct Line {
    /// Text key for the label, or `expense` with `label` set.
    pub key: &'static str,
    pub label: Option<String>,
    pub amount: i64,
    /// `heading`, `line`, `item` (indented under a heading), `less` (shown in
    /// brackets), `sub` (subtotal) or `total`
    pub style: &'static str,
    /// Margin etc., in basis points.
    pub pct: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub name: String,
    pub from: String,
    pub to: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Map<String, Value>>,
    pub totals: Option<Map<String, Value>>,
    pub lines: Vec<Line>,
}

#[derive(Debug, Deserialize)]
pub struct Params {
    pub from: String,
    pub to: String,
    /// Months ahead for the expiry report, days without sales for slow stock.
    pub n: Option<i64>,
}

pub const NAMES: &[&str] = &[
    "profit_loss",
    "product_profit",
    "discounts",
    "daily_sales",
    "sales_by_product",
    "sales_by_category",
    "sales_by_cashier",
    "sales_by_payment",
    "returns",
    "shifts",
    "cash_book",
    "stock_valuation",
    "stock_movement",
    "expiry",
    "slow_stock",
    "purchase_register",
    "payables",
    "receivables_aging",
    "sales_tax",
];

fn col(key: &'static str, kind: &'static str) -> Column {
    Column { key, kind }
}

fn row(pairs: Vec<(&str, Value)>) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn pct(part: i64, whole: i64) -> Value {
    if whole == 0 {
        Value::Null
    } else {
        json!(((part as f64 / whole as f64) * 10_000.0).round() as i64)
    }
}

/// "12 Strip + 4 Tab", "38.5 kg": stock in the units the shop counts in.
pub fn fmt_qty(base: i64, sell_mode: &str, base_unit: &str, units: &[UnitPrice]) -> String {
    if sell_mode == "weight" {
        return if base.abs() >= 1000 {
            let kg = base as f64 / 1000.0;
            format!("{} kg", format!("{kg:.3}").trim_end_matches('0').trim_end_matches('.'))
        } else {
            format!("{base} g")
        };
    }
    let pack = units.iter().filter(|u| u.base_qty > 1).min_by_key(|u| u.base_qty);
    match pack {
        Some(p) if base.abs() >= p.base_qty => {
            let (whole, loose) = (base / p.base_qty, base % p.base_qty);
            let small = units.iter().find(|u| u.base_qty == 1).map(|u| u.name.as_str()).unwrap_or(base_unit);
            if loose == 0 {
                format!("{whole} {}", p.name)
            } else {
                format!("{whole} {} + {} {small}", p.name, loose.abs())
            }
        }
        _ => format!("{base} {base_unit}"),
    }
}

struct ProductInfo {
    name: String,
    category: Option<String>,
    sell_mode: String,
    base_unit: String,
    units: Vec<UnitPrice>,
}

fn product_infos(conn: &Connection) -> AppResult<HashMap<String, ProductInfo>> {
    let mut stmt = conn.prepare("SELECT id, name, category, sell_mode, base_unit FROM products")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut all_units: HashMap<String, Vec<UnitPrice>> = HashMap::new();
    let mut ustmt =
        conn.prepare("SELECT product_id, id, name, base_qty, price FROM product_units ORDER BY base_qty")?;
    for u in ustmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            UnitPrice { id: r.get(1)?, name: r.get(2)?, base_qty: r.get(3)?, price: r.get(4)? },
        ))
    })? {
        let (pid, unit) = u?;
        all_units.entry(pid).or_default().push(unit);
    }
    Ok(rows
        .into_iter()
        .map(|(id, name, category, sell_mode, base_unit)| {
            let units = all_units.remove(&id).unwrap_or_default();
            (id, ProductInfo { name, category, sell_mode, base_unit, units })
        })
        .collect())
}

/// Sales per product in the period, after the bill discount, with returns.
#[derive(Default, Clone)]
struct ProductSales {
    qty: i64,
    sales: i64,
    tax: i64,
    cost: i64,
    returned_qty: i64,
    refunds: i64,
    refund_tax: i64,
    cost_back: i64,
}

fn product_sales(conn: &Connection, from: &str, to: &str) -> AppResult<HashMap<String, ProductSales>> {
    let mut out: HashMap<String, ProductSales> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT l.product_id, SUM(l.base_qty),
                SUM(CASE WHEN s.subtotal > 0 THEN (2 * l.line_total * s.total + s.subtotal) / (2 * s.subtotal) ELSE 0 END),
                SUM(CASE WHEN s.subtotal > 0 THEN (2 * l.tax_included * s.total + s.subtotal) / (2 * s.subtotal) ELSE 0 END),
                SUM(l.cost_total)
         FROM sale_lines l JOIN sales s ON s.id = l.sale_id
         WHERE s.status = 'completed' AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2
         GROUP BY l.product_id",
    )?;
    for r in stmt.query_map(params![from, to], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?))
    })? {
        let (pid, qty, sales, tax, cost) = r?;
        let e = out.entry(pid).or_default();
        e.qty += qty;
        e.sales += sales;
        e.tax += tax;
        e.cost += cost;
    }
    let mut stmt = conn.prepare(
        "SELECT l.product_id, SUM(rl.base_qty), SUM(rl.refund),
                SUM(CASE WHEN l.line_total > 0 THEN rl.refund * l.tax_included / l.line_total ELSE 0 END),
                SUM(CASE WHEN rl.condition = 'restock' AND l.qty > 0 THEN l.cost_total * rl.qty / l.qty ELSE 0 END)
         FROM return_lines rl JOIN returns r ON r.id = rl.return_id JOIN sale_lines l ON l.id = rl.sale_line_id
         WHERE substr(r.created_at, 1, 10) BETWEEN ?1 AND ?2
         GROUP BY l.product_id",
    )?;
    for r in stmt.query_map(params![from, to], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?))
    })? {
        let (pid, qty, refund, tax, cost_back) = r?;
        let e = out.entry(pid).or_default();
        e.returned_qty += qty;
        e.refunds += refund;
        e.refund_tax += tax;
        e.cost_back += cost_back;
    }
    Ok(out)
}

fn sum_rows(rows: &[Map<String, Value>], keys: &[&str]) -> Map<String, Value> {
    keys.iter()
        .map(|k| {
            (k.to_string(), json!(rows.iter().map(|r| r.get(*k).and_then(Value::as_i64).unwrap_or(0)).sum::<i64>()))
        })
        .collect()
}

fn base(name: &str, p: &Params) -> Report {
    Report {
        name: name.into(),
        from: p.from.clone(),
        to: p.to.clone(),
        columns: Vec::new(),
        rows: Vec::new(),
        totals: None,
        lines: Vec::new(),
    }
}

pub fn run(conn: &Connection, name: &str, p: &Params) -> AppResult<Report> {
    if !ledger::valid_date(&p.from) || !ledger::valid_date(&p.to) || p.from > p.to {
        return Err(AppError::BadRequest("Choose a valid date range.".into()));
    }
    let (from, to) = (p.from.as_str(), p.to.as_str());
    let mut r = base(name, p);
    match name {
        "profit_loss" => profit_loss(conn, from, to, &mut r)?,
        "product_profit" => product_profit(conn, from, to, &mut r)?,
        "discounts" => discounts(conn, from, to, &mut r)?,
        "daily_sales" => daily_sales(conn, from, to, &mut r)?,
        "sales_by_product" => sales_by_product(conn, from, to, &mut r)?,
        "sales_by_category" => sales_by_category(conn, from, to, &mut r)?,
        "sales_by_cashier" => sales_by_cashier(conn, from, to, &mut r)?,
        "sales_by_payment" => sales_by_payment(conn, from, to, &mut r)?,
        "returns" => returns(conn, from, to, &mut r)?,
        "shifts" => shifts(conn, from, to, &mut r)?,
        "cash_book" => cash_book(conn, from, to, &mut r)?,
        "stock_valuation" => stock_valuation(conn, &mut r)?,
        "stock_movement" => stock_movement(conn, from, to, &mut r)?,
        "expiry" => expiry(conn, p.n.unwrap_or(3).clamp(1, 24), &mut r)?,
        "slow_stock" => slow_stock(conn, p.n.unwrap_or(60).clamp(7, 365), &mut r)?,
        "purchase_register" => purchase_register(conn, from, to, &mut r)?,
        "payables" => payables(conn, &mut r)?,
        "receivables_aging" => receivables_aging(conn, &mut r)?,
        "sales_tax" => sales_tax(conn, from, to, &mut r)?,
        _ => return Err(AppError::NotFound(format!("No report called {name}."))),
    }
    Ok(r)
}

fn line(key: &'static str, amount: i64, style: &'static str) -> Line {
    Line { key, label: None, amount, style, pct: None }
}

fn profit_loss(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    let f = dashboard::figures(conn, from, to)?;
    let (gross, discount): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(subtotal), 0), COALESCE(SUM(discount), 0) FROM sales
         WHERE status = 'completed' AND substr(created_at, 1, 10) BETWEEN ?1 AND ?2",
        params![from, to],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    // Stock thrown away or found in corrections (damaged, expired, recount), at cost.
    let written_off: i64 = conn.query_row(
        "SELECT COALESCE(SUM(-m.qty_delta * b.cost_per_base), 0) FROM stock_moves m JOIN batches b ON b.id = m.batch_id
         WHERE m.reason IN ('adjust', 'count') AND substr(m.created_at, 1, 10) BETWEEN ?1 AND ?2",
        params![from, to],
        |r| r.get(0),
    )?;
    let net_excl_tax = f.net_sales - f.sales_tax;
    r.lines.push(line("pl_gross_sales", gross, "line"));
    r.lines.push(line("pl_discounts", discount, "less"));
    r.lines.push(line("pl_returns", f.refunds, "less"));
    r.lines.push(line("pl_net_sales", f.net_sales, "sub"));
    r.lines.push(line("pl_sales_tax", f.sales_tax, "less"));
    r.lines.push(line("pl_net_excl_tax", net_excl_tax, "sub"));
    r.lines.push(line("pl_cogs", f.cost_of_sales, "less"));
    let mut gp = line("pl_gross_profit", f.gross_profit, "sub");
    gp.pct = pct(f.gross_profit, net_excl_tax).as_i64();
    r.lines.push(gp);
    r.lines.push(line("pl_expenses", 0, "heading"));
    let mut stmt = conn.prepare(
        "SELECT category, SUM(amount) FROM expenses WHERE expense_date BETWEEN ?1 AND ?2
         GROUP BY category ORDER BY SUM(amount) DESC",
    )?;
    for e in stmt.query_map(params![from, to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (cat, amount) = e?;
        r.lines.push(Line { key: "expense", label: Some(cat), amount, style: "item", pct: None });
    }
    if written_off != 0 {
        r.lines.push(line("pl_written_off", written_off, "item"));
    }
    r.lines.push(line("pl_total_expenses", f.expenses + written_off, "less"));
    let net = f.gross_profit - f.expenses - written_off;
    let mut np = line("pl_net_profit", net, "total");
    np.pct = pct(net, net_excl_tax).as_i64();
    r.lines.push(np);
    Ok(())
}

fn product_profit(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    let info = product_infos(conn)?;
    r.columns = vec![
        col("product", "text"),
        col("qty", "text"),
        col("net_sales", "money"),
        col("cost", "money"),
        col("profit", "money"),
        col("margin", "pct"),
    ];
    let mut rows: Vec<(i64, Map<String, Value>)> = Vec::new();
    for (pid, s) in product_sales(conn, from, to)? {
        let Some(p) = info.get(&pid) else { continue };
        let net = s.sales - s.refunds - (s.tax - s.refund_tax);
        let cost = s.cost - s.cost_back;
        let profit = net - cost;
        rows.push((
            profit,
            row(vec![
                ("product", json!(p.name)),
                ("qty", json!(fmt_qty(s.qty - s.returned_qty, &p.sell_mode, &p.base_unit, &p.units))),
                ("net_sales", json!(net)),
                ("cost", json!(cost)),
                ("profit", json!(profit)),
                ("margin", pct(profit, net)),
            ]),
        ));
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    r.rows = rows.into_iter().map(|x| x.1).collect();
    let mut t = sum_rows(&r.rows, &["net_sales", "cost", "profit"]);
    let (n, pr) = (t["net_sales"].as_i64().unwrap_or(0), t["profit"].as_i64().unwrap_or(0));
    t.insert("margin".into(), pct(pr, n));
    r.totals = Some(t);
    Ok(())
}

fn discounts(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("datetime", "datetime"),
        col("receipt", "text"),
        col("cashier", "text"),
        col("subtotal", "money"),
        col("discount", "money"),
        col("discount_pct", "pct"),
        col("approved_by", "text"),
    ];
    let mut stmt = conn.prepare(
        "SELECT s.created_at, s.receipt_no, u.name, s.subtotal, s.discount, a.name
         FROM sales s JOIN users u ON u.id = s.user_id LEFT JOIN users a ON a.id = s.approved_by
         WHERE s.status = 'completed' AND s.discount > 0 AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2
         ORDER BY s.created_at",
    )?;
    r.rows = stmt
        .query_map(params![from, to], |x| {
            let sub: i64 = x.get(3)?;
            let disc: i64 = x.get(4)?;
            Ok(row(vec![
                ("datetime", json!(x.get::<_, String>(0)?)),
                ("receipt", json!(x.get::<_, String>(1)?)),
                ("cashier", json!(x.get::<_, String>(2)?)),
                ("subtotal", json!(sub)),
                ("discount", json!(disc)),
                ("discount_pct", pct(disc, sub)),
                ("approved_by", json!(x.get::<_, Option<String>>(5)?)),
            ]))
        })?
        .collect::<Result<_, _>>()?;
    r.totals = Some(sum_rows(&r.rows, &["subtotal", "discount"]));
    Ok(())
}

fn daily_sales(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("date", "date"),
        col("bills", "int"),
        col("gross", "money"),
        col("discount", "money"),
        col("returns", "money"),
        col("net_sales", "money"),
        col("cash", "money"),
        col("card", "money"),
        col("wallets", "money"),
        col("khata", "money"),
    ];
    let mut days: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut add = |day: String, key: &str, v: i64| {
        let e = days.entry(day.clone()).or_insert_with(|| {
            let mut m = row(vec![("date", json!(day))]);
            for k in ["bills", "gross", "discount", "returns", "net_sales", "cash", "card", "wallets", "khata"] {
                m.insert(k.into(), json!(0));
            }
            m
        });
        let cur = e[key].as_i64().unwrap_or(0);
        e.insert(key.into(), json!(cur + v));
    };
    let mut stmt = conn.prepare(
        "SELECT substr(created_at, 1, 10), COUNT(*), SUM(subtotal), SUM(discount), SUM(total) FROM sales
         WHERE status = 'completed' AND substr(created_at, 1, 10) BETWEEN ?1 AND ?2 GROUP BY 1",
    )?;
    for x in stmt.query_map(params![from, to], |x| {
        Ok((x.get::<_, String>(0)?, x.get::<_, i64>(1)?, x.get::<_, i64>(2)?, x.get::<_, i64>(3)?, x.get::<_, i64>(4)?))
    })? {
        let (d, bills, gross, disc, total) = x?;
        add(d.clone(), "bills", bills);
        add(d.clone(), "gross", gross);
        add(d.clone(), "discount", disc);
        add(d, "net_sales", total);
    }
    let mut stmt = conn.prepare(
        "SELECT substr(created_at, 1, 10), SUM(refund_total) FROM returns
         WHERE substr(created_at, 1, 10) BETWEEN ?1 AND ?2 GROUP BY 1",
    )?;
    for x in stmt.query_map(params![from, to], |x| Ok((x.get::<_, String>(0)?, x.get::<_, i64>(1)?)))? {
        let (d, refund) = x?;
        add(d.clone(), "returns", refund);
        add(d, "net_sales", -refund);
    }
    let mut stmt = conn.prepare(
        "SELECT substr(s.created_at, 1, 10), p.method, SUM(p.amount) FROM payments p JOIN sales s ON s.id = p.sale_id
         WHERE s.status = 'completed' AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2 GROUP BY 1, 2",
    )?;
    for x in stmt
        .query_map(params![from, to], |x| Ok((x.get::<_, String>(0)?, x.get::<_, String>(1)?, x.get::<_, i64>(2)?)))?
    {
        let (d, method, amount) = x?;
        let key = match method.as_str() {
            "cash" => "cash",
            "card" => "card",
            "khata" => "khata",
            _ => "wallets",
        };
        add(d, key, amount);
    }
    r.rows = days.into_values().collect();
    r.totals = Some(sum_rows(
        &r.rows,
        &["bills", "gross", "discount", "returns", "net_sales", "cash", "card", "wallets", "khata"],
    ));
    Ok(())
}

fn sales_by_product(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    let info = product_infos(conn)?;
    r.columns = vec![
        col("product", "text"),
        col("category", "text"),
        col("qty", "text"),
        col("sales", "money"),
        col("returns", "money"),
        col("net_sales", "money"),
        col("share", "pct"),
    ];
    let data = product_sales(conn, from, to)?;
    let total: i64 = data.values().map(|s| s.sales - s.refunds).sum();
    let mut rows: Vec<(i64, Map<String, Value>)> = data
        .into_iter()
        .filter_map(|(pid, s)| {
            let p = info.get(&pid)?;
            let net = s.sales - s.refunds;
            Some((
                net,
                row(vec![
                    ("product", json!(p.name)),
                    ("category", json!(p.category)),
                    ("qty", json!(fmt_qty(s.qty, &p.sell_mode, &p.base_unit, &p.units))),
                    ("sales", json!(s.sales)),
                    ("returns", json!(s.refunds)),
                    ("net_sales", json!(net)),
                    ("share", pct(net, total)),
                ]),
            ))
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    r.rows = rows.into_iter().map(|x| x.1).collect();
    r.totals = Some(sum_rows(&r.rows, &["sales", "returns", "net_sales"]));
    Ok(())
}

fn sales_by_category(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    let info = product_infos(conn)?;
    r.columns = vec![
        col("category", "text"),
        col("products", "int"),
        col("sales", "money"),
        col("returns", "money"),
        col("net_sales", "money"),
        col("profit", "money"),
        col("share", "pct"),
    ];
    let mut cats: BTreeMap<String, (i64, i64, i64, i64)> = BTreeMap::new();
    for (pid, s) in product_sales(conn, from, to)? {
        let cat = info.get(&pid).and_then(|p| p.category.clone()).unwrap_or_else(|| "—".into());
        let e = cats.entry(cat).or_default();
        e.0 += 1;
        e.1 += s.sales;
        e.2 += s.refunds;
        e.3 += (s.sales - s.refunds) - (s.tax - s.refund_tax) - (s.cost - s.cost_back);
    }
    let total: i64 = cats.values().map(|c| c.1 - c.2).sum();
    let mut rows: Vec<(i64, Map<String, Value>)> = cats
        .into_iter()
        .map(|(cat, (n, sales, refunds, profit))| {
            (
                sales - refunds,
                row(vec![
                    ("category", json!(cat)),
                    ("products", json!(n)),
                    ("sales", json!(sales)),
                    ("returns", json!(refunds)),
                    ("net_sales", json!(sales - refunds)),
                    ("profit", json!(profit)),
                    ("share", pct(sales - refunds, total)),
                ]),
            )
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    r.rows = rows.into_iter().map(|x| x.1).collect();
    r.totals = Some(sum_rows(&r.rows, &["products", "sales", "returns", "net_sales", "profit"]));
    Ok(())
}

fn sales_by_cashier(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("cashier", "text"),
        col("bills", "int"),
        col("net_sales", "money"),
        col("avg_bill", "money"),
        col("discount", "money"),
        col("returns", "money"),
        col("cash_difference", "money"),
    ];
    let mut stmt = conn.prepare(
        "SELECT u.name,
          (SELECT COUNT(*) FROM sales s WHERE s.user_id = u.id AND s.status = 'completed'
             AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2),
          (SELECT COALESCE(SUM(total), 0) FROM sales s WHERE s.user_id = u.id AND s.status = 'completed'
             AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2),
          (SELECT COALESCE(SUM(discount), 0) FROM sales s WHERE s.user_id = u.id AND s.status = 'completed'
             AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2),
          (SELECT COALESCE(SUM(refund_total), 0) FROM returns x WHERE x.user_id = u.id
             AND substr(x.created_at, 1, 10) BETWEEN ?1 AND ?2),
          (SELECT COALESCE(SUM(counted_cash - expected_cash), 0) FROM shifts h WHERE h.user_id = u.id
             AND h.status = 'closed' AND substr(h.closed_at, 1, 10) BETWEEN ?1 AND ?2)
         FROM users u ORDER BY u.name COLLATE NOCASE",
    )?;
    r.rows = stmt
        .query_map(params![from, to], |x| {
            let bills: i64 = x.get(1)?;
            let sales: i64 = x.get(2)?;
            let refunds: i64 = x.get(4)?;
            Ok(row(vec![
                ("cashier", json!(x.get::<_, String>(0)?)),
                ("bills", json!(bills)),
                ("net_sales", json!(sales - refunds)),
                ("avg_bill", json!(if bills > 0 { sales / bills } else { 0 })),
                ("discount", json!(x.get::<_, i64>(3)?)),
                ("returns", json!(refunds)),
                ("cash_difference", json!(x.get::<_, i64>(5)?)),
            ]))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|r| ["bills", "returns", "cash_difference"].iter().any(|k| r[*k].as_i64().unwrap_or(0) != 0))
        .collect();
    r.totals = Some(sum_rows(&r.rows, &["bills", "net_sales", "discount", "returns", "cash_difference"]));
    Ok(())
}

fn sales_by_payment(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    r.columns = vec![col("method", "text"), col("bills", "int"), col("amount", "money"), col("share", "pct")];
    let mut stmt = conn.prepare(
        "SELECT p.method, COUNT(DISTINCT p.sale_id), SUM(p.amount) FROM payments p JOIN sales s ON s.id = p.sale_id
         WHERE s.status = 'completed' AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2
         GROUP BY p.method ORDER BY SUM(p.amount) DESC",
    )?;
    let data: Vec<(String, i64, i64)> =
        stmt.query_map(params![from, to], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?)))?.collect::<Result<_, _>>()?;
    let total: i64 = data.iter().map(|d| d.2).sum();
    r.rows = data
        .into_iter()
        .map(|(m, bills, amount)| {
            row(vec![
                ("method", json!(m)),
                ("bills", json!(bills)),
                ("amount", json!(amount)),
                ("share", pct(amount, total)),
            ])
        })
        .collect();
    r.totals = Some(sum_rows(&r.rows, &["amount"]));
    Ok(())
}

fn returns(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("datetime", "datetime"),
        col("return_no", "text"),
        col("receipt", "text"),
        col("cashier", "text"),
        col("items", "text"),
        col("refund", "money"),
        col("method", "text"),
        col("approved_by", "text"),
    ];
    let mut stmt = conn.prepare(
        "SELECT r.created_at, r.return_no, s.receipt_no, u.name,
                (SELECT GROUP_CONCAT(p.name || ' × ' || rl.qty || ' ' || l.unit_name || ' (' || rl.condition || ')', '; ')
                 FROM return_lines rl JOIN sale_lines l ON l.id = rl.sale_line_id JOIN products p ON p.id = l.product_id
                 WHERE rl.return_id = r.id),
                r.refund_total, r.refund_method, a.name
         FROM returns r JOIN sales s ON s.id = r.sale_id JOIN users u ON u.id = r.user_id
         LEFT JOIN users a ON a.id = r.approved_by
         WHERE substr(r.created_at, 1, 10) BETWEEN ?1 AND ?2 ORDER BY r.created_at",
    )?;
    r.rows = stmt
        .query_map(params![from, to], |x| {
            Ok(row(vec![
                ("datetime", json!(x.get::<_, String>(0)?)),
                ("return_no", json!(x.get::<_, String>(1)?)),
                ("receipt", json!(x.get::<_, String>(2)?)),
                ("cashier", json!(x.get::<_, String>(3)?)),
                ("items", json!(x.get::<_, Option<String>>(4)?)),
                ("refund", json!(x.get::<_, i64>(5)?)),
                ("method", json!(x.get::<_, String>(6)?)),
                ("approved_by", json!(x.get::<_, Option<String>>(7)?)),
            ]))
        })?
        .collect::<Result<_, _>>()?;
    r.totals = Some(sum_rows(&r.rows, &["refund"]));
    Ok(())
}

fn shifts(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("till", "text"),
        col("cashier", "text"),
        col("opened", "datetime"),
        col("closed", "datetime"),
        col("bills", "int"),
        col("sales", "money"),
        col("expected", "money"),
        col("counted", "money"),
        col("difference", "money"),
        col("note", "text"),
    ];
    let mut stmt = conn.prepare(
        "SELECT h.till_code, u.name, h.opened_at, h.closed_at,
                (SELECT COUNT(*) FROM sales s WHERE s.shift_id = h.id AND s.status = 'completed'),
                (SELECT COALESCE(SUM(total), 0) FROM sales s WHERE s.shift_id = h.id AND s.status = 'completed'),
                h.expected_cash, h.counted_cash, h.note
         FROM shifts h JOIN users u ON u.id = h.user_id
         WHERE substr(h.opened_at, 1, 10) BETWEEN ?1 AND ?2 ORDER BY h.opened_at",
    )?;
    r.rows = stmt
        .query_map(params![from, to], |x| {
            let expected: Option<i64> = x.get(6)?;
            let counted: Option<i64> = x.get(7)?;
            let closed: Option<String> = x.get(3)?;
            Ok(row(vec![
                ("till", json!(x.get::<_, String>(0)?)),
                ("cashier", json!(x.get::<_, String>(1)?)),
                ("opened", json!(x.get::<_, String>(2)?)),
                ("closed", json!(closed)),
                ("bills", json!(x.get::<_, i64>(4)?)),
                ("sales", json!(x.get::<_, i64>(5)?)),
                ("expected", json!(expected)),
                ("counted", json!(counted.filter(|_| closed.is_some()))),
                ("difference", json!(expected.zip(counted).filter(|_| closed.is_some()).map(|(e, c)| c - e))),
                ("note", json!(x.get::<_, Option<String>>(8)?)),
            ]))
        })?
        .collect::<Result<_, _>>()?;
    r.totals = Some(sum_rows(&r.rows, &["bills", "sales", "difference"]));
    Ok(())
}

fn cash_book(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("datetime", "datetime"),
        col("till", "text"),
        col("detail", "text"),
        col("cash_in", "money"),
        col("cash_out", "money"),
        col("user", "text"),
    ];
    // Opening floats, cash sales and refunds per shift and day (a late shift
    // runs past midnight), plus every cash in/out.
    let mut stmt = conn.prepare(
        "SELECT * FROM (
           SELECT h.opened_at AS at, h.till_code, 'opening' AS what, NULL AS reason, h.opening_cash AS cin, 0 AS cout, u.name
             FROM shifts h JOIN users u ON u.id = h.user_id
           UNION ALL
           SELECT MAX(s.created_at), s.till_code, 'cash_sales', NULL, SUM(p.amount), 0, u.name
             FROM payments p JOIN sales s ON s.id = p.sale_id LEFT JOIN shifts h ON h.id = s.shift_id
             JOIN users u ON u.id = COALESCE(h.user_id, s.user_id)
             WHERE p.method = 'cash' AND s.status = 'completed'
             GROUP BY s.shift_id, s.till_code, substr(s.created_at, 1, 10)
           UNION ALL
           SELECT MAX(x.created_at), x.till_code, 'cash_refunds', NULL, 0, SUM(x.refund_total), u.name
             FROM returns x JOIN users u ON u.id = x.user_id WHERE x.refund_method = 'cash'
             GROUP BY x.shift_id, x.till_code, substr(x.created_at, 1, 10)
           UNION ALL
           SELECT m.created_at, h.till_code, 'move', m.reason,
                  CASE WHEN m.kind = 'in' THEN m.amount ELSE 0 END, CASE WHEN m.kind = 'out' THEN m.amount ELSE 0 END, u.name
             FROM cash_moves m JOIN shifts h ON h.id = m.shift_id JOIN users u ON u.id = m.user_id
         ) WHERE substr(at, 1, 10) BETWEEN ?1 AND ?2 ORDER BY at",
    )?;
    r.rows = stmt
        .query_map(params![from, to], |x| {
            let what: String = x.get(2)?;
            let reason: Option<String> = x.get(3)?;
            Ok(row(vec![
                ("datetime", json!(x.get::<_, String>(0)?)),
                ("till", json!(x.get::<_, String>(1)?)),
                ("detail", json!(reason.unwrap_or_else(|| format!("@{what}")))),
                ("cash_in", json!(x.get::<_, i64>(4)?)),
                ("cash_out", json!(x.get::<_, i64>(5)?)),
                ("user", json!(x.get::<_, String>(6)?)),
            ]))
        })?
        .collect::<Result<_, _>>()?;
    r.totals = Some(sum_rows(&r.rows, &["cash_in", "cash_out"]));
    Ok(())
}

/// Sale price of one base unit: the counting pack's price spread over its units.
fn price_per_base(row: &inventory::InventoryRow) -> f64 {
    if row.sell_mode == "weight" {
        return row.price_per_kg.unwrap_or(0) as f64 / 1000.0;
    }
    row.units
        .iter()
        .filter(|u| u.base_qty > 0)
        .min_by_key(|u| u.base_qty)
        .map(|u| u.price as f64 / u.base_qty as f64)
        .unwrap_or(0.0)
}

fn stock_valuation(conn: &Connection, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("product", "text"),
        col("category", "text"),
        col("qty", "text"),
        col("avg_cost", "money"),
        col("value_cost", "money"),
        col("value_sale", "money"),
        col("potential_profit", "money"),
    ];
    let mut rows = Vec::new();
    for mut p in inventory::all_rows(conn)?.into_iter().filter(|p| p.active && p.stock_base > 0) {
        p.units = inventory::unit_prices(conn, &p.id)?;
        let sale = (p.stock_base as f64 * price_per_base(&p)).round() as i64;
        rows.push(row(vec![
            ("product", json!(p.name)),
            ("category", json!(p.category)),
            ("qty", json!(fmt_qty(p.stock_base, &p.sell_mode, &p.base_unit, &p.units))),
            ("avg_cost", json!(p.avg_cost)),
            ("value_cost", json!(p.stock_value)),
            ("value_sale", json!(sale)),
            ("potential_profit", json!(sale - p.stock_value)),
        ]));
    }
    r.rows = rows;
    r.totals = Some(sum_rows(&r.rows, &["value_cost", "value_sale", "potential_profit"]));
    Ok(())
}

fn stock_movement(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    let info = product_infos(conn)?;
    r.columns = vec![
        col("product", "text"),
        col("opening", "text"),
        col("received", "text"),
        col("sold", "text"),
        col("returned", "text"),
        col("adjusted", "text"),
        col("closing", "text"),
    ];
    let mut stmt = conn.prepare(
        "SELECT product_id,
           SUM(CASE WHEN substr(created_at, 1, 10) < ?1 THEN qty_delta ELSE 0 END),
           SUM(CASE WHEN substr(created_at, 1, 10) BETWEEN ?1 AND ?2 AND reason IN ('purchase', 'opening') THEN qty_delta ELSE 0 END),
           SUM(CASE WHEN substr(created_at, 1, 10) BETWEEN ?1 AND ?2 AND reason = 'sale' THEN -qty_delta ELSE 0 END),
           SUM(CASE WHEN substr(created_at, 1, 10) BETWEEN ?1 AND ?2 AND reason = 'return' THEN qty_delta ELSE 0 END),
           SUM(CASE WHEN substr(created_at, 1, 10) BETWEEN ?1 AND ?2 AND reason NOT IN ('purchase', 'opening', 'sale', 'return')
                    THEN qty_delta ELSE 0 END)
         FROM stock_moves WHERE substr(created_at, 1, 10) <= ?2 GROUP BY product_id",
    )?;
    let mut rows: Vec<(String, Map<String, Value>)> = Vec::new();
    for x in stmt.query_map(params![from, to], |x| {
        Ok((
            x.get::<_, String>(0)?,
            [x.get::<_, i64>(1)?, x.get::<_, i64>(2)?, x.get::<_, i64>(3)?, x.get::<_, i64>(4)?, x.get::<_, i64>(5)?],
        ))
    })? {
        let (pid, [opening, received, sold, returned, adjusted]) = x?;
        if received == 0 && sold == 0 && returned == 0 && adjusted == 0 {
            continue;
        }
        let Some(p) = info.get(&pid) else { continue };
        let q =
            |v: i64| json!(if v == 0 { "—".to_string() } else { fmt_qty(v, &p.sell_mode, &p.base_unit, &p.units) });
        rows.push((
            p.name.to_lowercase(),
            row(vec![
                ("product", json!(p.name)),
                ("opening", q(opening)),
                ("received", q(received)),
                ("sold", q(sold)),
                ("returned", q(returned)),
                ("adjusted", q(adjusted)),
                ("closing", q(opening + received - sold + returned + adjusted)),
            ]),
        ));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    r.rows = rows.into_iter().map(|x| x.1).collect();
    Ok(())
}

fn expiry(conn: &Connection, months: i64, r: &mut Report) -> AppResult<()> {
    let info = product_infos(conn)?;
    r.columns = vec![
        col("product", "text"),
        col("batch", "text"),
        col("expiry", "month"),
        col("qty", "text"),
        col("value_cost", "money"),
        col("status", "text"),
    ];
    let until = inventory::month_after((months - 1).max(0) as u32);
    let now = crate::db::current_month();
    let mut stmt = conn.prepare(
        "SELECT b.product_id, b.batch_no, b.expiry, b.qty_on_hand, b.qty_on_hand * b.cost_per_base
         FROM batches b JOIN products p ON p.id = b.product_id
         WHERE p.active = 1 AND b.qty_on_hand > 0 AND b.expiry IS NOT NULL AND b.expiry <= ?1
         ORDER BY b.expiry, p.name",
    )?;
    r.rows = stmt
        .query_map([&until], |x| {
            Ok((
                x.get::<_, String>(0)?,
                x.get::<_, String>(1)?,
                x.get::<_, String>(2)?,
                x.get::<_, i64>(3)?,
                x.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter_map(|(pid, batch, exp, qty, value)| {
            let p = info.get(&pid)?;
            Some(row(vec![
                ("product", json!(p.name)),
                ("batch", json!(batch)),
                ("status", json!(if exp < now { "@expired" } else { "@expiring" })),
                ("expiry", json!(exp)),
                ("qty", json!(fmt_qty(qty, &p.sell_mode, &p.base_unit, &p.units))),
                ("value_cost", json!(value)),
            ]))
        })
        .collect();
    r.totals = Some(sum_rows(&r.rows, &["value_cost"]));
    Ok(())
}

fn slow_stock(conn: &Connection, days: i64, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("product", "text"),
        col("qty", "text"),
        col("value_cost", "money"),
        col("last_sold", "date"),
        col("days", "int"),
    ];
    let today = ledger::today();
    let since = ledger::add_days(&today, -days).unwrap_or_default();
    let mut rows = Vec::new();
    for mut p in inventory::all_rows(conn)?.into_iter().filter(|p| p.active && p.stock_base > 0) {
        let last: Option<String> = conn.query_row(
            "SELECT MAX(substr(s.created_at, 1, 10)) FROM sale_lines l JOIN sales s ON s.id = l.sale_id
             WHERE l.product_id = ?1 AND s.status = 'completed'",
            [&p.id],
            |x| x.get(0),
        )?;
        if last.as_deref().is_some_and(|d| d >= since.as_str()) {
            continue;
        }
        p.units = inventory::unit_prices(conn, &p.id)?;
        rows.push((
            p.stock_value,
            row(vec![
                ("product", json!(p.name)),
                ("qty", json!(fmt_qty(p.stock_base, &p.sell_mode, &p.base_unit, &p.units))),
                ("value_cost", json!(p.stock_value)),
                ("days", json!(last.as_deref().map(|d| ledger::days_between(d, &today)))),
                ("last_sold", json!(last)),
            ]),
        ));
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    r.rows = rows.into_iter().map(|x| x.1).collect();
    r.totals = Some(sum_rows(&r.rows, &["value_cost"]));
    Ok(())
}

fn purchase_register(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("date", "date"),
        col("purchase_no", "text"),
        col("supplier", "text"),
        col("invoice", "text"),
        col("gross", "money"),
        col("discount", "money"),
        col("freight", "money"),
        col("net", "money"),
        col("paid_now", "money"),
        col("due", "date"),
    ];
    let mut stmt = conn.prepare(
        "SELECT p.invoice_date, p.purchase_no, s.name, p.invoice_no, p.gross, p.discount, p.freight, p.net, p.paid_now,
                p.due_date
         FROM purchases p JOIN suppliers s ON s.id = p.supplier_id
         WHERE p.invoice_date BETWEEN ?1 AND ?2 ORDER BY p.invoice_date, p.purchase_no",
    )?;
    r.rows = stmt
        .query_map(params![from, to], |x| {
            Ok(row(vec![
                ("date", json!(x.get::<_, String>(0)?)),
                ("purchase_no", json!(x.get::<_, String>(1)?)),
                ("supplier", json!(x.get::<_, String>(2)?)),
                ("invoice", json!(x.get::<_, Option<String>>(3)?)),
                ("gross", json!(x.get::<_, i64>(4)?)),
                ("discount", json!(x.get::<_, i64>(5)?)),
                ("freight", json!(x.get::<_, i64>(6)?)),
                ("net", json!(x.get::<_, i64>(7)?)),
                ("paid_now", json!(x.get::<_, i64>(8)?)),
                ("due", json!(x.get::<_, Option<String>>(9)?)),
            ]))
        })?
        .collect::<Result<_, _>>()?;
    r.totals = Some(sum_rows(&r.rows, &["gross", "discount", "freight", "net", "paid_now"]));
    Ok(())
}

fn payables(conn: &Connection, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("supplier", "text"),
        col("phone", "text"),
        col("terms", "int"),
        col("balance", "money"),
        col("next_due", "date"),
        col("status", "text"),
    ];
    r.rows = purchases::suppliers(conn)?
        .into_iter()
        .filter(|s| s.balance != 0)
        .map(|s| {
            row(vec![
                ("supplier", json!(s.name)),
                ("phone", json!(s.phone)),
                ("terms", json!(s.terms_days)),
                ("balance", json!(s.balance)),
                ("next_due", json!(s.next_due)),
                ("status", json!(if s.overdue { "@overdue" } else { "@current" })),
            ])
        })
        .collect();
    r.totals = Some(sum_rows(&r.rows, &["balance"]));
    Ok(())
}

fn receivables_aging(conn: &Connection, r: &mut Report) -> AppResult<()> {
    r.columns = vec![
        col("customer", "text"),
        col("phone", "text"),
        col("balance", "money"),
        col("d0_30", "money"),
        col("d31_60", "money"),
        col("d61_90", "money"),
        col("d90_plus", "money"),
        col("last_paid", "date"),
    ];
    let today = ledger::today();
    let mut rows = Vec::new();
    for a in khata::list(conn, "", "balance")?.accounts {
        let mut buckets = [0i64; 4];
        for o in ledger::open_charges(&khata::entries(conn, &a.id)?) {
            let age = ledger::days_between(&o.date, &today);
            let i = match age {
                ..=30 => 0,
                31..=60 => 1,
                61..=90 => 2,
                _ => 3,
            };
            buckets[i] += o.unpaid;
        }
        rows.push(row(vec![
            ("customer", json!(a.name)),
            ("phone", json!(a.phone)),
            ("balance", json!(a.balance)),
            ("d0_30", json!(buckets[0])),
            ("d31_60", json!(buckets[1])),
            ("d61_90", json!(buckets[2])),
            ("d90_plus", json!(buckets[3])),
            ("last_paid", json!(a.last_paid)),
        ]));
    }
    rows.sort_by_key(|r| -r["balance"].as_i64().unwrap_or(0));
    r.rows = rows;
    r.totals = Some(sum_rows(&r.rows, &["balance", "d0_30", "d31_60", "d61_90", "d90_plus"]));
    Ok(())
}

fn sales_tax(conn: &Connection, from: &str, to: &str, r: &mut Report) -> AppResult<()> {
    r.columns = vec![col("rate", "pct"), col("sales_incl", "money"), col("tax", "money"), col("sales_excl", "money")];
    // Grouped by each product's current tax rate, net of refunds.
    let mut stmt = conn.prepare(
        "SELECT p.tax_rate_bp,
           (SELECT COALESCE(SUM(CASE WHEN s.subtotal > 0 THEN (2 * l.line_total * s.total + s.subtotal) / (2 * s.subtotal) ELSE 0 END), 0)
              FROM sale_lines l JOIN sales s ON s.id = l.sale_id JOIN products q ON q.id = l.product_id
              WHERE q.tax_rate_bp = p.tax_rate_bp AND s.status = 'completed'
                AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2),
           (SELECT COALESCE(SUM(CASE WHEN s.subtotal > 0 THEN (2 * l.tax_included * s.total + s.subtotal) / (2 * s.subtotal) ELSE 0 END), 0)
              FROM sale_lines l JOIN sales s ON s.id = l.sale_id JOIN products q ON q.id = l.product_id
              WHERE q.tax_rate_bp = p.tax_rate_bp AND s.status = 'completed'
                AND substr(s.created_at, 1, 10) BETWEEN ?1 AND ?2),
           (SELECT COALESCE(SUM(rl.refund), 0) FROM return_lines rl JOIN returns x ON x.id = rl.return_id
              JOIN sale_lines l ON l.id = rl.sale_line_id JOIN products q ON q.id = l.product_id
              WHERE q.tax_rate_bp = p.tax_rate_bp AND substr(x.created_at, 1, 10) BETWEEN ?1 AND ?2),
           (SELECT COALESCE(SUM(CASE WHEN l.line_total > 0 THEN rl.refund * l.tax_included / l.line_total ELSE 0 END), 0)
              FROM return_lines rl JOIN returns x ON x.id = rl.return_id
              JOIN sale_lines l ON l.id = rl.sale_line_id JOIN products q ON q.id = l.product_id
              WHERE q.tax_rate_bp = p.tax_rate_bp AND substr(x.created_at, 1, 10) BETWEEN ?1 AND ?2)
         FROM (SELECT DISTINCT tax_rate_bp FROM products) p ORDER BY p.tax_rate_bp DESC",
    )?;
    r.rows = stmt
        .query_map(params![from, to], |x| {
            let rate: i64 = x.get(0)?;
            let incl = x.get::<_, i64>(1)? - x.get::<_, i64>(3)?;
            let tax = x.get::<_, i64>(2)? - x.get::<_, i64>(4)?;
            Ok(row(vec![
                ("rate", json!(rate)),
                ("sales_incl", json!(incl)),
                ("tax", json!(tax)),
                ("sales_excl", json!(incl - tax)),
            ]))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|r| r["sales_incl"].as_i64().unwrap_or(0) != 0)
        .collect();
    r.totals = Some(sum_rows(&r.rows, &["sales_incl", "tax", "sales_excl"]));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(name: &str, base_qty: i64) -> UnitPrice {
        UnitPrice { id: name.into(), name: name.into(), base_qty, price: 0 }
    }

    #[test]
    fn quantities_read_like_the_shop_counts() {
        let units = vec![u("Tab", 1), u("Strip", 10), u("Box", 200)];
        assert_eq!(fmt_qty(1243, "unit", "tab", &units), "124 Strip + 3 Tab");
        assert_eq!(fmt_qty(1240, "unit", "tab", &units), "124 Strip");
        assert_eq!(fmt_qty(7, "unit", "tab", &units), "7 tab");
        assert_eq!(fmt_qty(38_500, "weight", "g", &[]), "38.5 kg");
        assert_eq!(fmt_qty(250, "weight", "g", &[]), "250 g");
        assert_eq!(fmt_qty(-20, "unit", "tab", &units), "-2 Strip");
    }
}
