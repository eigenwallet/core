CREATE TABLE trades (
    path_uuid TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    from_currency TEXT NOT NULL,
    from_network TEXT NOT NULL,
    to_currency TEXT NOT NULL,
    to_network TEXT NOT NULL,
    withdraw_address TEXT NOT NULL,
    json TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_change_unique ON trades(timestamp, path_uuid);
