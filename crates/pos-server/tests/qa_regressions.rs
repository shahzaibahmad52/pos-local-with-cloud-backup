//! Bugs found in the full QA round, kept fixed.

mod common;

use pos_server::{
    auth::{self, Auth, User},
    counts::{self, LineInput, StartInput},
    db, inventory, products, returns, rules, sales, seed, shifts, store,
};
use rusqlite::Connection;
use serde_json::{json, Value};

fn demo() -> Connection {
    let mut conn = db::open_in_memory().unwrap();
    seed::demo(&mut conn).unwrap();
    common::ensure_shift(&conn);
    conn
}

fn user(conn: &Connection, name: &str) -> User {
    let id: String = conn.query_row("SELECT id FROM users WHERE name = ?1", [name], |r| r.get(0)).unwrap();
    auth::load_user(conn, &id).unwrap().unwrap()
}

fn input<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}

fn panadol(conn: &Connection) -> products::ProductView {
    products::search(conn, "Panadol", 1).unwrap().remove(0)
}

fn unit(p: &products::ProductView, name: &str) -> String {
    p.units.iter().find(|u| u.name == name).unwrap().id.clone()
}

fn sale(
    conn: &mut Connection,
    lines: Value,
    payments: Value,
    extra: Value,
) -> Result<sales::Receipt, pos_server::error::AppError> {
    let ali = user(conn, "Ali");
    let mut v = json!({ "tillCode": "T01", "lines": lines, "discountBp": 0, "payments": payments });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    sales::create_sale(conn, &Auth::default(), &ali, &input(v))
}

fn strips(p: &products::ProductView, n: i64) -> Value {
    json!([{ "productId": p.id, "unitId": unit(p, "Strip"), "qty": n }])
}

#[test]
fn absurd_numbers_are_refused_not_crashed_on() {
    let mut conn = demo();
    let p = panadol(&conn);
    let err =
        sale(&mut conn, strips(&p, 4_611_686_018_427_387_904), json!([{ "method": "cash", "amount": 1 }]), json!({}))
            .unwrap_err();
    assert_eq!(err.code(), "bad_request");
    let max = i64::MAX;
    let err = sale(
        &mut conn,
        strips(&p, 1),
        json!([{ "method": "cash", "amount": max }, { "method": "cash", "amount": max }]),
        json!({}),
    )
    .unwrap_err();
    assert_eq!(err.code(), "bad_request");
}

#[test]
fn change_comes_out_once_across_two_cash_amounts() {
    let mut conn = demo();
    let p = panadol(&conn);
    // Rs 150 paid as Rs 100 + Rs 100: Rs 50 change, Rs 150 stays in the drawer.
    let r = sale(
        &mut conn,
        strips(&p, 5),
        json!([{ "method": "cash", "amount": 10_000 }, { "method": "cash", "amount": 10_000 }]),
        json!({}),
    )
    .unwrap();
    assert_eq!(r.change_due, 5_000);
    assert_eq!(r.payments.iter().map(|p| p.amount).sum::<i64>(), 15_000);
}

#[test]
fn no_open_shift_no_sale() {
    let mut conn = db::open_in_memory().unwrap();
    seed::demo(&mut conn).unwrap();
    let p = panadol(&conn);
    let err = sale(&mut conn, strips(&p, 1), json!([{ "method": "cash", "amount": 3_000 }]), json!({})).unwrap_err();
    assert_eq!(err.reason(), Some("shift.none"));
    let owner = user(&conn, "Owner");
    assert_eq!(store::open_shift(&conn, &owner, " t01", 0).unwrap_err().reason(), Some("till.bad_code"));
}

#[test]
fn refunds_follow_the_bill() {
    let mut conn = demo();
    let ali = user(&conn, "Ali");
    let p = panadol(&conn);
    let imran: String = conn.query_row("SELECT id FROM customers WHERE name = 'Mr. Imran'", [], |r| r.get(0)).unwrap();
    let r =
        sale(&mut conn, strips(&p, 3), json!([{ "method": "khata", "amount": 9_000 }]), json!({ "customerId": imran }))
            .unwrap();
    let line = returns::sale_for_return(&conn, &r.receipt_no).unwrap().lines[0].sale_line_id.clone();
    let ret = |method: &str, qty: i64| -> returns::ReturnInput {
        input(json!({ "saleId": r.id, "tillCode": "T01", "refundMethod": method,
                      "lines": [{ "saleLineId": line, "qty": qty, "condition": "restock" }] }))
    };
    let err = returns::create_return(&mut conn, &Auth::default(), &ali, &ret("cash", 3)).unwrap_err();
    assert_eq!(err.reason(), Some("return.khata_sale"), "khata bills aren't refunded in cash");
    returns::create_return(&mut conn, &Auth::default(), &ali, &ret("khata", 3)).unwrap();

    // Rs 600 of tablets returned in two halves still needs the PIN (limit Rs 500).
    let tabs = json!([{ "productId": p.id, "unitId": unit(&p, "Tab"), "qty": 200 }]);
    let big = sale(&mut conn, tabs, json!([{ "method": "cash", "amount": 60_000 }]), json!({})).unwrap();
    let line = returns::sale_for_return(&conn, &big.receipt_no).unwrap().lines[0].sale_line_id.clone();
    let half: returns::ReturnInput = input(json!({ "saleId": big.id, "tillCode": "T01", "refundMethod": "cash",
        "lines": [{ "saleLineId": line, "qty": 100, "condition": "restock" }] }));
    returns::create_return(&mut conn, &Auth::default(), &ali, &half).unwrap();
    let err = returns::create_return(&mut conn, &Auth::default(), &ali, &half).unwrap_err();
    assert_eq!(err.reason(), Some("approval.refund"));
}

#[test]
fn returning_item_by_item_refunds_the_whole_discounted_bill() {
    let mut conn = demo();
    let ali = user(&conn, "Ali");
    let p = panadol(&conn);
    let tabs = json!([{ "productId": p.id, "unitId": unit(&p, "Tab"), "qty": 5 }]);
    let r = sale(&mut conn, tabs, json!([{ "method": "cash", "amount": 1_500 }]), json!({ "discountBp": 17 })).unwrap();
    let line = returns::sale_for_return(&conn, &r.receipt_no).unwrap().lines[0].sale_line_id.clone();
    let mut refunded = 0;
    for _ in 0..5 {
        let one: returns::ReturnInput = input(json!({ "saleId": r.id, "tillCode": "T01", "refundMethod": "cash",
            "lines": [{ "saleLineId": line, "qty": 1, "condition": "restock" }] }));
        refunded += returns::create_return(&mut conn, &Auth::default(), &ali, &one).unwrap().refund_total;
    }
    assert_eq!(refunded, r.total);
}

#[test]
fn the_drawer_count_stays_blind_and_cash_out_is_capped() {
    let mut conn = demo();
    let owner = user(&conn, "Owner");
    let ali = user(&conn, "Ali");
    let p = panadol(&conn);
    sale(&mut conn, strips(&p, 1), json!([{ "method": "cash", "amount": 3_000 }]), json!({})).unwrap();
    let shift = shifts::open_shift_id(&conn, "T01").unwrap();
    let s = shifts::summary(&conn, &shift, false).unwrap();
    assert_eq!((s.expected_cash, s.cash_sales), (None, 0));
    let out = |amount| -> shifts::CashMoveInput {
        input(json!({ "tillCode": "T01", "kind": "out", "amount": amount, "reason": "tea" }))
    };
    let err = shifts::cash_move(&conn, &Auth::default(), &owner, &out(999_999)).unwrap_err();
    assert_eq!(err.reason(), Some("drawer.short"));
    shifts::cash_move(&conn, &Auth::default(), &owner, &out(3_000)).unwrap();
    let _ = ali;
}

#[test]
fn offline_sales_must_look_like_a_real_tills() {
    let mut conn = demo();
    let p = panadol(&conn);
    let now = chrono::Local::now();
    let list = products::catalogue(&conn, "T01").unwrap().price_list_id;
    let offline = |no: &str, at: chrono::DateTime<chrono::Local>, price: i64, total: i64| {
        json!({ "clientRef": format!("r-{no}-{total}-{}", at.timestamp()), "offline": {
            "receiptNo": no, "createdAt": at.to_rfc3339(), "priceListId": list,
            "lines": [{ "unitPrice": price, "lineTotal": total }] } })
    };
    let pay = json!([{ "method": "cash", "amount": 6_000 }]);
    let hour_ago = now - chrono::Duration::hours(1);
    assert!(sale(&mut conn, strips(&p, 2), pay.clone(), offline("B1-T01-X000001", hour_ago, 0, 0)).is_err(), "free");
    assert!(
        sale(&mut conn, strips(&p, 2), pay.clone(), offline("B1-T01-X1", hour_ago, 3_000, 6_000)).is_err(),
        "X1 vs X000001"
    );
    let next_month = now + chrono::Duration::days(30);
    let err =
        sale(&mut conn, strips(&p, 2), pay.clone(), offline("B1-T01-X000001", next_month, 3_000, 6_000)).unwrap_err();
    assert_eq!(err.reason(), Some("offline.time"));
    sale(&mut conn, strips(&p, 2), pay, offline("B1-T01-X000001", hour_ago, 3_000, 6_000)).unwrap();
}

#[test]
fn a_recount_takes_the_system_stock_again() {
    let mut conn = demo();
    let owner = user(&conn, "Owner");
    let c = counts::start(&mut conn, &owner, &StartInput { section: Some("A3".into()), no_rack: false, blind: true })
        .unwrap();
    let line = c.items.iter().find(|l| l.batch_no == "B-2291").unwrap().id.clone();
    let before: i64 =
        conn.query_row("SELECT qty_on_hand FROM batches WHERE batch_no = 'B-2291'", [], |r| r.get(0)).unwrap();
    counts::set_line(&conn, &c.head.id, &line, &LineInput { counted: Some(before - 1), reason: None }).unwrap();
    let p = panadol(&conn);
    sale(&mut conn, strips(&p, 1), json!([{ "method": "cash", "amount": 3_000 }]), json!({})).unwrap();
    // Counted again after the sale: the shelf now holds 11 fewer than at first.
    counts::set_line(&conn, &c.head.id, &line, &LineInput { counted: Some(before - 11), reason: None }).unwrap();
    counts::apply(&mut conn, &owner, &c.head.id).unwrap();
    let after: i64 =
        conn.query_row("SELECT qty_on_hand FROM batches WHERE batch_no = 'B-2291'", [], |r| r.get(0)).unwrap();
    assert_eq!(after, before - 11);
}

#[test]
fn a_bigger_pack_drops_the_old_batch_price() {
    let mut conn = demo();
    let owner = user(&conn, "Owner");
    let p = panadol(&conn);
    // Sell out the first batch so B-2310 (printed Rs 33 a strip of 10) is next.
    let first: String = conn.query_row("SELECT id FROM batches WHERE batch_no = 'B-2291'", [], |r| r.get(0)).unwrap();
    inventory::adjust(&mut conn, &owner, &input(json!({ "batchId": first, "newQty": 0, "reason": "damaged" })))
        .unwrap();
    assert_eq!(panadol(&conn).units.iter().find(|u| u.name == "Strip").unwrap().price, 3_300);

    let units: Vec<Value> = p
        .units
        .iter()
        .map(|u| {
            let (q, price) = if u.name == "Strip" { (12, 3_600) } else { (u.base_qty, u.price) };
            json!({ "id": u.id, "name": u.name, "baseQty": q, "price": price })
        })
        .collect();
    let update: inventory::ProductUpdate = input(json!({ "name": p.name, "kind": "medicine", "baseUnit": p.base_unit,
        "rack": p.rack, "units": units }));
    inventory::update(&mut conn, &owner, &p.id, &update).unwrap();
    assert_eq!(panadol(&conn).units.iter().find(|u| u.name == "Strip").unwrap().price, 3_600);
}

#[test]
fn a_cashier_guessing_a_managers_pin_cant_lock_the_manager_out() {
    let conn = demo();
    let auth = Auth::default();
    let ali = user(&conn, "Ali");
    let usman = user(&conn, "Usman");
    let sana = user(&conn, "Sana");
    let action = |pin: &str| -> rules::ActionInput {
        input(json!({ "action": "cancel_bill", "tillCode": "T01", "approval": { "userId": sana.id, "secret": pin } }))
    };
    for _ in 0..5 {
        let _ = rules::record_action(&conn, &auth, &ali, &action("0000"));
    }
    // Ali is stopped from trying Sana's PIN for a while…
    assert_eq!(rules::record_action(&conn, &auth, &ali, &action("2345")).unwrap_err().reason(), Some("auth.locked"));
    // …but Sana can still sign in, and approve for Usman.
    auth.check_secret(&conn, &sana.id, "2345").unwrap();
    rules::record_action(&conn, &auth, &usman, &action("2345")).unwrap();
}

#[test]
fn adding_a_product_is_checked_like_editing_one() {
    let mut conn = demo();
    let owner = user(&conn, "Owner");
    let new = |name: &str, base: &str, units: Value, reorder: i64, barcodes: Value| -> products::NewProduct {
        input(json!({ "name": name, "kind": "general", "baseUnit": base, "units": units,
                      "reorderLevel": reorder, "barcodes": barcodes }))
    };
    let pc = json!([{ "name": "Pc", "baseQty": 1, "price": 5_000 }]);
    let err =
        |p: products::NewProduct, conn: &mut Connection| products::create_product(conn, &p, &owner.id).unwrap_err();
    assert_eq!(
        err(new("panadol 500MG tablet", "Pc", pc.clone(), 0, json!([])), &mut conn).reason(),
        Some("product.name_taken")
    );
    assert_eq!(err(new("Comb", "", pc.clone(), 0, json!([])), &mut conn).code(), "bad_request");
    assert_eq!(err(new("Comb", "Pc", pc.clone(), -1, json!([])), &mut conn).code(), "bad_request");
    let twice = json!([{ "name": "Pc", "baseQty": 1, "price": 5_000 }, { "name": "pc", "baseQty": 2, "price": 9_000 }]);
    assert_eq!(err(new("Comb", "Pc", twice, 0, json!([])), &mut conn).code(), "bad_request");
    let wrong_pack = json!([{ "code": "1234567890123", "unitName": "Dozen" }]);
    assert_eq!(err(new("Comb", "Pc", pc.clone(), 0, wrong_pack), &mut conn).code(), "bad_request");
    products::create_product(&mut conn, &new("Comb", "Pc", pc, 0, json!([])), &owner.id).unwrap();

    // Renaming another product to the same name is refused too.
    let p = panadol(&conn);
    let units: Vec<Value> = p
        .units
        .iter()
        .map(|u| json!({ "id": u.id, "name": u.name, "baseQty": u.base_qty, "price": u.price }))
        .collect();
    let rename: inventory::ProductUpdate =
        input(json!({ "name": "COMB", "kind": "medicine", "baseUnit": p.base_unit, "units": units }));
    assert_eq!(inventory::update(&mut conn, &owner, &p.id, &rename).unwrap_err().reason(), Some("product.name_taken"));
}
