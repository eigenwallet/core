-- Progress of the Apollo transaction (the encrypted on-chain backup of a swap).
-- Swap start or recovery inserts the first row of a swap. The Apollo publisher appends each later transition.
-- The latest row of a swap is its current progress.
CREATE TABLE IF NOT EXISTS apollo_progress (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    swap_id TEXT NOT NULL,
    progress TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS apollo_progress_by_swap ON apollo_progress (swap_id, id);
