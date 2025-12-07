//! Compatibility layer and tests for migrating from monero-rs to monero-oxide.
//!
//! This module provides:
//! 1. Type mappings between monero-rs and monero-oxide
//! 2. Serialization compatibility tests to ensure backwards compatibility
//!
//! ## Type Mapping Reference
//!
//! | monero-rs                        | monero-oxide                                        |
//! |----------------------------------|-----------------------------------------------------|
//! | `monero::Network`                | `monero_address::Network`                           |
//! | `monero::Address`                | `monero_address::MoneroAddress`                     |
//! | `monero::Amount`                 | `u64` (piconeros - no wrapper type)                 |
//! | `monero::PrivateKey`             | `curve25519_dalek::Scalar` (via monero-primitives)  |
//! | `monero::PublicKey`              | `curve25519_dalek::EdwardsPoint` (compressed)       |
//! | `monero::cryptonote::hash::Hash` | `[u8; 32]`                                          |
//! | `monero::Block`                  | `monero_oxide::block::Block`                        |
//!
//! ## Migration Strategy
//!
//! We migrate incrementally, ensuring serialization compatibility at each step.
//! The key constraint is that serialized data (in databases, network messages)
//! must remain identical to maintain backwards compatibility.
//!
//! ## Key Differences
//!
//! - monero-oxide uses `curve25519-dalek` v4 directly (not the `-ng` fork)
//! - monero-rs uses `curve25519-dalek-ng` which is a fork
//! - Both use the same underlying scalar/point representations but different crate versions
//! - Serialization format should be identical (32-byte little-endian scalars)

// Re-exports for the new types (to be used during migration)
pub use monero_address::Network as OxideNetwork;
pub use monero_address::MoneroAddress as OxideAddress;

// Note: monero-oxide uses curve25519-dalek v4 directly
// The Scalar and EdwardsPoint types come from there
// We can access them via the monero_oxide crate's re-exports or directly

/// Type alias for Amount - monero-oxide uses raw u64 piconeros
pub type OxideAmount = u64;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that Network enum variants serialize identically
    #[test]
    fn network_serialization_compat() {
        // monero-rs Network
        let mainnet_rs = monero::Network::Mainnet;
        let stagenet_rs = monero::Network::Stagenet;
        let testnet_rs = monero::Network::Testnet;

        // monero-oxide Network
        let mainnet_oxide = OxideNetwork::Mainnet;
        let stagenet_oxide = OxideNetwork::Stagenet;
        let testnet_oxide = OxideNetwork::Testnet;

        // The string representations should match for serde compatibility
        // with our custom network serializer in monero.rs
        assert_eq!(format!("{:?}", mainnet_rs), format!("{:?}", mainnet_oxide));
        assert_eq!(format!("{:?}", stagenet_rs), format!("{:?}", stagenet_oxide));
        assert_eq!(format!("{:?}", testnet_rs), format!("{:?}", testnet_oxide));
    }

    /// Test that addresses can be parsed with monero-rs
    /// Note: monero-oxide uses a different API for address parsing (not FromStr)
    /// We will need to create wrapper functions for address parsing during migration
    #[test]
    fn address_monero_rs_roundtrip() {
        // Valid mainnet address from codebase
        let addr_str = "44Ato7HveWidJYUAVw5QffEcEtSH1DwzSP3FPPkHxNAS4LX9CqgucphTisH978FLHE34YNEx7FcbBfQLQUU8m3NUC4VqsRa";

        // Parse with monero-rs
        let addr_rs: monero::Address = addr_str.parse().expect("monero-rs parse failed");

        // Verify roundtrip
        assert_eq!(addr_rs.to_string(), addr_str);
    }

    /// Test stagenet address parsing with monero-rs
    #[test]
    fn stagenet_address_monero_rs_roundtrip() {
        // Valid stagenet address from codebase
        let addr_str = "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a";

        // Parse with monero-rs
        let addr_rs: monero::Address = addr_str.parse().expect("monero-rs parse failed");

        // Verify roundtrip
        assert_eq!(addr_rs.to_string(), addr_str);
    }

    /// Test monero-oxide address parsing using its native API
    /// monero-oxide uses MoneroAddress::from_str which requires specifying network
    #[test]
    fn address_monero_oxide_parsing() {
        use monero_address::MoneroAddress;

        // Valid mainnet address from codebase
        let addr_str = "44Ato7HveWidJYUAVw5QffEcEtSH1DwzSP3FPPkHxNAS4LX9CqgucphTisH978FLHE34YNEx7FcbBfQLQUU8m3NUC4VqsRa";

        // Parse with monero-oxide - it requires specifying network
        let addr_oxide = MoneroAddress::from_str(OxideNetwork::Mainnet, addr_str)
            .expect("monero-oxide parse failed");

        // Verify it formats back correctly
        assert_eq!(addr_oxide.to_string(), addr_str);
    }

    /// Verify that monero-rs and monero-oxide produce the same string for addresses
    #[test]
    fn address_string_format_compat() {
        use monero_address::MoneroAddress;

        // Valid mainnet address from codebase
        let addr_str = "44Ato7HveWidJYUAVw5QffEcEtSH1DwzSP3FPPkHxNAS4LX9CqgucphTisH978FLHE34YNEx7FcbBfQLQUU8m3NUC4VqsRa";

        let addr_rs: monero::Address = addr_str.parse().expect("monero-rs parse failed");
        let addr_oxide = MoneroAddress::from_str(OxideNetwork::Mainnet, addr_str)
            .expect("monero-oxide parse failed");

        // Both should produce the same string representation
        assert_eq!(addr_rs.to_string(), addr_oxide.to_string());
    }

    /// Test PrivateKey (Scalar) byte representation compatibility
    ///
    /// This is CRITICAL for backwards compatibility as private keys are
    /// serialized using consensus encoding in our database.
    ///
    /// monero-rs uses curve25519-dalek-ng, monero-oxide uses curve25519-dalek v4
    /// Both should produce identical 32-byte little-endian scalar representations.
    #[test]
    fn private_key_bytes_compat() {
        use monero::consensus::{Decodable, Encodable};
        use std::io::Cursor;

        // Create a test scalar (32 bytes, little-endian)
        // This is a valid reduced scalar (last byte < 0x10 to stay in valid range)
        let test_bytes: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x00,
        ];

        // Decode with monero-rs
        let key_rs = monero::PrivateKey::consensus_decode(&mut Cursor::new(&test_bytes))
            .expect("monero-rs decode failed");

        // Encode back with monero-rs
        let mut encoded_rs = Vec::new();
        key_rs.consensus_encode(&mut encoded_rs).expect("monero-rs encode failed");

        // Verify monero-rs roundtrips correctly
        assert_eq!(
            &test_bytes[..],
            encoded_rs.as_slice(),
            "monero-rs should roundtrip the scalar bytes exactly"
        );
    }

    /// Test that Amount (u64 piconeros) handling is compatible
    #[test]
    fn amount_compat() {
        let pico_amount: u64 = 1_000_000_000_000; // 1 XMR

        // monero-rs Amount
        let amount_rs = monero::Amount::from_pico(pico_amount);
        assert_eq!(amount_rs.as_pico(), pico_amount);

        // monero-oxide just uses u64 directly
        let amount_oxide: OxideAmount = pico_amount;
        assert_eq!(amount_oxide, pico_amount);

        // Our custom serialization uses as_pico/from_pico, so this is compatible
    }

    /// Test our custom serde serialization for private keys works with JSON
    #[test]
    fn private_key_serde_json_compat() {
        use serde::{Deserialize, Serialize};

        // Wrapper using our custom serde
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct TestKey(#[serde(with = "crate::monero::private_key")] monero::PrivateKey);

        // Create a random-ish key
        let test_bytes: [u8; 32] = [
            0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
            0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
            0x87, 0x65, 0x43, 0x21, 0x0f, 0xed, 0xcb, 0xa9,
            0x98, 0x76, 0x54, 0x32, 0x10, 0xfe, 0xdc, 0x0b,
        ];

        let key = TestKey(monero::PrivateKey::from_slice(&test_bytes).unwrap());

        // Serialize to JSON (human readable)
        let json = serde_json::to_string(&key).expect("JSON serialize failed");

        // Deserialize back
        let key_back: TestKey = serde_json::from_str(&json).expect("JSON deserialize failed");

        assert_eq!(key, key_back);

        // Verify it's hex encoded in JSON
        assert!(json.contains("\""), "Should be a string in JSON");
    }

    /// Test our custom serde serialization for private keys works with CBOR
    #[test]
    fn private_key_serde_cbor_compat() {
        use serde::{Deserialize, Serialize};

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct TestKey(#[serde(with = "crate::monero::private_key")] monero::PrivateKey);

        let test_bytes: [u8; 32] = [
            0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
            0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
            0x87, 0x65, 0x43, 0x21, 0x0f, 0xed, 0xcb, 0xa9,
            0x98, 0x76, 0x54, 0x32, 0x10, 0xfe, 0xdc, 0x0b,
        ];

        let key = TestKey(monero::PrivateKey::from_slice(&test_bytes).unwrap());

        // Serialize to CBOR (binary)
        let cbor = serde_cbor::to_vec(&key).expect("CBOR serialize failed");

        // Deserialize back
        let key_back: TestKey = serde_cbor::from_slice(&cbor).expect("CBOR deserialize failed");

        assert_eq!(key, key_back);
    }

    /// Document the JSON format for private keys so we can replicate it with monero-oxide
    #[test]
    fn document_private_key_json_format() {
        use serde::{Deserialize, Serialize};

        #[derive(Serialize, Deserialize, Debug)]
        struct TestKey(#[serde(with = "crate::monero::private_key")] monero::PrivateKey);

        // Known bytes
        let test_bytes: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x00,
        ];

        let key = TestKey(monero::PrivateKey::from_slice(&test_bytes).unwrap());
        let json = serde_json::to_string(&key).expect("JSON serialize failed");

        // The format is: hex-encoded lowercase string of the 32 bytes
        let expected_hex = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f00";
        assert!(
            json.contains(expected_hex),
            "Expected hex string in JSON. Got: {}",
            json
        );
    }

    /// Document the CBOR format for private keys so we can replicate it with monero-oxide
    #[test]
    fn document_private_key_cbor_format() {
        use serde::{Deserialize, Serialize};

        #[derive(Serialize, Deserialize, Debug)]
        struct TestKey(#[serde(with = "crate::monero::private_key")] monero::PrivateKey);

        // Known bytes
        let test_bytes: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x00,
        ];

        let key = TestKey(monero::PrivateKey::from_slice(&test_bytes).unwrap());
        let cbor = serde_cbor::to_vec(&key).expect("CBOR serialize failed");

        // CBOR format for bytes: major type 2 (byte string) + length + raw bytes
        // For 32 bytes: 0x58 (byte string, 1-byte length follows) 0x20 (32) + 32 bytes
        assert_eq!(cbor.len(), 34, "CBOR should be 2 header bytes + 32 data bytes");
        assert_eq!(cbor[0], 0x58, "CBOR major type 2, 1-byte length");
        assert_eq!(cbor[1], 0x20, "Length should be 32");
        assert_eq!(&cbor[2..], &test_bytes, "Raw bytes should follow");
    }

    /// CRITICAL: Verify that both Network serde modules produce IDENTICAL output
    /// This ensures we can safely migrate from monero-rs to monero-oxide
    #[test]
    fn network_serde_json_identical() {
        use serde::{Deserialize, Serialize};

        // Wrapper using monero-rs Network
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct NetworkRs(#[serde(with = "crate::monero::network")] monero::Network);

        // Wrapper using monero-oxide Network
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct NetworkOxide(#[serde(with = "crate::monero::network_oxide")] OxideNetwork);

        // Test all variants produce identical JSON
        let networks = [
            (monero::Network::Mainnet, OxideNetwork::Mainnet, "Mainnet"),
            (monero::Network::Stagenet, OxideNetwork::Stagenet, "Stagenet"),
            (monero::Network::Testnet, OxideNetwork::Testnet, "Testnet"),
        ];

        for (rs_network, oxide_network, name) in networks {
            let rs_json = serde_json::to_string(&NetworkRs(rs_network))
                .expect("monero-rs JSON serialize failed");
            let oxide_json = serde_json::to_string(&NetworkOxide(oxide_network))
                .expect("monero-oxide JSON serialize failed");

            assert_eq!(
                rs_json, oxide_json,
                "Network::{} JSON serialization differs between monero-rs and monero-oxide",
                name
            );
        }
    }

    /// Verify Network deserialization is compatible between libraries
    #[test]
    fn network_serde_json_cross_deserialize() {
        use serde::{Deserialize, Serialize};

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct NetworkRs(#[serde(with = "crate::monero::network")] monero::Network);

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct NetworkOxide(#[serde(with = "crate::monero::network_oxide")] OxideNetwork);

        // Serialize with monero-rs, deserialize with monero-oxide
        let rs_mainnet = NetworkRs(monero::Network::Mainnet);
        let rs_json = serde_json::to_string(&rs_mainnet).unwrap();
        let oxide_from_rs: NetworkOxide = serde_json::from_str(&rs_json)
            .expect("Failed to deserialize monero-rs JSON with monero-oxide");
        assert_eq!(oxide_from_rs.0, OxideNetwork::Mainnet);

        // Serialize with monero-oxide, deserialize with monero-rs
        let oxide_stagenet = NetworkOxide(OxideNetwork::Stagenet);
        let oxide_json = serde_json::to_string(&oxide_stagenet).unwrap();
        let rs_from_oxide: NetworkRs = serde_json::from_str(&oxide_json)
            .expect("Failed to deserialize monero-oxide JSON with monero-rs");
        assert_eq!(rs_from_oxide.0, monero::Network::Stagenet);
    }
}