-- Add migration script here
CREATE TABLE trade_states (
    path_uuid TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    status_type TEXT NOT NULL,
    is_terminal INTEGER NOT NULL,
    description TEXT NOT NULL,
    valid_for_secs INTEGER NOT NULL
);
