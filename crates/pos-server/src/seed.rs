//! Demo data for trying the app and for tests. Never loaded unless asked for
//! (`pos-server --demo`). Prices are samples, not real prices.

use rusqlite::Connection;

use crate::{
    auth::User,
    db,
    error::AppResult,
    expenses::{self, ExpenseInput},
    khata::{self, OpeningInput},
    ledger,
    products::{self, NewBarcode, NewProduct, NewUnit, ReceiveStock, UnitPrice},
    purchases::{self, PurchaseInput, PurchaseLineInput, SupplierInput},
    store::{self, NewCustomer, NewUser, SetupInput},
};

pub const DEMO_OWNER_PASSWORD: &str = "owner1234";

fn unit(name: &str, name_ur: &str, base_qty: i64, rupees: i64) -> NewUnit {
    NewUnit { name: name.into(), name_ur: Some(name_ur.into()), base_qty, price: rupees * 100 }
}

fn medicine(name: &str, generic: &str, company: &str, base_unit: &str, units: Vec<NewUnit>) -> NewProduct {
    NewProduct {
        name: name.into(),
        generic_name: Some(generic.into()),
        company: Some(company.into()),
        category: Some("Medicines".into()),
        kind: "medicine".into(),
        sell_mode: "unit".into(),
        base_unit: base_unit.into(),
        price_per_kg: None,
        tax_rate_bp: 0,
        rx_required: false,
        controlled: false,
        cold_chain: false,
        rack: None,
        reorder_level: 0,
        units,
        barcodes: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn receive(
    conn: &mut Connection,
    owner: &User,
    product_id: &str,
    batch_no: &str,
    expiry: Option<&str>,
    base_qty: i64,
    cost_paisa: i64,
    unit_prices: Vec<UnitPrice>,
) -> AppResult<()> {
    products::receive_stock(
        conn,
        &ReceiveStock {
            product_id: product_id.into(),
            batch_no: batch_no.into(),
            expiry: expiry.map(Into::into),
            base_qty,
            cost_per_base: cost_paisa,
            unit_prices,
            reason: "opening".into(),
        },
        &owner.id,
    )?;
    Ok(())
}

/// Loads the demo store. Does nothing if the database already has a store.
pub fn demo(conn: &mut Connection) -> AppResult<bool> {
    if store::is_set_up(conn)? {
        return Ok(false);
    }
    let owner = store::setup(
        conn,
        &SetupInput {
            store_code: "B1".into(),
            store_name: "Branch 1 — Gulberg".into(),
            address: Some("[Shop address], Gulberg, Lahore".into()),
            phone: Some("0300-0000000".into()),
            owner_name: "Owner".into(),
            owner_password: DEMO_OWNER_PASSWORD.into(),
        },
    )?;
    db::set_setting(conn, "receipt_footer", "Thank you! Get well soon.")?;
    db::set_setting(conn, "receipt_language", "en")?;

    for (name, pin, approve, lang) in
        [("Ali", "1234", false, "en"), ("Sana", "2345", true, "ur"), ("Usman", "3456", false, "en")]
    {
        store::create_user(
            conn,
            &NewUser {
                name: name.into(),
                role: "cashier".into(),
                secret: pin.into(),
                can_approve: approve,
                language: lang.into(),
            },
            &owner,
        )?;
    }

    let mut panadol = medicine(
        "Panadol 500mg Tablet",
        "Paracetamol",
        "GSK",
        "tab",
        vec![unit("Tab", "گولی", 1, 3), unit("Strip", "پتہ", 10, 30), unit("Box", "ڈبہ", 200, 590)],
    );
    panadol.rack = Some("A3".into());
    panadol.reorder_level = 200;
    panadol.barcodes = vec![
        NewBarcode { code: "8964000123456".into(), unit_name: Some("Box".into()) },
        NewBarcode { code: "8964000123463".into(), unit_name: Some("Strip".into()) },
    ];
    let panadol = products::create_product(conn, &panadol, &owner.id)?;
    receive(conn, &owner, &panadol.id, "B-2291", Some("2028-08"), 1_000, 250, vec![])?;
    let strip = panadol.units.iter().find(|u| u.name == "Strip").map(|u| u.id.clone()).unwrap_or_default();
    receive(
        conn,
        &owner,
        &panadol.id,
        "B-2310",
        Some("2028-11"),
        240,
        275,
        vec![UnitPrice { unit_id: strip, price: 3_300 }],
    )?;

    let mut augmentin = medicine(
        "Augmentin 625mg Tablet",
        "Amoxicillin + Clavulanic acid",
        "GSK",
        "tab",
        vec![unit("Tab", "گولی", 1, 70), unit("Strip", "پتہ", 6, 420)],
    );
    augmentin.rx_required = true;
    augmentin.rack = Some("C1".into());
    augmentin.reorder_level = 60;
    let augmentin = products::create_product(conn, &augmentin, &owner.id)?;
    receive(conn, &owner, &augmentin.id, "AG-118", Some("2026-12"), 84, 5_867, vec![])?;

    let mut risek = medicine(
        "Risek 20mg Capsule",
        "Omeprazole",
        "Getz",
        "cap",
        vec![unit("Cap", "کیپسول", 1, 26), unit("Strip", "پتہ", 14, 360)],
    );
    risek.rack = Some("B2".into());
    let risek = products::create_product(conn, &risek, &owner.id)?;
    receive(conn, &owner, &risek.id, "RK-772", Some("2028-05"), 310, 2_143, vec![])?;

    let mut brufen = medicine("Brufen Syrup 90ml", "Ibuprofen", "Abbott", "btl", vec![unit("Bottle", "بوتل", 1, 150)]);
    brufen.rack = Some("D4".into());
    brufen.reorder_level = 24;
    let brufen = products::create_product(conn, &brufen, &owner.id)?;
    receive(conn, &owner, &brufen.id, "BF-031", Some("2028-02"), 22, 12_500, vec![])?;

    let mut flagyl = medicine(
        "Flagyl 400mg Tablet",
        "Metronidazole",
        "Sanofi",
        "tab",
        vec![unit("Tab", "گولی", 1, 6), unit("Strip", "پتہ", 10, 62)],
    );
    flagyl.rx_required = true;
    let flagyl = products::create_product(conn, &flagyl, &owner.id)?;
    // An already-expired batch, so the till's "expired can't be sold" rule is visible.
    receive(conn, &owner, &flagyl.id, "FG-100", Some("2026-08"), 50, 520, vec![])?;
    receive(conn, &owner, &flagyl.id, "FG-209", Some("2026-11"), 120, 520, vec![])?;

    let olpers = products::create_product(
        conn,
        &NewProduct {
            name: "Olper's Milk 1L".into(),
            generic_name: None,
            company: Some("Engro".into()),
            category: Some("Grocery".into()),
            kind: "general".into(),
            sell_mode: "unit".into(),
            base_unit: "pc".into(),
            price_per_kg: None,
            tax_rate_bp: 1_800,
            rx_required: false,
            controlled: false,
            cold_chain: true,
            rack: Some("Fridge 1".into()),
            reorder_level: 24,
            units: vec![unit("Pack", "پیک", 1, 330)],
            barcodes: Vec::new(),
        },
        &owner.id,
    )?;
    receive(conn, &owner, &olpers.id, "-", Some("2026-11"), 36, 30_000, vec![])?;

    let soap = products::create_product(
        conn,
        &NewProduct {
            name: "Lifebuoy Soap 98g".into(),
            generic_name: None,
            company: Some("Unilever".into()),
            category: Some("Personal care".into()),
            kind: "general".into(),
            sell_mode: "unit".into(),
            base_unit: "pc".into(),
            price_per_kg: None,
            tax_rate_bp: 1_800,
            rx_required: false,
            controlled: false,
            cold_chain: false,
            rack: Some("G2".into()),
            reorder_level: 12,
            units: vec![unit("Pc", "عدد", 1, 120), unit("Box", "ڈبہ", 6, 700)],
            barcodes: Vec::new(),
        },
        &owner.id,
    )?;
    // Rs 85 a piece against Rs 120 (18% tax included): a normal margin.
    receive(conn, &owner, &soap.id, "-", None, 58, 8_500, vec![])?;

    let sugar = products::create_product(
        conn,
        &NewProduct {
            name: "Sugar".into(),
            generic_name: None,
            company: None,
            category: Some("Grocery".into()),
            kind: "general".into(),
            sell_mode: "weight".into(),
            base_unit: "g".into(),
            price_per_kg: Some(16_000),
            tax_rate_bp: 0,
            rx_required: false,
            controlled: false,
            cold_chain: false,
            rack: Some("G4".into()),
            reorder_level: 5_000,
            units: Vec::new(),
            barcodes: Vec::new(),
        },
        &owner.id,
    )?;
    receive(conn, &owner, &sugar.id, "-", None, 38_500, 14, vec![])?;

    store::create_customer(
        conn,
        &NewCustomer {
            name: "Mr. Imran".into(),
            phone: Some("0300-xxxxxxx".into()),
            credit_limit: 1_000_000,
            receipt_language: Some("en".into()),
        },
        &owner,
    )?;
    let rasheed = store::create_customer(
        conn,
        &NewCustomer {
            name: "Haji Rasheed".into(),
            phone: Some("0321-xxxxxxx".into()),
            credit_limit: 1_500_000,
            receipt_language: Some("ur".into()),
        },
        &owner,
    )?;
    khata::add_opening(conn, &owner, &rasheed.id, &OpeningInput { amount: 420_000, note: None })?;

    let distributor = purchases::create_supplier(
        conn,
        &SupplierInput { name: "Distributor A".into(), phone: None, notes: None, terms_days: 30 },
        &owner,
    )?;
    purchases::create_supplier(
        conn,
        &SupplierInput { name: "Grocery wholesaler".into(), phone: None, notes: None, terms_days: 0 },
        &owner,
    )?;
    let soap_box = soap.units.iter().find(|u| u.name == "Box").map(|u| u.id.clone());
    purchases::receive(
        conn,
        &owner,
        &PurchaseInput {
            supplier_id: distributor.id,
            invoice_no: Some("INV-55120".into()),
            invoice_date: ledger::today(),
            terms_days: 30,
            freight: 0,
            paid_now: 0,
            paid_from: "owner".into(),
            till_code: None,
            lines: vec![PurchaseLineInput {
                product_id: soap.id.clone(),
                unit_id: soap_box,
                batch_no: String::new(),
                expiry: None,
                qty: 4,
                bonus: 0,
                trade_price: 52_000,
                disc_bp: 200,
                sale_price: None,
            }],
            client_ref: None,
        },
    )?;
    expenses::add(
        conn,
        &owner,
        &ExpenseInput {
            category: "Generator fuel".into(),
            amount: 340_000,
            date: ledger::today(),
            paid_from: "owner".into(),
            till_code: None,
            note: Some("Petrol 12 L".into()),
        },
    )?;
    Ok(true)
}
