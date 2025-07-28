-- Adds a nullable and unique name column to the recent_wallets table

ALTER TABLE recent_wallets ADD COLUMN name TEXT;

-- Create unique index because sqlite doesn't support adding unique column directly
CREATE UNIQUE INDEX idx_recent_wallets_name ON recent_wallets(name) WHERE name IS NOT NULL;
