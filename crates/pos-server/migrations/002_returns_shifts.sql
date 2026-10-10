-- Milestone 2: returns, cash in/out, shift close, approval rules.

CREATE TABLE returns (
    id             TEXT PRIMARY KEY,
    return_no      TEXT NOT NULL UNIQUE,
    client_ref     TEXT UNIQUE,
    sale_id        TEXT NOT NULL REFERENCES sales (id),
    shift_id       TEXT REFERENCES shifts (id),
    till_code      TEXT NOT NULL,
    user_id        TEXT NOT NULL REFERENCES users (id),
    created_at     TEXT NOT NULL,
    refund_total   INTEGER NOT NULL,
    refund_method  TEXT NOT NULL CHECK (refund_method IN ('cash', 'khata', 'card', 'jazzcash', 'easypaisa')),
    approved_by    TEXT REFERENCES users (id)
);
CREATE INDEX idx_returns_sale ON returns (sale_id);

CREATE TABLE return_lines (
    id            TEXT PRIMARY KEY,
    return_id     TEXT NOT NULL REFERENCES returns (id),
    sale_line_id  TEXT NOT NULL REFERENCES sale_lines (id),
    qty           INTEGER NOT NULL CHECK (qty > 0),   -- same units as the sale line
    base_qty      INTEGER NOT NULL,
    refund        INTEGER NOT NULL,
    condition     TEXT NOT NULL CHECK (condition IN ('restock', 'damaged', 'expired')),
    reason        TEXT
);
CREATE INDEX idx_return_lines_sale_line ON return_lines (sale_line_id);

-- Cash put into or taken out of the drawer during a shift (change float,
-- generator fuel, tea...).
CREATE TABLE cash_moves (
    id           TEXT PRIMARY KEY,
    shift_id     TEXT NOT NULL REFERENCES shifts (id),
    user_id      TEXT NOT NULL REFERENCES users (id),
    kind         TEXT NOT NULL CHECK (kind IN ('in', 'out')),
    amount       INTEGER NOT NULL CHECK (amount > 0),
    reason       TEXT NOT NULL,
    approved_by  TEXT REFERENCES users (id),
    created_at   TEXT NOT NULL
);

ALTER TABLE shifts ADD COLUMN count_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE shifts ADD COLUMN denominations TEXT;      -- JSON {"5000": 3, ...} of the last count
ALTER TABLE shifts ADD COLUMN closed_by TEXT REFERENCES users (id);
