use crate::alice::AliceState;
use crate::alice::is_complete as alice_is_complete;
use crate::bob::BobState;
use crate::bob::is_complete as bob_is_complete;
use anyhow::Result;
use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId};
use rust_decimal::prelude::FromPrimitive;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sigma_fun::HashTranscript;
use sigma_fun::ext::dl_secp256k1_ed25519_eq::{CrossCurveDLEQ, CrossCurveDLEQProof};
use std::convert::TryInto;
use std::sync::LazyLock;
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
    pub tx_refund_fee: bitcoin::Amount,
    pub tx_partial_refund_fee: bitcoin::Amount,
    pub tx_reclaim_fee: bitcoin::Amount,
    pub tx_cancel_fee: bitcoin::Amount,
    pub tx_mercy_fee: bitcoin::Amount,
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
    pub tx_redeem_fee: bitcoin::Amount,
    pub tx_punish_fee: bitcoin::Amount,
    /// The amount of Bitcoin that Bob not get refunded unless Alice decides so.
    /// Introduced in [#675](https://github.com/eigenwallet/core/pull/675) to combat spam.
    pub amnesty_amount: bitcoin::Amount,
    pub tx_withhold_fee: bitcoin::Amount,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message2 {
    pub tx_lock_psbt: bitcoin::PartiallySignedTransaction,
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

/// Ensure the proposed fee for a transaction is in a sensible range
/// around our own estimate.
pub fn sanity_check_transaction_fee(
    proposed_fee: bitcoin::Amount,
    conservative_estimated_fee: bitcoin::Amount,
) -> Result<(), SanityCheckError> {
    // Allow maximum 300% of the fee we'd set
    const MAX_FEE_OVERPAY_FACTOR: u64 = 3;

    sanity_check_transaction_fee_floor(proposed_fee, conservative_estimated_fee)?;

    let ceiling = bitcoin::Amount::from_sat(
        conservative_estimated_fee
            .to_sat()
            .saturating_mul(MAX_FEE_OVERPAY_FACTOR),
    );

    if proposed_fee > ceiling {
        return Err(SanityCheckError::TransactionFeeTooHigh {
            proposed: proposed_fee,
            our_estimate: conservative_estimated_fee,
        });
    }

    Ok(())
}

/// Floor-only counterpart to [`sanity_check_transaction_fee`], for fees where
/// only underpayment concerns us.
pub fn sanity_check_transaction_fee_floor(
    proposed_fee: bitcoin::Amount,
    conservative_estimated_fee: bitcoin::Amount,
) -> Result<(), SanityCheckError> {
    // Require minimum 50% of the fee we'd set
    const MAX_FEE_UNDERPAY_FACTOR: u64 = 2;

    let floor = bitcoin::Amount::from_sat(
        conservative_estimated_fee
            .to_sat()
            .saturating_div(MAX_FEE_UNDERPAY_FACTOR),
    );

    if proposed_fee < floor {
        return Err(SanityCheckError::TransactionFeeTooLow {
            proposed: proposed_fee,
            our_estimate: conservative_estimated_fee,
        });
    }

    Ok(())
}

/// Number of transactions in the withhold path (TxPartialRefund + TxWithhold + TxMercy).
pub const NUM_WITHHOLD_PATH_TXS: u64 = 3;

#[derive(Clone, Debug, thiserror::Error)]
pub enum SanityCheckError {
    #[error("Anti-spam deposit ({amount}) doesn't cover fees (minimum: {minimum_to_cover_fees})")]
    AntiSpamDepositTooSmall {
        amount: bitcoin::Amount,
        minimum_to_cover_fees: bitcoin::Amount,
    },
    #[error("Anti-spam deposit ratio ({ratio}) exceeds maximum accepted ({max_accepted_ratio})")]
    AntiSpamDepositRatioTooHigh {
        ratio: rust_decimal::Decimal,
        max_accepted_ratio: rust_decimal::Decimal,
    },
    #[error(
        "Other party suggested a network fee which is too low compared to our estimate ({proposed} vs {our_estimate})"
    )]
    TransactionFeeTooLow {
        proposed: bitcoin::Amount,
        our_estimate: bitcoin::Amount,
    },
    #[error(
        "Other party suggested a network fee which is too high compared to our estimate ({proposed} vs {our_estimate})"
    )]
    TransactionFeeTooHigh {
        proposed: bitcoin::Amount,
        our_estimate: bitcoin::Amount,
    },
}

/// Validates that the amnesty amount is within sane bounds.
///
/// - If amnesty is zero, this is a full-refund swap and no checks are needed.
/// - Otherwise, the amnesty must cover all transaction fees that could be spent
///   from it (TxPartialRefund + TxReclaim, or TxPartialRefund + TxWithhold + TxMercy).
/// - The amnesty ratio (amnesty / lock amount) must not exceed
///   [`swap_env::config::MAX_ANTI_SPAM_DEPOSIT_RATIO`].
pub fn sanity_check_amnesty_amount(
    lock_amount: bitcoin::Amount,
    amnesty_amount: bitcoin::Amount,
    tx_partial_refund_fee: bitcoin::Amount,
    tx_reclaim_fee: bitcoin::Amount,
    tx_withhold_fee: bitcoin::Amount,
    tx_mercy_fee: bitcoin::Amount,
) -> std::result::Result<(), SanityCheckError> {
    if amnesty_amount == bitcoin::Amount::ZERO {
        return Ok(());
    }

    let reclaim_path = tx_partial_refund_fee + tx_reclaim_fee;
    if amnesty_amount <= reclaim_path {
        return Err(SanityCheckError::AntiSpamDepositTooSmall {
            amount: amnesty_amount,
            minimum_to_cover_fees: reclaim_path,
        });
    }

    let withhold_path = tx_partial_refund_fee + tx_withhold_fee + tx_mercy_fee;
    if amnesty_amount <= withhold_path {
        return Err(SanityCheckError::AntiSpamDepositTooSmall {
            amount: amnesty_amount,
            minimum_to_cover_fees: withhold_path,
        });
    }

    let amnesty_sats = rust_decimal::Decimal::from_u64(amnesty_amount.to_sat())
        .expect("amnesty sats to fit in Decimal");
    let lock_sats =
        rust_decimal::Decimal::from_u64(lock_amount.to_sat()).expect("lock sats to fit in Decimal");
    let ratio = amnesty_sats / lock_sats;

    if ratio > swap_env::config::MAX_ANTI_SPAM_DEPOSIT_RATIO {
        return Err(SanityCheckError::AntiSpamDepositRatioTooHigh {
            ratio,
            max_accepted_ratio: swap_env::config::MAX_ANTI_SPAM_DEPOSIT_RATIO,
        });
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

#[async_trait]
pub trait Database {
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
    async fn get_states(&self, swap_id: Uuid) -> Result<Vec<State>>;
    async fn all(&self) -> Result<Vec<(PeerId, Uuid, State)>>;
    /// Same as `all` but paginated, and returns the first and last state per
    /// swap. Implementations may filter out terminally-aborted swaps.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin_wallet::{MIN_ABSOLUTE_TX_FEE, MIN_ABSOLUTE_TX_FEE_SATS};
    use rand::SeedableRng;

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

    /// 1 BTC lock amount.
    const LOCK: bitcoin::Amount = bitcoin::Amount::from_sat(100_000_000);
    const FEE: bitcoin::Amount = MIN_ABSOLUTE_TX_FEE;
    /// Withhold path: TxPartialRefund + TxWithhold + TxMercy (the binding constraint when all fees are equal).
    const WITHHOLD_PATH: u64 = MIN_ABSOLUTE_TX_FEE_SATS * NUM_WITHHOLD_PATH_TXS;
    /// 20% of LOCK (the upper bound).
    const RATIO_CEILING: u64 = 20_000_000;

    #[test]
    fn zero_amnesty_always_passes() {
        sanity_check_amnesty_amount(LOCK, bitcoin::Amount::ZERO, FEE, FEE, FEE, FEE)
            .expect("zero amnesty should always pass");
    }

    #[test]
    fn reject_amnesty_below_withhold_path() {
        let amnesty = bitcoin::Amount::from_sat(WITHHOLD_PATH - 1);
        sanity_check_amnesty_amount(LOCK, amnesty, FEE, FEE, FEE, FEE)
            .expect_err("amnesty below withhold path fees should be rejected");
    }

    #[test]
    fn reject_amnesty_equal_to_withhold_path() {
        let amnesty = bitcoin::Amount::from_sat(WITHHOLD_PATH);
        sanity_check_amnesty_amount(LOCK, amnesty, FEE, FEE, FEE, FEE)
            .expect_err("amnesty equal to withhold path fees should be rejected");
    }

    #[test]
    fn pass_amnesty_above_withhold_path() {
        let amnesty = bitcoin::Amount::from_sat(WITHHOLD_PATH + 1);
        sanity_check_amnesty_amount(LOCK, amnesty, FEE, FEE, FEE, FEE)
            .expect("amnesty above withhold path fees should pass");
    }

    #[test]
    fn pass_medium_amnesty() {
        let amnesty = bitcoin::Amount::from_sat(10_000_000);
        sanity_check_amnesty_amount(LOCK, amnesty, FEE, FEE, FEE, FEE)
            .expect("10% amnesty should pass");
    }

    #[test]
    fn pass_amnesty_at_ratio_ceiling() {
        let amnesty = bitcoin::Amount::from_sat(RATIO_CEILING);
        sanity_check_amnesty_amount(LOCK, amnesty, FEE, FEE, FEE, FEE)
            .expect("amnesty exactly at 20% ratio should pass");
    }

    #[test]
    fn reject_amnesty_above_ratio_ceiling() {
        let amnesty = bitcoin::Amount::from_sat(RATIO_CEILING + 1);
        sanity_check_amnesty_amount(LOCK, amnesty, FEE, FEE, FEE, FEE)
            .expect_err("amnesty above 20% ratio should be rejected");
    }
}
