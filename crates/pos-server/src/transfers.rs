//! Moving stock between branches. Each store has its own database, so a
//! transfer travels as a self-contained "transfer note": everything the
//! receiving store needs to add the stock, even for products it has never
//! stocked. The receiving store gets the note from the sender's cloud copy
//! (if it has that store in its All stores list) or as a file.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::User,
    db,
    error::{AppError, AppResult},
    products::{self, NewBarcode, NewProduct, NewUnit},
    purchases::add_to_batch,
    sales,
};

/// One batch on a transfer note, with the product described in full.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteLine {
    pub product_name: String,
    pub generic_name: Option<String>,
    pub company: Option<String>,
    pub category: Option<String>,
    pub kind: String,
    pub sell_mode: String,
    pub base_unit: String,
    pub price_per_kg: Option<i64>,
    pub tax_rate_bp: i64,
    pub rx_required: bool,
    pub cold_chain: bool,
    pub units: Vec<NoteUnit>,
    pub barcodes: Vec<NoteBarcode>,
    pub batch_no: String,
    pub expiry: Option<String>,
    /// Base units sent.
    pub qty: i64,
    pub cost_per_base: i64,
    /// Prices printed on this batch, by pack name.
    pub batch_prices: Vec<(String, i64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteUnit {
    pub name: String,
    pub name_ur: Option<String>,
    pub base_qty: i64,
    pub price: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteBarcode {
    pub code: String,
    pub unit_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferNote {
    /// Marks the file as a transfer note.
    pub kind: String,
    pub transfer_id: String,
    pub transfer_no: String,
    pub from_code: String,
    pub from_name: String,
    pub to_code: String,
    pub sent_at: String,
    pub sent_by: String,
    pub note: Option<String>,
    pub lines: Vec<NoteLine>,
}

pub const NOTE_KIND: &str = "medstore-transfer";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferLineView {
    pub product_name: String,
    pub batch_no: String,
    pub expiry: Option<String>,
    pub sent_qty: i64,
    pub received_qty: Option<i64>,
    pub base_unit: String,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferView {
    pub id: String,
    pub transfer_no: String,
    pub direction: String,
    pub other_code: String,
    pub other_name: Option<String>,
    pub status: String,
    pub note: Option<String>,
    pub user_name: String,
    pub created_at: String,
    pub lines: Vec<TransferLineView>,
    /// For sent transfers: the note to hand over (file or cloud).
    pub payload: Option<TransferNote>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendLine {
    pub batch_id: String,
    /// Base units.
    pub qty: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendInput {
    pub to_code: String,
    pub to_name: Option<String>,
    pub note: Option<String>,
    pub lines: Vec<SendLine>,
}

fn note_line(conn: &Connection, batch_id: &str, qty: i64) -> AppResult<(String, NoteLine)> {
    let (product_id, batch_no, expiry, cost, on_hand): (String, String, Option<String>, i64, i64) = conn
        .query_row(
            "SELECT product_id, batch_no, expiry, cost_per_base, qty_on_hand FROM batches WHERE id = ?1",
            [batch_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("A batch on this transfer no longer exists.".into()))?;
    let p = conn.query_row(
        "SELECT name, generic_name, company, category, kind, sell_mode, base_unit, price_per_kg, tax_rate_bp,
                rx_required, cold_chain FROM products WHERE id = ?1",
        [&product_id],
        |r| {
            Ok(NoteLine {
                product_name: r.get(0)?,
                generic_name: r.get(1)?,
                company: r.get(2)?,
                category: r.get(3)?,
                kind: r.get(4)?,
                sell_mode: r.get(5)?,
                base_unit: r.get(6)?,
                price_per_kg: r.get(7)?,
                tax_rate_bp: r.get(8)?,
                rx_required: r.get(9)?,
                cold_chain: r.get(10)?,
                units: Vec::new(),
                barcodes: Vec::new(),
                batch_no: batch_no.clone(),
                expiry: expiry.clone(),
                qty,
                cost_per_base: cost,
                batch_prices: Vec::new(),
            })
        },
    )?;
    if expiry.as_deref().is_some_and(|e| e < db::current_month().as_str()) {
        return Err(AppError::BadRequest(format!(
            "{} batch {batch_no} has expired and can't be sent.",
            p.product_name
        ))
        .because("transfer.expired", json!({ "product": p.product_name, "batch": batch_no })));
    }
    if qty <= 0 || qty > on_hand {
        return Err(AppError::BadRequest(format!(
            "{} batch {batch_no}: only {on_hand} {} in stock.",
            p.product_name, p.base_unit
        ))
        .because("transfer.qty", json!({ "product": p.product_name, "batch": batch_no, "have": on_hand })));
    }
    let mut line = p;
    line.units = conn
        .prepare("SELECT name, name_ur, base_qty, price FROM product_units WHERE product_id = ?1 AND sellable = 1 ORDER BY base_qty")?
        .query_map([&product_id], |r| Ok(NoteUnit { name: r.get(0)?, name_ur: r.get(1)?, base_qty: r.get(2)?, price: r.get(3)? }))?
        .collect::<Result<_, _>>()?;
    line.barcodes = conn
        .prepare("SELECT b.code, u.name FROM barcodes b LEFT JOIN product_units u ON u.id = b.unit_id WHERE b.product_id = ?1")?
        .query_map([&product_id], |r| Ok(NoteBarcode { code: r.get(0)?, unit_name: r.get(1)? }))?
        .collect::<Result<_, _>>()?;
    line.batch_prices = conn
        .prepare("SELECT u.name, bp.price FROM batch_unit_prices bp JOIN product_units u ON u.id = bp.unit_id WHERE bp.batch_id = ?1")?
        .query_map([batch_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok((product_id, line))
}

/// Takes the stock off this store's shelves and writes the transfer note.
pub fn send(conn: &mut Connection, user: &User, input: &SendInput) -> AppResult<TransferView> {
    let to_code = input.to_code.trim().to_uppercase();
    let own = sales::store_header(conn)?;
    if to_code.is_empty() || to_code == own.code {
        return Err(
            AppError::BadRequest("Choose the branch the stock is going to.".into()).because("transfer.to", json!({}))
        );
    }
    if input.lines.is_empty() {
        return Err(AppError::BadRequest("Add at least one item.".into()).because("transfer.empty", json!({})));
    }
    let tx = conn.transaction()?;
    let n: i64 = tx.query_row("SELECT COUNT(*) FROM transfers WHERE direction = 'out'", [], |r| r.get(0))?;
    let id = db::new_id();
    let transfer_no = format!("{}-TR-{:04}", own.code, n + 1);
    let now = db::now();
    let note = input.note.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    tx.execute(
        "INSERT INTO transfers (id, transfer_no, direction, other_code, other_name, status, note, user_id, created_at)
         VALUES (?1, ?2, 'out', ?3, ?4, 'sent', ?5, ?6, ?7)",
        params![
            id,
            transfer_no,
            to_code,
            input.to_name.as_deref().map(str::trim).filter(|s| !s.is_empty()),
            note,
            user.id,
            now
        ],
    )?;
    let mut lines = Vec::new();
    for (i, l) in input.lines.iter().enumerate() {
        let (product_id, line) = note_line(&tx, &l.batch_id, l.qty)?;
        tx.execute("UPDATE batches SET qty_on_hand = qty_on_hand - ?1 WHERE id = ?2", params![l.qty, l.batch_id])?;
        tx.execute(
            "INSERT INTO stock_moves (id, product_id, batch_id, qty_delta, reason, ref_id, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, 'transfer_out', ?5, ?6, ?7)",
            params![db::new_id(), product_id, l.batch_id, -l.qty, id, user.id, now],
        )?;
        tx.execute(
            "INSERT INTO transfer_lines (id, transfer_id, line_no, product_id, batch_no, expiry, sent_qty, cost_per_base)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![db::new_id(), id, i as i64 + 1, product_id, line.batch_no, line.expiry, l.qty, line.cost_per_base],
        )?;
        lines.push(line);
    }
    let payload = TransferNote {
        kind: NOTE_KIND.into(),
        transfer_id: id.clone(),
        transfer_no: transfer_no.clone(),
        from_code: own.code.clone(),
        from_name: own.name.clone(),
        to_code: to_code.clone(),
        sent_at: now.clone(),
        sent_by: user.name.clone(),
        note: note.clone(),
        lines,
    };
    tx.execute(
        "UPDATE transfers SET payload = ?1 WHERE id = ?2",
        params![serde_json::to_string(&payload).unwrap_or_default(), id],
    )?;
    db::audit(
        &tx,
        Some(&user.id),
        None,
        "transfer.send",
        &json!({ "transferNo": transfer_no, "to": to_code, "lines": input.lines.len() }),
        None,
    )?;
    tx.commit()?;
    view(conn, &id)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Received {
    /// Base units that actually arrived, per note line (same order).
    pub qty: i64,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveInput {
    pub note: TransferNote,
    pub received: Vec<Received>,
}

/// Finds this store's product for a note line: same barcode, else same
/// name. Only a product on sale, counted in the same unit, can take the stock.
fn match_product(conn: &Connection, l: &NoteLine) -> AppResult<Option<String>> {
    for b in &l.barcodes {
        let hit: Option<String> = conn
            .query_row(
                "SELECT b.product_id FROM barcodes b JOIN products p ON p.id = b.product_id
                 WHERE b.code = ?1 AND p.active = 1 AND p.base_unit = ?2 COLLATE NOCASE AND p.sell_mode = ?3",
                params![b.code, l.base_unit, l.sell_mode],
                |r| r.get(0),
            )
            .optional()?;
        if hit.is_some() {
            return Ok(hit);
        }
    }
    Ok(conn
        .query_row(
            "SELECT id FROM products WHERE name = ?1 COLLATE NOCASE AND active = 1
                AND base_unit = ?2 COLLATE NOCASE AND sell_mode = ?3",
            params![l.product_name, l.base_unit, l.sell_mode],
            |r| r.get(0),
        )
        .optional()?)
}

/// Adds the stock that arrived. Products this store has never had are
/// created from the note. Receiving the same note twice does nothing.
pub fn receive(conn: &mut Connection, user: &User, input: &ReceiveInput) -> AppResult<TransferView> {
    let n = &input.note;
    if n.kind != NOTE_KIND {
        return Err(
            AppError::BadRequest("That file isn't a transfer note.".into()).because("transfer.bad_file", json!({}))
        );
    }
    let own = sales::store_header(conn)?;
    if n.to_code != own.code {
        return Err(AppError::Conflict(format!("This transfer is for store {}, not {}.", n.to_code, own.code))
            .because("transfer.wrong_store", json!({ "to": n.to_code })));
    }
    if let Some(done) = conn
        .query_row("SELECT id FROM transfers WHERE source_id = ?1", [&n.transfer_id], |r| r.get::<_, String>(0))
        .optional()?
    {
        return view(conn, &done);
    }
    if input.received.len() != n.lines.len()
        || input.received.iter().zip(&n.lines).any(|(r, l)| r.qty < 0 || r.qty > l.qty)
    {
        return Err(AppError::BadRequest("Received quantities must be between 0 and what was sent.".into())
            .because("transfer.received", json!({})));
    }
    let tx = conn.transaction()?;
    let id = db::new_id();
    let now = db::now();
    tx.execute(
        "INSERT INTO transfers (id, transfer_no, direction, other_code, other_name, status, note, source_id, user_id, created_at)
         VALUES (?1, ?2, 'in', ?3, ?4, 'received', ?5, ?6, ?7, ?8)",
        params![id, n.transfer_no, n.from_code, n.from_name, n.note, n.transfer_id, user.id, now],
    )?;
    for (i, (l, r)) in n.lines.iter().zip(&input.received).enumerate() {
        let product_id = match match_product(&tx, l)? {
            Some(p) => p,
            None => products::insert_product(
                &tx,
                &NewProduct {
                    name: l.product_name.clone(),
                    generic_name: l.generic_name.clone(),
                    company: l.company.clone(),
                    category: l.category.clone(),
                    kind: l.kind.clone(),
                    sell_mode: l.sell_mode.clone(),
                    base_unit: l.base_unit.clone(),
                    price_per_kg: l.price_per_kg,
                    tax_rate_bp: l.tax_rate_bp,
                    rx_required: l.rx_required,
                    controlled: false,
                    cold_chain: l.cold_chain,
                    rack: None,
                    reorder_level: 0,
                    units: l
                        .units
                        .iter()
                        .map(|u| NewUnit {
                            name: u.name.clone(),
                            name_ur: u.name_ur.clone(),
                            base_qty: u.base_qty,
                            price: u.price,
                        })
                        .collect(),
                    // Barcodes already used here by another product are left off.
                    barcodes: l
                        .barcodes
                        .iter()
                        .filter(|b| {
                            tx.query_row("SELECT 1 FROM barcodes WHERE code = ?1", [&b.code], |_| Ok(()))
                                .optional()
                                .ok()
                                .flatten()
                                .is_none()
                        })
                        .map(|b| NewBarcode { code: b.code.clone(), unit_name: b.unit_name.clone() })
                        .collect(),
                },
                &user.id,
            )?,
        };
        if r.qty > 0 {
            let batch_id = add_to_batch(&tx, &product_id, &l.batch_no, l.expiry.as_deref(), r.qty, l.cost_per_base)?;
            for (unit_name, price) in &l.batch_prices {
                let unit_id: Option<String> = tx
                    .query_row(
                        "SELECT id FROM product_units WHERE product_id = ?1 AND name = ?2 COLLATE NOCASE",
                        params![product_id, unit_name],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(u) = unit_id {
                    tx.execute(
                        "INSERT INTO batch_unit_prices (batch_id, unit_id, price) VALUES (?1, ?2, ?3)
                         ON CONFLICT (batch_id, unit_id) DO UPDATE SET price = excluded.price",
                        params![batch_id, u, price],
                    )?;
                }
            }
            tx.execute(
                "INSERT INTO stock_moves (id, product_id, batch_id, qty_delta, reason, ref_id, user_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'transfer_in', ?5, ?6, ?7)",
                params![db::new_id(), product_id, batch_id, r.qty, id, user.id, now],
            )?;
        }
        tx.execute(
            "INSERT INTO transfer_lines (id, transfer_id, line_no, product_id, batch_no, expiry, sent_qty, received_qty,
                cost_per_base, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                db::new_id(),
                id,
                i as i64 + 1,
                product_id,
                l.batch_no,
                l.expiry,
                l.qty,
                r.qty,
                l.cost_per_base,
                r.reason.as_deref().filter(|s| !s.is_empty())
            ],
        )?;
    }
    let short: i64 = n.lines.iter().zip(&input.received).map(|(l, r)| l.qty - r.qty).filter(|d| *d > 0).count() as i64;
    db::audit(
        &tx,
        Some(&user.id),
        None,
        "transfer.receive",
        &json!({ "transferNo": n.transfer_no, "from": n.from_code, "lines": n.lines.len(), "linesShort": short }),
        None,
    )?;
    tx.commit()?;
    view(conn, &id)
}

pub fn view(conn: &Connection, id: &str) -> AppResult<TransferView> {
    let (transfer_no, direction, other_code, other_name, status, note, payload, user_name, created_at) = conn
        .query_row(
            "SELECT t.transfer_no, t.direction, t.other_code, t.other_name, t.status, t.note, t.payload, u.name, t.created_at
             FROM transfers t JOIN users u ON u.id = t.user_id WHERE t.id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("Transfer not found.".into()))?;
    let lines = conn
        .prepare(
            "SELECT p.name, l.batch_no, l.expiry, l.sent_qty, l.received_qty, p.base_unit, l.reason
             FROM transfer_lines l JOIN products p ON p.id = l.product_id WHERE l.transfer_id = ?1 ORDER BY l.line_no",
        )?
        .query_map([id], |r| {
            Ok(TransferLineView {
                product_name: r.get(0)?,
                batch_no: r.get(1)?,
                expiry: r.get(2)?,
                sent_qty: r.get(3)?,
                received_qty: r.get(4)?,
                base_unit: r.get(5)?,
                reason: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(TransferView {
        id: id.to_string(),
        transfer_no,
        direction,
        other_code,
        other_name,
        status,
        note,
        user_name,
        created_at,
        lines,
        payload: payload.and_then(|p| serde_json::from_str(&p).ok()),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferRow {
    pub id: String,
    pub transfer_no: String,
    pub direction: String,
    pub other_code: String,
    pub other_name: Option<String>,
    pub status: String,
    pub created_at: String,
    pub lines: i64,
    pub differences: i64,
}

pub fn list(conn: &Connection) -> AppResult<Vec<TransferRow>> {
    let rows = conn
        .prepare(
            "SELECT t.id, t.transfer_no, t.direction, t.other_code, t.other_name, t.status, t.created_at,
                (SELECT COUNT(*) FROM transfer_lines l WHERE l.transfer_id = t.id),
                (SELECT COUNT(*) FROM transfer_lines l WHERE l.transfer_id = t.id AND l.received_qty < l.sent_qty)
             FROM transfers t ORDER BY t.created_at DESC LIMIT 100",
        )?
        .query_map([], |r| {
            Ok(TransferRow {
                id: r.get(0)?,
                transfer_no: r.get(1)?,
                direction: r.get(2)?,
                other_code: r.get(3)?,
                other_name: r.get(4)?,
                status: r.get(5)?,
                created_at: r.get(6)?,
                lines: r.get(7)?,
                differences: r.get(8)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}
