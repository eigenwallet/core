//! Compatibility functions for converting between different Monero type representations.
use anyhow::{Context, Result};
use monero_oxide_wallet::ed25519::{Point, Scalar};
use swap_core::monero::primitives::{PrivateViewKey, TxHash};
use zeroize::Zeroizing;

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

/// Convert a monero::PublicKey to monero_oxide_wallet::ed25519::Point.
pub fn public_key_to_point(public_key: monero::PublicKey) -> Result<Point> {
    let public_bytes = public_key.as_bytes();
    let compressed = curve25519_dalek::edwards::CompressedEdwardsY::from_slice(public_bytes)
        .context("Failed to create CompressedEdwardsY from public key bytes")?;
    let edwards = compressed
        .decompress()
        .context("Failed to decompress public key")?;
    Ok(Point::from(edwards))
}

/// Convert a PrivateViewKey to Zeroizing<Scalar>.
pub fn private_view_key_to_scalar(private_view_key: PrivateViewKey) -> Result<Zeroizing<Scalar>> {
    let view_key: monero::PrivateKey = private_view_key.into();
    let view_key_bytes = view_key.to_bytes();
    let view_key_scalar = curve25519_dalek::Scalar::from_canonical_bytes(view_key_bytes)
        .into_option()
        .context("Failed to convert view key bytes to Scalar")?;
    Ok(Zeroizing::new(Scalar::from(view_key_scalar)))
}
