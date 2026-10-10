-- Milestone 3: backoffice. Suppliers and purchases, expenses, store
-- settings, product editing.

CREATE TABLE suppliers (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    phone       TEXT,
    notes       TEXT,
    terms_days  INTEGER NOT NULL DEFAULT 0,   -- usual credit days; 0 = cash on delivery
    active      INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_suppliers_name ON suppliers (name COLLATE NOCASE);

-- A goods received note: stock bought from a supplier on one invoice.
CREATE TABLE purchases (
    id            TEXT PRIMARY KEY,
    purchase_no   TEXT NOT NULL UNIQUE,         -- P-00001
    client_ref    TEXT UNIQUE,
    supplier_id   TEXT NOT NULL REFERENCES suppliers (id),
    invoice_no    TEXT,
    invoice_date  TEXT NOT NULL,                -- YYYY-MM-DD
    due_date      TEXT,                         -- YYYY-MM-DD; NULL = paid on delivery
    gross         INTEGER NOT NULL,             -- qty × trade price, before line discounts
    discount      INTEGER NOT NULL DEFAULT 0,
    freight       INTEGER NOT NULL DEFAULT 0,
    net           INTEGER NOT NULL,             -- what is owed for this invoice
    paid_now      INTEGER NOT NULL DEFAULT 0,
    user_id       TEXT NOT NULL REFERENCES users (id),
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_purchases_supplier ON purchases (supplier_id, invoice_date);

CREATE TABLE purchase_lines (
    id             TEXT PRIMARY KEY,
    purchase_id    TEXT NOT NULL REFERENCES purchases (id),
    line_no        INTEGER NOT NULL,
    product_id     TEXT NOT NULL REFERENCES products (id),
    unit_name      TEXT NOT NULL,               -- pack bought in: Box, Strip, kg...
    unit_base_qty  INTEGER NOT NULL,            -- base units in one of those packs
    batch_id       TEXT NOT NULL REFERENCES batches (id),
    batch_no       TEXT NOT NULL,
    expiry         TEXT,
    qty            INTEGER NOT NULL CHECK (qty > 0),
    bonus          INTEGER NOT NULL DEFAULT 0 CHECK (bonus >= 0),
    trade_price    INTEGER NOT NULL CHECK (trade_price >= 0),   -- paisa per pack
    disc_bp        INTEGER NOT NULL DEFAULT 0,
    sale_price     INTEGER,                     -- MRP per pack printed on this batch
    line_total     INTEGER NOT NULL,            -- after line discount, before freight
    cost_per_base  INTEGER NOT NULL             -- incl. bonus and share of freight
);

-- What the store owes each supplier. amount > 0 means the store owes more
-- (a purchase), < 0 means the store paid.
CREATE TABLE supplier_entries (
    id           TEXT PRIMARY KEY,
    supplier_id  TEXT NOT NULL REFERENCES suppliers (id),
    purchase_id  TEXT REFERENCES purchases (id),
    amount       INTEGER NOT NULL,
    paid_from    TEXT,
    note         TEXT,
    user_id      TEXT REFERENCES users (id),
    created_at   TEXT NOT NULL
);
CREATE INDEX idx_supplier_entries ON supplier_entries (supplier_id, created_at);

CREATE TABLE expenses (
    id            TEXT PRIMARY KEY,
    category      TEXT NOT NULL,
    amount        INTEGER NOT NULL CHECK (amount > 0),
    expense_date  TEXT NOT NULL,                -- YYYY-MM-DD
    paid_from     TEXT NOT NULL CHECK (paid_from IN ('drawer', 'owner', 'bank', 'wallet')),
    cash_move_id  TEXT REFERENCES cash_moves (id),
    note          TEXT,
    user_id       TEXT NOT NULL REFERENCES users (id),
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_expenses_date ON expenses (expense_date);

ALTER TABLE customers ADD COLUMN active INTEGER NOT NULL DEFAULT 1;
ALTER TABLE products ADD COLUMN supplier_id TEXT REFERENCES suppliers (id);
