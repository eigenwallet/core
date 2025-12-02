use crate::alice::is_complete as alice_is_complete;
use crate::alice::AliceState;
use crate::bob::is_complete as bob_is_complete;
use crate::bob::BobState;
use anyhow::Result;
use async_trait::async_trait;
use conquer_once::Lazy;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sigma_fun::ext::dl_secp256k1_ed25519_eq::{CrossCurveDLEQ, CrossCurveDLEQProof};
use sigma_fun::HashTranscript;
use std::convert::TryInto;
use swap_core::bitcoin;
use swap_core::monero::{self, MoneroAddressPool};
use uuid::Uuid;

pub static CROSS_CURVE_PROOF_SYSTEM: Lazy<
    CrossCurveDLEQ<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>,
> = Lazy::new(|| {
    CrossCurveDLEQ::<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>::new(
        (*ecdsa_fun::fun::G).normalize(),
        curve25519_dalek::constants::ED25519_BASEPOINT_POINT,
    )
});

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message0 {
    pub swap_id: Uuid,
    pub B: bitcoin::PublicKey,
    pub S_b_monero: monero::PublicKey,
    pub S_b_bitcoin: bitcoin::PublicKey,
    pub dleq_proof_s_b: CrossCurveDLEQProof,
    pub v_b: monero::PrivateViewKey,
    #[serde(with = "swap_serde::bitcoin::address_serde")]
    pub refund_address: bitcoin::Address,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_refund_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_partial_refund_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_cancel_fee: bitcoin::Amount,
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
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_redeem_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_punish_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    /// The amount of Bitcoin that Bob not get refunded unless Alice decides so.
    /// Introduced in [#675](https://github.com/eigenwallet/core/pull/675) to combat spam.
    pub amnesty_amount: bitcoin::Amount,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message2 {
    pub psbt: bitcoin::PartiallySignedTransaction,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message3 {
    pub tx_cancel_sig: bitcoin::Signature,
    /// The following fields were reworked in [#675](https://github.com/eigenwallet/core/pull/675).
    /// Alice _may_ choose to commit to a full refund during the swap setup already, but doesn't
    /// have to.
    pub tx_partial_refund_encsig: bitcoin::EncryptedSignature,
    pub tx_full_refund_encsig: Option<bitcoin::EncryptedSignature>,
    pub tx_refund_amnesty_sig: Option<bitcoin::Signature>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message4 {
    pub tx_punish_sig: bitcoin::Signature,
    pub tx_cancel_sig: bitcoin::Signature,
    pub tx_early_refund_sig: bitcoin::Signature,
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
    async fn get_monero_addresses(&self) -> Result<Vec<::monero::Address>>;
    async fn insert_address(&self, peer_id: PeerId, address: Multiaddr) -> Result<()>;
    async fn get_addresses(&self, peer_id: PeerId) -> Result<Vec<Multiaddr>>;
    async fn get_all_peer_addresses(&self) -> Result<Vec<(PeerId, Vec<Multiaddr>)>>;
    async fn get_swap_start_date(&self, swap_id: Uuid) -> Result<String>;
    async fn insert_latest_state(&self, swap_id: Uuid, state: State) -> Result<()>;
    async fn get_state(&self, swap_id: Uuid) -> Result<State>;
    async fn get_states(&self, swap_id: Uuid) -> Result<Vec<State>>;
    async fn all(&self) -> Result<Vec<(Uuid, State)>>;
    async fn insert_buffered_transfer_proof(
        &self,
        swap_id: Uuid,
        proof: monero::TransferProof,
    ) -> Result<()>;
    async fn get_buffered_transfer_proof(
        &self,
        swap_id: Uuid,
    ) -> Result<Option<monero::TransferProof>>;
}
