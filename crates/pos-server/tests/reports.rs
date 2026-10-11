//! Reports agree with each other and with the dashboard on the same day's trading.

use pos_server::{
    auth::{self, Auth, User},
    dashboard, db,
    inventory::{self, AdjustInput, InventoryQuery},
    ledger, products,
    reports::{self, Params, Report, NAMES},
    returns::{self, ReturnInput, ReturnLineInput},
    sales::{self, PaymentInput, SaleInput, SaleLineInput},
    seed, shifts, store,
};
use rusqlite::Connection;
use serde_json::Value;

fn user(conn: &Connection, name: &str) -> User {
    let id: String = conn.query_row("SELECT id FROM users WHERE name = ?1", [name], |r| r.get(0)).unwrap();
    auth::load_user(conn, &id).unwrap().unwrap()
}

fn unit_line(conn: &Connection, product: &str, unit: &str, qty: i64) -> SaleLineInput {
    let p = products::search(conn, product, 1).unwrap().remove(0);
    let unit_id = p.units.iter().find(|u| u.name == unit).unwrap().id.clone();
    SaleLineInput { product_id: p.id, unit_id: Some(unit_id), qty: Some(qty), grams: None, amount: None }
}

fn sell(
    conn: &mut Connection,
    lines: Vec<SaleLineInput>,
    discount_bp: i64,
    method: &str,
    amount: i64,
) -> sales::Receipt {
    let owner = user(conn, "Owner");
    let input = SaleInput {
        till_code: "T01".into(),
        customer_id: None,
        lines,
        discount_bp,
        payments: vec![PaymentInput { method: method.into(), amount }],
        client_ref: None,
        approval: None,
    };
    sales::create_sale(conn, &Auth::default(), &owner, &input).unwrap()
}

/// A day of trading: discounted sale, taxed milk, sugar by weight, a card
/// sale, a return to stock and a damaged box written off.
fn trading_day() -> Connection {
    let mut conn = db::open_in_memory().unwrap();
    seed::demo(&mut conn).unwrap();
    let ali = user(&conn, "Ali");
    store::open_shift(&conn, &ali, "T01", 500_000).unwrap();
    let panadol = unit_line(&conn, "Panadol", "Strip", 3);
    let milk = unit_line(&conn, "Olper", "Pack", 2);
    let sale = sell(&mut conn, vec![panadol, milk], 1_000, "cash", 100_000);
    let sugar = products::search(&conn, "Sugar", 1).unwrap().remove(0);
    let sugar_line = SaleLineInput { product_id: sugar.id, unit_id: None, qty: None, grams: Some(750), amount: None };
    let risek = unit_line(&conn, "Risek", "Strip", 1);
    sell(&mut conn, vec![sugar_line, risek], 0, "card", 48_000);

    let owner = user(&conn, "Owner");
    let found = returns::sale_for_return(&conn, &sale.receipt_no).unwrap();
    let panadol_line = found.lines.iter().find(|l| l.product_name.starts_with("Panadol")).unwrap().sale_line_id.clone();
    returns::create_return(
        &mut conn,
        &Auth::default(),
        &owner,
        &ReturnInput {
            sale_id: sale.id,
            till_code: "T01".into(),
            lines: vec![ReturnLineInput {
                sale_line_id: panadol_line,
                qty: 1,
                condition: "restock".into(),
                reason: None,
            }],
            refund_method: "cash".into(),
            client_ref: None,
            approval: None,
        },
    )
    .unwrap();
    let brufen: String = conn.query_row("SELECT id FROM batches WHERE batch_no = 'BF-031'", [], |r| r.get(0)).unwrap();
    inventory::adjust(
        &mut conn,
        &owner,
        &AdjustInput { batch_id: brufen, new_qty: 20, reason: "damaged".into(), note: None },
    )
    .unwrap();
    conn
}

fn run(conn: &Connection, name: &str) -> Report {
    let today = ledger::today();
    reports::run(conn, name, &Params { from: today.clone(), to: today, n: None }).unwrap()
}

fn total(r: &Report, key: &str) -> i64 {
    r.totals.as_ref().unwrap()[key].as_i64().unwrap()
}

fn line(r: &Report, key: &str) -> i64 {
    r.lines.iter().find(|l| l.key == key).unwrap().amount
}

#[test]
fn every_report_runs() {
    let conn = trading_day();
    for name in NAMES {
        let r = run(&conn, name);
        assert!(!r.rows.is_empty() || !r.lines.is_empty(), "{name} is empty");
    }
    let book = run(&conn, "cash_book");
    // The cash book's in − out is what the drawer should hold.
    assert_eq!(book.rows[0]["detail"], "@opening");
    let shift: String = conn.query_row("SELECT id FROM shifts", [], |r| r.get(0)).unwrap();
    assert_eq!(total(&book, "cash_in") - total(&book, "cash_out"), shifts::expected_cash(&conn, &shift).unwrap());
    assert_eq!(run(&conn, "shifts").rows.len(), 1);
    let today = ledger::today();
    assert!(reports::run(&conn, "nope", &Params { from: today.clone(), to: today, n: None }).is_err());
}

#[test]
fn profit_and_loss_matches_the_dashboard() {
    let conn = trading_day();
    let today = ledger::today();
    let f = dashboard::figures(&conn, &today, &today).unwrap();
    let pl = run(&conn, "profit_loss");
    assert_eq!(
        line(&pl, "pl_gross_sales") - line(&pl, "pl_discounts") - line(&pl, "pl_returns"),
        line(&pl, "pl_net_sales")
    );
    assert_eq!(line(&pl, "pl_net_sales"), f.net_sales);
    assert_eq!(line(&pl, "pl_gross_profit"), f.gross_profit);
    // 2 bottles of Brufen at Rs 125 thrown away.
    assert_eq!(line(&pl, "pl_written_off"), 25_000);
    assert_eq!(line(&pl, "pl_net_profit"), f.net_profit - 25_000);
    assert!(pl.lines.iter().any(|l| l.label.as_deref() == Some("Generator fuel")));
}

#[test]
fn breakdowns_add_up_to_the_totals() {
    let conn = trading_day();
    let today = ledger::today();
    let f = dashboard::figures(&conn, &today, &today).unwrap();

    let daily = run(&conn, "daily_sales");
    assert_eq!(total(&daily, "net_sales"), f.net_sales);
    assert_eq!(total(&daily, "bills"), 2);
    assert_eq!(total(&daily, "card"), 48_000);

    let pay = run(&conn, "sales_by_payment");
    assert_eq!(total(&pay, "amount"), f.sales);

    // Splitting the bill discount over lines rounds each line down, so allow a paisa a line.
    let by_product = run(&conn, "sales_by_product");
    assert!((total(&by_product, "net_sales") - f.net_sales).abs() <= 4);
    let profit = run(&conn, "product_profit");
    assert!(
        (total(&profit, "profit") - f.gross_profit).abs() <= 8,
        "{} vs {}",
        total(&profit, "profit"),
        f.gross_profit
    );
    let cats = run(&conn, "sales_by_category");
    assert_eq!(total(&cats, "net_sales"), total(&by_product, "net_sales"));

    let tax = run(&conn, "sales_tax");
    assert!((total(&tax, "tax") - f.sales_tax).abs() <= 2);
    let rate_18 = tax.rows.iter().find(|r| r["rate"] == 1_800).unwrap();
    assert!(rate_18["tax"].as_i64().unwrap() > 0);

    let cashier = run(&conn, "sales_by_cashier");
    assert_eq!(total(&cashier, "net_sales"), f.net_sales);
    let discounts = run(&conn, "discounts");
    assert_eq!(discounts.rows.len(), 1);
    assert_eq!(discounts.rows[0]["discount_pct"], 1_000);
}

#[test]
fn stock_reports_match_the_shelves() {
    let conn = trading_day();
    let page = inventory::list(
        &conn,
        &InventoryQuery { q: String::new(), tab: "all".into(), category: String::new(), offset: 0, limit: 200 },
    )
    .unwrap();
    let valuation = run(&conn, "stock_valuation");
    assert_eq!(total(&valuation, "value_cost"), page.summary.stock_value);
    assert!(total(&valuation, "potential_profit") > 0);

    let moves = run(&conn, "stock_movement");
    let panadol = moves.rows.iter().find(|r| r["product"] == "Panadol 500mg Tablet").unwrap();
    // 1,240 tablets − 3 strips sold + 1 strip back = 1,220.
    assert_eq!(panadol["closing"], "122 Strip");
    assert_eq!(panadol["sold"], "3 Strip");
    assert_eq!(panadol["returned"], "1 Strip");
    let brufen = moves.rows.iter().find(|r| r["product"] == "Brufen Syrup 90ml").unwrap();
    assert_eq!(brufen["adjusted"], "-2 btl");

    let expiry = run(&conn, "expiry");
    let fg100 = expiry.rows.iter().find(|r| r["batch"] == "FG-100").unwrap();
    assert_eq!(fg100["status"], "@expired");

    // Everything with stock and no sale in 60 days is slow; Panadol sold today isn't.
    let slow = run(&conn, "slow_stock");
    assert!(slow.rows.iter().all(|r| r["product"] != "Panadol 500mg Tablet"));
    assert!(slow.rows.iter().any(|r| r["product"] == "Augmentin 625mg Tablet" && r["last_sold"] == Value::Null));
}

#[test]
fn khata_ageing_buckets() {
    let conn = trading_day();
    let aging = run(&conn, "receivables_aging");
    assert_eq!(aging.rows[0]["d0_30"], 420_000);
    let old = ledger::add_days(&ledger::today(), -45).unwrap();
    conn.execute("UPDATE khata_entries SET created_at = ?1 || 'T10:00:00+05:00'", [&old]).unwrap();
    let aging = run(&conn, "receivables_aging");
    assert_eq!(aging.rows[0]["d31_60"], 420_000);
    assert_eq!(total(&aging, "balance"), 420_000);

    let payables = run(&conn, "payables");
    assert_eq!(payables.rows[0]["supplier"], "Distributor A");
    let register = run(&conn, "purchase_register");
    assert_eq!(total(&register, "net"), total(&payables, "balance"));
}
