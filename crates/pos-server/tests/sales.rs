//! End-to-end sale rules against the demo store.

mod common;

use pos_server::{
    auth::{self, Auth, User},
    db,
    products::{self, ProductView},
    sales::{self, Approval, PaymentInput, SaleInput, SaleLineInput},
    seed,
};
use rusqlite::Connection;

fn store() -> Connection {
    let mut conn = db::open_in_memory().unwrap();
    assert!(seed::demo(&mut conn).unwrap());
    common::ensure_shift(&conn);
    conn
}

fn user(conn: &Connection, name: &str) -> User {
    let id: String = conn.query_row("SELECT id FROM users WHERE name = ?1", [name], |r| r.get(0)).unwrap();
    auth::load_user(conn, &id).unwrap().unwrap()
}

fn product(conn: &Connection, name: &str) -> ProductView {
    products::search(conn, name, 5).unwrap().into_iter().next().expect(name)
}

fn unit_id(p: &ProductView, name: &str) -> String {
    p.units.iter().find(|u| u.name == name).unwrap().id.clone()
}

fn units_line(p: &ProductView, unit: &str, qty: i64) -> SaleLineInput {
    SaleLineInput {
        product_id: p.id.clone(),
        unit_id: Some(unit_id(p, unit)),
        qty: Some(qty),
        grams: None,
        amount: None,
    }
}

fn cash(amount: i64) -> Vec<PaymentInput> {
    vec![PaymentInput { method: "cash".into(), amount }]
}

fn sale(lines: Vec<SaleLineInput>, payments: Vec<PaymentInput>) -> SaleInput {
    SaleInput {
        till_code: "T01".into(),
        customer_id: None,
        lines,
        discount_bp: 0,
        payments,
        client_ref: None,
        approval: None,
        offline: None,
    }
}

fn batch_qty(conn: &Connection, batch_no: &str) -> i64 {
    conn.query_row("SELECT qty_on_hand FROM batches WHERE batch_no = ?1", [batch_no], |r| r.get(0)).unwrap()
}

#[test]
fn sells_from_the_earliest_expiry_batch() {
    let mut conn = store();
    let ali = user(&conn, "Ali");
    let panadol = product(&conn, "Panadol");
    assert_eq!(panadol.stock_base, 1_240);
    assert_eq!(panadol.next_batch_no.as_deref(), Some("B-2291"));

    let r = sales::create_sale(
        &mut conn,
        &Auth::default(),
        &ali,
        &sale(vec![units_line(&panadol, "Strip", 3)], cash(10_000)),
    )
    .unwrap();
    assert_eq!(r.receipt_no, "B1-T01-000001");
    assert_eq!(r.total, 9_000);
    assert_eq!(r.change_due, 1_000);
    assert_eq!(r.lines[0].batch_no.as_deref(), Some("B-2291"));
    assert_eq!(batch_qty(&conn, "B-2291"), 970);
    assert_eq!(batch_qty(&conn, "B-2310"), 240);
    // Cash is recorded as what stayed in the drawer.
    assert_eq!(r.payments[0].amount, 9_000);
}

#[test]
fn new_batch_price_applies_once_old_stock_is_gone() {
    let mut conn = store();
    let ali = user(&conn, "Ali");
    let panadol = product(&conn, "Panadol");
    // 5 boxes = 1,000 tablets = all of batch B-2291.
    sales::create_sale(&mut conn, &Auth::default(), &ali, &sale(vec![units_line(&panadol, "Box", 5)], cash(295_000)))
        .unwrap();
    assert_eq!(batch_qty(&conn, "B-2291"), 0);

    let panadol = product(&conn, "Panadol");
    assert_eq!(panadol.next_batch_no.as_deref(), Some("B-2310"));
    let strip = panadol.units.iter().find(|u| u.name == "Strip").unwrap();
    assert_eq!(strip.price, 3_300, "batch B-2310 is printed Rs 33 per strip");
    let r = sales::create_sale(
        &mut conn,
        &Auth::default(),
        &ali,
        &sale(vec![units_line(&panadol, "Strip", 1)], cash(3_300)),
    )
    .unwrap();
    assert_eq!(r.total, 3_300);
    assert_eq!(r.receipt_no, "B1-T01-000002");
}

#[test]
fn expired_stock_is_never_sold() {
    let mut conn = store();
    let ali = user(&conn, "Ali");
    let flagyl = product(&conn, "Flagyl");
    assert_eq!(flagyl.stock_base, 120, "the expired FG-100 batch is not counted");
    let err = sales::create_sale(
        &mut conn,
        &Auth::default(),
        &ali,
        &sale(vec![units_line(&flagyl, "Strip", 13)], cash(100_000)),
    )
    .unwrap_err();
    assert_eq!(err.code(), "insufficient_stock");
    assert!(err.to_string().contains("expired"), "{err}");
    assert_eq!(batch_qty(&conn, "FG-100"), 50);
    assert_eq!(batch_qty(&conn, "FG-209"), 120);
}

#[test]
fn two_lines_of_one_product_cannot_oversell() {
    let mut conn = store();
    let ali = user(&conn, "Ali");
    let aug = product(&conn, "Augmentin");
    assert_eq!(aug.stock_base, 84);
    let lines = vec![units_line(&aug, "Strip", 14), units_line(&aug, "Tab", 1)];
    let err = sales::create_sale(&mut conn, &Auth::default(), &ali, &sale(lines, cash(1_000_000))).unwrap_err();
    assert!(err.code() == "insufficient_stock", "{err:?}");
    assert_eq!(batch_qty(&conn, "AG-118"), 84, "nothing is taken when the sale fails");
}

#[test]
fn sugar_by_amount_or_by_weight() {
    let mut conn = store();
    let ali = user(&conn, "Ali");
    let sugar = product(&conn, "Sugar");
    let by_amount =
        SaleLineInput { product_id: sugar.id.clone(), unit_id: None, qty: None, grams: None, amount: Some(1_000) };
    let by_weight =
        SaleLineInput { product_id: sugar.id.clone(), unit_id: None, qty: None, grams: Some(250), amount: None };
    let r =
        sales::create_sale(&mut conn, &Auth::default(), &ali, &sale(vec![by_amount, by_weight], cash(5_000))).unwrap();
    assert_eq!(r.lines[0].qty, 63); // Rs 10 at Rs 160/kg
    assert_eq!(r.lines[0].line_total, 1_000);
    assert_eq!(r.lines[1].line_total, 4_000); // 250 g
    assert_eq!(r.total, 5_000);
    assert_eq!(r.item_count, 2);
    assert_eq!(product(&conn, "Sugar").stock_base, 38_500 - 63 - 250);
}

#[test]
fn included_sales_tax_is_reported() {
    let mut conn = store();
    let ali = user(&conn, "Ali");
    let milk = product(&conn, "Olper");
    let r =
        sales::create_sale(&mut conn, &Auth::default(), &ali, &sale(vec![units_line(&milk, "Pack", 2)], cash(66_000)))
            .unwrap();
    assert_eq!(r.total, 66_000);
    assert_eq!(r.tax_included, 10_068);
}

#[test]
fn big_discount_needs_a_manager_pin() {
    let mut conn = store();
    let auth = Auth::default();
    let ali = user(&conn, "Ali");
    let sana = user(&conn, "Sana");
    let risek = product(&conn, "Risek");

    let mut input = sale(vec![units_line(&risek, "Strip", 1)], cash(36_000));
    input.discount_bp = 1_500;
    let err = sales::create_sale(&mut conn, &auth, &ali, &input).unwrap_err();
    assert!(err.code() == "approval_required", "{err:?}");

    input.approval = Some(Approval { user_id: sana.id.clone(), secret: "0000".into() });
    assert!(sales::create_sale(&mut conn, &auth, &ali, &input).unwrap_err().code() == "forbidden");

    input.approval = Some(Approval { user_id: sana.id.clone(), secret: "2345".into() });
    let r = sales::create_sale(&mut conn, &auth, &ali, &input).unwrap();
    assert_eq!(r.discount, 5_400);
    assert_eq!(r.total, 30_600);
    let approved_by: String =
        conn.query_row("SELECT approved_by FROM audit_log WHERE action = 'sale.approved'", [], |r| r.get(0)).unwrap();
    assert_eq!(approved_by, sana.id);

    // A 5% discount is within the cashier's own limit.
    let mut small = sale(vec![units_line(&risek, "Strip", 1)], cash(36_000));
    small.discount_bp = 500;
    assert!(sales::create_sale(&mut conn, &auth, &ali, &small).is_ok());
}

#[test]
fn khata_needs_a_customer_and_respects_the_limit() {
    let mut conn = store();
    let auth = Auth::default();
    let ali = user(&conn, "Ali");
    let brufen = product(&conn, "Brufen");
    let imran: String = conn.query_row("SELECT id FROM customers WHERE name = 'Mr. Imran'", [], |r| r.get(0)).unwrap();
    let khata = |amount| vec![PaymentInput { method: "khata".into(), amount }];

    let err = sales::create_sale(&mut conn, &auth, &ali, &sale(vec![units_line(&brufen, "Bottle", 1)], khata(15_000)))
        .unwrap_err();
    assert!(err.code() == "bad_request", "{err:?}");

    let mut input = sale(vec![units_line(&brufen, "Bottle", 2)], khata(30_000));
    input.customer_id = Some(imran.clone());
    let r = sales::create_sale(&mut conn, &auth, &ali, &input).unwrap();
    assert_eq!(r.khata_balance, Some(30_000));
    assert_eq!(r.customer_name.as_deref(), Some("Mr. Imran"));

    // Limit is Rs 10,000. Rs 300 owed + Rs 9,780 more = Rs 10,080, over it.
    let aug = product(&conn, "Augmentin");
    let panadol = product(&conn, "Panadol");
    let risek = product(&conn, "Risek");
    let lines = vec![units_line(&aug, "Strip", 14), units_line(&panadol, "Box", 6), units_line(&risek, "Strip", 1)];
    let mut over = sale(lines, khata(978_000));
    over.customer_id = Some(imran.clone());
    let err = sales::create_sale(&mut conn, &auth, &ali, &over).unwrap_err();
    assert!(err.code() == "approval_required", "{err:?}");

    // The owner can allow it.
    let owner = user(&conn, "Owner");
    let r = sales::create_sale(&mut conn, &auth, &owner, &over).unwrap();
    assert_eq!(r.khata_balance, Some(1_008_000));
}

#[test]
fn payments_must_cover_the_total_and_only_cash_gives_change() {
    let mut conn = store();
    let auth = Auth::default();
    let ali = user(&conn, "Ali");
    let brufen = product(&conn, "Brufen");
    let short = sale(vec![units_line(&brufen, "Bottle", 1)], cash(10_000));
    assert!(sales::create_sale(&mut conn, &auth, &ali, &short).unwrap_err().code() == "bad_request");
    let card_over =
        sale(vec![units_line(&brufen, "Bottle", 1)], vec![PaymentInput { method: "card".into(), amount: 20_000 }]);
    assert!(sales::create_sale(&mut conn, &auth, &ali, &card_over).unwrap_err().code() == "bad_request");
    let split = sale(
        vec![units_line(&brufen, "Bottle", 1)],
        vec![
            PaymentInput { method: "jazzcash".into(), amount: 10_000 },
            PaymentInput { method: "cash".into(), amount: 10_000 },
        ],
    );
    let r = sales::create_sale(&mut conn, &auth, &ali, &split).unwrap();
    assert_eq!(r.change_due, 5_000);
}

#[test]
fn retrying_with_the_same_client_ref_does_not_double_sell() {
    let mut conn = store();
    let auth = Auth::default();
    let ali = user(&conn, "Ali");
    let brufen = product(&conn, "Brufen");
    let mut input = sale(vec![units_line(&brufen, "Bottle", 1)], cash(15_000));
    input.client_ref = Some("till-T01-abc".into());
    let first = sales::create_sale(&mut conn, &auth, &ali, &input).unwrap();
    let again = sales::create_sale(&mut conn, &auth, &ali, &input).unwrap();
    assert_eq!(first.id, again.id);
    assert_eq!(batch_qty(&conn, "BF-031"), 21);
    let outbox: i64 = conn.query_row("SELECT COUNT(*) FROM outbox WHERE entity = 'sale'", [], |r| r.get(0)).unwrap();
    assert_eq!(outbox, 1);
}

fn an_hour_ago() -> String {
    (chrono::Local::now() - chrono::Duration::hours(1)).to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

fn offline_sale(no: &str, lines: Vec<(SaleLineInput, i64, i64)>, paid: i64) -> SaleInput {
    SaleInput {
        till_code: "T02".into(),
        customer_id: None,
        lines: lines.iter().map(|l| l.0.clone()).collect(),
        discount_bp: 0,
        payments: vec![sales::PaymentInput { method: "cash".into(), amount: paid }],
        client_ref: Some(format!("ref-{no}")),
        approval: None,
        offline: Some(sales::OfflineSale {
            receipt_no: no.into(),
            created_at: an_hour_ago(),
            lines: lines.iter().map(|l| sales::OfflineLine { unit_price: l.1, line_total: l.2 }).collect(),
        }),
    }
}

#[test]
fn offline_sales_are_recorded_as_they_happened() {
    let mut conn = store();
    let ali = user(&conn, "Ali");
    let aug = product(&conn, "Augmentin");
    // The till sold 15 strips at last week's price while the network was down; only 14 are on record.
    let input = offline_sale("B1-T02-X000001", vec![(units_line(&aug, "Strip", 15), 40_000, 600_000)], 600_000);
    let r = sales::create_sale(&mut conn, &Auth::default(), &ali, &input).unwrap();
    assert_eq!(r.receipt_no, "B1-T02-X000001");
    assert_eq!(r.total, 600_000);
    assert_eq!(r.created_at, input.offline.as_ref().unwrap().created_at);
    assert_eq!(batch_qty(&conn, "AG-118"), -6, "the shortfall shows up for the next count");
    let (action, details): (String, String) = conn
        .query_row("SELECT action, details FROM audit_log WHERE action = 'sale.offline'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(action, "sale.offline");
    assert!(details.contains("\"oversold\":1") && details.contains("\"priceDiffers\":true"), "{details}");

    // Sending it again (the till retries) records it once.
    let again = sales::create_sale(&mut conn, &Auth::default(), &ali, &input).unwrap();
    assert_eq!(again.id, r.id);

    // Another till's number, or khata, is refused.
    let bad = offline_sale("B1-T09-X000001", vec![(units_line(&aug, "Tab", 1), 7_000, 7_000)], 7_000);
    assert!(sales::create_sale(&mut conn, &Auth::default(), &ali, &bad).is_err());
}

#[test]
fn the_catalogue_carries_everything_an_offline_till_needs() {
    let mut conn = store();
    let ali = user(&conn, "Ali");
    let cat = products::catalogue(&conn, "T02").unwrap();
    assert_eq!(cat.store.code, "B1");
    assert!(cat.products.iter().any(|p| p.name.starts_with("Panadol")));
    assert!(cat.barcodes.iter().any(|b| b.0 == "8964000123463"));
    assert_eq!(cat.last_offline_no, 0);

    let aug = product(&conn, "Augmentin");
    let input = offline_sale("B1-T02-X000041", vec![(units_line(&aug, "Tab", 1), 7_000, 7_000)], 7_000);
    sales::create_sale(&mut conn, &Auth::default(), &ali, &input).unwrap();
    assert_eq!(products::catalogue(&conn, "T02").unwrap().last_offline_no, 41);
    assert_eq!(products::catalogue(&conn, "T01").unwrap().last_offline_no, 0);
}
