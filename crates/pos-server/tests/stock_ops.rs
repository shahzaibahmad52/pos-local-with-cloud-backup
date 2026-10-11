//! Stock counts and transfers between branches.

mod common;

use std::sync::{Arc, Mutex};

use common::{fake_cloud, TOKEN};
use pos_server::{
    auth::{self, Auth, User},
    cloud::{
        hq::{self, AddStore},
        sync::{self, Cloud},
    },
    counts::{self, AddProduct, LineInput, StartInput},
    db, products, sales, seed,
    store::{self, SetupInput},
    transfers::{self, ReceiveInput, Received, SendInput, SendLine},
};
use rusqlite::Connection;

fn user(conn: &Connection, name: &str) -> User {
    let id: String = conn.query_row("SELECT id FROM users WHERE name = ?1", [name], |r| r.get(0)).unwrap();
    auth::load_user(conn, &id).unwrap().unwrap()
}

fn demo() -> Connection {
    let mut conn = db::open_in_memory().unwrap();
    seed::demo(&mut conn).unwrap();
    common::ensure_shift(&conn);
    conn
}

fn branch2() -> Connection {
    let mut conn = db::open_in_memory().unwrap();
    store::setup(
        &mut conn,
        &SetupInput {
            store_code: "B2".into(),
            store_name: "Branch 2 — Model Town".into(),
            address: None,
            phone: None,
            owner_name: "Owner".into(),
            owner_password: "owner1234".into(),
        },
    )
    .unwrap();
    conn
}

fn batch_id(conn: &Connection, batch_no: &str) -> String {
    conn.query_row("SELECT id FROM batches WHERE batch_no = ?1", [batch_no], |r| r.get(0)).unwrap()
}

fn qty(conn: &Connection, batch_no: &str) -> i64 {
    conn.query_row("SELECT qty_on_hand FROM batches WHERE batch_no = ?1", [batch_no], |r| r.get(0)).unwrap()
}

fn sell_strip(conn: &mut Connection) {
    let owner = user(conn, "Owner");
    let p = products::search(conn, "Panadol", 1).unwrap().remove(0);
    let strip = p.units.iter().find(|u| u.name == "Strip").unwrap().id.clone();
    sales::create_sale(
        conn,
        &Auth::default(),
        &owner,
        &sales::SaleInput {
            till_code: "T01".into(),
            customer_id: None,
            lines: vec![sales::SaleLineInput {
                product_id: p.id,
                unit_id: Some(strip),
                qty: Some(1),
                grams: None,
                amount: None,
            }],
            discount_bp: 0,
            payments: vec![sales::PaymentInput { method: "cash".into(), amount: 3_000 }],
            client_ref: None,
            approval: None,
            offline: None,
        },
    )
    .unwrap();
}

#[test]
fn a_count_keeps_sales_made_while_counting() {
    let mut conn = demo();
    let owner = user(&conn, "Owner");
    let c = counts::start(&mut conn, &owner, &StartInput { section: Some("a3".into()), no_rack: false, blind: true })
        .unwrap();
    assert_eq!(c.items.len(), 2, "both Panadol batches on rack A3");
    assert!(c.items.iter().all(|l| l.system_qty.is_none()), "a blind count hides the system quantity");

    let line = c.items.iter().find(|l| l.batch_no == "B-2291").unwrap().id.clone();
    let c =
        counts::set_line(&conn, &c.head.id, &line, &LineInput { counted: Some(990), reason: Some("damaged".into()) })
            .unwrap();
    let counted = c.items.iter().find(|l| l.batch_no == "B-2291").unwrap();
    assert_eq!((counted.system_qty, counted.difference), (Some(1_000), Some(-10)));
    assert_eq!(c.short_value, 10 * 250);

    // A strip is sold from that batch after it was counted.
    sell_strip(&mut conn);
    assert_eq!(qty(&conn, "B-2291"), 990);

    // Brufen wasn't on the shelf list; the counter adds it.
    let brufen = products::search(&conn, "Brufen", 1).unwrap().remove(0);
    let c = counts::add_product(&conn, &c.head.id, &AddProduct { product_id: brufen.id }).unwrap();
    assert_eq!(c.items.len(), 3);

    let done = counts::apply(&mut conn, &owner, &c.head.id).unwrap();
    assert_eq!(done.head.status, "applied");
    // 990 counted then 10 sold: 980 left. The uncounted batch is untouched.
    assert_eq!(qty(&conn, "B-2291"), 980);
    assert_eq!(qty(&conn, "B-2310"), 240);
    assert_eq!(qty(&conn, "BF-031"), 22);
    let err = counts::apply(&mut conn, &owner, &c.head.id).unwrap_err();
    assert_eq!(err.reason(), Some("count.closed"));
}

#[test]
fn a_whole_store_count_lists_every_batch_with_stock() {
    let mut conn = demo();
    let owner = user(&conn, "Owner");
    let c = counts::start(&mut conn, &owner, &StartInput { section: None, no_rack: false, blind: false }).unwrap();
    let with_stock: i64 =
        conn.query_row("SELECT COUNT(*) FROM batches WHERE qty_on_hand > 0", [], |r| r.get(0)).unwrap();
    assert_eq!(c.items.len() as i64, with_stock);
    assert!(c.items.iter().all(|l| l.system_qty.is_some()), "an open count shows the system quantity");
    let err =
        counts::start(&mut conn, &owner, &StartInput { section: Some("nowhere".into()), no_rack: false, blind: true })
            .unwrap_err();
    assert_eq!(err.reason(), Some("count.empty"));
}

fn send_from_b1(b1: &mut Connection) -> transfers::TransferView {
    let owner = user(b1, "Owner");
    let input = SendInput {
        to_code: "b2".into(),
        to_name: Some("Model Town".into()),
        note: Some("Rider: Kashif".into()),
        lines: vec![
            SendLine { batch_id: batch_id(b1, "B-2310"), qty: 100 },
            SendLine { batch_id: batch_id(b1, "AG-118"), qty: 12 },
        ],
    };
    transfers::send(b1, &owner, &input).unwrap()
}

#[test]
fn stock_moves_from_one_branch_to_another() {
    let mut b1 = demo();
    let mut b2 = branch2();
    let sent = send_from_b1(&mut b1);
    assert_eq!(sent.transfer_no, "B1-TR-0001");
    assert_eq!(qty(&b1, "B-2310"), 140);
    assert_eq!(qty(&b1, "AG-118"), 72);
    let note = sent.payload.clone().unwrap();
    assert_eq!(note.to_code, "B2");

    // Too much can't be sent.
    let owner = user(&b1, "Owner");
    let aug = batch_id(&b1, "AG-118");
    let too_much = SendInput {
        to_code: "B2".into(),
        to_name: None,
        note: None,
        lines: vec![SendLine { batch_id: aug, qty: 999 }],
    };
    let err = transfers::send(&mut b1, &owner, &too_much).unwrap_err();
    assert_eq!(err.reason(), Some("transfer.qty"));

    // It arrives at Branch 2, which had never stocked these; one strip of Augmentin was broken.
    let b2_owner = user(&b2, "Owner");
    let input = ReceiveInput {
        note: note.clone(),
        received: vec![Received { qty: 100, reason: None }, Received { qty: 6, reason: Some("damaged".into()) }],
    };
    let got = transfers::receive(&mut b2, &b2_owner, &input).unwrap();
    assert_eq!(got.direction, "in");
    assert_eq!(got.lines[1].received_qty, Some(6));
    let panadol = products::search(&b2, "Panadol", 1).unwrap().remove(0);
    assert_eq!(panadol.stock_base, 100);
    // The batch keeps its printed strip price (Rs 33) and the barcodes came along.
    assert_eq!(panadol.units.iter().find(|u| u.name == "Strip").unwrap().price, 3_300);
    assert!(products::by_barcode(&b2, "8964000123463").unwrap().is_some());
    assert_eq!(qty(&b2, "AG-118"), 6);

    // Receiving the same note again changes nothing.
    let again = transfers::receive(&mut b2, &b2_owner, &input).unwrap();
    assert_eq!(again.id, got.id);
    assert_eq!(products::search(&b2, "Panadol", 1).unwrap()[0].stock_base, 100);

    // A note for another store is refused.
    let err = transfers::receive(&mut b1, &owner, &ReceiveInput { note, received: vec![] }).unwrap_err();
    assert_eq!(err.reason(), Some("transfer.wrong_store"));
}

#[tokio::test]
async fn a_branch_finds_transfers_in_the_senders_cloud_copy() {
    let cloud = Cloud::default();
    let (url1, _f1) = fake_cloud().await;
    let b1 = Arc::new(Mutex::new(demo()));
    sync::enable(&b1, &cloud, &url1, TOKEN).await.unwrap();
    let sent = send_from_b1(&mut b1.lock().unwrap());
    sync::sync_once(&b1, &cloud).await.unwrap();

    let b2 = Arc::new(Mutex::new(branch2()));
    hq::add(&b2, &cloud, &AddStore { url: url1, token: TOKEN.into(), name: None }).await.unwrap();
    let incoming = hq::incoming_transfers(&b2, &cloud).await.unwrap();
    assert!(incoming.errors.is_empty());
    assert_eq!(incoming.notes.len(), 1);
    assert_eq!(incoming.notes[0].transfer_id, sent.id);

    let note = incoming.notes[0].clone();
    let received = note.lines.iter().map(|l| Received { qty: l.qty, reason: None }).collect();
    {
        let mut conn = b2.lock().unwrap();
        let owner = user(&conn, "Owner");
        transfers::receive(&mut conn, &owner, &ReceiveInput { note, received }).unwrap();
    }
    assert!(
        hq::incoming_transfers(&b2, &cloud).await.unwrap().notes.is_empty(),
        "received transfers drop off the list"
    );
}
