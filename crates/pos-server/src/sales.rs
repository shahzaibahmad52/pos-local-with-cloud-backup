use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::{Auth, User},
    db,
    error::{AppError, AppResult},
    pricing, products,
};

pub const PAYMENT_METHODS: &[&str] = &["cash", "card", "jazzcash", "easypaisa", "khata"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleLineInput {
    pub product_id: String,
    /// Pack level for unit products.
    pub unit_id: Option<String>,
    /// Number of packs for unit products.
    pub qty: Option<i64>,
    /// Weight products: either grams…
    pub grams: Option<i64>,
    /// …or the amount the customer asked for, in paisa ("Rs 10 of sugar").
    pub amount: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaymentInput {
    pub method: String,
    pub amount: i64,
}

pub use crate::auth::Approval;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleInput {
    pub till_code: String,
    pub customer_id: Option<String>,
    pub lines: Vec<SaleLineInput>,
    /// Bill discount in basis points (500 = 5%).
    #[serde(default)]
    pub discount_bp: i64,
    pub payments: Vec<PaymentInput>,
    /// Random id from the till; a retry with the same id returns the first sale.
    pub client_ref: Option<String>,
    /// Manager PIN, when the sale needs one (big discount, khata over limit).
    pub approval: Option<Approval>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreHeader {
    pub code: String,
    pub name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub ntn: Option<String>,
    pub drug_license: Option<String>,
    pub receipt_footer: Option<String>,
    pub receipt_language: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptLine {
    pub product_name: String,
    pub unit_name: String,
    pub sell_mode: String,
    pub qty: i64,
    pub unit_price: i64,
    pub line_total: i64,
    pub tax_included: i64,
    pub batch_no: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptPayment {
    pub method: String,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub id: String,
    pub receipt_no: String,
    pub created_at: String,
    pub till_code: String,
    pub cashier_name: String,
    pub customer_name: Option<String>,
    pub customer_receipt_language: Option<String>,
    pub lines: Vec<ReceiptLine>,
    pub item_count: i64,
    pub subtotal: i64,
    pub discount: i64,
    pub tax_included: i64,
    pub total: i64,
    pub paid: i64,
    pub change_due: i64,
    pub payments: Vec<ReceiptPayment>,
    pub khata_balance: Option<i64>,
    pub store: StoreHeader,
}

struct PricedLine {
    product_id: String,
    unit_id: Option<String>,
    unit_name: String,
    qty: i64,
    base_qty: i64,
    unit_price: i64,
    line_total: i64,
    tax: i64,
    allocations: Vec<(String, i64, i64)>, // batch id, base qty, cost
}

/// Takes `base_qty` from the batches expiring soonest. Expired batches are
/// never sold.
fn allocate_fefo(
    tx: &Transaction,
    reserved: &mut HashMap<String, i64>,
    product_id: &str,
    product_name: &str,
    base_qty: i64,
    base_unit: &str,
) -> AppResult<Vec<(String, i64, i64)>> {
    let month = db::current_month();
    let mut stmt = tx.prepare(
        "SELECT id, qty_on_hand, cost_per_base FROM batches
         WHERE product_id = ?1 AND qty_on_hand > 0 AND (expiry IS NULL OR expiry >= ?2)
         ORDER BY expiry IS NULL, expiry, received_at",
    )?;
    let batches = stmt
        .query_map(params![product_id, month], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut left = base_qty;
    let mut out = Vec::new();
    for (id, on_hand, cost) in batches {
        if left == 0 {
            break;
        }
        // Earlier lines of the same bill may already have claimed some of it.
        let free = on_hand - reserved.get(&id).copied().unwrap_or(0);
        if free <= 0 {
            continue;
        }
        let take = left.min(free);
        *reserved.entry(id.clone()).or_default() += take;
        out.push((id, take, take * cost));
        left -= take;
    }
    if left > 0 {
        let available = base_qty - left;
        let expired: i64 = tx.query_row(
            "SELECT COALESCE(SUM(qty_on_hand), 0) FROM batches
             WHERE product_id = ?1 AND expiry IS NOT NULL AND expiry < ?2",
            params![product_id, month],
            |r| r.get(0),
        )?;
        let mut msg =
            format!("Not enough stock of {product_name}: need {base_qty} {base_unit}, only {available} in stock.");
        if expired > 0 {
            msg.push_str(&format!(" ({expired} {base_unit} are expired and can't be sold.)"));
        }
        return Err(AppError::Stock(msg).because(
            "stock.short",
            json!({ "product": product_name, "need": base_qty, "have": available, "unit": base_unit, "expired": expired }),
        ));
    }
    Ok(out)
}

fn price_line(
    tx: &Transaction,
    reserved: &mut HashMap<String, i64>,
    line: &SaleLineInput,
    weight_round_to: i64,
) -> AppResult<PricedLine> {
    let product: Option<(String, String, String, Option<i64>, i64)> = tx
        .query_row(
            "SELECT name, sell_mode, base_unit, price_per_kg, tax_rate_bp
             FROM products WHERE id = ?1 AND active = 1",
            [&line.product_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((name, sell_mode, base_unit, price_per_kg, tax_rate_bp)) = product else {
        return Err(AppError::NotFound("A product on this bill no longer exists.".into()));
    };

    let (unit_id, unit_name, qty, base_qty, unit_price, line_total) = if sell_mode == "weight" {
        let per_kg = price_per_kg.unwrap_or(0);
        if per_kg <= 0 {
            return Err(AppError::BadRequest(format!("{name} has no price per kg.")));
        }
        let (grams, total) = match (line.grams, line.amount) {
            (Some(g), _) if g > 0 => (g, pricing::amount_for_grams(g, per_kg, weight_round_to)),
            (_, Some(a)) if a > 0 => (pricing::grams_for_amount(a, per_kg), a),
            _ => return Err(AppError::BadRequest(format!("Enter grams or an amount for {name}."))),
        };
        if grams <= 0 {
            return Err(AppError::BadRequest(format!("The amount for {name} is too small.")));
        }
        (None, "g".to_string(), grams, grams, per_kg, total)
    } else {
        let unit_id = line
            .unit_id
            .as_deref()
            .ok_or_else(|| AppError::BadRequest(format!("Choose how {name} is sold (Tab, Strip…).")))?;
        let qty = line.qty.unwrap_or(0);
        if qty <= 0 {
            return Err(AppError::BadRequest(format!("Quantity for {name} must be at least 1.")));
        }
        let unit: Option<(String, i64, i64)> = tx
            .query_row(
                "SELECT name, base_qty, price FROM product_units
                 WHERE id = ?1 AND product_id = ?2 AND sellable = 1",
                params![unit_id, line.product_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((unit_name, per_unit, default_price)) = unit else {
            return Err(AppError::BadRequest(format!("That pack size isn't sold for {name}.")));
        };
        // Charge the price printed on the batch that will be handed over.
        let first = products::first_sellable_batch(tx, &line.product_id)?;
        let price = products::effective_unit_price(tx, first.as_ref().map(|b| b.0.as_str()), unit_id, default_price)?;
        (Some(unit_id.to_string()), unit_name, qty, qty * per_unit, price, qty * price)
    };

    let allocations = allocate_fefo(tx, reserved, &line.product_id, &name, base_qty, &base_unit)?;
    Ok(PricedLine {
        product_id: line.product_id.clone(),
        unit_id,
        unit_name,
        qty,
        base_qty,
        unit_price,
        line_total,
        tax: pricing::included_tax(line_total, tax_rate_bp),
        allocations,
    })
}

fn next_receipt_no(tx: &Transaction, store_code: &str, till_code: &str) -> AppResult<String> {
    tx.execute(
        "INSERT INTO receipt_counters (till_code, last_no) VALUES (?1, 1)
         ON CONFLICT (till_code) DO UPDATE SET last_no = last_no + 1",
        [till_code],
    )?;
    let n: i64 =
        tx.query_row("SELECT last_no FROM receipt_counters WHERE till_code = ?1", [till_code], |r| r.get(0))?;
    Ok(format!("{store_code}-{till_code}-{n:06}"))
}

pub fn store_code(conn: &Connection) -> AppResult<String> {
    conn.query_row("SELECT code FROM stores LIMIT 1", [], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::Conflict("This store isn't set up yet.".into()))
}

pub fn khata_balance(conn: &Connection, customer_id: &str) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM khata_entries WHERE customer_id = ?1",
        [customer_id],
        |r| r.get(0),
    )?)
}

fn valid_till_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= 12 && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn create_sale(conn: &mut Connection, auth: &Auth, user: &User, input: &SaleInput) -> AppResult<Receipt> {
    let till_code = input.till_code.trim();
    if !valid_till_code(till_code) {
        return Err(AppError::BadRequest("Till code must be letters, digits or '-' (e.g. T01).".into()));
    }
    if input.lines.is_empty() {
        return Err(AppError::BadRequest("The bill is empty.".into()).because("bill.empty", json!({})));
    }
    if !(0..=10_000).contains(&input.discount_bp) {
        return Err(AppError::BadRequest("Discount must be between 0% and 100%.".into()));
    }

    if let Some(r) = &input.client_ref {
        let existing: Option<String> =
            conn.query_row("SELECT id FROM sales WHERE client_ref = ?1", [r], |row| row.get(0)).optional()?;
        if let Some(id) = existing {
            return receipt(conn, &id)?.ok_or_else(|| AppError::Internal("sale vanished".into()));
        }
    }

    // Manager approval is checked once, up front, and only if supplied.
    let approver = auth.approver(conn, input.approval.as_ref())?;
    let approved = user.is_admin() || approver.is_some();

    let max_discount = db::setting_i64(conn, "cashier_max_discount_bp", 500)?;
    if input.discount_bp > max_discount && !approved {
        return Err(AppError::ApprovalRequired(format!(
            "A discount above {}% needs a manager PIN.",
            max_discount as f64 / 100.0
        ))
        .because("approval.discount", json!({ "maxPercent": max_discount as f64 / 100.0 })));
    }
    let weight_round_to = db::setting_i64(conn, "weight_round_to_paisa", 100)?;
    let store_code = store_code(conn)?;

    let tx = conn.transaction()?;

    let mut reserved = HashMap::new();
    let mut lines = Vec::with_capacity(input.lines.len());
    for line in &input.lines {
        lines.push(price_line(&tx, &mut reserved, line, weight_round_to)?);
    }

    let subtotal: i64 = lines.iter().map(|l| l.line_total).sum();
    let discount = pricing::percent_of(subtotal, input.discount_bp);
    let total = subtotal - discount;
    let line_tax: i64 = lines.iter().map(|l| l.tax).sum();
    let tax_included = if subtotal > 0 { pricing::round_div(line_tax * total, subtotal) } else { 0 };

    let mut paid = 0;
    let mut cash = 0;
    let mut khata = 0;
    for p in &input.payments {
        if !PAYMENT_METHODS.contains(&p.method.as_str()) {
            return Err(AppError::BadRequest(format!("Unknown payment method: {}", p.method)));
        }
        if p.amount <= 0 {
            return Err(AppError::BadRequest("Payment amounts must be above 0.".into()));
        }
        paid += p.amount;
        match p.method.as_str() {
            "cash" => cash += p.amount,
            "khata" => khata += p.amount,
            _ => {}
        }
    }
    if paid < total {
        return Err(AppError::BadRequest(format!("Payment is short by Rs {}.", fmt_rs(total - paid)))
            .because("payment.short", json!({ "amount": total - paid })));
    }
    let change_due = paid - total;
    if change_due > cash {
        return Err(AppError::BadRequest("Only cash can be given back as change.".into())
            .because("payment.change_not_cash", json!({})));
    }

    let customer: Option<(String, i64)> = match &input.customer_id {
        Some(cid) => Some(
            tx.query_row("SELECT name, credit_limit FROM customers WHERE id = ?1", [cid], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?
            .ok_or_else(|| AppError::NotFound("Customer not found.".into()))?,
        ),
        None => None,
    };
    if khata > 0 {
        let Some((cname, limit)) = &customer else {
            return Err(
                AppError::BadRequest("Choose the khata customer first.".into()).because("khata.no_customer", json!({}))
            );
        };
        let cid = input.customer_id.as_deref().unwrap_or_default();
        let balance = khata_balance(&tx, cid)?;
        if balance + khata > *limit && !approved {
            return Err(AppError::ApprovalRequired(format!(
                "{cname}'s khata would be Rs {} (limit Rs {}). A manager PIN is needed.",
                fmt_rs(balance + khata),
                fmt_rs(*limit)
            ))
            .because("approval.khata", json!({ "customer": cname, "balance": balance + khata, "limit": limit })));
        }
    }

    let shift_id: Option<String> = tx
        .query_row("SELECT id FROM shifts WHERE till_code = ?1 AND status = 'open'", [till_code], |r| r.get(0))
        .optional()?;

    let sale_id = db::new_id();
    let receipt_no = next_receipt_no(&tx, &store_code, till_code)?;
    let now = db::now();
    tx.execute(
        "INSERT INTO sales (id, receipt_no, client_ref, shift_id, till_code, user_id, customer_id, created_at,
            subtotal, discount, tax_included, total, paid, change_due, approved_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            sale_id,
            receipt_no,
            input.client_ref,
            shift_id,
            till_code,
            user.id,
            input.customer_id,
            now,
            subtotal,
            discount,
            tax_included,
            total,
            paid,
            change_due,
            approver.as_ref().map(|a| a.id.as_str())
        ],
    )?;

    for (i, l) in lines.iter().enumerate() {
        let line_id = db::new_id();
        let cost_total: i64 = l.allocations.iter().map(|a| a.2).sum();
        tx.execute(
            "INSERT INTO sale_lines (id, sale_id, line_no, product_id, unit_id, unit_name, qty, base_qty,
                unit_price, line_total, tax_included, cost_total)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                line_id,
                sale_id,
                i as i64 + 1,
                l.product_id,
                l.unit_id,
                l.unit_name,
                l.qty,
                l.base_qty,
                l.unit_price,
                l.line_total,
                l.tax,
                cost_total
            ],
        )?;
        for (batch_id, qty, cost) in &l.allocations {
            tx.execute(
                "INSERT INTO sale_line_batches (sale_line_id, batch_id, base_qty, cost) VALUES (?1, ?2, ?3, ?4)",
                params![line_id, batch_id, qty, cost],
            )?;
            tx.execute("UPDATE batches SET qty_on_hand = qty_on_hand - ?1 WHERE id = ?2", params![qty, batch_id])?;
            tx.execute(
                "INSERT INTO stock_moves (id, product_id, batch_id, qty_delta, reason, ref_id, user_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'sale', ?5, ?6, ?7)",
                params![db::new_id(), l.product_id, batch_id, -qty, sale_id, user.id, now],
            )?;
        }
    }

    for p in &input.payments {
        // Record cash as what stayed in the drawer, not what was handed over.
        let amount = if p.method == "cash" { p.amount - change_due.min(p.amount) } else { p.amount };
        if amount <= 0 {
            continue;
        }
        tx.execute(
            "INSERT INTO payments (id, sale_id, method, amount) VALUES (?1, ?2, ?3, ?4)",
            params![db::new_id(), sale_id, p.method, amount],
        )?;
    }

    if khata > 0 {
        tx.execute(
            "INSERT INTO khata_entries (id, customer_id, sale_id, amount, method, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, 'sale', ?5, ?6)",
            params![db::new_id(), input.customer_id, sale_id, khata, user.id, now],
        )?;
    }

    if let Some(a) = &approver {
        db::audit(
            &tx,
            Some(&user.id),
            Some(till_code),
            "sale.approved",
            &json!({ "saleId": sale_id, "receiptNo": receipt_no, "discountBp": input.discount_bp, "khata": khata }),
            Some(&a.id),
        )?;
    }

    tx.commit()?;
    let receipt = receipt(conn, &sale_id)?.ok_or_else(|| AppError::Internal("sale vanished".into()))?;
    db::outbox(conn, "sale", &sale_id, "create", &serde_json::to_value(&receipt).unwrap_or_default())?;
    Ok(receipt)
}

pub fn store_header(conn: &Connection) -> AppResult<StoreHeader> {
    let (code, name, address, phone, ntn, drug_license): (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row("SELECT code, name, address, phone, ntn, drug_license FROM stores LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })
        .optional()?
        .ok_or_else(|| AppError::Conflict("This store isn't set up yet.".into()))?;
    Ok(StoreHeader {
        code,
        name,
        address,
        phone,
        ntn,
        drug_license,
        receipt_footer: db::get_setting(conn, "receipt_footer")?,
        receipt_language: db::get_setting(conn, "receipt_language")?.unwrap_or_else(|| "en".into()),
    })
}

pub fn receipt(conn: &Connection, sale_id: &str) -> AppResult<Option<Receipt>> {
    let head = conn
        .query_row(
            "SELECT s.id, s.receipt_no, s.created_at, s.till_code, u.name, c.name, c.receipt_language,
                    s.subtotal, s.discount, s.tax_included, s.total, s.paid, s.change_due, s.customer_id
             FROM sales s
             JOIN users u ON u.id = s.user_id
             LEFT JOIN customers c ON c.id = s.customer_id
             WHERE s.id = ?1",
            [sale_id],
            |r| {
                Ok((
                    Receipt {
                        id: r.get(0)?,
                        receipt_no: r.get(1)?,
                        created_at: r.get(2)?,
                        till_code: r.get(3)?,
                        cashier_name: r.get(4)?,
                        customer_name: r.get(5)?,
                        customer_receipt_language: r.get(6)?,
                        lines: Vec::new(),
                        item_count: 0,
                        subtotal: r.get(7)?,
                        discount: r.get(8)?,
                        tax_included: r.get(9)?,
                        total: r.get(10)?,
                        paid: r.get(11)?,
                        change_due: r.get(12)?,
                        payments: Vec::new(),
                        khata_balance: None,
                        store: StoreHeader {
                            code: String::new(),
                            name: String::new(),
                            address: None,
                            phone: None,
                            ntn: None,
                            drug_license: None,
                            receipt_footer: None,
                            receipt_language: "en".into(),
                        },
                    },
                    r.get::<_, Option<String>>(13)?,
                ))
            },
        )
        .optional()?;
    let Some((mut rec, customer_id)) = head else { return Ok(None) };

    let mut stmt = conn.prepare(
        "SELECT l.id, p.name, l.unit_name, p.sell_mode, l.qty, l.unit_price, l.line_total, l.tax_included
         FROM sale_lines l JOIN products p ON p.id = l.product_id
         WHERE l.sale_id = ?1 ORDER BY l.line_no",
    )?;
    let rows = stmt
        .query_map([sale_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                ReceiptLine {
                    product_name: r.get(1)?,
                    unit_name: r.get(2)?,
                    sell_mode: r.get(3)?,
                    qty: r.get(4)?,
                    unit_price: r.get(5)?,
                    line_total: r.get(6)?,
                    tax_included: r.get(7)?,
                    batch_no: None,
                },
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (line_id, mut line) in rows {
        line.batch_no = conn
            .query_row(
                "SELECT b.batch_no FROM sale_line_batches lb JOIN batches b ON b.id = lb.batch_id
                 WHERE lb.sale_line_id = ?1 ORDER BY b.expiry IS NULL, b.expiry LIMIT 1",
                [&line_id],
                |r| r.get(0),
            )
            .optional()?;
        rec.item_count += if line.sell_mode == "weight" { 1 } else { line.qty };
        rec.lines.push(line);
    }

    let mut stmt = conn.prepare("SELECT method, amount FROM payments WHERE sale_id = ?1")?;
    rec.payments = stmt
        .query_map([sale_id], |r| Ok(ReceiptPayment { method: r.get(0)?, amount: r.get(1)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(cid) = customer_id {
        rec.khata_balance = Some(khata_balance(conn, &cid)?);
    }
    rec.store = store_header(conn)?;
    Ok(Some(rec))
}

pub fn fmt_rs(paisa: i64) -> String {
    let rupees = paisa / 100;
    let rest = (paisa % 100).abs();
    let mut digits = rupees.abs().to_string();
    let mut grouped = String::new();
    while digits.len() > 3 {
        let tail = digits.split_off(digits.len() - 3);
        grouped = format!(",{tail}{grouped}");
    }
    let sign = if paisa < 0 { "-" } else { "" };
    if rest == 0 {
        format!("{sign}{digits}{grouped}")
    } else {
        format!("{sign}{digits}{grouped}.{rest:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::fmt_rs;

    #[test]
    fn formats_rupees() {
        assert_eq!(fmt_rs(129_000), "1,290");
        assert_eq!(fmt_rs(8_435_000), "84,350");
        assert_eq!(fmt_rs(160), "1.60");
        assert_eq!(fmt_rs(-15_000), "-150");
    }
}
