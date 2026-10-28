use crate::alice::AliceState;
use crate::alice::is_complete as alice_is_complete;
use crate::bob::BobState;
use crate::bob::is_complete as bob_is_complete;
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

/// Require at least half our local fee estimate, preserving the lock-fee
/// floor used before shared fee negotiation. Higher lock fees are allowed.
pub fn sanity_check_transaction_fee_floor(
    proposed_fee: bitcoin::Amount,
    conservative_estimated_fee: bitcoin::Amount,
) -> Result<()> {
    let floor = bitcoin::Amount::from_sat(conservative_estimated_fee.to_sat() / 2);
    anyhow::ensure!(
        proposed_fee >= floor,
        SetupValidationError::TransactionFeeTooLow {
            fee: proposed_fee,
            estimate: conservative_estimated_fee,
        }
    );
    Ok(())
}

/// Local acceptance limits, derived without capping the agreed fee schedule.
pub fn swap_fee_limits(
    rate: bitcoin::SwapFeeRate,
    amount: bitcoin::Amount,
) -> Result<bitcoin::SwapFeeLimits> {
    use anyhow::Context;
    anyhow::ensure!(rate.0 > 0, "Local fee estimate must be positive");
    Ok(bitcoin::SwapFeeLimits {
        min_rate: bitcoin::SwapFeeRate(rate.0 / 2 + rate.0 % 2),
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
    /// Returns the Alice swaps whose latest state is `Started` or
    /// `BtcLockTransactionSeen` and was recorded at or after `updated_since`
    /// (UTC Unix seconds). Filter before decoding state JSON. Skip and log rows
    /// that cannot be read, so that one bad row does not stop every swap setup.
    async fn get_alice_swaps_with_pending_tx_lock(
        &self,
        updated_since: i64,
    ) -> Result<Vec<(Uuid, State)>>;
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

    #[test]
    fn lock_fee_floor_preserves_the_acceptance_boundary() {
        let estimate = bitcoin::Amount::from_sat(100);
        assert!(
            sanity_check_transaction_fee_floor(bitcoin::Amount::from_sat(49), estimate).is_err()
        );
        assert!(
            sanity_check_transaction_fee_floor(bitcoin::Amount::from_sat(50), estimate).is_ok()
        );
        assert!(
            sanity_check_transaction_fee_floor(bitcoin::Amount::from_sat(1000), estimate).is_ok()
        );
        // Preserve the original whole-satoshi rounding for odd estimates.
        assert!(
            sanity_check_transaction_fee_floor(
                bitcoin::Amount::from_sat(50),
                bitcoin::Amount::from_sat(101),
            )
            .is_ok()
        );
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
