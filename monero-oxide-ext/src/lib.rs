use anyhow::{anyhow, Error};
use monero_wallet::ed25519::Scalar;
use std::str::FromStr;
use std::{fmt, ops};

#[derive(Copy, Clone)]
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
