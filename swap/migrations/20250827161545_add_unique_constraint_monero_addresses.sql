-- Ensure uniqueness of (swap_id, address) including NULL-address rows

-- 3) Create null-safe unique indexes
-- Prevent duplicates when address IS NOT NULL
CREATE UNIQUE INDEX IF NOT EXISTS idx_monero_addresses_unique_nonnull
ON monero_addresses(swap_id, address)
WHERE address IS NOT NULL;

-- Prevent duplicates when address IS NULL (treat one NULL per swap_id as unique)
CREATE UNIQUE INDEX IF NOT EXISTS idx_monero_addresses_unique_null
ON monero_addresses(swap_id)
WHERE address IS NULL;
