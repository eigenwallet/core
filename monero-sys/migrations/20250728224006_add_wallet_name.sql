-- Adds a nullable and unique wallet_name column to the recent_wallets table

ALTER TABLE recent_wallets ADD COLUMN wallet_name TEXT;

-- Create unique index because sqlite doesn't support adding unique column directly
CREATE UNIQUE INDEX idx_recent_wallets_name ON recent_wallets(wallet_name) WHERE wallet_name IS NOT NULL;
