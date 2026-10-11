//! Reporting sales and refunds to FBR, against a stand-in for PRAL's service.

use std::sync::{Arc, Mutex};

use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use pos_server::{
    auth::{self, Auth, User},
    db,
    fbr::{self, FbrInput},
    products, returns, sales, seed,
};
use rusqlite::Connection;
use serde_json::{json, Value};

#[derive(Clone, Default)]
struct Pral {
    got: Arc<Mutex<Vec<Value>>>,
    /// What to answer: accept, or refuse with FBR's error code.
    refuse: Arc<Mutex<bool>>,
}

async fn post_data(State(p): State<Pral>, headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
    assert_eq!(headers.get("authorization").unwrap(), "Bearer pral-token");
    p.got.lock().unwrap().push(body.clone());
    if *p.refuse.lock().unwrap() {
        return Json(json!({ "InvoiceNumber": null, "Code": "102", "Response": "Invalid POSID", "Errors": null }));
    }
    let n = p.got.lock().unwrap().len();
    Json(
        json!({ "InvoiceNumber": format!("123456FBR{n:04}"), "Code": "100", "Response": "Invoice received successfully", "Errors": null }),
    )
}

async fn fake_pral() -> (String, Pral) {
    let pral = Pral::default();
    let app = Router::new().route("/PostData", post(post_data)).with_state(pral.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/PostData"), pral)
}

fn user(conn: &Connection, name: &str) -> User {
    let id: String = conn.query_row("SELECT id FROM users WHERE name = ?1", [name], |r| r.get(0)).unwrap();
    auth::load_user(conn, &id).unwrap().unwrap()
}

fn switch_on(conn: &mut Connection, url: &str) {
    let owner = user(conn, "Owner");
    fbr::save(
        conn,
        &owner,
        &FbrInput {
            enabled: true,
            mode: "sandbox".into(),
            pos_id: Some("123456".into()),
            token: Some("pral-token".into()),
            default_pct: None,
            url: Some(url.into()),
        },
    )
    .unwrap();
}

fn sell(conn: &mut Connection, discount_bp: i64) -> sales::Receipt {
    let ali = user(conn, "Ali");
    let p = products::search(conn, "Panadol", 1).unwrap().remove(0);
    let strip = p.units.iter().find(|u| u.name == "Strip").unwrap().id.clone();
    let soap = products::search(conn, "Lifebuoy", 1).unwrap().remove(0);
    let line = |p: &products::ProductView, unit: Option<String>, qty| sales::SaleLineInput {
        product_id: p.id.clone(),
        unit_id: unit,
        qty: Some(qty),
        grams: None,
        amount: None,
    };
    let soap_unit = soap.units[0].id.clone();
    sales::create_sale(
        conn,
        &Auth::default(),
        &ali,
        &sales::SaleInput {
            till_code: "T01".into(),
            customer_id: None,
            lines: vec![line(&p, Some(strip), 2), line(&soap, Some(soap_unit), 1)],
            discount_bp,
            payments: vec![sales::PaymentInput { method: "cash".into(), amount: 100_000 }],
            client_ref: None,
            approval: None,
            offline: None,
        },
    )
    .unwrap()
}

#[tokio::test]
async fn sales_and_refunds_go_to_fbr_with_their_invoice_numbers() {
    let (url, pral) = fake_pral().await;
    let mut conn = db::open_in_memory().unwrap();
    seed::demo(&mut conn).unwrap();

    // Off by default: nothing is queued.
    sell(&mut conn, 0);
    assert_eq!(fbr::status(&conn).unwrap().waiting, 0);

    switch_on(&mut conn, &url);
    let r = sell(&mut conn, 500);
    assert_eq!(fbr::status(&conn).unwrap().waiting, 1);
    let db = Mutex::new(conn);
    let client = reqwest::Client::new();
    let no = fbr::send_now(&db, &client, &r.id).await.unwrap();
    assert_eq!(no.as_deref(), Some("123456FBR0001"));
    let conn = db.into_inner().unwrap();
    assert_eq!(sales::receipt(&conn, &r.id).unwrap().unwrap().fbr_invoice_no.as_deref(), Some("123456FBR0001"));
    assert_eq!(fbr::status(&conn).unwrap().waiting, 0);

    let body = pral.got.lock().unwrap()[0].clone();
    assert_eq!(body["POSID"], 123456);
    assert_eq!(body["USIN"], r.receipt_no.as_str());
    assert_eq!(body["InvoiceType"], 1);
    assert_eq!(body["PaymentMode"], 1);
    assert_eq!(body["TotalBillAmount"].as_f64().unwrap(), r.total as f64 / 100.0);
    assert_eq!(body["Discount"].as_f64().unwrap(), r.discount as f64 / 100.0);
    let items = body["Items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let item_total: f64 = items.iter().map(|i| i["TotalAmount"].as_f64().unwrap()).sum();
    assert!((item_total - r.total as f64 / 100.0).abs() < 0.001, "the items add up to the bill");
    assert_eq!(items[0]["PCTCode"], fbr::DEFAULT_PCT);
    assert_eq!(items[0]["ItemCode"], "8964000123463", "the barcode identifies the item");

    // A refund goes as a credit note pointing at the sale.
    let mut conn = conn;
    let ali = user(&conn, "Ali");
    let found = returns::sale_for_return(&conn, &r.receipt_no).unwrap();
    let slip = returns::create_return(
        &mut conn,
        &Auth::default(),
        &ali,
        &returns::ReturnInput {
            sale_id: r.id.clone(),
            till_code: "T01".into(),
            lines: vec![returns::ReturnLineInput {
                sale_line_id: found.lines[0].sale_line_id.clone(),
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
    let db = Mutex::new(conn);
    assert_eq!(fbr::send_due(&db, &client).await.unwrap(), 1);
    let conn = db.into_inner().unwrap();
    assert!(returns::return_slip(&conn, &slip.id).unwrap().unwrap().fbr_invoice_no.is_some());
    let credit = pral.got.lock().unwrap()[1].clone();
    assert_eq!(credit["InvoiceType"], 3);
    assert_eq!(credit["RefUSIN"], r.receipt_no.as_str());
}

#[tokio::test]
async fn refused_invoices_wait_and_are_retried() {
    let (url, pral) = fake_pral().await;
    let mut conn = db::open_in_memory().unwrap();
    seed::demo(&mut conn).unwrap();
    switch_on(&mut conn, &url);
    *pral.refuse.lock().unwrap() = true;
    let r = sell(&mut conn, 0);
    let db = Mutex::new(conn);
    let client = reqwest::Client::new();
    assert_eq!(fbr::send_now(&db, &client, &r.id).await.unwrap(), None);
    {
        let conn = db.lock().unwrap();
        let s = fbr::status(&conn).unwrap();
        assert_eq!((s.waiting, s.failing), (1, 1));
        assert!(s.last_error.unwrap().contains("Invalid POSID"));
        assert!(sales::receipt(&conn, &r.id).unwrap().unwrap().fbr_invoice_no.is_none());
    }
    // Not due again yet.
    assert_eq!(fbr::send_due(&db, &client).await.unwrap(), 0);
    assert_eq!(pral.got.lock().unwrap().len(), 1);

    // FBR fixed; the owner presses "send now".
    *pral.refuse.lock().unwrap() = false;
    db.lock().unwrap().execute("UPDATE fbr_queue SET next_try_at = NULL", []).unwrap();
    assert_eq!(fbr::send_due(&db, &client).await.unwrap(), 1);
    assert_eq!(fbr::status(&db.lock().unwrap()).unwrap().waiting, 0);
}

#[test]
fn switching_on_needs_the_pos_id_and_token() {
    let mut conn = db::open_in_memory().unwrap();
    seed::demo(&mut conn).unwrap();
    let owner = user(&conn, "Owner");
    let input = FbrInput {
        enabled: true,
        mode: "live".into(),
        pos_id: Some("123456".into()),
        token: None,
        default_pct: None,
        url: None,
    };
    assert_eq!(fbr::save(&mut conn, &owner, &input).unwrap_err().reason(), Some("fbr.incomplete"));
    let ali = user(&conn, "Ali");
    assert!(fbr::save(&mut conn, &ali, &input).is_err());
    let s = fbr::status(&conn).unwrap();
    assert!(!s.settings.enabled && !s.settings.has_token);
}
