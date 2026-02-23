use crate::bitcoin;
use anyhow::{Result, bail};
use monero_address::{MoneroAddress, Network};
pub use monero_oxide_wallet::ed25519::Scalar;
use rand::{CryptoRng, RngCore};
use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Add;
use typeshare::typeshare;

pub use ::monero_oxide_ext::{Amount, PrivateKey, PublicKey};

pub const PICONERO_OFFSET: u64 = 1_000_000_000_000;

/// A Monero block height.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockHeight {
    pub height: u64,
}

impl fmt::Display for BlockHeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.height)
    }
}

pub fn private_key_from_secp256k1_scalar(
    scalar: bitcoin::Scalar,
) -> curve25519_dalek::scalar::Scalar {
    let mut bytes = scalar.to_bytes();

    // we must reverse the bytes because a secp256k1 scalar is big endian, whereas a
    // ed25519 scalar is little endian
    bytes.reverse();

    curve25519_dalek::scalar::Scalar::from_bytes_mod_order(bytes)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateViewKey(#[serde(with = "swap_serde::monero::private_key")] pub PrivateKey);

impl fmt::Display for PrivateViewKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Delegate to the Display implementation of PrivateKey
        write!(f, "{}", self.0)
    }
}

impl PrivateViewKey {
    pub fn new_random<R: RngCore + CryptoRng>(rng: &mut R) -> Self {
        let scalar = Scalar::random(rng);
        let private_key = PrivateKey::from_slice(&PrivateKey { scalar }.as_bytes()).expect("bytes of curve25519-dalek Scalar should by decodable to a PrivateKey which uses curve25519-dalek-ng under the hood");

        Self(private_key)
    }

    pub fn public(&self) -> PublicViewKey {
        PublicViewKey(PublicKey::from_private_key(&self.0))
    }
}

impl Add for PrivateViewKey {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl From<PrivateViewKey> for PrivateKey {
    fn from(from: PrivateViewKey) -> Self {
        from.0
    }
}

impl From<PublicViewKey> for PublicKey {
    fn from(from: PublicViewKey) -> Self {
        from.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PublicViewKey(pub PublicKey);

// TX Fees on Monero can be found here:
// - https://www.monero.how/monero-transaction-fees
// - https://bitinfocharts.com/comparison/monero-transactionfees.html#1y
//
// In the last year the highest avg fee on any given day was around 0.00075 XMR
// We use a multiplier of 4x to stay safe
// 0.00075 XMR * 4 = 0.003 XMR (around $1 as of Jun. 4th 2025)
// We DO NOT use this fee to construct any transactions. It is only to **estimate** how much
// we need to reserve for the fee when determining our max giveable amount
// We use a VERY conservative value here to stay on the safe side. We want to avoid not being able
// to lock as much as we previously estimated.
pub const CONSERVATIVE_MONERO_FEE: Amount = Amount::from_pico(3_000_000_000);

pub trait AmountExt {
    fn max_conservative_giveable(&self) -> Self;
    fn min_conservative_balance_to_spend(&self) -> Self;
    fn max_bitcoin_for_price(&self, ask_price: bitcoin::Amount) -> Option<bitcoin::Amount>;
}
impl AmountExt for Amount {
    /// Calculate the conservative max giveable of Monero we can spent given [`self`] is the balance
    /// of a Monero wallet
    /// This is going to be LESS than we can really spent because we assume a high fee
    fn max_conservative_giveable(&self) -> Self {
        let pico_minus_fee = self
            .as_pico()
            .saturating_sub(CONSERVATIVE_MONERO_FEE.as_pico());

        Self::from_pico(pico_minus_fee)
    }

    /// Calculate the Monero balance needed to send the [`self`] Amount to another address
    /// E.g: Amount(1 XMR).min_conservative_balance_to_spend() with a fee of 0.1 XMR would be 1.1 XMR
    /// This is going to be MORE than we really need because we assume a high fee
    fn min_conservative_balance_to_spend(&self) -> Self {
        let pico_minus_fee = self
            .as_pico()
            .saturating_add(CONSERVATIVE_MONERO_FEE.as_pico());

        Self::from_pico(pico_minus_fee)
    }

    /// Calculate the maximum amount of Bitcoin that can be bought at a given
    /// asking price for this amount of Monero including the median fee.
    fn max_bitcoin_for_price(&self, ask_price: bitcoin::Amount) -> Option<bitcoin::Amount> {
        let pico_minus_fee = self.max_conservative_giveable();

        if pico_minus_fee.as_pico() == 0 {
            return Some(bitcoin::Amount::ZERO);
        }

        // safely convert the BTC/XMR rate to sat/pico
        let ask_sats = Decimal::from(ask_price.to_sat());
        let pico_per_xmr = Decimal::from(PICONERO_OFFSET);
        let ask_sats_per_pico = ask_sats / pico_per_xmr;

        let pico = Decimal::from(pico_minus_fee.as_pico());
        let max_sats = pico.checked_mul(ask_sats_per_pico)?;
        let satoshi = max_sats.to_u64()?;

        Some(bitcoin::Amount::from_sat(satoshi))
    }
}

/// A Monero address with an associated percentage and human-readable label.
///
/// This structure represents a destination address for Monero transactions
/// along with the percentage of funds it should receive and a descriptive label.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[typeshare]
pub struct LabeledMoneroAddress {
    // If this is None, we will use an address of the internal Monero wallet
    // TODO: This should be string | null but typeshare cannot do that yet
    #[typeshare(serialized_as = "string")]
    #[serde(with = "swap_serde::monero::address_serde::opt")]
    address: Option<monero_address::MoneroAddress>,
    #[typeshare(serialized_as = "number")]
    percentage: Decimal,
    label: String,
}

impl LabeledMoneroAddress {
    /// Creates a new labeled Monero address.
    ///
    /// # Arguments
    ///
    /// * `address` - The Monero address
    /// * `percentage` - The percentage of funds (between 0.0 and 1.0)
    /// * `label` - A human-readable label for this address
    ///
    /// # Errors
    ///
    /// Returns an error if the percentage is not between 0.0 and 1.0 inclusive.
    fn new(
        address: impl Into<Option<monero_address::MoneroAddress>>,
        percentage: Decimal,
        label: String,
    ) -> Result<Self> {
        if percentage < Decimal::ZERO || percentage > Decimal::ONE {
            bail!(
                "Percentage must be between 0 and 1 inclusive, got: {}",
                percentage
            );
        }

        Ok(Self {
            address: address.into(),
            percentage,
            label,
        })
    }

    pub fn with_address(
        address: monero_address::MoneroAddress,
        percentage: Decimal,
        label: String,
    ) -> Result<Self> {
        Self::new(address, percentage, label)
    }

    pub fn with_internal_address(percentage: Decimal, label: String) -> Result<Self> {
        Self::new(None, percentage, label)
    }

    /// Returns the Monero address.
    pub fn address(&self) -> Option<monero_address::MoneroAddress> {
        self.address
    }

    /// Returns the percentage as a decimal.
    pub fn percentage(&self) -> Decimal {
        self.percentage
    }

    /// Returns the human-readable label.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// A collection of labeled Monero addresses that can receive funds in a transaction.
///
/// This structure manages multiple destination addresses with their associated
/// percentages and labels. It's used for splitting Monero transactions across
/// multiple recipients, such as for donations or multi-destination swaps.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[typeshare]
pub struct MoneroAddressPool(Vec<LabeledMoneroAddress>);

use rust_decimal::prelude::ToPrimitive;

impl MoneroAddressPool {
    /// Creates a new address pool from a vector of labeled addresses.
    ///
    /// # Arguments
    ///
    /// * `addresses` - Vector of labeled Monero addresses
    pub fn new(addresses: Vec<LabeledMoneroAddress>) -> Self {
        Self(addresses)
    }

    /// Returns a vector of all Monero addresses in the pool.
    pub fn addresses(&self) -> Vec<Option<monero_address::MoneroAddress>> {
        self.0.iter().map(|address| address.address()).collect()
    }

    /// Returns a vector of all percentages as f64 values (0-1 range).
    pub fn percentages(&self) -> Vec<f64> {
        self.0
            .iter()
            .map(|address| {
                address
                    .percentage()
                    .to_f64()
                    .expect("Decimal should convert to f64")
            })
            .collect()
    }

    /// Returns an iterator over the labeled addresses.
    pub fn iter(&self) -> impl Iterator<Item = &LabeledMoneroAddress> {
        self.0.iter()
    }

    /// Validates that all addresses in the pool are on the expected network.
    ///
    /// # Arguments
    ///
    /// * `network` - The expected Monero network
    ///
    /// # Errors
    ///
    /// Returns an error if any address is on a different network than expected.
    pub fn assert_network(&self, network: Network) -> Result<()> {
        for address in self.0.iter() {
            if let Some(address) = address.address {
                if address.network() != network {
                    bail!(
                        "Address pool contains addresses on the wrong network (address {} is on {:?}, expected {:?})",
                        address,
                        address.network(),
                        network
                    );
                }
            }
        }

        Ok(())
    }

    /// Assert that the sum of the percentages in the address pool is 1 (allowing for a small tolerance)
    pub fn assert_sum_to_one(&self) -> Result<()> {
        let sum = self
            .0
            .iter()
            .map(|address| address.percentage())
            .sum::<Decimal>();

        const TOLERANCE: f64 = 1e-6;

        if (sum - Decimal::ONE).abs()
            > Decimal::from_f64(TOLERANCE).expect("TOLERANCE constant should be a valid f64")
        {
            bail!("Address pool percentages do not sum to 1");
        }

        Ok(())
    }

    /// Returns a vector of addresses with the empty addresses filled with the given primary address
    pub fn fill_empty_addresses(
        &self,
        primary_address: monero_address::MoneroAddress,
    ) -> Vec<monero_address::MoneroAddress> {
        self.0
            .iter()
            .map(|address| address.address().unwrap_or(primary_address))
            .collect()
    }
}

impl From<::monero_address::MoneroAddress> for MoneroAddressPool {
    fn from(address: ::monero_address::MoneroAddress) -> Self {
        Self(vec![
            LabeledMoneroAddress::new(address, Decimal::from(1), "user address".to_string())
                .expect("Percentage 1 is always valid"),
        ])
    }
}

/// Transfer a specified amount of money to a specified address.
pub struct TransferRequest {
    pub public_spend_key: PublicKey,
    pub public_view_key: super::PublicViewKey,
    pub amount: ::monero_oxide_ext::Amount,
}

impl TransferRequest {
    pub fn address_and_amount(
        &self,
        network: Network,
    ) -> (MoneroAddress, ::monero_oxide_ext::Amount) {
        (
            MoneroAddress::new(
                network,
                monero_address::AddressType::Legacy,
                self.public_spend_key.decompress(),
                self.public_view_key.0.decompress(),
            ),
            self.amount,
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferProof {
    pub tx_hash: TxHash,
    #[serde(with = "swap_serde::monero::private_key")]
    pub tx_key: PrivateKey,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferProofMaybeWithTxKey {
    pub tx_hash: TxHash,
    #[serde(with = "swap_serde::monero::optional_private_key")]
    pub tx_key: Option<PrivateKey>,
}

impl TransferProof {
    pub fn new(tx_hash: TxHash, tx_key: PrivateKey) -> Self {
        Self { tx_hash, tx_key }
    }

    pub fn tx_hash(&self) -> TxHash {
        self.tx_hash.clone()
    }

    pub fn tx_key(&self) -> PrivateKey {
        self.tx_key
    }
}

impl TransferProofMaybeWithTxKey {
    pub fn new_without_tx_key(tx_hash: TxHash) -> Self {
        Self {
            tx_hash,
            tx_key: None,
        }
    }

    pub fn tx_hash(&self) -> TxHash {
        self.tx_hash.clone()
    }
}

impl From<TransferProof> for TransferProofMaybeWithTxKey {
    fn from(proof: TransferProof) -> Self {
        Self {
            tx_hash: proof.tx_hash,
            tx_key: Some(proof.tx_key),
        }
    }
}

// TODO: add constructor/ change String to fixed length byte array
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxHash(pub String);

impl From<TxHash> for String {
    fn from(from: TxHash) -> Self {
        from.0
    }
}

impl fmt::Debug for TxHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for TxHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("expected {expected}, got {actual}")]
pub struct InsufficientFunds {
    pub expected: Amount,
    pub actual: Amount,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[error("Overflow, cannot convert {0} to u64")]
pub struct OverflowError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_monero_min() {
        let min_pics = 1;
        let amount = Amount::from_pico(min_pics);
        let monero = amount.to_string();
        assert_eq!("0.000000000001 XMR", monero);
    }

    #[test]
    fn display_monero_one() {
        let min_pics = 1000000000000;
        let amount = Amount::from_pico(min_pics);
        let monero = amount.to_string();
        assert_eq!("1.000000000000 XMR", monero);
    }

    #[test]
    fn display_monero_max() {
        let max_pics = 18_446_744_073_709_551_615;
        let amount = Amount::from_pico(max_pics);
        let monero = amount.to_string();
        assert_eq!("18446744.073709551615 XMR", monero);
    }

    #[test]
    fn parse_monero_min() {
        let monero_min = "0.000000000001";
        let amount = Amount::parse_monero(monero_min).unwrap();
        let pics = amount.as_pico();
        assert_eq!(1, pics);
    }

    #[test]
    fn parse_monero() {
        let monero = "123";
        let amount = Amount::parse_monero(monero).unwrap();
        let pics = amount.as_pico();
        assert_eq!(123000000000000, pics);
    }

    #[test]
    fn parse_monero_max() {
        let monero = "18446744.073709551615";
        let amount = Amount::parse_monero(monero).unwrap();
        let pics = amount.as_pico();
        assert_eq!(18446744073709551615, pics);
    }

    #[test]
    fn parse_monero_overflows() {
        let overflow_pics = "18446744.073709551616";
        let error = Amount::parse_monero(overflow_pics).unwrap_err();
        assert_eq!(
            error.downcast_ref::<OverflowError>().unwrap(),
            &OverflowError(overflow_pics.to_owned())
        );
    }

    #[test]
    fn max_bitcoin_to_trade() {
        // sanity check: if the asking price is 1 BTC / 1 XMR
        // and we have μ XMR + fee
        // then max BTC we can buy is μ
        let ask = bitcoin::Amount::from_btc(1.0).unwrap();

        let xmr = Amount::parse_monero("1.0").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(1.0).unwrap());

        let xmr = Amount::parse_monero("0.5").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(0.5).unwrap());

        let xmr = Amount::parse_monero("2.5").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(2.5).unwrap());

        let xmr = Amount::parse_monero("420").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(420.0).unwrap());

        let xmr = Amount::parse_monero("0.00001").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(0.00001).unwrap());

        // other ask prices

        let ask = bitcoin::Amount::from_btc(0.5).unwrap();
        let xmr = Amount::parse_monero("2").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(1.0).unwrap());

        let ask = bitcoin::Amount::from_btc(2.0).unwrap();
        let xmr = Amount::parse_monero("1").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(2.0).unwrap());

        let ask = bitcoin::Amount::from_sat(382_900);
        let xmr = Amount::parse_monero("10").unwrap();
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_sat(3_827_851));

        // example from https://github.com/comit-network/xmr-btc-swap/issues/1084
        // with rate from kraken at that time
        let ask = bitcoin::Amount::from_sat(685_800);
        let xmr = Amount::parse_monero("0.826286435921").unwrap();
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_sat(564_609));
    }

    #[test]
    fn max_bitcoin_to_trade_overflow() {
        let xmr = Amount::parse_monero("30.0").unwrap();
        let ask = bitcoin::Amount::from_sat(728_688);
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(bitcoin::Amount::from_sat(21_858_453), btc);

        let xmr = Amount::from_pico(u64::MAX);
        let ask = bitcoin::Amount::from_sat(u64::MAX);
        let btc = xmr.max_bitcoin_for_price(ask);

        assert!(btc.is_none());
    }

    #[test]
    fn geting_max_bitcoin_to_trade_with_balance_smaller_than_locking_fee() {
        let ask = bitcoin::Amount::from_sat(382_900);
        let xmr = Amount::parse_monero("0.00001").unwrap();
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(bitcoin::Amount::ZERO, btc);
    }

    use rand::rngs::OsRng;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct MoneroPrivateKey(
        #[serde(with = "swap_serde::monero::private_key")] ::monero_oxide_ext::PrivateKey,
    );

    #[test]
    fn serde_monero_private_key_json() {
        let key = MoneroPrivateKey(::monero_oxide_ext::PrivateKey::from_scalar(Scalar::random(
            &mut OsRng,
        )));
        let encoded = serde_json::to_vec(&key).unwrap();
        let decoded: MoneroPrivateKey = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn serde_monero_private_key_cbor() {
        let key = MoneroPrivateKey(::monero_oxide_ext::PrivateKey::from_scalar(Scalar::random(
            &mut OsRng,
        )));
        let encoded = serde_cbor::to_vec(&key).unwrap();
        let decoded: MoneroPrivateKey = serde_cbor::from_slice(&encoded).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn serde_monero_amount() {
        let amount = Amount::from_pico(1000);
        let encoded = serde_cbor::to_vec(&amount).unwrap();
        let decoded: Amount = serde_cbor::from_slice(&encoded).unwrap();
        assert_eq!(amount, decoded);
    }

    #[test]
    fn max_conservative_giveable_basic() {
        // Test with balance larger than fee
        let balance = Amount::parse_monero("1.0").unwrap();
        let giveable = balance.max_conservative_giveable();
        let expected = balance.as_pico() - CONSERVATIVE_MONERO_FEE.as_pico();
        assert_eq!(giveable.as_pico(), expected);
    }

    #[test]
    fn max_conservative_giveable_exact_fee() {
        // Test with balance exactly equal to fee
        let balance = CONSERVATIVE_MONERO_FEE;
        let giveable = balance.max_conservative_giveable();
        assert_eq!(giveable, Amount::ZERO);
    }

    #[test]
    fn max_conservative_giveable_less_than_fee() {
        // Test with balance less than fee (should saturate to 0)
        let balance = Amount::from_pico(CONSERVATIVE_MONERO_FEE.as_pico() / 2);
        let giveable = balance.max_conservative_giveable();
        assert_eq!(giveable, Amount::ZERO);
    }

    #[test]
    fn max_conservative_giveable_zero_balance() {
        // Test with zero balance
        let balance = Amount::ZERO;
        let giveable = balance.max_conservative_giveable();
        assert_eq!(giveable, Amount::ZERO);
    }

    #[test]
    fn max_conservative_giveable_large_balance() {
        // Test with large balance
        let balance = Amount::parse_monero("100.0").unwrap();
        let giveable = balance.max_conservative_giveable();
        let expected = balance.as_pico() - CONSERVATIVE_MONERO_FEE.as_pico();
        assert_eq!(giveable.as_pico(), expected);

        // Ensure the result makes sense
        assert!(giveable.as_pico() > 0);
        assert!(giveable < balance);
    }

    #[test]
    fn min_conservative_balance_to_spend_basic() {
        // Test with 1 XMR amount to send
        let amount_to_send = Amount::parse_monero("1.0").unwrap();
        let min_balance = amount_to_send.min_conservative_balance_to_spend();
        let expected = amount_to_send.as_pico() + CONSERVATIVE_MONERO_FEE.as_pico();
        assert_eq!(min_balance.as_pico(), expected);
    }

    #[test]
    fn min_conservative_balance_to_spend_zero() {
        // Test with zero amount to send
        let amount_to_send = Amount::ZERO;
        let min_balance = amount_to_send.min_conservative_balance_to_spend();
        assert_eq!(min_balance, CONSERVATIVE_MONERO_FEE);
    }

    #[test]
    fn min_conservative_balance_to_spend_small_amount() {
        // Test with small amount
        let amount_to_send = Amount::from_pico(1000);
        let min_balance = amount_to_send.min_conservative_balance_to_spend();
        let expected = 1000 + CONSERVATIVE_MONERO_FEE.as_pico();
        assert_eq!(min_balance.as_pico(), expected);
    }

    #[test]
    fn min_conservative_balance_to_spend_large_amount() {
        // Test with large amount
        let amount_to_send = Amount::parse_monero("50.0").unwrap();
        let min_balance = amount_to_send.min_conservative_balance_to_spend();
        let expected = amount_to_send.as_pico() + CONSERVATIVE_MONERO_FEE.as_pico();
        assert_eq!(min_balance.as_pico(), expected);

        // Ensure the result makes sense
        assert!(min_balance > amount_to_send);
        assert!(min_balance > CONSERVATIVE_MONERO_FEE);
    }

    #[test]
    fn conservative_fee_functions_are_inverse() {
        // Test that the functions are somewhat inverse of each other
        let original_balance = Amount::parse_monero("5.0").unwrap();

        // Get max giveable amount
        let max_giveable = original_balance.max_conservative_giveable();

        // Calculate min balance needed to send that amount
        let min_balance_needed = max_giveable.min_conservative_balance_to_spend();

        // The min balance needed should be equal to or slightly more than the original balance
        // (due to the conservative nature of the fee estimation)
        assert!(min_balance_needed >= original_balance);

        // The difference should be at most the conservative fee
        let difference = min_balance_needed.as_pico() - original_balance.as_pico();
        assert!(difference <= CONSERVATIVE_MONERO_FEE.as_pico());
    }

    #[test]
    fn conservative_fee_edge_cases() {
        // Test with maximum possible amount
        let max_amount = Amount::from_pico(u64::MAX - CONSERVATIVE_MONERO_FEE.as_pico());
        let giveable = max_amount.max_conservative_giveable();
        assert!(giveable.as_pico() > 0);

        // Test min balance calculation doesn't overflow
        let large_amount = Amount::from_pico(u64::MAX / 2);
        let min_balance = large_amount.min_conservative_balance_to_spend();
        assert!(min_balance > large_amount);
    }

    #[test]
    fn labeled_monero_address_percentage_validation() {
        use rust_decimal::Decimal;

        let address = monero_address::MoneroAddress::from_str_with_unchecked_network("53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a").unwrap();

        // Valid percentages should work (0-1 range)
        assert!(LabeledMoneroAddress::new(address, Decimal::ZERO, "test".to_string()).is_ok());
        assert!(LabeledMoneroAddress::new(address, Decimal::ONE, "test".to_string()).is_ok());
        assert!(LabeledMoneroAddress::new(address, Decimal::new(5, 1), "test".to_string()).is_ok()); // 0.5
        assert!(
            LabeledMoneroAddress::new(address, Decimal::new(9925, 4), "test".to_string()).is_ok()
        ); // 0.9925

        // Invalid percentages should fail
        assert!(
            LabeledMoneroAddress::new(address, Decimal::new(-1, 0), "test".to_string()).is_err()
        );
        assert!(
            LabeledMoneroAddress::new(address, Decimal::new(11, 1), "test".to_string()).is_err()
        ); // 1.1
        assert!(
            LabeledMoneroAddress::new(address, Decimal::new(2, 0), "test".to_string()).is_err()
        ); // 2.0
    }
}
