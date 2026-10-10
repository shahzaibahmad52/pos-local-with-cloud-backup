-- Store database. One database per store; it lives on the store's main PC.
-- Money is stored in paisa (1 Rs = 100 paisa). Stock is stored in the
-- product's smallest unit (tablet, capsule, piece, gram...).

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE stores (
    id            TEXT PRIMARY KEY,
    code          TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    address       TEXT,
    phone         TEXT,
    ntn           TEXT,
    drug_license  TEXT,
    created_at    TEXT NOT NULL
);

CREATE TABLE users (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    role         TEXT NOT NULL CHECK (role IN ('admin', 'cashier')),
    secret_hash  TEXT NOT NULL,            -- admin: password, cashier: PIN
    can_approve  INTEGER NOT NULL DEFAULT 0,
    language     TEXT NOT NULL DEFAULT 'en' CHECK (language IN ('en', 'ur')),
    active       INTEGER NOT NULL DEFAULT 1,
    created_at   TEXT NOT NULL
);

CREATE TABLE products (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    generic_name   TEXT,
    company        TEXT,
    category       TEXT,
    kind           TEXT NOT NULL CHECK (kind IN ('medicine', 'general')),
    sell_mode      TEXT NOT NULL DEFAULT 'unit' CHECK (sell_mode IN ('unit', 'weight')),
    base_unit      TEXT NOT NULL,          -- smallest unit: tab, cap, pc, btl, g ...
    price_per_kg   INTEGER,                -- paisa, only for sell_mode = 'weight'
    tax_rate_bp    INTEGER NOT NULL DEFAULT 0,   -- included in price; 1800 = 18%
    rx_required    INTEGER NOT NULL DEFAULT 0,
    controlled     INTEGER NOT NULL DEFAULT 0,
    cold_chain     INTEGER NOT NULL DEFAULT 0,
    rack           TEXT,
    reorder_level  INTEGER NOT NULL DEFAULT 0,  -- in base units
    active         INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    CHECK (sell_mode = 'unit' OR price_per_kg IS NOT NULL)
);
CREATE INDEX idx_products_name ON products (name COLLATE NOCASE);
CREATE INDEX idx_products_generic ON products (generic_name COLLATE NOCASE);

-- Pack levels a product can be sold in, e.g. Tab (1), Strip (10), Box (200).
CREATE TABLE product_units (
    id          TEXT PRIMARY KEY,
    product_id  TEXT NOT NULL REFERENCES products (id),
    name        TEXT NOT NULL,
    name_ur     TEXT,
    base_qty    INTEGER NOT NULL CHECK (base_qty > 0),
    price       INTEGER NOT NULL CHECK (price >= 0),  -- default sale price in paisa
    sellable    INTEGER NOT NULL DEFAULT 1,
    sort        INTEGER NOT NULL DEFAULT 0,
    UNIQUE (product_id, name)
);

CREATE TABLE barcodes (
    code        TEXT PRIMARY KEY,
    product_id  TEXT NOT NULL REFERENCES products (id),
    unit_id     TEXT REFERENCES product_units (id)
);

CREATE TABLE batches (
    id             TEXT PRIMARY KEY,
    product_id     TEXT NOT NULL REFERENCES products (id),
    batch_no       TEXT NOT NULL,
    expiry         TEXT,                   -- 'YYYY-MM'; usable until the end of that month
    cost_per_base  INTEGER NOT NULL DEFAULT 0,  -- paisa per base unit
    qty_on_hand    INTEGER NOT NULL DEFAULT 0,  -- base units
    received_at    TEXT NOT NULL
);
CREATE INDEX idx_batches_product ON batches (product_id, expiry);

-- Price printed on a batch's pack, when it differs from the product's default.
CREATE TABLE batch_unit_prices (
    batch_id  TEXT NOT NULL REFERENCES batches (id),
    unit_id   TEXT NOT NULL REFERENCES product_units (id),
    price     INTEGER NOT NULL CHECK (price >= 0),
    PRIMARY KEY (batch_id, unit_id)
);

CREATE TABLE stock_moves (
    id          TEXT PRIMARY KEY,
    product_id  TEXT NOT NULL REFERENCES products (id),
    batch_id    TEXT REFERENCES batches (id),
    qty_delta   INTEGER NOT NULL,
    reason      TEXT NOT NULL,   -- opening, purchase, sale, return, adjust, transfer_in, transfer_out
    ref_id      TEXT,
    user_id     TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_stock_moves_product ON stock_moves (product_id, created_at);

CREATE TABLE customers (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    phone             TEXT,
    credit_limit      INTEGER NOT NULL DEFAULT 0,   -- paisa; 0 = no khata allowed
    receipt_language  TEXT CHECK (receipt_language IN ('en', 'ur', 'both')),
    notes             TEXT,
    created_at        TEXT NOT NULL
);

CREATE TABLE shifts (
    id             TEXT PRIMARY KEY,
    till_code      TEXT NOT NULL,
    user_id        TEXT NOT NULL REFERENCES users (id),
    opened_at      TEXT NOT NULL,
    opening_cash   INTEGER NOT NULL DEFAULT 0,
    closed_at      TEXT,
    counted_cash   INTEGER,
    expected_cash  INTEGER,
    note           TEXT,
    status         TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed'))
);
CREATE UNIQUE INDEX idx_shifts_one_open_per_till ON shifts (till_code) WHERE status = 'open';

CREATE TABLE receipt_counters (
    till_code  TEXT PRIMARY KEY,
    last_no    INTEGER NOT NULL
);

CREATE TABLE sales (
    id             TEXT PRIMARY KEY,
    receipt_no     TEXT NOT NULL UNIQUE,
    client_ref     TEXT UNIQUE,            -- lets a till retry safely without double sales
    shift_id       TEXT REFERENCES shifts (id),
    till_code      TEXT NOT NULL,
    user_id        TEXT NOT NULL REFERENCES users (id),
    customer_id    TEXT REFERENCES customers (id),
    created_at     TEXT NOT NULL,
    subtotal       INTEGER NOT NULL,
    discount       INTEGER NOT NULL DEFAULT 0,
    tax_included   INTEGER NOT NULL DEFAULT 0,
    total          INTEGER NOT NULL,
    paid           INTEGER NOT NULL,
    change_due     INTEGER NOT NULL DEFAULT 0,
    approved_by    TEXT REFERENCES users (id),
    status         TEXT NOT NULL DEFAULT 'completed' CHECK (status IN ('completed', 'void'))
);
CREATE INDEX idx_sales_created ON sales (created_at);

CREATE TABLE sale_lines (
    id            TEXT PRIMARY KEY,
    sale_id       TEXT NOT NULL REFERENCES sales (id),
    line_no       INTEGER NOT NULL,
    product_id    TEXT NOT NULL REFERENCES products (id),
    unit_id       TEXT REFERENCES product_units (id),
    unit_name     TEXT NOT NULL,
    qty           INTEGER NOT NULL,        -- in unit_name units; grams for weight lines
    base_qty      INTEGER NOT NULL,
    unit_price    INTEGER NOT NULL,        -- paisa per unit; per kg for weight lines
    line_total    INTEGER NOT NULL,
    tax_included  INTEGER NOT NULL DEFAULT 0,
    cost_total    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE sale_line_batches (
    sale_line_id  TEXT NOT NULL REFERENCES sale_lines (id),
    batch_id      TEXT NOT NULL REFERENCES batches (id),
    base_qty      INTEGER NOT NULL,
    cost          INTEGER NOT NULL,
    PRIMARY KEY (sale_line_id, batch_id)
);

CREATE TABLE payments (
    id       TEXT PRIMARY KEY,
    sale_id  TEXT NOT NULL REFERENCES sales (id),
    method   TEXT NOT NULL CHECK (method IN ('cash', 'card', 'jazzcash', 'easypaisa', 'khata')),
    amount   INTEGER NOT NULL CHECK (amount > 0)
);

-- Khata (udhaar). amount > 0 means the customer owes more, < 0 means they paid.
CREATE TABLE khata_entries (
    id           TEXT PRIMARY KEY,
    customer_id  TEXT NOT NULL REFERENCES customers (id),
    sale_id      TEXT REFERENCES sales (id),
    amount       INTEGER NOT NULL,
    method       TEXT,
    note         TEXT,
    user_id      TEXT REFERENCES users (id),
    created_at   TEXT NOT NULL
);
CREATE INDEX idx_khata_customer ON khata_entries (customer_id, created_at);

-- Append-only record of sensitive actions. Never updated or deleted.
CREATE TABLE audit_log (
    id           TEXT PRIMARY KEY,
    created_at   TEXT NOT NULL,
    user_id      TEXT REFERENCES users (id),
    till_code    TEXT,
    action       TEXT NOT NULL,
    details      TEXT NOT NULL,            -- JSON
    approved_by  TEXT REFERENCES users (id)
);
CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END;

-- Changes waiting to be uploaded to the cloud (Turso). Filled now, sent by the
-- sync worker in a later milestone.
CREATE TABLE outbox (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    entity      TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    op          TEXT NOT NULL,
    payload     TEXT NOT NULL,             -- JSON
    created_at  TEXT NOT NULL,
    synced_at   TEXT
);
