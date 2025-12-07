use monero::{Amount, Network};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(remote = "Network")]
#[allow(non_camel_case_types)]
pub enum network {
    Mainnet,
    Stagenet,
    Testnet,
}

/// Serde module for monero_address::Network (monero-oxide)
/// This produces IDENTICAL serialization as the monero-rs version above.
pub mod network_oxide {
    use monero_address::Network;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    enum NetworkHelper {
        Mainnet,
        Stagenet,
        Testnet,
    }

    impl From<Network> for NetworkHelper {
        fn from(n: Network) -> Self {
            match n {
                Network::Mainnet => NetworkHelper::Mainnet,
                Network::Stagenet => NetworkHelper::Stagenet,
                Network::Testnet => NetworkHelper::Testnet,
            }
        }
    }

    impl From<NetworkHelper> for Network {
        fn from(n: NetworkHelper) -> Self {
            match n {
                NetworkHelper::Mainnet => Network::Mainnet,
                NetworkHelper::Stagenet => Network::Stagenet,
                NetworkHelper::Testnet => Network::Testnet,
            }
        }
    }

    pub fn serialize<S>(network: &Network, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        NetworkHelper::from(*network).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Network, D::Error>
    where
        D: Deserializer<'de>,
    {
        NetworkHelper::deserialize(deserializer).map(Network::from)
    }
}

pub mod private_key {
    use monero::consensus::{Decodable, Encodable};
    use monero::PrivateKey;
    use serde::de::Visitor;
    use serde::ser::Error;
    use serde::{de, Deserializer, Serializer};
    use std::fmt;
    use std::io::Cursor;

    struct BytesVisitor;

    impl Visitor<'_> for BytesVisitor {
        type Value = PrivateKey;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a byte array representing a Monero private key")
        }

        fn visit_bytes<E>(self, s: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let mut s = s;
            PrivateKey::consensus_decode(&mut s).map_err(|err| E::custom(format!("{err:?}")))
        }

        fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let bytes = data_encoding::HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())
                .map_err(|err| E::custom(format!("{err:?}")))?;
            PrivateKey::consensus_decode(&mut bytes.as_slice())
                .map_err(|err| E::custom(format!("{err:?}")))
        }
    }

    pub fn serialize<S>(x: &PrivateKey, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut bytes = Cursor::new(vec![]);
        x.consensus_encode(&mut bytes)
            .map_err(|err| S::Error::custom(format!("{err:?}")))?;
        if s.is_human_readable() {
            s.serialize_str(&data_encoding::HEXLOWER.encode(&bytes.into_inner()))
        } else {
            s.serialize_bytes(bytes.into_inner().as_ref())
        }
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<PrivateKey, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let key = {
            if deserializer.is_human_readable() {
                deserializer.deserialize_string(BytesVisitor)?
            } else {
                deserializer.deserialize_bytes(BytesVisitor)?
            }
        };
        Ok(key)
    }
}

pub mod amount {
    use super::*;

    pub fn serialize<S>(x: &Amount, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_u64(x.as_pico())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Amount, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let picos = u64::deserialize(deserializer)?;
        let amount = Amount::from_pico(picos);

        Ok(amount)
    }
}

pub mod address {
    use anyhow::{bail, Context, Result};
    use std::str::FromStr;

    #[derive(thiserror::Error, Debug, Clone, Copy, PartialEq)]
    #[error(
        "Invalid monero address provided, expected address on network {expected:?} but address provided is on {actual:?}"
    )]
    pub struct MoneroAddressNetworkMismatch {
        pub expected: monero::Network,
        pub actual: monero::Network,
    }

    pub fn parse(s: &str) -> Result<monero::Address> {
        monero::Address::from_str(s).with_context(|| {
            format!(
                "Failed to parse {s} as a monero address, please make sure it is a valid address",
            )
        })
    }

    pub fn validate(
        address: monero::Address,
        expected_network: monero::Network,
    ) -> Result<monero::Address> {
        if address.network != expected_network {
            bail!(MoneroAddressNetworkMismatch {
                expected: expected_network,
                actual: address.network,
            });
        }
        Ok(address)
    }

    pub fn validate_is_testnet(
        address: monero::Address,
        is_testnet: bool,
    ) -> Result<monero::Address> {
        let expected_network = if is_testnet {
            monero::Network::Stagenet
        } else {
            monero::Network::Mainnet
        };
        validate(address, expected_network)
    }
}

/// Serde module for curve25519_dalek::Scalar (monero-oxide private keys)
/// This produces IDENTICAL serialization as the monero-rs private_key module above.
///
/// Format:
/// - Human readable (JSON): lowercase hex string of 32 bytes
/// - Binary (CBOR): raw 32 bytes
pub mod private_key_oxide {
    use curve25519_dalek::Scalar;
    use serde::de::Visitor;
    use serde::{de, Deserializer, Serializer};
    use std::fmt;

    struct ScalarVisitor;

    impl Visitor<'_> for ScalarVisitor {
        type Value = Scalar;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a byte array representing a Monero private key (scalar)")
        }

        fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let arr: [u8; 32] = bytes
                .try_into()
                .map_err(|_| E::custom("expected 32 bytes for scalar"))?;
            Ok(Scalar::from_bytes_mod_order(arr))
        }

        fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let bytes = data_encoding::HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())
                .map_err(|err| E::custom(format!("invalid hex: {err:?}")))?;
            let arr: [u8; 32] = bytes
                .try_into()
                .map_err(|_| E::custom("expected 32 bytes for scalar"))?;
            Ok(Scalar::from_bytes_mod_order(arr))
        }
    }

    pub fn serialize<S>(scalar: &Scalar, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = scalar.to_bytes();
        if serializer.is_human_readable() {
            serializer.serialize_str(&data_encoding::HEXLOWER.encode(&bytes))
        } else {
            serializer.serialize_bytes(&bytes)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Scalar, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            deserializer.deserialize_string(ScalarVisitor)
        } else {
            deserializer.deserialize_bytes(ScalarVisitor)
        }
    }
}

/// Serde module for monero_address::MoneroAddress (monero-oxide addresses)
/// This produces IDENTICAL serialization as monero-rs Address.
///
/// Format: Base58 encoded string (same as monero-rs)
///
/// Note: Deserialization infers network from address prefix:
/// - '4' = Mainnet
/// - '5' = Stagenet  
/// - '9'/'A' = Subaddress variants
pub mod address_oxide {
    use monero_address::{MoneroAddress, Network};
    use serde::de::Visitor;
    use serde::{de, Deserializer, Serializer};
    use std::fmt;

    struct AddressVisitor;

    impl Visitor<'_> for AddressVisitor {
        type Value = MoneroAddress;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a Monero address string")
        }

        fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // Infer network from address prefix
            let network = infer_network_from_address(s)
                .map_err(|e| E::custom(format!("invalid address: {e}")))?;
            
            MoneroAddress::from_str(network, s)
                .map_err(|e| E::custom(format!("failed to parse address: {e:?}")))
        }
    }

    /// Infer the network from a Monero address prefix
    pub fn infer_network_from_address(addr: &str) -> Result<Network, &'static str> {
        let first_char = addr.chars().next().ok_or("empty address")?;
        match first_char {
            '4' => Ok(Network::Mainnet),  // Standard mainnet or mainnet subaddress
            '8' => Ok(Network::Mainnet),  // Mainnet integrated address
            '5' => Ok(Network::Stagenet), // Stagenet standard
            '7' => Ok(Network::Stagenet), // Stagenet subaddress/integrated
            '9' => Ok(Network::Testnet),  // Testnet standard
            'A' => Ok(Network::Testnet),  // Testnet subaddress
            'B' => Ok(Network::Testnet),  // Testnet integrated
            _ => Err("unknown address prefix"),
        }
    }

    pub fn serialize<S>(address: &MoneroAddress, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&address.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<MoneroAddress, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(AddressVisitor)
    }
}

/// Utilities for address validation with monero-oxide types
pub mod address_oxide_validate {
    use anyhow::{bail, Context, Result};
    use monero_address::{MoneroAddress, Network};

    #[derive(thiserror::Error, Debug, Clone, Copy, PartialEq)]
    #[error(
        "Invalid monero address provided, expected address on network {expected:?} but address provided is on {actual:?}"
    )]
    pub struct MoneroAddressNetworkMismatch {
        pub expected: Network,
        pub actual: Network,
    }

    /// Parse an address string, inferring the network from the prefix
    pub fn parse(s: &str) -> Result<MoneroAddress> {
        let network = super::address_oxide::infer_network_from_address(s)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        
        MoneroAddress::from_str(network, s).with_context(|| {
            format!(
                "Failed to parse {s} as a monero address, please make sure it is a valid address",
            )
        })
    }

    /// Parse an address with a specific expected network
    pub fn parse_with_network(s: &str, network: Network) -> Result<MoneroAddress> {
        MoneroAddress::from_str(network, s).with_context(|| {
            format!(
                "Failed to parse {s} as a monero address on {:?}",
                network
            )
        })
    }

    /// Validate that an address is on the expected network
    pub fn validate(
        address: &MoneroAddress,
        expected_network: Network,
    ) -> Result<()> {
        let actual_network = address.network();
        if actual_network != expected_network {
            bail!(MoneroAddressNetworkMismatch {
                expected: expected_network,
                actual: actual_network,
            });
        }
        Ok(())
    }

    /// Validate address is on mainnet or stagenet based on testnet flag
    pub fn validate_is_testnet(
        address: &MoneroAddress,
        is_testnet: bool,
    ) -> Result<()> {
        let expected_network = if is_testnet {
            Network::Stagenet
        } else {
            Network::Mainnet
        };
        validate(address, expected_network)
    }
}
