-- Add migration script here
-- Add migration script here

CREATE TABLE change (
    peer_id TEXT NOT NULL,
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    change BLOB NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_change_unique ON change(peer_id, change);