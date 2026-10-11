//! Suppliers, purchases (goods received) and what the store owes suppliers.

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::User,
    db,
    error::{AppError, AppResult},
    ledger::{self, Entry},
    pricing::round_div,
    products::valid_month,
    shifts,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierView {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub terms_days: i64,
    /// What the store owes this supplier now (paisa).
    pub balance: i64,
    /// Earliest due date among unpaid invoices.
    pub next_due: Option<String>,
    pub overdue: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierInput {
    pub name: String,
    pub phone: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub terms_days: i64,
}

fn supplier_entries(conn: &Connection, supplier_id: &str) -> AppResult<Vec<Entry>> {
    let mut stmt = conn.prepare(
        "SELECT e.amount, COALESCE(p.invoice_date, substr(e.created_at, 1, 10)), p.due_date
         FROM supplier_entries e LEFT JOIN purchases p ON p.id = e.purchase_id
         WHERE e.supplier_id = ?1 ORDER BY e.created_at, e.rowid",
    )?;
    let rows = stmt
        .query_map([supplier_id], |r| Ok(Entry { amount: r.get(0)?, date: r.get(1)?, due: r.get(2)?, bill: None }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn supplier_balance(conn: &Connection, supplier_id: &str) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM supplier_entries WHERE supplier_id = ?1",
        [supplier_id],
        |r| r.get(0),
    )?)
}

fn supplier_view(conn: &Connection, id: &str) -> AppResult<Option<SupplierView>> {
    let row = conn
        .query_row("SELECT id, name, phone, notes, terms_days FROM suppliers WHERE id = ?1", [id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .optional()?;
    let Some((id, name, phone, notes, terms_days)) = row else { return Ok(None) };
    let open = ledger::open_charges(&supplier_entries(conn, &id)?);
    let next_due = open.iter().map(|o| o.due.clone().unwrap_or_else(|| o.date.clone())).min();
    let overdue = next_due.as_deref().is_some_and(|d| d < ledger::today().as_str());
    Ok(Some(SupplierView {
        balance: supplier_balance(conn, &id)?,
        id,
        name,
        phone,
        notes,
        terms_days,
        next_due,
        overdue,
    }))
}

pub fn suppliers(conn: &Connection) -> AppResult<Vec<SupplierView>> {
    let ids = conn
        .prepare("SELECT id FROM suppliers WHERE active = 1 ORDER BY name COLLATE NOCASE")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(s) = supplier_view(conn, &id)? {
            out.push(s);
        }
    }
    Ok(out)
}

fn check_supplier(input: &SupplierInput) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(AppError::BadRequest("Supplier name is required.".into()));
    }
    if !(0..=365).contains(&input.terms_days) {
        return Err(AppError::BadRequest("Credit days must be between 0 and 365.".into()));
    }
    Ok(())
}

fn name_taken(conn: &Connection, name: &str, except: Option<&str>) -> AppResult<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM suppliers WHERE name = ?1 COLLATE NOCASE AND id IS NOT ?2",
            params![name.trim(), except],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub fn create_supplier(conn: &Connection, input: &SupplierInput, by: &User) -> AppResult<SupplierView> {
    check_supplier(input)?;
    if name_taken(conn, &input.name, None)? {
        return Err(AppError::Conflict(format!("A supplier called {} already exists.", input.name.trim())));
    }
    let id = db::new_id();
    conn.execute(
        "INSERT INTO suppliers (id, name, phone, notes, terms_days, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, input.name.trim(), input.phone, input.notes, input.terms_days, db::now()],
    )?;
    db::audit(conn, Some(&by.id), None, "supplier.create", &json!({ "name": input.name.trim() }), None)?;
    db::outbox(conn, "supplier", &id, "upsert", &json!({ "id": id, "name": input.name.trim() }))?;
    supplier_view(conn, &id)?.ok_or_else(|| AppError::Internal("supplier missing".into()))
}

pub fn update_supplier(conn: &Connection, id: &str, input: &SupplierInput, by: &User) -> AppResult<SupplierView> {
    check_supplier(input)?;
    if name_taken(conn, &input.name, Some(id))? {
        return Err(AppError::Conflict(format!("A supplier called {} already exists.", input.name.trim())));
    }
    let n = conn.execute(
        "UPDATE suppliers SET name = ?1, phone = ?2, notes = ?3, terms_days = ?4 WHERE id = ?5",
        params![input.name.trim(), input.phone, input.notes, input.terms_days, id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound("Supplier not found.".into()));
    }
    db::audit(conn, Some(&by.id), None, "supplier.update", &json!({ "name": input.name.trim() }), None)?;
    db::outbox(conn, "supplier", id, "upsert", &json!({ "id": id, "name": input.name.trim() }))?;
    supplier_view(conn, id)?.ok_or_else(|| AppError::Internal("supplier missing".into()))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseLineInput {
    pub product_id: String,
    /// Pack bought in. Leave out for weight items, which are bought by the kg.
    pub unit_id: Option<String>,
    #[serde(default)]
    pub batch_no: String,
    /// `YYYY-MM`. Required for medicines.
    pub expiry: Option<String>,
    pub qty: i64,
    #[serde(default)]
    pub bonus: i64,
    /// Paisa per pack, before the line discount.
    pub trade_price: i64,
    #[serde(default)]
    pub disc_bp: i64,
    /// Price printed on this batch's pack (paisa), if any.
    pub sale_price: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseInput {
    pub supplier_id: String,
    pub invoice_no: Option<String>,
    /// `YYYY-MM-DD`
    pub invoice_date: String,
    /// Credit days for this invoice; 0 means paid on delivery.
    #[serde(default)]
    pub terms_days: i64,
    #[serde(default)]
    pub freight: i64,
    #[serde(default)]
    pub paid_now: i64,
    #[serde(default = "default_source")]
    pub paid_from: String,
    /// Till whose drawer pays, when `paid_from` is `drawer`.
    pub till_code: Option<String>,
    pub lines: Vec<PurchaseLineInput>,
    pub client_ref: Option<String>,
}

fn default_source() -> String {
    "owner".into()
}

/// Money for one purchase line, worked out the same way on the till's screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineMoney {
    pub gross: i64,
    pub discount: i64,
    pub total: i64,
}

pub fn line_money(qty: i64, trade_price: i64, disc_bp: i64) -> LineMoney {
    let gross = qty * trade_price;
    let discount = round_div(gross * disc_bp, 10_000);
    LineMoney { gross, discount, total: gross - discount }
}

/// Splits freight across lines in proportion to their value. The shares
/// always add up to exactly `freight`.
pub fn split_freight(line_totals: &[i64], freight: i64) -> Vec<i64> {
    let sum: i64 = line_totals.iter().sum();
    let mut shares = vec![0; line_totals.len()];
    if line_totals.is_empty() || freight == 0 {
        return shares;
    }
    if sum == 0 {
        shares[0] = freight;
        return shares;
    }
    let mut given = 0;
    for (i, t) in line_totals.iter().enumerate() {
        shares[i] = freight * t / sum;
        given += shares[i];
    }
    // Rounding leftovers go to the biggest line (the first, if two are equal,
    // as the purchase screen shows it).
    let biggest = (0..line_totals.len()).fold(0, |b, i| if line_totals[i] > line_totals[b] { i } else { b });
    shares[biggest] += freight - given;
    shares
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseLineView {
    pub product_id: String,
    pub product_name: String,
    pub unit_name: String,
    pub batch_no: String,
    pub expiry: Option<String>,
    pub qty: i64,
    pub bonus: i64,
    pub trade_price: i64,
    pub disc_bp: i64,
    pub sale_price: Option<i64>,
    pub line_total: i64,
    pub cost_per_base: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseView {
    pub id: String,
    pub purchase_no: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub invoice_no: Option<String>,
    pub invoice_date: String,
    pub due_date: Option<String>,
    pub gross: i64,
    pub discount: i64,
    pub freight: i64,
    pub net: i64,
    pub paid_now: i64,
    pub user_name: String,
    pub created_at: String,
    pub line_count: i64,
    pub lines: Vec<PurchaseLineView>,
}

struct ProductInfo {
    name: String,
    kind: String,
    sell_mode: String,
}

fn product_info(tx: &Transaction, id: &str) -> AppResult<ProductInfo> {
    tx.query_row("SELECT name, kind, sell_mode FROM products WHERE id = ?1 AND active = 1", [id], |r| {
        Ok(ProductInfo { name: r.get(0)?, kind: r.get(1)?, sell_mode: r.get(2)? })
    })
    .optional()?
    .ok_or_else(|| AppError::NotFound("A product on this purchase no longer exists.".into()))
}

fn next_purchase_no(tx: &Transaction) -> AppResult<String> {
    let n: i64 = tx.query_row("SELECT COUNT(*) FROM purchases", [], |r| r.get(0))?;
    Ok(format!("P-{:05}", n + 1))
}

/// Adds stock to the batch with this number and expiry, or starts a new
/// batch. Receiving more of an existing batch averages its cost.
pub fn add_to_batch(
    tx: &Connection,
    product_id: &str,
    batch_no: &str,
    expiry: Option<&str>,
    base_qty: i64,
    cost_per_base: i64,
) -> AppResult<String> {
    let existing: Option<(String, i64, i64)> = tx
        .query_row(
            "SELECT id, qty_on_hand, cost_per_base FROM batches
             WHERE product_id = ?1 AND batch_no = ?2 AND expiry IS ?3",
            params![product_id, batch_no, expiry],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    match existing {
        Some((id, on_hand, old_cost)) => {
            let old = on_hand.max(0);
            let cost = round_div(old * old_cost + base_qty * cost_per_base, old + base_qty);
            tx.execute(
                "UPDATE batches SET qty_on_hand = qty_on_hand + ?1, cost_per_base = ?2 WHERE id = ?3",
                params![base_qty, cost, id],
            )?;
            Ok(id)
        }
        None => {
            let id = db::new_id();
            tx.execute(
                "INSERT INTO batches (id, product_id, batch_no, expiry, cost_per_base, qty_on_hand, received_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, product_id, batch_no, expiry, cost_per_base, base_qty, db::now()],
            )?;
            Ok(id)
        }
    }
}

/// Receives a purchase: stock goes into batches at its true cost (bonus
/// units and freight included), and the supplier's balance goes up.
pub fn receive(conn: &mut Connection, user: &User, input: &PurchaseInput) -> AppResult<PurchaseView> {
    if let Some(r) = &input.client_ref {
        let done: Option<String> =
            conn.query_row("SELECT id FROM purchases WHERE client_ref = ?1", [r], |r| r.get(0)).optional()?;
        if let Some(id) = done {
            return purchase(conn, &id)?.ok_or_else(|| AppError::Internal("purchase missing".into()));
        }
    }
    if input.lines.is_empty() {
        return Err(AppError::BadRequest("Add at least one item.".into()).because("purchase.empty", json!({})));
    }
    if !ledger::valid_date(&input.invoice_date) {
        return Err(AppError::BadRequest("Invoice date must look like 2026-10-10.".into()));
    }
    if !(0..=365).contains(&input.terms_days) {
        return Err(AppError::BadRequest("Credit days must be between 0 and 365.".into()));
    }
    if input.freight < 0 || input.paid_now < 0 {
        return Err(AppError::BadRequest("Freight and amount paid can't be negative.".into()));
    }
    let this_month = db::current_month();
    for (i, l) in input.lines.iter().enumerate() {
        let n = i + 1;
        if l.qty <= 0 || l.bonus < 0 || l.qty > crate::pricing::MAX_QTY || l.bonus > crate::pricing::MAX_QTY {
            return Err(AppError::BadRequest(format!("Line {n}: quantity must be more than 0."))
                .because("purchase.qty", json!({ "line": n })));
        }
        if !(0..=crate::pricing::MAX_AMOUNT).contains(&l.trade_price)
            || l.sale_price.is_some_and(|p| !(0..=crate::pricing::MAX_AMOUNT).contains(&p))
            || !(0..=10_000).contains(&l.disc_bp)
        {
            return Err(AppError::BadRequest(format!("Line {n}: check the prices and discount."))
                .because("purchase.price", json!({ "line": n })));
        }
        if let Some(e) = &l.expiry {
            if !valid_month(e) {
                return Err(AppError::BadRequest(format!("Line {n}: expiry must look like 2028-08."))
                    .because("purchase.expiry_format", json!({ "line": n })));
            }
            if *e < this_month {
                return Err(AppError::BadRequest(format!("Line {n}: this batch has already expired ({e})."))
                    .because("purchase.expired", json!({ "line": n, "expiry": e })));
            }
        }
    }

    let tx = conn.transaction()?;
    let supplier_name: String = tx
        .query_row("SELECT name FROM suppliers WHERE id = ?1 AND active = 1", [&input.supplier_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::NotFound("Supplier not found.".into()))?;
    let purchase_id = db::new_id();
    let purchase_no = next_purchase_no(&tx)?;

    // Work out each line's pack and money before touching stock.
    struct Prepared {
        input: PurchaseLineInput,
        info: ProductInfo,
        unit_name: String,
        unit_base_qty: i64,
        money: LineMoney,
    }
    let mut prepared = Vec::with_capacity(input.lines.len());
    for (i, l) in input.lines.iter().enumerate() {
        let n = i + 1;
        let info = product_info(&tx, &l.product_id)?;
        let (unit_name, unit_base_qty) = if info.sell_mode == "weight" {
            ("kg".to_string(), 1000)
        } else {
            let unit_id = l.unit_id.as_deref().ok_or_else(|| {
                AppError::BadRequest(format!("Line {n}: choose the pack ({}).", info.name))
                    .because("purchase.unit", json!({ "line": n }))
            })?;
            tx.query_row(
                "SELECT name, base_qty FROM product_units WHERE id = ?1 AND product_id = ?2",
                params![unit_id, l.product_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::BadRequest(format!("Line {n}: that pack belongs to another product.")))?
        };
        if info.kind == "medicine" && l.expiry.is_none() {
            return Err(AppError::BadRequest(format!("Line {n}: enter the expiry for {}.", info.name))
                .because("purchase.expiry_required", json!({ "line": n, "product": info.name })));
        }
        prepared.push(Prepared {
            money: line_money(l.qty, l.trade_price, l.disc_bp),
            input: l.clone(),
            info,
            unit_name,
            unit_base_qty,
        });
    }
    let gross: i64 = prepared.iter().map(|p| p.money.gross).sum();
    let discount: i64 = prepared.iter().map(|p| p.money.discount).sum();
    let goods: i64 = prepared.iter().map(|p| p.money.total).sum();
    let net = goods + input.freight;
    if input.paid_now > net {
        return Err(AppError::BadRequest("Paid now is more than the invoice total.".into())
            .because("purchase.overpaid", json!({ "net": net })));
    }
    let freight = split_freight(&prepared.iter().map(|p| p.money.total).collect::<Vec<_>>(), input.freight);
    let due_date = if input.terms_days > 0 { ledger::add_days(&input.invoice_date, input.terms_days) } else { None };
    let now = db::now();
    let invoice_no = input.invoice_no.as_deref().map(str::trim).filter(|s| !s.is_empty());

    tx.execute(
        "INSERT INTO purchases (id, purchase_no, client_ref, supplier_id, invoice_no, invoice_date, due_date,
            gross, discount, freight, net, paid_now, user_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            purchase_id,
            purchase_no,
            input.client_ref,
            input.supplier_id,
            invoice_no,
            input.invoice_date,
            due_date,
            gross,
            discount,
            input.freight,
            net,
            input.paid_now,
            user.id,
            now
        ],
    )?;

    for (i, (p, share)) in prepared.iter().zip(freight).enumerate() {
        let l = &p.input;
        let base_qty = (l.qty + l.bonus) * p.unit_base_qty;
        let cost_per_base = round_div(p.money.total + share, base_qty);
        let batch_no = match l.batch_no.trim() {
            "" => purchase_no.clone(),
            b => b.to_string(),
        };
        let batch_id = add_to_batch(&tx, &l.product_id, &batch_no, l.expiry.as_deref(), base_qty, cost_per_base)?;
        // The price printed on this batch, scaled to every pack size.
        if let (Some(mrp), "unit") = (l.sale_price, p.info.sell_mode.as_str()) {
            let units = tx
                .prepare("SELECT id, base_qty FROM product_units WHERE product_id = ?1")?
                .query_map([&l.product_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            for (unit_id, unit_qty) in units {
                tx.execute(
                    "INSERT INTO batch_unit_prices (batch_id, unit_id, price) VALUES (?1, ?2, ?3)
                     ON CONFLICT (batch_id, unit_id) DO UPDATE SET price = excluded.price",
                    params![batch_id, unit_id, round_div(mrp * unit_qty, p.unit_base_qty)],
                )?;
            }
        }
        tx.execute(
            "INSERT INTO purchase_lines (id, purchase_id, line_no, product_id, unit_name, unit_base_qty, batch_id,
                batch_no, expiry, qty, bonus, trade_price, disc_bp, sale_price, line_total, cost_per_base)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                db::new_id(),
                purchase_id,
                i as i64 + 1,
                l.product_id,
                p.unit_name,
                p.unit_base_qty,
                batch_id,
                batch_no,
                l.expiry,
                l.qty,
                l.bonus,
                l.trade_price,
                l.disc_bp,
                l.sale_price,
                p.money.total,
                cost_per_base
            ],
        )?;
        tx.execute(
            "INSERT INTO stock_moves (id, product_id, batch_id, qty_delta, reason, ref_id, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, 'purchase', ?5, ?6, ?7)",
            params![db::new_id(), l.product_id, batch_id, base_qty, purchase_id, user.id, now],
        )?;
    }

    tx.execute(
        "INSERT INTO supplier_entries (id, supplier_id, purchase_id, amount, note, user_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![db::new_id(), input.supplier_id, purchase_id, net, purchase_no, user.id, now],
    )?;
    if input.paid_now > 0 {
        let reason = format!("Supplier {supplier_name} · {purchase_no}");
        shifts::move_money(
            &tx,
            &input.paid_from,
            input.till_code.as_deref(),
            &user.id,
            "out",
            input.paid_now,
            &reason,
        )?;
        tx.execute(
            "INSERT INTO supplier_entries (id, supplier_id, purchase_id, amount, paid_from, note, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                db::new_id(),
                input.supplier_id,
                purchase_id,
                -input.paid_now,
                input.paid_from,
                format!("Paid with {purchase_no}"),
                user.id,
                now
            ],
        )?;
    }
    db::audit(
        &tx,
        Some(&user.id),
        input.till_code.as_deref().filter(|_| input.paid_from == "drawer" && input.paid_now > 0),
        "purchase.receive",
        &json!({
            "purchaseNo": purchase_no,
            "supplier": supplier_name,
            "invoiceNo": invoice_no,
            "net": net,
            "paidNow": input.paid_now,
            "lines": prepared.len(),
        }),
        None,
    )?;
    tx.commit()?;
    let view = purchase(conn, &purchase_id)?.ok_or_else(|| AppError::Internal("purchase missing".into()))?;
    db::outbox(conn, "purchase", &purchase_id, "create", &serde_json::to_value(&view).unwrap_or_default())?;
    Ok(view)
}

fn purchase_head(r: &rusqlite::Row) -> rusqlite::Result<PurchaseView> {
    Ok(PurchaseView {
        id: r.get(0)?,
        purchase_no: r.get(1)?,
        supplier_id: r.get(2)?,
        supplier_name: r.get(3)?,
        invoice_no: r.get(4)?,
        invoice_date: r.get(5)?,
        due_date: r.get(6)?,
        gross: r.get(7)?,
        discount: r.get(8)?,
        freight: r.get(9)?,
        net: r.get(10)?,
        paid_now: r.get(11)?,
        user_name: r.get(12)?,
        created_at: r.get(13)?,
        line_count: r.get(14)?,
        lines: Vec::new(),
    })
}

const PURCHASE_SELECT: &str = "SELECT p.id, p.purchase_no, p.supplier_id, s.name, p.invoice_no, p.invoice_date,
        p.due_date, p.gross, p.discount, p.freight, p.net, p.paid_now, u.name, p.created_at,
        (SELECT COUNT(*) FROM purchase_lines l WHERE l.purchase_id = p.id)
     FROM purchases p JOIN suppliers s ON s.id = p.supplier_id JOIN users u ON u.id = p.user_id";

pub fn purchase(conn: &Connection, id: &str) -> AppResult<Option<PurchaseView>> {
    let head = conn.query_row(&format!("{PURCHASE_SELECT} WHERE p.id = ?1"), [id], purchase_head).optional()?;
    let Some(mut view) = head else { return Ok(None) };
    let mut stmt = conn.prepare(
        "SELECT l.product_id, pr.name, l.unit_name, l.batch_no, l.expiry, l.qty, l.bonus, l.trade_price, l.disc_bp,
                l.sale_price, l.line_total, l.cost_per_base
         FROM purchase_lines l JOIN products pr ON pr.id = l.product_id
         WHERE l.purchase_id = ?1 ORDER BY l.line_no",
    )?;
    view.lines = stmt
        .query_map([id], |r| {
            Ok(PurchaseLineView {
                product_id: r.get(0)?,
                product_name: r.get(1)?,
                unit_name: r.get(2)?,
                batch_no: r.get(3)?,
                expiry: r.get(4)?,
                qty: r.get(5)?,
                bonus: r.get(6)?,
                trade_price: r.get(7)?,
                disc_bp: r.get(8)?,
                sale_price: r.get(9)?,
                line_total: r.get(10)?,
                cost_per_base: r.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(view))
}

/// Recent purchases, newest first, optionally for one supplier.
pub fn purchases(conn: &Connection, supplier_id: Option<&str>, limit: i64) -> AppResult<Vec<PurchaseView>> {
    let mut stmt = conn.prepare(&format!(
        "{PURCHASE_SELECT} WHERE ?1 IS NULL OR p.supplier_id = ?1 ORDER BY p.created_at DESC, p.rowid DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![supplier_id, limit], purchase_head)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The last price paid for a product, so a new purchase line starts filled in.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastBuy {
    pub unit_name: String,
    pub trade_price: i64,
    pub disc_bp: i64,
    pub sale_price: Option<i64>,
    pub supplier_name: String,
    pub invoice_date: String,
}

pub fn last_buy(conn: &Connection, product_id: &str) -> AppResult<Option<LastBuy>> {
    Ok(conn
        .query_row(
            "SELECT l.unit_name, l.trade_price, l.disc_bp, l.sale_price, s.name, p.invoice_date
             FROM purchase_lines l JOIN purchases p ON p.id = l.purchase_id JOIN suppliers s ON s.id = p.supplier_id
             WHERE l.product_id = ?1 ORDER BY p.created_at DESC, l.line_no LIMIT 1",
            [product_id],
            |r| {
                Ok(LastBuy {
                    unit_name: r.get(0)?,
                    trade_price: r.get(1)?,
                    disc_bp: r.get(2)?,
                    sale_price: r.get(3)?,
                    supplier_name: r.get(4)?,
                    invoice_date: r.get(5)?,
                })
            },
        )
        .optional()?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierPaymentInput {
    pub amount: i64,
    pub paid_from: String,
    pub till_code: Option<String>,
    pub note: Option<String>,
}

pub fn pay_supplier(
    conn: &mut Connection,
    user: &User,
    supplier_id: &str,
    input: &SupplierPaymentInput,
) -> AppResult<SupplierView> {
    if input.amount <= 0 || input.amount > crate::pricing::MAX_AMOUNT {
        return Err(AppError::BadRequest("Amount must be above 0.".into()));
    }
    let tx = conn.transaction()?;
    let name: String = tx
        .query_row("SELECT name FROM suppliers WHERE id = ?1", [supplier_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::NotFound("Supplier not found.".into()))?;
    let balance = supplier_balance(&tx, supplier_id)?;
    if input.amount > balance {
        return Err(AppError::BadRequest(format!(
            "That is more than the Rs {} owed to {name}.",
            crate::sales::fmt_rs(balance.max(0))
        ))
        .because("supplier.overpay", json!({ "balance": balance.max(0), "name": name })));
    }
    shifts::move_money(
        &tx,
        &input.paid_from,
        input.till_code.as_deref(),
        &user.id,
        "out",
        input.amount,
        &format!("Supplier {name}"),
    )?;
    let note = input.note.as_deref().map(str::trim).filter(|s| !s.is_empty());
    tx.execute(
        "INSERT INTO supplier_entries (id, supplier_id, amount, paid_from, note, user_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![db::new_id(), supplier_id, -input.amount, input.paid_from, note, user.id, db::now()],
    )?;
    db::audit(
        &tx,
        Some(&user.id),
        input.till_code.as_deref().filter(|_| input.paid_from == "drawer"),
        "supplier.pay",
        &json!({ "supplier": name, "amount": input.amount, "paidFrom": input.paid_from }),
        None,
    )?;
    db::outbox(&tx, "supplier_payment", supplier_id, "create", &json!({ "amount": input.amount }))?;
    tx.commit()?;
    supplier_view(conn, supplier_id)?.ok_or_else(|| AppError::Internal("supplier missing".into()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpeningInput {
    pub amount: i64,
    pub note: Option<String>,
}

/// Carries over what the store already owed a supplier before using the app.
pub fn add_opening(conn: &Connection, user: &User, supplier_id: &str, input: &OpeningInput) -> AppResult<SupplierView> {
    if input.amount <= 0 || input.amount > crate::pricing::MAX_AMOUNT {
        return Err(AppError::BadRequest("Amount must be above 0.".into()));
    }
    let name: String = conn
        .query_row("SELECT name FROM suppliers WHERE id = ?1", [supplier_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::NotFound("Supplier not found.".into()))?;
    let note = input.note.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("Old balance");
    conn.execute(
        "INSERT INTO supplier_entries (id, supplier_id, amount, note, user_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![db::new_id(), supplier_id, input.amount, note, user.id, db::now()],
    )?;
    db::audit(
        conn,
        Some(&user.id),
        None,
        "supplier.opening",
        &json!({ "supplier": name, "amount": input.amount }),
        None,
    )?;
    db::outbox(conn, "supplier_opening", supplier_id, "create", &json!({ "amount": input.amount }))?;
    supplier_view(conn, supplier_id)?.ok_or_else(|| AppError::Internal("supplier missing".into()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerLine {
    pub created_at: String,
    pub detail: String,
    pub purchase_id: Option<String>,
    pub paid_from: Option<String>,
    /// > 0: purchase (owed more); < 0: payment.
    pub amount: i64,
    pub balance: i64,
}

pub fn supplier_ledger(conn: &Connection, supplier_id: &str) -> AppResult<Vec<LedgerLine>> {
    let mut stmt = conn.prepare(
        "SELECT e.created_at, COALESCE(e.note, ''), e.purchase_id, e.paid_from, e.amount, p.invoice_no
         FROM supplier_entries e LEFT JOIN purchases p ON p.id = e.purchase_id
         WHERE e.supplier_id = ?1 ORDER BY e.created_at, e.rowid",
    )?;
    let mut balance = 0;
    let rows = stmt
        .query_map([supplier_id], |r| {
            let note: String = r.get(1)?;
            let amount: i64 = r.get(4)?;
            let invoice: Option<String> = r.get(5)?;
            Ok((r.get::<_, String>(0)?, note, r.get(2)?, r.get(3)?, amount, invoice))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(created_at, note, purchase_id, paid_from, amount, invoice)| {
            balance += amount;
            let detail = match (amount > 0, invoice) {
                (true, Some(inv)) => format!("{note} · invoice {inv}"),
                _ => note,
            };
            LedgerLine { created_at, detail, purchase_id, paid_from, amount, balance }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_discount() {
        // 5 boxes at Rs 500, 2% off.
        assert_eq!(line_money(5, 50_000, 200), LineMoney { gross: 250_000, discount: 5_000, total: 245_000 });
    }

    #[test]
    fn freight_leftover_goes_to_the_first_biggest_line() {
        assert_eq!(split_freight(&[1_000, 1_000], 1), vec![1, 0]);
    }

    #[test]
    fn freight_adds_up() {
        let shares = split_freight(&[245_000, 704_000, 1_050_000, 139_680], 30_000);
        assert_eq!(shares.iter().sum::<i64>(), 30_000);
        assert!(shares[2] > shares[0]);
        assert_eq!(split_freight(&[0, 0], 500), vec![500, 0]);
        assert_eq!(split_freight(&[100], 0), vec![0]);
    }
}
