//! Type bridge for monero-oxide types.
//!
//! This module provides:
//! 1. Re-exports of monero-oxide types for use in the codebase
//! 2. Conversion functions between monero-rs and monero-oxide types
//!
//! ## Usage
//!
//! During migration, use this module to convert between libraries:
//!
//! ```ignore
//! use swap_core::monero::oxide::{self, Network, Address};
//!
//! // Convert from monero-rs to monero-oxide
//! let oxide_network = oxide::convert::network_to_oxide(monero_rs_network);
//!
//! // Convert from monero-oxide to monero-rs  
//! let rs_network = oxide::convert::network_from_oxide(oxide_network);
//! ```

// =============================================================================
// Re-exports from monero-oxide
// =============================================================================

/// Network type from monero-oxide
pub use monero_address::Network;

/// Address type from monero-oxide
pub use monero_address::MoneroAddress as Address;

/// Scalar type for private keys (curve25519-dalek v4, the real one not -ng)
pub use curve25519_dalek_real::scalar::Scalar;

/// Edwards point type for public keys (curve25519-dalek v4)
pub use curve25519_dalek_real::edwards::EdwardsPoint;

/// Compressed Edwards point (32 bytes)
pub use curve25519_dalek_real::edwards::CompressedEdwardsY;

// =============================================================================
// Conversion functions between monero-rs and monero-oxide
// =============================================================================

pub mod convert {
    use super::*;
    use anyhow::{Context, Result};

    // =========================================================================
    // Network conversions
    // =========================================================================

    /// Convert monero-rs Network to monero-oxide Network
    pub fn network_to_oxide(n: monero::Network) -> Network {
        match n {
            monero::Network::Mainnet => Network::Mainnet,
            monero::Network::Stagenet => Network::Stagenet,
            monero::Network::Testnet => Network::Testnet,
        }
    }

    /// Convert monero-oxide Network to monero-rs Network
    pub fn network_from_oxide(n: Network) -> monero::Network {
        match n {
            Network::Mainnet => monero::Network::Mainnet,
            Network::Stagenet => monero::Network::Stagenet,
            Network::Testnet => monero::Network::Testnet,
        }
    }

    // =========================================================================
    // Address conversions
    // =========================================================================

    /// Convert monero-rs Address to monero-oxide Address
    ///
    /// This uses string conversion which is verified to be compatible.
    pub fn address_to_oxide(addr: &monero::Address) -> Result<Address> {
        let network = network_to_oxide(addr.network);
        Address::from_str(network, &addr.to_string())
            .with_context(|| format!("Failed to convert address to oxide: {}", addr))
    }

    /// Convert monero-oxide Address to monero-rs Address
    ///
    /// This uses string conversion which is verified to be compatible.
    pub fn address_from_oxide(addr: &Address) -> Result<monero::Address> {
        addr.to_string()
            .parse()
            .with_context(|| format!("Failed to convert oxide address to monero-rs: {}", addr))
    }

    // =========================================================================
    // Scalar (PrivateKey) conversions
    // =========================================================================

    /// Convert monero-rs PrivateKey to curve25519-dalek Scalar
    ///
    /// Both use the same 32-byte little-endian representation.
    pub fn private_key_to_oxide(key: &monero::PrivateKey) -> Scalar {
        Scalar::from_bytes_mod_order(key.to_bytes())
    }

    /// Convert curve25519-dalek Scalar to monero-rs PrivateKey
    ///
    /// Both use the same 32-byte little-endian representation.
    pub fn private_key_from_oxide(scalar: &Scalar) -> Result<monero::PrivateKey> {
        monero::PrivateKey::from_slice(&scalar.to_bytes())
            .with_context(|| "Failed to convert oxide scalar to monero-rs PrivateKey")
    }

    // =========================================================================
    // PublicKey conversions  
    // =========================================================================

    /// Convert monero-rs PublicKey to curve25519-dalek CompressedEdwardsY
    ///
    /// Both use the same 32-byte compressed point representation.
    pub fn public_key_to_oxide(key: &monero::PublicKey) -> CompressedEdwardsY {
        let bytes: [u8; 32] = key.as_bytes().try_into().expect("PublicKey is always 32 bytes");
        CompressedEdwardsY(bytes)
    }

    /// Convert curve25519-dalek CompressedEdwardsY to monero-rs PublicKey
    ///
    /// Both use the same 32-byte compressed point representation.
    pub fn public_key_from_oxide(point: &CompressedEdwardsY) -> Result<monero::PublicKey> {
        monero::PublicKey::from_slice(&point.0)
            .with_context(|| "Failed to convert oxide point to monero-rs PublicKey")
    }

    // =========================================================================
    // Amount conversions (trivial - both use u64 piconeros)
    // =========================================================================

    /// Convert monero-rs Amount to u64 piconeros
    #[inline]
    pub fn amount_to_oxide(amount: monero::Amount) -> u64 {
        amount.as_pico()
    }

    /// Convert u64 piconeros to monero-rs Amount
    #[inline]
    pub fn amount_from_oxide(pico: u64) -> monero::Amount {
        monero::Amount::from_pico(pico)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::convert::*;

    #[test]
    fn network_roundtrip() {
        let networks = [
            monero::Network::Mainnet,
            monero::Network::Stagenet,
            monero::Network::Testnet,
        ];

        for network in networks {
            let oxide = network_to_oxide(network);
            let back = network_from_oxide(oxide);
            assert_eq!(network, back);
        }
    }

    #[test]
    fn address_roundtrip() {
        let addr_str = "44Ato7HveWidJYUAVw5QffEcEtSH1DwzSP3FPPkHxNAS4LX9CqgucphTisH978FLHE34YNEx7FcbBfQLQUU8m3NUC4VqsRa";
        let addr: monero::Address = addr_str.parse().unwrap();

        let oxide = address_to_oxide(&addr).unwrap();
        let back = address_from_oxide(&oxide).unwrap();

        assert_eq!(addr.to_string(), back.to_string());
    }

    #[test]
    fn private_key_roundtrip() {
        let bytes: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x00,
        ];

        let key = monero::PrivateKey::from_slice(&bytes).unwrap();
        let oxide = private_key_to_oxide(&key);
        let back = private_key_from_oxide(&oxide).unwrap();

        assert_eq!(key.to_bytes(), back.to_bytes());
    }

    #[test]
    fn public_key_roundtrip() {
        // Generate a public key from a private key
        let bytes: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x00,
        ];

        let private_key = monero::PrivateKey::from_slice(&bytes).unwrap();
        let public_key = monero::PublicKey::from_private_key(&private_key);

        let oxide = public_key_to_oxide(&public_key);
        let back = public_key_from_oxide(&oxide).unwrap();

        assert_eq!(public_key.as_bytes(), back.as_bytes());
    }

    #[test]
    fn amount_roundtrip() {
        let amounts = [0u64, 1, 1_000_000_000_000, u64::MAX];

        for pico in amounts {
            let amount = monero::Amount::from_pico(pico);
            let oxide = amount_to_oxide(amount);
            let back = amount_from_oxide(oxide);
            assert_eq!(amount.as_pico(), back.as_pico());
        }
    }
}

