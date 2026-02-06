use crate::alice::AliceState;
use crate::alice::is_complete as alice_is_complete;
use crate::bob::BobState;
use crate::bob::is_complete as bob_is_complete;
use anyhow::{Result, bail};
use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId};
use rust_decimal::prelude::FromPrimitive;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sigma_fun::HashTranscript;
use sigma_fun::ext::dl_secp256k1_ed25519_eq::{CrossCurveDLEQ, CrossCurveDLEQProof};
use std::convert::TryInto;
use std::sync::LazyLock;
use swap_core::bitcoin;
use swap_core::monero::{self, MoneroAddressPool};
use uuid::Uuid;

pub static CROSS_CURVE_PROOF_SYSTEM: LazyLock<
    CrossCurveDLEQ<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>,
> = LazyLock::new(|| {
    CrossCurveDLEQ::<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>::new(
        (*ecdsa_fun::fun::G).normalize(),
        curve25519_dalek_ng::constants::ED25519_BASEPOINT_POINT,
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
    pub tx_refund_amnesty_fee: bitcoin::Amount,
    pub tx_cancel_fee: bitcoin::Amount,
    pub tx_final_amnesty_fee: bitcoin::Amount,
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
    pub tx_refund_burn_fee: bitcoin::Amount,
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
    pub tx_refund_amnesty_sig: Option<bitcoin::Signature>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message4 {
    pub tx_punish_sig: bitcoin::Signature,
    pub tx_cancel_sig: bitcoin::Signature,
    pub tx_early_refund_sig: bitcoin::Signature,
    pub tx_refund_amnesty_sig: Option<bitcoin::Signature>,
    pub tx_refund_burn_sig: Option<bitcoin::Signature>,
    pub tx_final_amnesty_sig: Option<bitcoin::Signature>,
}

/// Validates that the amnesty amount is within sane bounds.
///
/// - If amnesty is zero, this is a full-refund swap and no checks are needed.
/// - Otherwise, the amnesty must cover all transaction fees that could be spent
///   from it (TxPartialRefund, TxReclaim, TxWithhold, TxMercy).
/// - The amnesty ratio (amnesty / lock amount) must not exceed
///   [`swap_env::config::MAX_ANTI_SPAM_DEPOSIT_RATIO`].
pub fn sanity_check_amnesty_amount(
    lock_amount: bitcoin::Amount,
    amnesty_amount: bitcoin::Amount,
    tx_partial_refund_fee: bitcoin::Amount,
    tx_reclaim_fee: bitcoin::Amount,
    tx_withhold_fee: bitcoin::Amount,
    tx_mercy_fee: bitcoin::Amount,
) -> Result<()> {
    if amnesty_amount == bitcoin::Amount::ZERO {
        return Ok(());
    }

    let min_amnesty = tx_partial_refund_fee + tx_reclaim_fee + tx_withhold_fee + tx_mercy_fee;
    if amnesty_amount < min_amnesty {
        bail!(
            "Amnesty amount ({amnesty_amount}) is less than the combined fees \
             for TxPartialRefund ({tx_partial_refund_fee}), TxReclaim ({tx_reclaim_fee}), \
             TxWithhold ({tx_withhold_fee}), and TxMercy ({tx_mercy_fee}). \
             The deposit would be consumed by fees.",
        );
    }

    let amnesty_sats = rust_decimal::Decimal::from_u64(amnesty_amount.to_sat())
        .expect("amnesty sats to fit in Decimal");
    let lock_sats =
        rust_decimal::Decimal::from_u64(lock_amount.to_sat()).expect("lock sats to fit in Decimal");
    let ratio = amnesty_sats / lock_sats;

    if ratio > swap_env::config::MAX_ANTI_SPAM_DEPOSIT_RATIO {
        bail!(
            "Amnesty ratio ({ratio}) exceeds maximum allowed ratio of {}. \
             The requested deposit is unreasonably high.",
            swap_env::config::MAX_ANTI_SPAM_DEPOSIT_RATIO,
        );
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
    async fn all(&self) -> Result<Vec<(Uuid, State)>>;

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

    /// 1 BTC lock amount.
    const LOCK: bitcoin::Amount = bitcoin::Amount::from_sat(100_000_000);
    const FEE: bitcoin::Amount = MIN_ABSOLUTE_TX_FEE;
    /// Sum of all 4 fees (the lower bound).
    const FEE_FLOOR: u64 = MIN_ABSOLUTE_TX_FEE_SATS * 4;
    /// 20% of LOCK (the upper bound).
    const RATIO_CEILING: u64 = 20_000_000;

    #[test]
    fn zero_amnesty_always_passes() {
        sanity_check_amnesty_amount(LOCK, bitcoin::Amount::ZERO, FEE, FEE, FEE, FEE)
            .expect("zero amnesty should always pass");
    }

    #[test]
    fn reject_amnesty_below_fee_floor() {
        let amnesty = bitcoin::Amount::from_sat(FEE_FLOOR - 1);
        sanity_check_amnesty_amount(LOCK, amnesty, FEE, FEE, FEE, FEE)
            .expect_err("amnesty below fee floor should be rejected");
    }

    #[test]
    fn pass_amnesty_at_fee_floor() {
        let amnesty = bitcoin::Amount::from_sat(FEE_FLOOR);
        sanity_check_amnesty_amount(LOCK, amnesty, FEE, FEE, FEE, FEE)
            .expect("amnesty exactly at fee floor should pass");
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
