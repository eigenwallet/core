use crate::alice::AliceState;
use crate::alice::is_complete as alice_is_complete;
use crate::bob::is_swap_complete as bob_is_complete;
use crate::bob::{ApolloProgress, BobState};
use anyhow::{Context, Result};
use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sigma_fun::HashTranscript;
use sigma_fun::ext::dl_secp256k1_ed25519_eq::{CrossCurveDLEQ, CrossCurveDLEQProof};
use std::convert::TryInto;
use std::sync::LazyLock;
use std::time::Duration;
use swap_core::bitcoin;
use swap_core::monero::{self, MoneroAddressPool};
use uuid::Uuid;

/// BIP-341 NUMS point `H = lift_x(SHA256(uncompressed_encoding(G)))`: <https://github.com/bitcoin/bips/blob/master/bip-0341.mediawiki#constructing-and-spending-taproot-outputs>
static PEDERSEN_BLINDING_H_SECP256K1: LazyLock<ecdsa_fun::fun::Point> = LazyLock::new(|| {
    let generator = (*ecdsa_fun::fun::G).normalize().to_bytes_uncompressed();
    let x_coordinate = Sha256::digest(generator).into();

    ecdsa_fun::fun::Point::<ecdsa_fun::fun::marker::EvenY>::from_xonly_bytes(x_coordinate)
        .expect("SHA-256 of the uncompressed secp256k1 generator is a valid x-coordinate")
        .normalize()
});

fn pedersen_blinding_h_ed25519() -> curve25519_dalek_ng::edwards::EdwardsPoint {
    curve25519_dalek_ng::edwards::CompressedEdwardsY::from_slice(
        &monero_oxide_wallet::ed25519::CompressedPoint::H.to_bytes(),
    )
    .decompress()
    .expect("Monero Pedersen H is a valid ed25519 point")
}

pub static CROSS_CURVE_PROOF_SYSTEM: LazyLock<
    CrossCurveDLEQ<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>,
> = LazyLock::new(|| {
    CrossCurveDLEQ::<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>::new(
        *PEDERSEN_BLINDING_H_SECP256K1,
        pedersen_blinding_h_ed25519(),
    )
});

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message0 {
    pub swap_id: Uuid,
    pub B: bitcoin::PublicKey,
    pub S_b_monero: monero_oxide_ext::PublicKey,
    pub S_b_bitcoin: bitcoin::PublicKey,
    pub dleq_proof_s_b: CrossCurveDLEQProof,
    pub v_b: monero::PrivateViewKey,
    #[serde(with = "swap_serde::bitcoin::address_serde")]
    pub refund_address: bitcoin::Address,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message1 {
    pub A: bitcoin::PublicKey,
    pub S_a_monero: monero::PublicKey,
    pub S_a_bitcoin: bitcoin::PublicKey,
    pub dleq_proof_s_a: CrossCurveDLEQProof,
    pub v_a: monero::PrivateViewKey,
    #[serde(with = "swap_serde::bitcoin::address_serde")]
    pub redeem_address: bitcoin::Address,
    #[serde(with = "swap_serde::bitcoin::address_serde")]
    pub punish_address: bitcoin::Address,
    pub fee_rate: bitcoin::SwapFeeRate,
    pub redeem_multiplier: bitcoin::RedeemFeeMultiplier,
    /// The amount of Bitcoin that Bob not get refunded unless Alice decides so.
    /// Introduced in [#675](https://github.com/eigenwallet/core/pull/675) to combat spam.
    pub amnesty_amount: bitcoin::Amount,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message2 {
    pub tx_lock_psbt: bitcoin::PartiallySignedTransaction,
    /// One proof per TxLock input that Bob controls the keys behind it.
    pub tx_lock_proofs: Vec<utxo_ownership_proof::OwnershipProof>,
}

/// Binds a TxLock ownership proof to the swap session and the exact lock transaction.
#[allow(non_snake_case)]
pub fn tx_lock_ownership_challenge(
    swap_id: Uuid,
    A: bitcoin::PublicKey,
    B: bitcoin::PublicKey,
    tx_lock_txid: bitcoin::Txid,
) -> utxo_ownership_proof::Challenge {
    let mut context = Vec::with_capacity(16 + 33 + 33 + 32);
    context.extend_from_slice(swap_id.as_bytes());
    context.extend_from_slice(&ecdsa_fun::fun::Point::from(A).to_bytes());
    context.extend_from_slice(&ecdsa_fun::fun::Point::from(B).to_bytes());
    context.extend_from_slice(tx_lock_txid.as_ref());
    utxo_ownership_proof::challenge(&context)
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message3 {
    pub tx_cancel_sig: bitcoin::Signature,
    // The following fields were reworked in [#675](https://github.com/eigenwallet/core/pull/675).
    // Alice will send either the full refund encsig or signatures for both partial refund
    // and tx refund amnesty.
    pub tx_full_refund_encsig: Option<bitcoin::EncryptedSignature>,
    pub tx_partial_refund_encsig: Option<bitcoin::EncryptedSignature>,
    pub tx_reclaim_sig: Option<bitcoin::Signature>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message4 {
    pub tx_punish_sig: bitcoin::Signature,
    pub tx_cancel_sig: bitcoin::Signature,
    pub tx_early_refund_sig: bitcoin::Signature,
    pub tx_reclaim_sig: Option<bitcoin::Signature>,
    pub tx_withhold_sig: Option<bitcoin::Signature>,
    pub tx_mercy_sig: Option<bitcoin::Signature>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SetupValidationError {
    #[error("Invalid DLEQ proof")]
    InvalidDleqProof,
    #[error("Anti-spam deposit ratio {ratio} exceeds maximum {max}")]
    AntiSpamDepositRatioTooHigh {
        ratio: rust_decimal::Decimal,
        max: rust_decimal::Decimal,
    },
    #[error("Transaction fee is too low compared to our estimate ({fee} vs {estimate})")]
    TransactionFeeTooLow {
        fee: bitcoin::Amount,
        estimate: bitcoin::Amount,
    },
}

/// Require at least half our local fee estimate for a transaction of
/// `signed_weight`, so the floor holds for the fee rate, not just the absolute
/// fee. Higher fees are allowed.
///
/// `estimated_rate` must already include the relay minimum.
pub fn sanity_check_transaction_fee_floor(
    proposed_fee: bitcoin::Amount,
    signed_weight: ::bitcoin::Weight,
    estimated_rate: ::bitcoin::FeeRate,
    transfer_amount: bitcoin::Amount,
) -> Result<()> {
    let estimate = bitcoin_wallet::estimate_fee(
        signed_weight,
        Some(transfer_amount),
        estimated_rate,
        estimated_rate,
    )
    .context("Failed to estimate transaction fee")?;
    let floor = bitcoin::Amount::from_sat(estimate.to_sat() / 2);
    anyhow::ensure!(
        proposed_fee >= floor,
        SetupValidationError::TransactionFeeTooLow {
            fee: proposed_fee,
            estimate,
        }
    );
    Ok(())
}

/// Local acceptance limits, derived without capping the agreed fee schedule.
/// The lower limit is half our estimate, but never below [`bitcoin::SwapFeeRate::MIN`].
pub fn swap_fee_limits(
    rate: bitcoin::SwapFeeRate,
    amount: bitcoin::Amount,
) -> Result<bitcoin::SwapFeeLimits> {
    use anyhow::Context;
    anyhow::ensure!(rate.0 > 0, "Local fee estimate must be positive");
    Ok(bitcoin::SwapFeeLimits {
        min_rate: bitcoin::SwapFeeRate((rate.0 / 2 + rate.0 % 2).max(bitcoin::SwapFeeRate::MIN.0)),
        max_rate: bitcoin::SwapFeeRate(rate.0.checked_mul(3).context("Fee rate limit overflow")?),
        max_path_fee: bitcoin_wallet::MAX_ABSOLUTE_TX_FEE
            .min(bitcoin::Amount::from_sat(amount.to_sat() / 3)),
    })
}

pub fn calculate_swap_fees(
    inputs: bitcoin::SwapFeeInputs<'_>,
    rate: bitcoin::SwapFeeRate,
    limits: bitcoin::SwapFeeLimits,
) -> Result<bitcoin::SwapFees> {
    let ratio = rust_decimal::Decimal::from(inputs.deposit.to_sat())
        .checked_div(rust_decimal::Decimal::from(inputs.lock_amount.to_sat()))
        .context("Cannot calculate anti-spam deposit ratio for a zero lock amount")?;
    anyhow::ensure!(
        ratio <= swap_env::config::MAX_ANTI_SPAM_DEPOSIT_RATIO,
        SetupValidationError::AntiSpamDepositRatioTooHigh {
            ratio,
            max: swap_env::config::MAX_ANTI_SPAM_DEPOSIT_RATIO,
        }
    );
    inputs.calculate(rate, limits)
}

/// The part of a swap of `swap_amount` that goes into the anti-spam deposit if
/// the taker refunds.
///
/// ZERO means that the taker gets a full refund, without a deposit output.
pub fn anti_spam_deposit(
    ratio: rust_decimal::Decimal,
    swap_amount: bitcoin::Amount,
) -> Result<bitcoin::Amount> {
    if ratio == rust_decimal::Decimal::ZERO {
        return Ok(bitcoin::Amount::ZERO);
    }

    let deposit: u64 = rust_decimal::Decimal::from(swap_amount.to_sat())
        .checked_mul(ratio)
        .context("Decimal overflow when the anti-spam deposit is calculated")?
        .floor()
        .try_into()
        .context("The anti-spam deposit does not fit in u64")?;

    anyhow::ensure!(
        deposit > 0,
        "Positive anti-spam deposit rounds down to zero"
    );

    Ok(bitcoin::Amount::from_sat(deposit))
}

/// How near [`minimum_swap_amount`] gets to the boundary.
///
/// An exact answer would be false precision. The deposit of a swap is a ratio
/// of its amount, so at a ratio of one per mille the deposit only grows by one
/// satoshi per 1,000 satoshi of amount. The dust limit of the taker's refund
/// address, which the quote does not know, is worth a few hundred satoshi of
/// deposit on its own.
pub const MINIMUM_SWAP_AMOUNT_TOLERANCE: bitcoin::Amount = bitcoin::Amount::from_sat(100);

/// The smallest amount in `amounts` that setup accepts for `ratio` and `rate`,
/// or None if setup accepts none of them.
///
/// Setup pays for its spending paths out of the swap amount, and it withholds
/// the anti-spam deposit from the refund. The fees of the presigned
/// transactions do not change with the swap amount, while the deposit and the
/// fee budget do. A small swap therefore cannot pay for its refund path, and
/// setup rejects it. Quotes use this amount as their minimum, so that every
/// amount they offer is one the maker can also set up.
///
/// Neither the taker's refund address nor the maker's own addresses are known
/// when the quote is made. The calculation thus uses the output scripts that
/// need the largest deposit. The result is an amount that setup accepts, up to
/// [`MINIMUM_SWAP_AMOUNT_TOLERANCE`] above the smallest such amount.
pub fn minimum_swap_amount(
    amounts: std::ops::RangeInclusive<bitcoin::Amount>,
    ratio: rust_decimal::Decimal,
    rate: bitcoin::SwapFeeRate,
    redeem_multiplier: bitcoin::RedeemFeeMultiplier,
    network: bitcoin::Network,
) -> Option<bitcoin::Amount> {
    if amounts.is_empty() {
        return None;
    }

    let accepts =
        |amount| setup_accepts_worst_case(amount, ratio, rate, redeem_multiplier, network);

    let (mut smallest, mut largest) = (*amounts.start(), *amounts.end());

    // Usually the maker can set up its smallest amount, and one calculation
    // answers the question.
    if accepts(smallest).is_ok() {
        return Some(smallest);
    }

    // The maker offers nothing if it cannot set up its largest amount either.
    if let Err(error) = accepts(largest) {
        tracing::debug!(
            %largest,
            error = %format!("{error:#}"),
            "The maker cannot set up a swap of its maximum amount"
        );
        return None;
    }

    // Every check that setup makes on the amount only becomes easier as the
    // amount grows: the deposit and the fee budget of a spending path grow
    // with the amount, and the transaction fees do not. A binary search
    // therefore finds the smallest amount that passes all of them.
    while largest.to_sat() - smallest.to_sat() > MINIMUM_SWAP_AMOUNT_TOLERANCE.to_sat() {
        let middle = bitcoin::Amount::from_sat(
            smallest.to_sat() + (largest.to_sat() - smallest.to_sat()) / 2,
        );
        if accepts(middle).is_ok() {
            largest = middle;
        } else {
            smallest = middle + bitcoin::Amount::ONE_SAT;
        }
    }

    // Setup accepts the upper end of the remaining window, but not necessarily
    // the lower one.
    Some(largest)
}

/// Whether setup accepts a swap of `amount`, for the output scripts that need
/// the largest anti-spam deposit.
///
/// This is the same calculation that setup makes, with placeholder keys and
/// addresses. The keys do not change the size of a transaction. The addresses
/// do, and so does their dust limit, so the swap must pass the calculation for
/// each address type the taker may use.
fn setup_accepts_worst_case(
    amount: bitcoin::Amount,
    ratio: rust_decimal::Decimal,
    rate: bitcoin::SwapFeeRate,
    redeem_multiplier: bitcoin::RedeemFeeMultiplier,
    network: bitcoin::Network,
) -> Result<()> {
    use ::bitcoin::hashes::Hash;

    // A 34-byte witness program is the largest standard output script, and
    // thus gives the largest transaction fees. A P2PKH output has the largest
    // dust limit. No address has both properties, so the swap must pay for
    // both.
    let largest_output = bitcoin::Address::from_script(
        &::bitcoin::ScriptBuf::new_p2wsh(&::bitcoin::WScriptHash::all_zeros()),
        network,
    )
    .context("Failed to build a placeholder P2WSH address")?;
    let largest_dust = bitcoin::Address::from_script(
        &::bitcoin::ScriptBuf::new_p2pkh(&::bitcoin::PubkeyHash::all_zeros()),
        network,
    )
    .context("Failed to build a placeholder P2PKH address")?;

    let deposit = anti_spam_deposit(ratio, amount)?;
    let limits = swap_fee_limits(rate, amount)?;

    for refund_address in [&largest_output, &largest_dust] {
        calculate_swap_fees(
            bitcoin::SwapFeeInputs {
                a: swap_core::bitcoin::PublicKey::random(),
                b: swap_core::bitcoin::PublicKey::random(),
                redeem_address: &largest_output,
                refund_address,
                punish_address: &largest_output,
                lock_amount: amount,
                deposit,
                redeem_multiplier,
            },
            rate,
            limits,
        )?;
    }

    Ok(())
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Alice(AliceState),
    Bob(BobState),
}

impl State {
    pub fn swap_finished(&self) -> bool {
        match self {
            State::Alice(state) => alice_is_complete(state),
            State::Bob(state) => bob_is_complete(state),
        }
    }
}

impl From<AliceState> for State {
    fn from(alice: AliceState) -> Self {
        Self::Alice(alice)
    }
}

impl From<BobState> for State {
    fn from(bob: BobState) -> Self {
        Self::Bob(bob)
    }
}

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
#[error("Not in the role of Alice")]
pub struct NotAlice;

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
#[error("Not in the role of Bob")]
pub struct NotBob;

impl TryInto<BobState> for State {
    type Error = NotBob;

    fn try_into(self) -> std::result::Result<BobState, Self::Error> {
        match self {
            State::Alice(_) => Err(NotBob),
            State::Bob(state) => Ok(state),
        }
    }
}

impl TryInto<AliceState> for State {
    type Error = NotAlice;

    fn try_into(self) -> std::result::Result<AliceState, Self::Error> {
        match self {
            State::Alice(state) => Ok(state),
            State::Bob(_) => Err(NotAlice),
        }
    }
}

/// Recovery data for insertion into the database.
#[derive(Clone, Debug)]
pub struct RecoveredSwap {
    pub swap_id: Uuid,
    pub alice_peer_id: PeerId,
    pub monero_receive_pool: MoneroAddressPool,
    pub state: BobState,
    /// The Apollo transaction that holds the swap backup, as found on chain.
    pub apollo: ApolloProgress,
}

/// Pending progress rows from [`Database::pending_apollo_progress`], split into rows that decode and rows that do not.
#[derive(Debug, Default)]
pub struct PendingApollo {
    pub ready: Vec<(Uuid, ApolloProgress)>,
    /// The stored swap ID and the reason the progress row cannot be decoded.
    pub corrupt: Vec<(String, anyhow::Error)>,
}

#[async_trait]
pub trait Database {
    /// Restores a missing swap and its first progress row in one atomic write.
    /// Returns false and changes nothing if a state already exists. A failed write must leave no partial data.
    async fn insert_recovered_swap(&self, recovered: RecoveredSwap) -> Result<bool>;

    async fn insert_peer_id(&self, swap_id: Uuid, peer_id: PeerId) -> Result<()>;
    async fn get_peer_id(&self, swap_id: Uuid) -> Result<PeerId>;
    async fn insert_monero_address_pool(
        &self,
        swap_id: Uuid,
        address: MoneroAddressPool,
    ) -> Result<()>;
    async fn get_monero_address_pool(&self, swap_id: Uuid) -> Result<MoneroAddressPool>;
    async fn get_monero_addresses(&self) -> Result<Vec<::monero_address::MoneroAddress>>;
    async fn insert_address(&self, peer_id: PeerId, address: Multiaddr) -> Result<()>;
    async fn get_addresses(&self, peer_id: PeerId) -> Result<Vec<Multiaddr>>;
    async fn get_all_peer_addresses(&self) -> Result<Vec<(PeerId, Vec<Multiaddr>)>>;
    async fn get_swap_start_date(&self, swap_id: Uuid) -> Result<String>;
    async fn insert_latest_state(&self, swap_id: Uuid, state: State) -> Result<()>;
    async fn get_state(&self, swap_id: Uuid) -> Result<State>;
    /// Returns the latest progress row of a swap, or `None` if the swap has no progress row.
    async fn get_apollo_progress(&self, swap_id: Uuid) -> Result<Option<ApolloProgress>>;
    /// Saves `progress` as the first progress row of a swap, only if the swap has no progress row.
    /// Returns whether it saved the row. The check and the write are atomic.
    async fn insert_apollo_if_absent(&self, swap_id: Uuid, progress: ApolloProgress)
    -> Result<bool>;
    /// Appends `progress` as a new progress row of a swap, without a check.
    /// After `insert_apollo_if_absent`, only the publisher calls this function.
    async fn append_apollo_progress(&self, swap_id: Uuid, progress: ApolloProgress) -> Result<()>;
    /// Returns the latest progress row of each swap whose Apollo transaction is neither confirmed nor abandoned.
    /// Rows that cannot be decoded are returned separately in `corrupt`, so one corrupt row does not hide the others.
    async fn pending_apollo_progress(&self) -> Result<PendingApollo>;
    async fn get_states(&self, swap_id: Uuid) -> Result<Vec<State>>;
    async fn all(&self) -> Result<Vec<(PeerId, Uuid, State)>>;
    /// Returns the Alice swaps whose latest state is `Started` or
    /// `BtcLockTransactionSeen` and was recorded at or after `updated_since`
    /// (UTC Unix seconds). Rows that cannot be read are logged and skipped.
    async fn get_alice_swaps_with_pending_tx_lock(
        &self,
        updated_since: i64,
    ) -> Result<Vec<(Uuid, State)>>;
    /// Same as `all` but paginated, and returns the first and last state per
    /// swap. `Alice::Started` does not count as a first state, so swaps whose
    /// lock transaction was never seen are filtered out. Implementations may
    /// also filter out terminally-aborted swaps.
    async fn all_paginated(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<(PeerId, Uuid, State, State)>>;

    /// Returns the current (latest) state and the starting state for a swap.
    async fn get_current_and_starting_state(&self, swap_id: Uuid) -> Result<(State, State)> {
        use anyhow::Context;

        let states = self
            .get_states(swap_id)
            .await
            .context("Error fetching all states of swap from database")?;
        let starting = states.first().cloned().context("No states found")?;
        let current = states.last().cloned().context("No states found")?;

        // Sanity check: both states must be from the same role
        match (&current, &starting) {
            (State::Alice(_), State::Alice(_)) | (State::Bob(_), State::Bob(_)) => {}
            _ => anyhow::bail!("Current and starting states have mismatched roles"),
        }

        Ok((current, starting))
    }
    async fn insert_buffered_transfer_proof(
        &self,
        swap_id: Uuid,
        proof: monero::TransferProof,
    ) -> Result<()>;
    async fn get_buffered_transfer_proof(
        &self,
        swap_id: Uuid,
    ) -> Result<Option<monero::TransferProof>>;
    async fn has_swap(&self, swap_id: Uuid) -> Result<bool>;
    /// Persists the peer and the first state of a new swap in one transaction.
    /// Gives up and persists nothing if it cannot get there within `timeout`.
    async fn insert_new_swap(
        &self,
        swap_id: Uuid,
        peer_id: PeerId,
        state: State,
        timeout: Duration,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn lock_psbt(inputs: usize) -> bitcoin::PartiallySignedTransaction {
        use ::bitcoin::hashes::Hash;
        bitcoin::PartiallySignedTransaction::from_unsigned_tx(bitcoin::Transaction {
            version: ::bitcoin::transaction::Version::TWO,
            lock_time: ::bitcoin::absolute::LockTime::ZERO,
            input: vec![::bitcoin::TxIn::default(); inputs],
            output: vec![::bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(100_000),
                script_pubkey: ::bitcoin::ScriptBuf::new_p2wsh(
                    &::bitcoin::WScriptHash::all_zeros(),
                ),
            }],
        })
        .unwrap()
    }

    fn check_lock_fee_floor(fee: u64, inputs: usize) -> Result<()> {
        sanity_check_transaction_fee_floor(
            bitcoin::Amount::from_sat(fee),
            bitcoin::estimated_signed_lock_weight(&lock_psbt(inputs)),
            ::bitcoin::FeeRate::from_sat_per_vb(10).unwrap(),
            bitcoin::Amount::ONE_BTC,
        )
    }

    fn fee_too_low(result: Result<()>) -> SetupValidationError {
        *result.unwrap_err().downcast_ref().unwrap()
    }

    fn accepts(amount: u64, ratio: rust_decimal::Decimal, rate: u64) -> Result<()> {
        setup_accepts_worst_case(
            bitcoin::Amount::from_sat(amount),
            ratio,
            bitcoin::SwapFeeRate(rate),
            bitcoin::RedeemFeeMultiplier::default(),
            bitcoin::Network::Regtest,
        )
    }

    fn smallest_accepted(ratio: rust_decimal::Decimal, rate: u64) -> Option<bitcoin::Amount> {
        minimum_swap_amount(
            bitcoin::Amount::ONE_SAT..=bitcoin::Amount::MAX_MONEY,
            ratio,
            bitcoin::SwapFeeRate(rate),
            bitcoin::RedeemFeeMultiplier::default(),
            bitcoin::Network::Regtest,
        )
    }

    /// The quote offers this amount, so setup must accept it. One tolerance
    /// less must fail, or the quote asks for more than it has to.
    #[test]
    fn the_minimum_swap_amount_is_the_smallest_one_setup_accepts() {
        let tolerance = MINIMUM_SWAP_AMOUNT_TOLERANCE.to_sat();

        for ratio in ["0", "0.0001", "0.001", "0.01", "0.2"] {
            let ratio: rust_decimal::Decimal = ratio.parse().unwrap();
            for rate in [1_000, 2_500, 10_000, 50_000] {
                let minimum = smallest_accepted(ratio, rate).unwrap().to_sat();

                accepts(minimum, ratio, rate).unwrap_or_else(|error| {
                    panic!(
                        "Setup rejects {minimum} sat at ratio {ratio} and {rate} sat/kvB: {error:#}"
                    )
                });
                assert!(
                    accepts(minimum - tolerance - 1, ratio, rate).is_err(),
                    "Setup also accepts {} sat at ratio {ratio} and {rate} sat/kvB",
                    minimum - tolerance - 1
                );
                // The binary search relies on this: a larger swap pays for its
                // refund path whenever a smaller one does.
                for larger in [minimum + 1, minimum * 2, minimum * 100] {
                    accepts(larger, ratio, rate).unwrap();
                }
            }
        }
    }

    /// The maker pays for a refund path only up to an absolute limit. Above a
    /// fee rate that no swap can pay for, it cannot offer an amount at all.
    #[test]
    fn no_swap_amount_pays_for_the_refund_path_at_a_very_high_fee_rate() {
        assert_eq!(smallest_accepted("0.01".parse().unwrap(), 1_000_000), None);
    }

    /// The search stays inside the amounts the maker is configured to trade.
    #[test]
    fn the_minimum_swap_amount_stays_within_the_configured_amounts() {
        let ratio: rust_decimal::Decimal = "0.001".parse().unwrap();
        let rate = bitcoin::SwapFeeRate(5_000);
        let multiplier = bitcoin::RedeemFeeMultiplier::default();
        let smallest = smallest_accepted(ratio, rate.0).unwrap();

        // A configured minimum that setup accepts is quoted as it is.
        let min_buy = smallest * 2;
        assert_eq!(
            minimum_swap_amount(
                min_buy..=min_buy * 4,
                ratio,
                rate,
                multiplier,
                bitcoin::Network::Regtest
            ),
            Some(min_buy)
        );

        // The maker offers nothing when setup accepts none of its amounts. The
        // search is exact only up to its tolerance, so stay below that.
        assert_eq!(
            minimum_swap_amount(
                bitcoin::Amount::ONE_SAT
                    ..=smallest - MINIMUM_SWAP_AMOUNT_TOLERANCE - bitcoin::Amount::ONE_SAT,
                ratio,
                rate,
                multiplier,
                bitcoin::Network::Regtest
            ),
            None
        );
    }

    /// An anti-spam deposit of a tenth of a percent needs a swap of at least
    /// a few hundred times the fee of one transaction.
    #[test]
    fn a_small_deposit_ratio_needs_a_large_swap() {
        let ratio: rust_decimal::Decimal = "0.001".parse().unwrap();
        let minimum = smallest_accepted(ratio, 5_000).unwrap();
        let without_deposit = smallest_accepted(rust_decimal::Decimal::ZERO, 5_000).unwrap();

        assert!(
            minimum > without_deposit * 100,
            "{minimum} is not much larger than {without_deposit}"
        );
        assert_eq!(
            anti_spam_deposit(ratio, minimum).unwrap(),
            bitcoin::Amount::from_sat(minimum.to_sat() / 1000)
        );
    }

    #[test]
    fn lock_fee_floor_preserves_the_single_input_boundary() {
        // 122 vB at 10 sat/vB: the fixed-weight estimate rounded up to a virtual byte.
        assert!(check_lock_fee_floor(610, 1).is_ok());
        assert!(check_lock_fee_floor(10_000, 1).is_ok());
        assert_eq!(
            fee_too_low(check_lock_fee_floor(609, 1)),
            SetupValidationError::TransactionFeeTooLow {
                fee: bitcoin::Amount::from_sat(609),
                estimate: bitcoin::Amount::from_sat(1_220),
            }
        );
    }

    #[test]
    fn lock_fee_floor_scales_with_the_number_of_inputs() {
        let inputs = bitcoin::MAX_TX_LOCK_INPUTS;
        // 1,754 vB at 10 sat/vB.
        assert_eq!(
            fee_too_low(check_lock_fee_floor(610, inputs)),
            SetupValidationError::TransactionFeeTooLow {
                fee: bitcoin::Amount::from_sat(610),
                estimate: bitcoin::Amount::from_sat(17_540),
            }
        );
        assert!(check_lock_fee_floor(8_769, inputs).is_err());
        assert!(check_lock_fee_floor(8_770, inputs).is_ok());
    }

    #[test]
    fn lock_fee_floor_keeps_the_absolute_minimum_estimate() {
        // A 1 sat/vB estimate is raised to the 1,000 sat minimum relay fee.
        let check = |fee| {
            sanity_check_transaction_fee_floor(
                bitcoin::Amount::from_sat(fee),
                bitcoin::estimated_signed_lock_weight(&lock_psbt(1)),
                ::bitcoin::FeeRate::from_sat_per_vb(1).unwrap(),
                bitcoin::Amount::ONE_BTC,
            )
        };
        assert!(check(499).is_err());
        assert!(check(500).is_ok());
    }

    #[test]
    fn pedersen_blinding_generators_are_not_the_curve_generators() {
        let _force_init = &*CROSS_CURVE_PROOF_SYSTEM;

        let secp_generator = (*ecdsa_fun::fun::G).normalize();
        let ed25519_generator = curve25519_dalek_ng::constants::ED25519_BASEPOINT_POINT;
        let blinding_h_ed25519 = pedersen_blinding_h_ed25519();

        assert_ne!(
            *PEDERSEN_BLINDING_H_SECP256K1, secp_generator,
            "secp256k1 Pedersen blinding generator must differ from the curve generator"
        );
        assert_ne!(
            blinding_h_ed25519, ed25519_generator,
            "ed25519 Pedersen blinding generator must differ from the curve generator"
        );
    }

    #[test]
    fn secp256k1_pedersen_blinding_generator_matches_bip341() {
        const BIP341_NUMS_X_COORDINATE: [u8; 32] = [
            0x50, 0x92, 0x9b, 0x74, 0xc1, 0xa0, 0x49, 0x54, 0xb7, 0x8b, 0x4b, 0x60, 0x35, 0xe9,
            0x7a, 0x5e, 0x07, 0x8a, 0x5a, 0x0f, 0x28, 0xec, 0x96, 0xd5, 0x47, 0xbf, 0xee, 0x9a,
            0xce, 0x80, 0x3a, 0xc0,
        ];

        assert_eq!(
            PEDERSEN_BLINDING_H_SECP256K1.coordinates().0,
            BIP341_NUMS_X_COORDINATE
        );
        assert!(PEDERSEN_BLINDING_H_SECP256K1.is_y_even());
    }

    #[test]
    fn honest_cross_curve_proof_verifies() {
        use curve25519_dalek_ng::scalar::Scalar;

        let mut rng = rand_chacha::ChaCha20Rng::from_seed([7u8; 32]);
        let secret = clamp_to_252_bits(Scalar::random(&mut rng));

        let (proof, claim) = CROSS_CURVE_PROOF_SYSTEM.prove(&secret, &mut rng);

        assert!(
            CROSS_CURVE_PROOF_SYSTEM.verify(&proof, claim),
            "an honestly generated cross-curve proof must verify"
        );
    }

    #[test]
    fn cross_curve_proof_does_not_verify_against_mismatched_ed25519_key() {
        use curve25519_dalek_ng::constants::ED25519_BASEPOINT_TABLE;
        use curve25519_dalek_ng::scalar::Scalar;

        let mut rng = rand_chacha::ChaCha20Rng::from_seed([9u8; 32]);
        let secret = clamp_to_252_bits(Scalar::random(&mut rng));

        let (proof, (claim_secp, _claim_ed25519)) =
            CROSS_CURVE_PROOF_SYSTEM.prove(&secret, &mut rng);

        let unrelated_ed25519 =
            &clamp_to_252_bits(Scalar::random(&mut rng)) * &ED25519_BASEPOINT_TABLE;

        assert!(
            !CROSS_CURVE_PROOF_SYSTEM.verify(&proof, (claim_secp, unrelated_ed25519)),
            "a proof must not verify when the ed25519 key has a different discrete log"
        );
    }

    fn clamp_to_252_bits(
        scalar: curve25519_dalek_ng::scalar::Scalar,
    ) -> curve25519_dalek_ng::scalar::Scalar {
        let mut bytes = scalar.to_bytes();
        bytes[31] &= 0b0000_1111;
        curve25519_dalek_ng::scalar::Scalar::from_bytes_mod_order(bytes)
    }
}
