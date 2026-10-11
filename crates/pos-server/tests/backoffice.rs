//! Backoffice: purchases and supplier payables, expenses, khata, product
//! editing, Excel import, the dashboard and the recovery code.

use std::collections::BTreeMap;

use pos_server::{
    auth::{self, Auth, User},
    dashboard, db,
    expenses::{self, ExpenseInput},
    import::{self, ImportRow},
    inventory::{self, AdjustInput, InventoryQuery, ProductUpdate, UnitEdit},
    khata::{self, PaymentInput},
    products::{self, NewBarcode, NewUnit, ProductView},
    purchases::{self, PurchaseInput, PurchaseLineInput, SupplierPaymentInput},
    returns::{self, ReturnInput, ReturnLineInput},
    sales::{self, SaleInput, SaleLineInput},
    seed,
    settings::{self, RecoverInput, UserUpdate},
    shifts::{self, CloseInput, CountInput},
    store,
};
use rusqlite::Connection;

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

fn unit_id(p: &ProductView, name: &str) -> String {
    p.units.iter().find(|u| u.name == name).unwrap().id.clone()
}

fn supplier_id(conn: &Connection, name: &str) -> String {
    conn.query_row("SELECT id FROM suppliers WHERE name = ?1", [name], |r| r.get(0)).unwrap()
}

fn batch(conn: &Connection, batch_no: &str) -> (i64, i64) {
    conn.query_row("SELECT qty_on_hand, cost_per_base FROM batches WHERE batch_no = ?1", [batch_no], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .unwrap()
}

fn panadol_purchase(conn: &Connection, client_ref: Option<&str>) -> PurchaseInput {
    let panadol = product(conn, "Panadol");
    PurchaseInput {
        supplier_id: supplier_id(conn, "Distributor A"),
        invoice_no: Some("INV-1".into()),
        invoice_date: "2026-10-01".into(),
        terms_days: 30,
        freight: 30_000,
        paid_now: 0,
        paid_from: "owner".into(),
        till_code: None,
        lines: vec![PurchaseLineInput {
            product_id: panadol.id.clone(),
            unit_id: Some(unit_id(&panadol, "Box")),
            batch_no: "B-2400".into(),
            expiry: Some("2029-01".into()),
            qty: 5,
            bonus: 1,
            trade_price: 50_000,
            disc_bp: 200,
            sale_price: Some(60_000),
        }],
        client_ref: client_ref.map(Into::into),
    }
}

fn open_shift(conn: &Connection, who: &User, cash: i64) -> String {
    store::open_shift(conn, who, "T01", cash).unwrap().id
}

#[test]
fn purchase_cost_includes_bonus_and_freight() {
    let mut conn = store_db();
    let owner = user(&conn, "Owner");
    let before = purchases::suppliers(&conn).unwrap().into_iter().find(|s| s.name == "Distributor A").unwrap();
    let grn = panadol_purchase(&conn, Some("grn-1"));
    let p = purchases::receive(&mut conn, &owner, &grn).unwrap();
    // 5 boxes × Rs 500 − 2% = Rs 2,450, plus Rs 300 freight.
    assert_eq!((p.gross, p.discount, p.freight, p.net), (250_000, 5_000, 30_000, 275_000));
    assert_eq!(p.due_date.as_deref(), Some("2026-10-31"));
    // 6 boxes arrive (one free) = 1,200 tablets.
    assert_eq!(batch(&conn, "B-2400"), (1_200, 229));

    // The printed box price is spread over every pack size of that batch.
    let batch_id: String =
        conn.query_row("SELECT id FROM batches WHERE batch_no = 'B-2400'", [], |r| r.get(0)).unwrap();
    let mut prices: Vec<i64> = conn
        .prepare("SELECT price FROM batch_unit_prices WHERE batch_id = ?1 ORDER BY price")
        .unwrap()
        .query_map([&batch_id], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    prices.sort();
    assert_eq!(prices, vec![300, 3_000, 60_000]);

    let after = purchases::suppliers(&conn).unwrap().into_iter().find(|s| s.name == "Distributor A").unwrap();
    assert_eq!(after.balance - before.balance, 275_000);

    // Sending the same purchase again (a retry) doesn't add stock twice.
    let again = purchases::receive(&mut conn, &owner, &grn).unwrap();
    assert_eq!(again.purchase_no, p.purchase_no);
    assert_eq!(batch(&conn, "B-2400").0, 1_200);

    // Receiving more of the same batch averages its cost.
    let mut more = panadol_purchase(&conn, None);
    more.freight = 0;
    more.lines[0].bonus = 0;
    more.lines[0].disc_bp = 0;
    more.lines[0].qty = 1;
    more.lines[0].trade_price = 40_000; // Rs 2 a tablet
    purchases::receive(&mut conn, &owner, &more).unwrap();
    assert_eq!(batch(&conn, "B-2400"), (1_400, (1_200 * 229 + 200 * 200 + 700) / 1_400));
}

#[test]
fn purchase_lines_are_checked() {
    let mut conn = store_db();
    let owner = user(&conn, "Owner");
    let mut input = panadol_purchase(&conn, None);
    input.lines[0].expiry = Some("2020-01".into());
    let err = purchases::receive(&mut conn, &owner, &input).unwrap_err();
    assert_eq!(err.reason(), Some("purchase.expired"));

    input.lines[0].expiry = None;
    let err = purchases::receive(&mut conn, &owner, &input).unwrap_err();
    assert_eq!(err.reason(), Some("purchase.expiry_required"));

    input.lines[0].expiry = Some("2029-01".into());
    input.paid_now = 999_999;
    let err = purchases::receive(&mut conn, &owner, &input).unwrap_err();
    assert_eq!(err.reason(), Some("purchase.overpaid"));
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM batches WHERE batch_no = 'B-2400'", [], |r| r.get::<_, i64>(0)).unwrap(),
        0
    );
}

#[test]
fn paying_a_supplier_from_the_drawer_lowers_expected_cash() {
    let mut conn = store_db();
    let owner = user(&conn, "Owner");
    let ali = user(&conn, "Ali");
    let grn = panadol_purchase(&conn, None);
    purchases::receive(&mut conn, &owner, &grn).unwrap();
    let dist = supplier_id(&conn, "Distributor A");
    let pay =
        |amount| SupplierPaymentInput { amount, paid_from: "drawer".into(), till_code: Some("T01".into()), note: None };

    let err = purchases::pay_supplier(&mut conn, &owner, &dist, &pay(100_000)).unwrap_err();
    assert_eq!(err.reason(), Some("drawer.no_shift"));

    let shift = open_shift(&conn, &ali, 500_000);
    let s = purchases::pay_supplier(&mut conn, &owner, &dist, &pay(100_000)).unwrap();
    assert_eq!(shifts::expected_cash(&conn, &shift).unwrap(), 400_000);

    let err = purchases::pay_supplier(&mut conn, &owner, &dist, &pay(s.balance + 1)).unwrap_err();
    assert_eq!(err.reason(), Some("supplier.overpay"));

    let ledger = purchases::supplier_ledger(&conn, &dist).unwrap();
    assert_eq!(ledger.last().unwrap().balance, s.balance);
    assert_eq!(ledger.last().unwrap().amount, -100_000);
}

#[test]
fn drawer_expenses_count_in_the_shift_and_lock_when_it_closes() {
    let mut conn = store_db();
    let owner = user(&conn, "Owner");
    let ali = user(&conn, "Ali");
    let shift = open_shift(&conn, &ali, 500_000);
    let fuel = |amount| ExpenseInput {
        category: "Generator fuel".into(),
        amount,
        date: pos_server::ledger::today(),
        paid_from: "drawer".into(),
        till_code: Some("T01".into()),
        note: Some("Petrol".into()),
    };
    let wrong = expenses::add(&mut conn, &owner, &fuel(99_000)).unwrap();
    assert_eq!(shifts::expected_cash(&conn, &shift).unwrap(), 401_000);
    expenses::delete(&mut conn, &owner, &wrong.id).unwrap();
    assert_eq!(shifts::expected_cash(&conn, &shift).unwrap(), 500_000);

    let e = expenses::add(&mut conn, &owner, &fuel(34_000)).unwrap();
    let month = expenses::month(&conn, &db::current_month()).unwrap();
    assert_eq!(month.total, 340_000 + 34_000, "the demo store already has Rs 3,400 of fuel");
    assert_eq!(month.by_category, vec![("Generator fuel".to_string(), 374_000)]);

    // Rs 4,660 in the drawer: exactly what is expected.
    let counted: BTreeMap<String, i64> =
        [("1000", 4), ("500", 1), ("100", 1), ("50", 1), ("10", 1)].into_iter().map(|(k, v)| (k.into(), v)).collect();
    let summary = shifts::count(&conn, &ali, &shift, &CountInput { denominations: counted }).unwrap();
    assert_eq!(summary.difference, Some(0));
    shifts::close(&conn, &ali, &shift, &CloseInput { note: None }).unwrap();

    let err = expenses::delete(&mut conn, &owner, &e.id).unwrap_err();
    assert_eq!(err.reason(), Some("expense.locked"));
}

#[test]
fn khata_payments_ageing_and_statement() {
    let mut conn = store_db();
    let owner = user(&conn, "Owner");
    let ali = user(&conn, "Ali");
    let rasheed: String =
        conn.query_row("SELECT id FROM customers WHERE name = 'Haji Rasheed'", [], |r| r.get(0)).unwrap();

    let list = khata::list(&conn, "", "balance").unwrap();
    assert_eq!(list.summary.total_due, 420_000);
    assert_eq!(list.accounts.len(), 1);
    assert!(!list.accounts[0].overdue);

    // The old balance was from 40 days ago.
    let old = pos_server::ledger::add_days(&pos_server::ledger::today(), -40).unwrap();
    conn.execute("UPDATE khata_entries SET created_at = ?1 || 'T10:00:00+05:00'", [&old]).unwrap();
    let list = khata::list(&conn, "", "overdue").unwrap();
    assert_eq!(list.accounts.len(), 1);
    assert_eq!(list.accounts[0].age_days, 40);
    assert_eq!(list.summary.overdue_total, 420_000);

    let pay = |amount| PaymentInput { amount, method: "cash".into(), till_code: Some("T01".into()), note: None };
    let err = khata::receive_payment(&mut conn, &owner, &rasheed, &pay(500_000)).unwrap_err();
    assert_eq!(err.reason(), Some("khata.overpay"));

    let shift = open_shift(&conn, &ali, 0);
    let slip = khata::receive_payment(&mut conn, &owner, &rasheed, &pay(200_000)).unwrap();
    assert_eq!((slip.balance_before, slip.balance_after), (420_000, 220_000));
    assert_eq!(shifts::expected_cash(&conn, &shift).unwrap(), 200_000, "cash went into the T01 drawer");

    let st = khata::statement(&conn, &rasheed).unwrap();
    assert_eq!(st.lines.len(), 2);
    assert_eq!(st.lines[0].kind, "opening");
    assert_eq!(st.lines[1].balance, 220_000);
    assert_eq!(st.account.last_paid, Some(pos_server::ledger::today()));
    assert_eq!(khata::list(&conn, "", "all").unwrap().summary.collected_this_month, 200_000);
}

#[test]
fn editing_a_product_keeps_old_pack_levels_for_history() {
    let mut conn = store_db();
    let owner = user(&conn, "Owner");
    let panadol = product(&conn, "Panadol");
    let detail = inventory::detail(&conn, &panadol.id).unwrap();
    let units = detail
        .all_units
        .iter()
        .filter(|u| u.name != "Tab")
        .map(|u| UnitEdit {
            id: Some(u.id.clone()),
            unit: NewUnit {
                name: u.name.clone(),
                name_ur: u.name_ur.clone(),
                base_qty: u.base_qty,
                price: if u.name == "Strip" { 3_500 } else { u.price },
            },
            sellable: true,
        })
        .collect();
    let mut update = ProductUpdate {
        name: "Panadol 500mg Tablet".into(),
        generic_name: Some("Paracetamol".into()),
        company: Some("GSK".into()),
        category: Some("Medicines".into()),
        kind: "medicine".into(),
        base_unit: "tab".into(),
        price_per_kg: None,
        tax_rate_bp: 0,
        rx_required: false,
        controlled: false,
        cold_chain: false,
        rack: Some("A4".into()),
        reorder_level: 300,
        units,
        barcodes: vec![NewBarcode { code: "8964000123463".into(), unit_name: Some("Strip".into()) }],
        active: true,
    };
    let saved = inventory::update(&mut conn, &owner, &panadol.id, &update).unwrap();
    assert_eq!(saved.row.rack.as_deref(), Some("A4"));
    let view = product(&conn, "Panadol");
    assert!(view.units.iter().all(|u| u.name != "Tab"), "loose tablets are no longer sold");
    assert_eq!(view.units.iter().find(|u| u.name == "Strip").unwrap().price, 3_500);
    assert!(products::by_barcode(&conn, "8964000123456").unwrap().is_none(), "the box barcode was removed");

    // A barcode can belong to only one product.
    let risek = product(&conn, "Risek");
    let risek_units = risek
        .units
        .iter()
        .map(|u| UnitEdit {
            id: Some(u.id.clone()),
            unit: NewUnit { name: u.name.clone(), name_ur: None, base_qty: u.base_qty, price: u.price },
            sellable: true,
        })
        .collect();
    update.barcodes = vec![NewBarcode { code: "8964000123463".into(), unit_name: None }];
    let risek_update =
        ProductUpdate { name: risek.name.clone(), base_unit: "cap".into(), units: risek_units, ..update };
    let err = inventory::update(&mut conn, &owner, &risek.id, &risek_update).unwrap_err();
    assert_eq!(err.reason(), Some("barcode.taken"), "{err}");
    assert_eq!(product(&conn, "Risek").name, "Risek 20mg Capsule", "nothing was saved");
}

#[test]
fn stock_adjustments_and_inventory_filters() {
    let mut conn = store_db();
    let owner = user(&conn, "Owner");
    let batch_id: String =
        conn.query_row("SELECT id FROM batches WHERE batch_no = 'B-2291'", [], |r| r.get(0)).unwrap();
    let d = inventory::adjust(
        &mut conn,
        &owner,
        &AdjustInput { batch_id, new_qty: 990, reason: "damaged".into(), note: Some("Wet box".into()) },
    )
    .unwrap();
    assert_eq!(d.row.stock_base, 990 + 240);
    assert_eq!(d.moves[0].qty_delta, -10);

    let q = |tab: &str, q: &str| InventoryQuery {
        q: q.into(),
        tab: tab.into(),
        category: String::new(),
        offset: 0,
        limit: 50,
    };
    let low = inventory::list(&conn, &q("low", "")).unwrap();
    assert!(low.rows.iter().any(|r| r.name.starts_with("Brufen")), "22 bottles is below the reorder level of 24");
    let expired = inventory::list(&conn, &q("expired", "")).unwrap();
    assert_eq!(expired.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["Flagyl 400mg Tablet"]);
    let scanned = inventory::list(&conn, &q("all", "8964000123463")).unwrap();
    assert_eq!(scanned.rows.len(), 1);
    assert!(scanned.summary.stock_value > 0);
}

fn calpol(row: i64) -> ImportRow {
    ImportRow {
        row,
        name: "Calpol Syrup 120ml".into(),
        kind: "medicine".into(),
        base_unit: "Btl".into(),
        base_price: Some(18_000),
        barcode: Some("8964999000011".into()),
        opening_qty: Some(12),
        batch_no: Some("CP-1".into()),
        expiry: Some("2028-03".into()),
        cost_per_base: Some(15_000),
        ..Default::default()
    }
}

#[test]
fn excel_import_previews_then_saves() {
    let mut conn = store_db();
    let owner = user(&conn, "Owner");
    let rows = vec![
        calpol(2),
        ImportRow {
            row: 3,
            name: "panadol 500mg tablet".into(),
            base_unit: "Tab".into(),
            pack2_name: Some("Strip".into()),
            pack2_qty: Some(10),
            pack2_price: Some(3_200),
            pack3_name: Some("Box".into()),
            pack3_qty: Some(200),
            opening_qty: Some(10),
            ..Default::default()
        },
        ImportRow { row: 4, name: String::new(), base_unit: "Tab".into(), ..Default::default() },
        ImportRow { row: 5, ..calpol(5) },
        ImportRow { row: 6, cost_per_base: Some(20_000), name: "Dear Syrup".into(), barcode: None, ..calpol(6) },
    ];
    let preview = import::run(&mut conn, &owner, &rows, false).unwrap();
    assert_eq!((preview.new_count, preview.update_count, preview.error_count), (1, 1, 3));
    assert!(preview.rows[3].message.as_deref().unwrap().contains("row 2"));
    assert!(preview.rows[4].message.as_deref().unwrap().contains("lower than cost"));
    assert!(products::search(&conn, "Calpol", 5).unwrap().is_empty(), "a preview saves nothing");

    let done = import::run(&mut conn, &owner, &rows, true).unwrap();
    assert!(done.committed);
    let calpol = product(&conn, "Calpol");
    assert_eq!(calpol.stock_base, 12);
    let panadol = product(&conn, "Panadol");
    assert_eq!(panadol.units.iter().find(|u| u.name == "Strip").unwrap().price, 3_200);
    assert_eq!(panadol.stock_base, 1_240, "stock of existing products is not changed by an import");
}

#[test]
fn dashboard_profit_after_sale_return_and_expense() {
    let mut conn = store_db();
    let owner = user(&conn, "Owner");
    let panadol = product(&conn, "Panadol");
    let sale = sales::create_sale(
        &mut conn,
        &Auth::default(),
        &owner,
        &SaleInput {
            till_code: "T01".into(),
            customer_id: None,
            lines: vec![SaleLineInput {
                product_id: panadol.id.clone(),
                unit_id: Some(unit_id(&panadol, "Strip")),
                qty: Some(3),
                grams: None,
                amount: None,
            }],
            discount_bp: 0,
            payments: vec![sales::PaymentInput { method: "cash".into(), amount: 9_000 }],
            client_ref: None,
            approval: None,
            offline: None,
        },
    )
    .unwrap();
    let line = returns::sale_for_return(&conn, &sale.receipt_no).unwrap().lines[0].sale_line_id.clone();
    returns::create_return(
        &mut conn,
        &Auth::default(),
        &owner,
        &ReturnInput {
            sale_id: sale.id.clone(),
            till_code: "T01".into(),
            lines: vec![ReturnLineInput { sale_line_id: line, qty: 1, condition: "restock".into(), reason: None }],
            refund_method: "cash".into(),
            client_ref: None,
            approval: None,
        },
    )
    .unwrap();
    let today = pos_server::ledger::today();
    let d = dashboard::dashboard(&conn, &dashboard::Period { from: today.clone(), to: today }).unwrap();
    let f = &d.figures;
    assert_eq!((f.sales, f.bills, f.refunds, f.net_sales), (9_000, 1, 3_000, 6_000));
    // 20 tablets sold for good at Rs 2.50.
    assert_eq!(f.cost_of_sales, 5_000);
    assert_eq!(f.gross_profit, 1_000);
    assert_eq!(f.expenses, 340_000);
    assert_eq!(f.net_profit, 1_000 - 340_000);
    assert_eq!(d.days.len(), 14);
    assert_eq!(d.days.last().unwrap().net_sales, 6_000);
    assert_eq!(d.khata_due, 420_000);
    assert!(d.payables > 0);
    assert!(d.low_stock.iter().any(|r| r.name.starts_with("Brufen")));
    assert!(d.expiring.iter().any(|b| b.batch_no == "FG-100" && b.expired));
}

#[test]
fn recovery_code_resets_the_owner_password_once() {
    let conn = store_db();
    let owner = user(&conn, "Owner");
    let auth = Auth::default();
    let code = settings::new_recovery_code(&conn, &owner).unwrap();
    assert_eq!(code.len(), 14);

    let input =
        |code: &str| RecoverInput { user_id: owner.id.clone(), code: code.into(), new_password: "new-pass-123".into() };
    assert_eq!(settings::recover(&conn, &auth, &input("AAAA-BBBB-CCCC")).unwrap_err().code(), "forbidden");
    let typed = code.replace('-', " ").to_lowercase();
    settings::recover(&conn, &auth, &input(&typed)).unwrap();
    assert!(auth.check_secret(&conn, &owner.id, "new-pass-123").is_ok());
    assert!(!settings::has_recovery_code(&conn).unwrap(), "the code works only once");
    assert!(settings::recover(&conn, &auth, &input(&code)).is_err());
}

#[test]
fn users_can_be_edited_but_the_last_admin_stays() {
    let conn = store_db();
    let owner = user(&conn, "Owner");
    let ali = user(&conn, "Ali");
    let edit = |active, secret: Option<&str>| UserUpdate {
        name: "Ali".into(),
        can_approve: true,
        language: "ur".into(),
        active,
        secret: secret.map(Into::into),
    };
    assert!(settings::update_user(&conn, &owner, &ali.id, &edit(true, Some("12"))).is_err());
    let row = settings::update_user(&conn, &owner, &ali.id, &edit(true, Some("4321"))).unwrap();
    assert!(row.can_approve);
    assert!(Auth::default().check_secret(&conn, &ali.id, "4321").is_ok());

    let me = UserUpdate { name: "Owner".into(), can_approve: true, language: "en".into(), active: false, secret: None };
    assert!(settings::update_user(&conn, &owner, &owner.id, &me).is_err());
    // Switched-off users can't sign in and disappear from the sign-in tiles.
    settings::update_user(&conn, &owner, &ali.id, &edit(false, None)).unwrap();
    assert!(store::login_tiles(&conn).unwrap().iter().all(|t| t.name != "Ali"));
}
