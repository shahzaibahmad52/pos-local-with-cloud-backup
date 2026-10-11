-- Retrying safely after a dropped connection.

-- A transfer sent twice (a retry, a double click) is recorded once.
ALTER TABLE transfers ADD COLUMN client_ref TEXT;
CREATE UNIQUE INDEX idx_transfers_client_ref ON transfers (client_ref) WHERE client_ref IS NOT NULL;

-- Other numbers printed for a sale. When the network dropped just as a sale
-- was saved, the till printed its own offline number for it; that number
-- still finds the sale for a return.
CREATE TABLE receipt_aliases (
    receipt_no  TEXT PRIMARY KEY,
    sale_id     TEXT NOT NULL REFERENCES sales (id)
);
