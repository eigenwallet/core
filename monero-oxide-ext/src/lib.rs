use anyhow::{anyhow, Error};
use monero_oxide_wallet::ed25519::{CompressedPoint, Point, Scalar};
use std::str::FromStr;
use std::{fmt, ops};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PrivateKey {
    pub scalar: Scalar,
}

impl PrivateKey {
    /// Serialize the private key to bytes.
    pub fn as_bytes(&self) -> [u8; 32] {
        let mut output = [0u8; 32];
        self.scalar
            .write(&mut &mut output[..])
            .expect("writing 32 into 32");
        output
    }

    /// Serialize the private key to bytes.
    pub fn to_bytes(self) -> [u8; 32] {
        self.as_bytes()
    }

    /// Deserialize a private key from a slice.
    pub fn from_slice(mut data: &[u8]) -> Result<PrivateKey, Error> {
        if data.len() != 32 {
            return Err(anyhow!("invalid length scalar"));
        }
        let scalar = Scalar::read(&mut data)?;
        Ok(PrivateKey { scalar })
    }

    /// Create a secret key from a raw curve25519 scalar.
    pub fn from_scalar(scalar: Scalar) -> PrivateKey {
        PrivateKey { scalar }
    }
}

impl fmt::Display for PrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.as_bytes()))
    }
}

impl FromStr for PrivateKey {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s)?;
        Self::from_slice(&bytes[..])
    }
}

impl std::ops::Add<PrivateKey> for PrivateKey {
    type Output = PrivateKey;

    fn add(self, other: PrivateKey) -> Self::Output {
        // https://docs.rs/monero/0.21.0/src/monero/util/key.rs.html#152
        PrivateKey::from_slice(
            &(curve25519_dalek::Scalar::from_bytes_mod_order(self.as_bytes())
                + curve25519_dalek::Scalar::from_bytes_mod_order(other.as_bytes()))
            .to_bytes(),
        )
        .unwrap()
    }
}

pub mod serde_compressed_edwards {
    // https://docs.rs/curve25519-dalek/4.1.3/src/curve25519_dalek/edwards.rs.html#279-292
    // https://docs.rs/curve25519-dalek/4.1.3/src/curve25519_dalek/edwards.rs.html#330-362

    use serde::de::Visitor;
    use serde::{Deserializer, Serializer};

    use super::PublicKey;
    use monero_oxide_wallet::ed25519::CompressedPoint;

    pub fn serialize<S>(
        compressed_point: &CompressedPoint,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeTuple;
        let mut buf = [0u8; 32];
        compressed_point
            .write(&mut &mut buf[..])
            .expect("writing 32 into 32 bytes can't panic");

        let mut tup = serializer.serialize_tuple(32)?;
        for byte in &buf {
            tup.serialize_element(byte)?;
        }
        tup.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<CompressedPoint, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CompressedEdwardsYVisitor;

        impl<'de> Visitor<'de> for CompressedEdwardsYVisitor {
            type Value = CompressedPoint;

            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("32 bytes of data")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<CompressedPoint, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut bytes = [0u8; 32];
                #[allow(clippy::needless_range_loop)]
                for i in 0..32 {
                    bytes[i] = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(i, &"expected 32 bytes"))?;
                }
                Ok(PublicKey::from_slice(&bytes)
                    .map_err(|e| serde::de::Error::custom(e))?
                    .point)
            }
        }

        deserializer.deserialize_tuple(32, CompressedEdwardsYVisitor)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PublicKey {
    #[serde(with = "serde_compressed_edwards")]
    pub point: CompressedPoint,
}

impl PublicKey {
    /// Serialize the public key to bytes.
    pub fn as_bytes(&self) -> [u8; 32] {
        self.point.to_bytes()
    }

    /// Serialize the public key to bytes.
    pub fn to_bytes(self) -> [u8; 32] {
        self.point.to_bytes()
    }

    /// Deserialize a public key from a slice.
    pub fn from_slice(data: &[u8]) -> Result<PublicKey, Error> {
        if data.len() != 32 {
            return Err(anyhow!("invalid length scalar"));
        }

        let point = CompressedPoint::read(&mut &data[..])?;
        // Check that the point is valid and canonical.
        // https://github.com/dalek-cryptography/curve25519-dalek/issues/380
        match point.decompress() {
            Some(point) => {
                if point.compress().to_bytes() != data {
                    return Err(anyhow!("invalid point"));
                }
            }
            None => {
                return Err(anyhow!("invalid point"));
            }
        };
        Ok(PublicKey { point })
    }

    /// Generate a public key from the private key.
    pub fn from_private_key(privkey: &PrivateKey) -> PublicKey {
        let point = &curve25519_dalek::Scalar::from_canonical_bytes(privkey.as_bytes())
            .expect("invalid private key")
            * curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
        PublicKey {
            point: CompressedPoint::read(&mut &point.compress().to_bytes()[..])
                .expect("invalid freshly-compressed point?"),
        }
    }

    pub fn decompress(&self) -> Point {
        self.point.decompress().expect("validated in constructor")
    }

    pub fn decompress_ng(&self) -> curve25519_dalek_ng::edwards::EdwardsPoint {
        curve25519_dalek_ng::edwards::CompressedEdwardsY::from_slice(&self.as_bytes())
            .decompress()
            .expect("validated in constructor")
    }
}

impl From<curve25519_dalek_ng::edwards::CompressedEdwardsY> for PublicKey {
    fn from(ng: curve25519_dalek_ng::edwards::CompressedEdwardsY) -> Self {
        PublicKey::from_slice(&ng.to_bytes()).expect("validated by curve25519-dalek-ng")
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.as_bytes()))
    }
}

impl FromStr for PublicKey {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s)?;
        Self::from_slice(&bytes[..])
    }
}

impl ops::Add<PublicKey> for PublicKey {
    type Output = PublicKey;

    fn add(self, other: PublicKey) -> Self::Output {
        let point = self.decompress_ng() + other.decompress_ng();
        point.compress().into()
    }
}

/// Represent an unsigned quantity of Monero, internally as piconero.
///
/// The [`Amount`] type can be used to express Monero amounts that supports arithmetic and
/// conversion to various denominations.
///
/// Replicates a reduced [monero-rs `Amount` API](https://docs.rs/monero/0.21.0/monero/util/amount/struct.Amount.html).
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Amount(u64);

impl Amount {
    /// The zero amount.
    pub const ZERO: Amount = Amount(0);
    /// Exactly one piconero.
    pub const ONE_PICO: Amount = Amount(1);
    /// Exactly one monero.
    pub const ONE_XMR: Amount = Amount(1_000_000_000_000);

    /// Create an [`Amount`] with piconero precision and the given number of piconero.
    pub const fn from_pico(piconero: u64) -> Amount {
        Amount(piconero)
    }

    /// Get the number of piconeros in this [`Amount`].
    pub const fn as_pico(self) -> u64 {
        self.0
    }

    /// Express this [`Amount`] as a floating-point value in Monero.
    ///
    /// Please be aware of the risk of using floating-point numbers.
    pub fn as_xmr(self) -> f64 {
        let mut buf = String::new();
        self.fmt_value_in_xmr(&mut buf).unwrap();
        f64::from_str(&buf).unwrap()
    }

    fn fmt_value_in_xmr(self, f: &mut dyn fmt::Write) -> fmt::Result {
        fmt_piconero_in_xmr(self.as_pico(), f)
    }

    // Some arithmetic that doesn't fit in `std::ops` traits.

    /// Checked addition.
    /// Returns [`None`] if overflow occurred.
    pub fn checked_add(self, rhs: Amount) -> Option<Amount> {
        self.0.checked_add(rhs.0).map(Amount)
    }

    /// Checked subtraction.
    /// Returns [`None`] if overflow occurred.
    pub fn checked_sub(self, rhs: Amount) -> Option<Amount> {
        self.0.checked_sub(rhs.0).map(Amount)
    }

    /// Checked multiplication.
    /// Returns [`None`] if overflow occurred.
    pub fn checked_mul(self, rhs: u64) -> Option<Amount> {
        self.0.checked_mul(rhs).map(Amount)
    }

    /// Checked integer division.
    /// Be aware that integer division loses the remainder if no exact division
    /// can be made.
    /// Returns [`None`] if overflow occurred.
    pub fn checked_div(self, rhs: u64) -> Option<Amount> {
        self.0.checked_div(rhs).map(Amount)
    }

    /// Checked remainder.
    /// Returns [`None`] if overflow occurred.
    pub fn checked_rem(self, rhs: u64) -> Option<Amount> {
        self.0.checked_rem(rhs).map(Amount)
    }
}

impl fmt::Debug for Amount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Amount({:.12} xmr)", self.as_xmr())
    }
}

// No one should depend on a binding contract for Display for this type.
// Just using Monero denominated string.
impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.fmt_value_in_xmr(f)?;
        f.write_str(" XMR")
    }
}

impl ops::Add for Amount {
    type Output = Amount;

    fn add(self, rhs: Amount) -> Self::Output {
        self.checked_add(rhs).expect("Amount addition error")
    }
}

impl ops::AddAssign for Amount {
    fn add_assign(&mut self, other: Amount) {
        *self = *self + other
    }
}

impl ops::Sub for Amount {
    type Output = Amount;

    fn sub(self, rhs: Amount) -> Self::Output {
        self.checked_sub(rhs).expect("Amount subtraction error")
    }
}

impl ops::SubAssign for Amount {
    fn sub_assign(&mut self, other: Amount) {
        *self = *self - other
    }
}

impl ops::Rem<u64> for Amount {
    type Output = Amount;

    fn rem(self, modulus: u64) -> Self {
        self.checked_rem(modulus).expect("Amount remainder error")
    }
}

impl ops::RemAssign<u64> for Amount {
    fn rem_assign(&mut self, modulus: u64) {
        *self = *self % modulus
    }
}

impl ops::Mul<u64> for Amount {
    type Output = Amount;

    fn mul(self, rhs: u64) -> Self::Output {
        self.checked_mul(rhs).expect("Amount multiplication error")
    }
}

impl ops::MulAssign<u64> for Amount {
    fn mul_assign(&mut self, rhs: u64) {
        *self = *self * rhs
    }
}

impl ops::Div<u64> for Amount {
    type Output = Amount;

    fn div(self, rhs: u64) -> Self::Output {
        self.checked_div(rhs).expect("Amount division error")
    }
}

impl ops::DivAssign<u64> for Amount {
    fn div_assign(&mut self, rhs: u64) {
        *self = *self / rhs
    }
}

/// Format the given piconero amount in the given denomination without including the denomination.
fn fmt_piconero_in_xmr(piconero: u64, f: &mut dyn fmt::Write) -> fmt::Result {
    // need to inject a comma in the number
    let nb_decimals = 12usize;
    let real = format!("{:0width$}", piconero, width = nb_decimals);
    if real.len() == nb_decimals {
        write!(f, "0.{}", &real[real.len() - nb_decimals..])
    } else {
        write!(
            f,
            "{}.{}",
            &real[0..(real.len() - nb_decimals)],
            &real[real.len() - nb_decimals..]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PICOS_XMR: &[(u64, &str)] = &[
        (123456789, "0.000123456789"),
        (1234567891011, "1.234567891011"),
    ];

    #[test]
    fn display() {
        for &(pico, xmr) in PICOS_XMR {
            assert_eq!(Amount::from_pico(pico).to_string(), format!("{xmr} XMR"));
        }
    }

    #[test]
    fn debug() {
        for &(pico, xmr) in PICOS_XMR {
            assert_eq!(
                format!("{:?}", Amount::from_pico(pico)),
                format!("Amount({xmr} XMR)")
            );
        }
    }
}

pub mod serde_address {
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
            #[serde(transparent)]
            struct Helper(#[serde(with = "super")] monero_address::MoneroAddress);

            Option::<Helper>::deserialize(deserializer).map(|opt| opt.map(|h| h.0))
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Address(#[serde(with = "serde_address")] pub monero_address::MoneroAddress);

impl std::ops::Deref for Address {
    type Target = monero_address::MoneroAddress;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
