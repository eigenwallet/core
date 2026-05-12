//! Compatibility functions for converting between different Monero type representations.
use anyhow::{Context, Result};
use swap_core::monero::primitives::TxHash;

/// Convert a TxHash (hex string) to a 32-byte array.
pub fn tx_hash_to_bytes(tx_hash: &TxHash) -> Result<[u8; 32]> {
    hex::decode(&tx_hash.0)
        .context("Failed to decode tx_hash from hex")?
        .try_into()
        .map_err(|v: Vec<u8>| {
            anyhow::anyhow!(
                "tx_hash has wrong length: expected 32 bytes, got {}",
                v.len()
            )
        })
}
