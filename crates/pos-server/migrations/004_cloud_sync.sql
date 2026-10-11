-- Milestone 4: changes waiting to be copied to the store's cloud database.
-- Triggers on every table add a row here (only while cloud backup is on);
-- the sync worker uploads the current version of each row, then deletes
-- the queue entries. See src/cloud/sync.rs.
CREATE TABLE sync_queue (
    seq  INTEGER PRIMARY KEY AUTOINCREMENT,
    tbl  TEXT NOT NULL,
    pk   TEXT NOT NULL             -- JSON array of the row's primary key values
);
