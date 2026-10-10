//! Returns, cash in/out, shift close and approval rules against the demo store.

use std::collections::BTreeMap;

use pos_server::{
    auth::{self, Approval, Auth, User},
    db,
    products::{self, ProductView},
    returns::{self, ReturnInput, ReturnLineInput},
    rules::{self, ActionInput},
    sales::{self, PaymentInput, Receipt, SaleInput, SaleLineInput},
    seed,
    shifts::{self, CashMoveInput, CloseInput, CountInput},
    store,
};
use rusqlite::Connection;
use serde_json::json;

fn store_db() -> Connection {
    let mut conn = db::open_in_memory().unwrap();
    seed::demo(&mut conn).unwrap();
    conn
}

fn user(conn: &Connection, name: &str) -> User {
    let id: String = conn.query_row("SELECT id FROM users WHERE name = ?1", [name], |r| r.get(0)).unwrap();
    auth::load_user(conn, &id).unwrap().unwrap()
}

fn product(conn: &Connection, name: &str) -> ProductView {
    products::search(conn, name, 5).unwrap().into_iter().next().unwrap()
}

fn line(p: &ProductView, unit: &str, qty: i64) -> SaleLineInput {
    let unit_id = p.units.iter().find(|u| u.name == unit).unwrap().id.clone();
    SaleLineInput { product_id: p.id.clone(), unit_id: Some(unit_id), qty: Some(qty), grams: None, amount: None }
}

fn sell(
    conn: &mut Connection,
    who: &User,
    lines: Vec<SaleLineInput>,
    method: &str,
    amount: i64,
    discount_bp: i64,
    customer: Option<String>,
) -> Receipt {
    let input = SaleInput {
        till_code: "T01".into(),
        customer_id: customer,
        lines,
        discount_bp,
        payments: vec![PaymentInput { method: method.into(), amount }],
        client_ref: None,
        approval: None,
    };
    sales::create_sale(conn, &Auth::default(), who, &input).unwrap()
}

fn batch_qty(conn: &Connection, batch_no: &str) -> i64 {
    conn.query_row("SELECT qty_on_hand FROM batches WHERE batch_no = ?1", [batch_no], |r| r.get(0)).unwrap()
}

fn return_input(sale_id: &str, lines: Vec<(String, i64, &str)>, method: &str) -> ReturnInput {
    ReturnInput {
        sale_id: sale_id.into(),
        till_code: "T01".into(),
        lines: lines
            .into_iter()
            .map(|(id, qty, condition)| ReturnLineInput {
                sale_line_id: id,
                qty,
                condition: condition.into(),
                reason: None,
            })
            .collect(),
        refund_method: method.into(),
        client_ref: None,
        approval: None,
    }
}

#[test]
fn return_refunds_after_discount_and_restocks_the_same_batch() {
    let mut conn = store_db();
    let owner = user(&conn, "Owner");
    let ali = user(&conn, "Ali");
    let panadol = product(&conn, "Panadol");
    let sale = sell(&mut conn, &owner, vec![line(&panadol, "Strip", 3)], "cash", 8_100, 1_000, None);
    assert_eq!(sale.total, 8_100);
    assert_eq!(batch_qty(&conn, "B-2291"), 970);

    let found = returns::sale_for_return(&conn, &sale.receipt_no.to_lowercase()).unwrap();
    assert_eq!(found.lines.len(), 1);
    let line_id = found.lines[0].sale_line_id.clone();

    let slip = returns::create_return(
        &mut conn,
        &Auth::default(),
        &ali,
        &return_input(&sale.id, vec![(line_id.clone(), 1, "restock")], "cash"),
    )
    .unwrap();
    assert_eq!(slip.refund_total, 2_700, "Rs 30 strip less the 10% bill discount");
    assert_eq!(slip.return_no, "B1-T01-R00001");
    assert_eq!(batch_qty(&conn, "B-2291"), 980);

    let err = returns::create_return(
        &mut conn,
        &Auth::default(),
        &ali,
        &return_input(&sale.id, vec![(line_id.clone(), 3, "restock")], "cash"),
    )
    .unwrap_err();
    assert_eq!(err.reason(), Some("return.too_many"));
    let found = returns::sale_for_return(&conn, &sale.receipt_no).unwrap();
    assert_eq!(found.lines[0].returned_qty, 1);
    assert_eq!(found.refunded_so_far, 2_700);
}

#[test]
fn fridge_items_are_not_returnable() {
    let mut conn = store_db();
    let ali = user(&conn, "Ali");
    let milk = product(&conn, "Olper");
    let sale = sell(&mut conn, &ali, vec![line(&milk, "Pack", 1)], "cash", 33_000, 0, None);
    let found = returns::sale_for_return(&conn, &sale.receipt_no).unwrap();
    assert_eq!(found.lines[0].blocked.as_deref(), Some("cold_chain"));
    let err = returns::create_return(
        &mut conn,
        &Auth::default(),
        &ali,
        &return_input(&sale.id, vec![(found.lines[0].sale_line_id.clone(), 1, "restock")], "cash"),
    )
    .unwrap_err();
    assert_eq!(err.reason(), Some("return.cold_chain"));
}

#[test]
fn big_refunds_need_a_pin_and_damaged_stock_stays_out() {
    let mut conn = store_db();
    let auth = Auth::default();
    let ali = user(&conn, "Ali");
    let sana = user(&conn, "Sana");
    let aug = product(&conn, "Augmentin");
    let panadol = product(&conn, "Panadol");
    let sale = sell(&mut conn, &ali, vec![line(&aug, "Strip", 1), line(&panadol, "Box", 1)], "cash", 101_000, 0, None);
    let found = returns::sale_for_return(&conn, &sale.receipt_no).unwrap();
    let ids: Vec<String> = found.lines.iter().map(|l| l.sale_line_id.clone()).collect();

    let mut input =
        return_input(&sale.id, vec![(ids[0].clone(), 1, "damaged"), (ids[1].clone(), 1, "restock")], "cash");
    let err = returns::create_return(&mut conn, &auth, &ali, &input).unwrap_err();
    assert_eq!(err.code(), "approval_required");
    assert_eq!(err.reason(), Some("approval.refund"));

    input.approval = Some(Approval { user_id: sana.id.clone(), secret: "2345".into() });
    let slip = returns::create_return(&mut conn, &auth, &ali, &input).unwrap();
    assert_eq!(slip.refund_total, 101_000);
    assert_eq!(slip.approved_by_name.as_deref(), Some("Sana"));
    assert_eq!(batch_qty(&conn, "AG-118"), 78, "damaged strip is not put back");
    assert_eq!(batch_qty(&conn, "B-2291"), 1_000, "box goes back to its batch");
}

#[test]
fn khata_refunds_reduce_the_balance() {
    let mut conn = store_db();
    let ali = user(&conn, "Ali");
    let brufen = product(&conn, "Brufen");
    let imran: String = conn.query_row("SELECT id FROM customers WHERE name = 'Mr. Imran'", [], |r| r.get(0)).unwrap();
    let sale = sell(&mut conn, &ali, vec![line(&brufen, "Bottle", 2)], "khata", 30_000, 0, Some(imran.clone()));
    let id = returns::sale_for_return(&conn, &sale.receipt_no).unwrap().lines[0].sale_line_id.clone();
    let slip = returns::create_return(
        &mut conn,
        &Auth::default(),
        &ali,
        &return_input(&sale.id, vec![(id, 1, "restock")], "khata"),
    )
    .unwrap();
    assert_eq!(slip.khata_balance, Some(15_000));
    assert_eq!(sales::khata_balance(&conn, &imran).unwrap(), 15_000);

    let cash_sale = sell(&mut conn, &ali, vec![line(&brufen, "Bottle", 1)], "cash", 15_000, 0, None);
    let id = returns::sale_for_return(&conn, &cash_sale.receipt_no).unwrap().lines[0].sale_line_id.clone();
    let err = returns::create_return(
        &mut conn,
        &Auth::default(),
        &ali,
        &return_input(&cash_sale.id, vec![(id, 1, "restock")], "khata"),
    )
    .unwrap_err();
    assert_eq!(err.reason(), Some("return.no_khata_customer"));
}

#[test]
fn shift_close_with_blind_count() {
    let mut conn = store_db();
    let auth = Auth::default();
    let ali = user(&conn, "Ali");
    let usman = user(&conn, "Usman");
    let panadol = product(&conn, "Panadol");
    let shift = store::open_shift(&conn, &ali, "T01", 500_000).unwrap();

    let sale = sell(&mut conn, &ali, vec![line(&panadol, "Strip", 3)], "cash", 10_000, 0, None);
    let id = returns::sale_for_return(&conn, &sale.receipt_no).unwrap().lines[0].sale_line_id.clone();
    returns::create_return(&mut conn, &auth, &ali, &return_input(&sale.id, vec![(id, 1, "restock")], "cash")).unwrap();
    shifts::cash_move(
        &conn,
        &auth,
        &ali,
        &CashMoveInput {
            till_code: "T01".into(),
            kind: "out".into(),
            amount: 1_000,
            reason: "Tea".into(),
            approval: None,
        },
    )
    .unwrap();

    // The cashier can't see the expected amount before counting.
    let before = shifts::summary(&conn, &shift.id, false).unwrap();
    assert_eq!(before.expected_cash, None);
    assert_eq!(before.cash_sales, 9_000);
    assert_eq!(before.cash_refunds, 3_000);
    assert_eq!(before.cash_out, 1_000);

    assert!(
        shifts::count(&conn, &usman, &shift.id, &CountInput { denominations: BTreeMap::new() }).is_err(),
        "only Ali or the owner"
    );

    // Expected: 5,000 + 90 - 30 - 10 = Rs 5,050. Counted Rs 4,900: Rs 150 short.
    let counted = shifts::count(
        &conn,
        &ali,
        &shift.id,
        &CountInput { denominations: BTreeMap::from([("1000".into(), 4), ("500".into(), 1), ("100".into(), 4)]) },
    )
    .unwrap();
    assert_eq!(counted.expected_cash, Some(505_000));
    assert_eq!(counted.difference, Some(-15_000));
    let err = shifts::close(&conn, &ali, &shift.id, &CloseInput { note: None }).unwrap_err();
    assert_eq!(err.reason(), Some("shift.note_required"));

    // One recount is allowed.
    let recount = shifts::count(
        &conn,
        &ali,
        &shift.id,
        &CountInput { denominations: BTreeMap::from([("1000".into(), 5), ("50".into(), 1)]) },
    )
    .unwrap();
    assert_eq!(recount.difference, Some(0));
    let err = shifts::count(&conn, &ali, &shift.id, &CountInput { denominations: BTreeMap::new() }).unwrap_err();
    assert_eq!(err.reason(), Some("shift.recount_limit"));

    let closed = shifts::close(&conn, &ali, &shift.id, &CloseInput { note: None }).unwrap();
    assert_eq!(closed.status, "closed");
    assert!(store::open_shift_for(&conn, "T01").unwrap().is_none());
    let err = shifts::cash_move(
        &conn,
        &auth,
        &ali,
        &CashMoveInput { till_code: "T01".into(), kind: "in".into(), amount: 100, reason: "x".into(), approval: None },
    )
    .unwrap_err();
    assert_eq!(err.code(), "conflict");
}

#[test]
fn till_actions_follow_the_rules_and_are_logged() {
    let conn = store_db();
    let auth = Auth::default();
    let ali = user(&conn, "Ali");
    let sana = user(&conn, "Sana");
    let owner = user(&conn, "Owner");
    let mut input = ActionInput {
        action: "remove_item".into(),
        till_code: "T01".into(),
        details: json!({ "product": "Risek" }),
        approval: None,
    };

    let err = rules::record_action(&conn, &auth, &ali, &input).unwrap_err();
    assert_eq!(err.reason(), Some("approval.action"));
    input.approval = Some(Approval { user_id: sana.id.clone(), secret: "2345".into() });
    rules::record_action(&conn, &auth, &ali, &input).unwrap();

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let approvals = rules::activity(&conn, &today, "approvals").unwrap();
    assert!(approvals.iter().any(|e| e.action == "till.remove_item" && e.approved_by_name.as_deref() == Some("Sana")));

    // The owner can switch the rule off.
    let mut r = rules::load(&conn).unwrap();
    r.remove_item = false;
    r.cashier_max_discount_bp = 1_000;
    rules::save(&conn, &r, &owner).unwrap();
    input.approval = None;
    rules::record_action(&conn, &auth, &ali, &input).unwrap();
    assert_eq!(rules::load(&conn).unwrap().cashier_max_discount_bp, 1_000);
}
