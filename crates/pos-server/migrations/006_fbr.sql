-- Optional reporting of invoices to FBR through PRAL's Integrated Invoicing
-- System. When it is switched on, each sale and refund is queued here and
-- sent in the background; FBR's invoice number is kept for the receipt.

ALTER TABLE sales ADD COLUMN fbr_invoice_no TEXT;
ALTER TABLE returns ADD COLUMN fbr_invoice_no TEXT;

CREATE TABLE fbr_queue (
    doc_id       TEXT PRIMARY KEY,           -- sale or return id
    kind         TEXT NOT NULL CHECK (kind IN ('sale', 'return')),
    created_at   TEXT NOT NULL,
    attempts     INTEGER NOT NULL DEFAULT 0,
    last_error   TEXT,
    next_try_at  TEXT,
    sent_at      TEXT
);
CREATE INDEX idx_fbr_queue_waiting ON fbr_queue (sent_at, next_try_at);
