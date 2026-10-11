-- Milestone 6: stock counts and transfers between branches.

CREATE TABLE stock_counts (
    id           TEXT PRIMARY KEY,
    count_no     TEXT NOT NULL UNIQUE,          -- SC-0001
    section      TEXT,                          -- rack counted; NULL = whole store
    blind        INTEGER NOT NULL DEFAULT 1,    -- hide the system quantity while counting
    status       TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'applied', 'cancelled')),
    started_by   TEXT NOT NULL REFERENCES users (id),
    started_at   TEXT NOT NULL,
    finished_by  TEXT REFERENCES users (id),
    finished_at  TEXT
);

-- One row per batch on the counted shelf. The system quantity is taken
-- when that batch is counted, so sales during the count are kept.
CREATE TABLE stock_count_lines (
    id              TEXT PRIMARY KEY,
    count_id        TEXT NOT NULL REFERENCES stock_counts (id),
    product_id      TEXT NOT NULL REFERENCES products (id),
    batch_id        TEXT NOT NULL REFERENCES batches (id),
    system_qty      INTEGER,                    -- base units, when counted
    counted_qty     INTEGER,                    -- base units; NULL = not counted yet
    reason          TEXT,
    counted_at      TEXT,
    UNIQUE (count_id, batch_id)
);

CREATE TABLE transfers (
    id               TEXT PRIMARY KEY,
    transfer_no      TEXT NOT NULL,             -- B2-TR-0001 (the sender's number)
    direction        TEXT NOT NULL CHECK (direction IN ('out', 'in')),
    other_code       TEXT NOT NULL,             -- the other store's code
    other_name       TEXT,
    status           TEXT NOT NULL CHECK (status IN ('sent', 'received')),
    note             TEXT,                      -- who carried it, etc.
    source_id        TEXT UNIQUE,               -- for 'in': the sender's transfer id
    payload          TEXT,                      -- for 'out': what the receiver needs (JSON)
    user_id          TEXT NOT NULL REFERENCES users (id),
    created_at       TEXT NOT NULL
);

CREATE TABLE transfer_lines (
    id              TEXT PRIMARY KEY,
    transfer_id     TEXT NOT NULL REFERENCES transfers (id),
    line_no         INTEGER NOT NULL,
    product_id      TEXT NOT NULL REFERENCES products (id),
    batch_no        TEXT NOT NULL,
    expiry          TEXT,
    sent_qty        INTEGER NOT NULL,           -- base units
    received_qty    INTEGER,                    -- base units, for 'in'
    cost_per_base   INTEGER NOT NULL,
    reason          TEXT
);
