-- Add HLC counter (using entered_at as logical seconds) to swap_states and backfill

-- Create new table with HLC columns
CREATE TABLE swap_states_new (
    id               INTEGER PRIMARY KEY autoincrement NOT NULL,
    swap_id          TEXT                NOT NULL,
    entered_at       INTEGER             NOT NULL,
    state            TEXT                NOT NULL,
    hlc_counter      INTEGER             NOT NULL
);

-- Backfill with deterministic counters per (swap_id, entered_at)
INSERT INTO swap_states_new (id, swap_id, entered_at, state, hlc_counter)
SELECT 
    id,
    swap_id,
    CAST(strftime('%s', substr(entered_at, 1, 19)) AS INTEGER) AS entered_at_seconds,
    state,
    (
        ROW_NUMBER() OVER (
            PARTITION BY swap_id,
            CAST(strftime('%s', substr(entered_at, 1, 19)) AS INTEGER)
            ORDER BY id
        ) - 1
    ) AS hlc_counter
FROM swap_states;

-- Replace old table
DROP TABLE swap_states;
ALTER TABLE swap_states_new RENAME TO swap_states;

-- Indices for HLC-based uniqueness and performance
CREATE UNIQUE INDEX IF NOT EXISTS swap_states_unique_hlc ON swap_states(swap_id, entered_at, hlc_counter);
CREATE INDEX IF NOT EXISTS idx_swap_states_entered_at ON swap_states(entered_at);
CREATE INDEX IF NOT EXISTS idx_swap_states_swap_id ON swap_states(swap_id);


