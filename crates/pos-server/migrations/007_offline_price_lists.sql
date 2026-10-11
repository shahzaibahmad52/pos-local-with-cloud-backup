-- Each product list handed to an extra till, so the prices an offline sale
-- was made at can be checked against what this PC actually gave the till.

CREATE TABLE offline_price_lists (
    id               TEXT PRIMARY KEY,
    created_at       TEXT NOT NULL,
    prices           TEXT NOT NULL,          -- {product id: {"u": {pack id: price}, "kg": price per kg}}
    max_discount_bp  INTEGER NOT NULL,
    weight_round_to  INTEGER NOT NULL
);
CREATE INDEX idx_offline_price_lists_created ON offline_price_lists (created_at);
