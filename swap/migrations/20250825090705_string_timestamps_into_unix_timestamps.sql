-- Migration: Convert string timestamps to Unix timestamps
-- This migration converts the entered_at field from string format to Unix timestamp integers
-- and updates the database schema accordingly.

-- First, create a temporary table with the new schema
CREATE TABLE swap_states_new (
    id          INTEGER PRIMARY KEY autoincrement NOT NULL,
    swap_id     TEXT                NOT NULL,
    entered_at  INTEGER             NOT NULL,  -- Changed from TEXT to INTEGER
    state       TEXT                NOT NULL
);

-- Copy data from the old table to the new table, converting timestamps
INSERT INTO swap_states_new (id, swap_id, entered_at, state)
SELECT 
    id,
    swap_id,
    -- Convert string timestamp to Unix timestamp
    strftime('%s', substr(entered_at, 1, 19)) AS entered_at,
    state
FROM swap_states;

-- Drop the old table
DROP TABLE swap_states;

-- Rename the new table to the original name
ALTER TABLE swap_states_new RENAME TO swap_states;

-- Recreate the unique constraint on (state, entered_at)
CREATE UNIQUE INDEX swap_states_unique_over_state_and_timestamp ON swap_states(state, entered_at);

-- Create an index on entered_at for better performance
CREATE INDEX idx_swap_states_entered_at ON swap_states(entered_at);

-- Create an index on swap_id for better performance
CREATE INDEX idx_swap_states_swap_id ON swap_states(swap_id);

-- Verify the migration by checking a few sample records
-- This will help identify any conversion issues
SELECT 
    swap_id,
    entered_at,
    datetime(entered_at, 'unixepoch') as human_readable_timestamp,
    state
FROM swap_states 
LIMIT 5;
