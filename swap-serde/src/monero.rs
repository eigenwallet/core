use monero_address::Network;
use monero_oxide_ext::Amount;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(remote = "Network")]
#[allow(non_camel_case_types)]
pub enum network {
    Mainnet,
    Stagenet,
    Testnet,
}

pub mod private_key {
    use monero_oxide_ext::PrivateKey;
    use serde::de::Visitor;
    use serde::{de, Deserializer, Serializer};
    use std::fmt;

    fn trunc_at_32(s: &[u8]) -> &[u8] {
        match s.split_at_checked(32) {
            Some((trunc, _)) => trunc,
            None => s,
        }
    }

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
            PrivateKey::from_slice(trunc_at_32(s)).map_err(E::custom)
        }

        fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let bytes = data_encoding::HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())
                .map_err(|err| E::custom(format!("{err:?}")))?;
            self.visit_bytes(&bytes)
        }
    }

    pub fn serialize<S>(x: &PrivateKey, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if s.is_human_readable() {
            s.serialize_str(&data_encoding::HEXLOWER.encode(&x.as_bytes()))
        } else {
            s.serialize_bytes(&x.as_bytes())
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

pub mod optional_private_key {
    use monero_oxide_ext::PrivateKey;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(x: &Option<PrivateKey>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match x {
            Some(key) => super::private_key::serialize(key, s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<Option<PrivateKey>, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Deserialize;
        Option::<PrivateKeyHelper>::deserialize(deserializer).map(|opt| opt.map(|h| h.0))
    }

    #[derive(serde::Deserialize)]
    struct PrivateKeyHelper(#[serde(with = "super::private_key")] PrivateKey);
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

    #[derive(thiserror::Error, Debug, Clone, Copy, PartialEq)]
    #[error(
        "Invalid monero address provided, expected address on network {expected:?} but address provided is on {actual:?}"
    )]
    pub struct MoneroAddressNetworkMismatch {
        pub expected: monero_address::Network,
        pub actual: monero_address::Network,
    }

    pub fn parse(s: &str) -> Result<monero_address::MoneroAddress> {
        monero_address::MoneroAddress::from_str_with_unchecked_network(s).with_context(|| {
            format!(
                "Failed to parse {s} as a monero address, please make sure it is a valid address",
            )
        })
    }

    pub fn validate(
        address: monero_address::MoneroAddress,
        expected_network: monero_address::Network,
    ) -> Result<monero_address::MoneroAddress> {
        if address.network() != expected_network {
            bail!(MoneroAddressNetworkMismatch {
                expected: expected_network,
                actual: address.network(),
            });
        }
        Ok(address)
    }

    pub fn validate_is_testnet(
        address: monero_address::MoneroAddress,
        is_testnet: bool,
    ) -> Result<monero_address::MoneroAddress> {
        let expected_network = if is_testnet {
            monero_address::Network::Stagenet
        } else {
            monero_address::Network::Mainnet
        };
        validate(address, expected_network)
    }
}

pub mod address_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        address: &monero_address::MoneroAddress,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        address.to_string().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<monero_address::MoneroAddress, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        monero_address::MoneroAddress::from_str_with_unchecked_network(&s)
            .map_err(serde::de::Error::custom)
    }

    pub mod opt {
        use super::*;

        pub fn serialize<S>(
            x: &Option<monero_address::MoneroAddress>,
            s: S,
        ) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match x {
                Some(key) => super::serialize(key, s),
                None => s.serialize_none(),
            }
        }

        pub fn deserialize<'de, D>(
            deserializer: D,
        ) -> Result<Option<monero_address::MoneroAddress>, <D as Deserializer<'de>>::Error>
        where
            D: Deserializer<'de>,
        {
            use serde::de::Deserialize;

            #[derive(serde::Deserialize)]
            struct Helper(#[serde(with = "super")] monero_address::MoneroAddress);

            Option::<Helper>::deserialize(deserializer).map(|opt| opt.map(|h| h.0))
        }
    }
}
