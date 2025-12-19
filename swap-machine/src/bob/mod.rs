#![allow(non_snake_case)]

use crate::common::{CROSS_CURVE_PROOF_SYSTEM, Message0, Message1, Message2, Message3, Message4};
use anyhow::{Context, Result, anyhow, bail};
use bitcoin_wallet::primitives::Subscription;
use ecdsa_fun::Signature;
use ecdsa_fun::adaptor::{Adaptor, HashTranscript};
use ecdsa_fun::nonce::Deterministic;
use monero::BlockHeight;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sigma_fun::ext::dl_secp256k1_ed25519_eq::CrossCurveDLEQProof;
use std::fmt;
use std::sync::Arc;
use swap_core::bitcoin::{
    self, CancelTimelock, ExpiredTimelocks, PunishTimelock, RemainingRefundTimelock, Transaction,
    TxCancel, TxFinalAmnesty, TxFullRefund, TxLock, TxPartialRefund, TxRefundAmnesty, TxRefundBurn,
    Txid, current_epoch,
};
use swap_core::compat::IntoDalekNg;
use swap_core::monero;
use swap_core::monero::{ScalarExt, TransferProofMaybeWithTxKey};
use swap_serde::bitcoin::address_serde;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum BobState {
    Started {
        btc_amount: bitcoin::Amount,
        tx_lock_fee: bitcoin::Amount,
        #[serde(with = "address_serde")]
        change_address: bitcoin::Address,
    },
    SwapSetupCompleted(State2),
    BtcLockReadyToPublish {
        btc_lock_tx_signed: Transaction,
        state3: State3,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    BtcLocked {
        state3: State3,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    /// Bob has found a candidate for the Monero lock transaction.
    /// This could either be that Alice sent us the transfer proof or that we have observed an incoming transfer
    /// into the view-only wallet that matches the expected amount.
    ///
    /// Bob has not yet verified that the transaction sends the correct amount of Monero to the wallet.
    /// Bob has not yet ensured that the transaction is fully confirmed.
    XmrLockTransactionCandidate {
        state: State3,
        lock_transfer_proof: TransferProofMaybeWithTxKey,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    /// Bob has seen a Monero transaction for which he has verified that it transfers the correct amount of Monero to the wallet.
    ///
    /// Bob has not yet verified that the transaction is fully confirmed.
    XmrLockTransactionSeen {
        state: State3,
        lock_transfer_proof: TransferProofMaybeWithTxKey,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    /// Bob has verified that the correct amount of Monero has been locked and fully confirmed.
    /// It is safe to transmit the encrypted signature to Alice.
    XmrLocked(State4),
    EncSigSent(State4),
    BtcRedeemed(State5),
    WaitingForCancelTimelockExpiration {
        state: State3,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    CancelTimelockExpired(State6),
    BtcCancelled(State6),
    BtcRefundPublished(State6),
    BtcEarlyRefundPublished(State6),
    BtcPartialRefundPublished(State6),
    BtcRefunded(State6),
    BtcEarlyRefunded(State6),
    BtcPartiallyRefunded(State6),
    BtcAmnestyPublished(State6),
    BtcAmnestyConfirmed(State6),
    /// Waiting for RemainingRefundTimelock to expire after partial refund confirmed.
    /// During this time, Alice may publish TxRefundBurn.
    WaitingForRemainingRefundTimelockExpiration(State6),
    /// RemainingRefundTimelock has expired, we can now publish TxRefundAmnesty.
    RemainingRefundTimelockExpired(State6),
    /// Alice published TxRefundBurn before we could publish TxRefundAmnesty.
    BtcRefundBurnPublished(State6),
    /// TxRefundBurn has been confirmed. The amnesty output is now burnt.
    BtcRefundBurnt(State6),
    /// Alice published TxFinalAmnesty (using our presigned signature) to refund us.
    BtcFinalAmnestyPublished(State6),
    /// TxFinalAmnesty has been confirmed. We received the burnt funds back.
    BtcFinalAmnestyConfirmed(State6),
    XmrRedeemed {
        tx_lock_id: bitcoin::Txid,
    },
    BtcPunished {
        state: State6,
        // TODO: This attribute is redundant and unused
        tx_lock_id: bitcoin::Txid,
    },
    SafelyAborted,
}

/// An enum abstracting over the different combination of
/// refund signatures Alice could have sent us.
/// Maintains backward compatibility with old swaps (which only had the full refund signature).
///
/// # IMPORTANT
/// This enum must be `#[untagged]` and maintain the field names in order to be backwards compatible
/// with the database.
/// Changing any of that is a breaking change.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RefundSignatures {
    /// Alice has only signed the partial refund transaction (most cases).
    /// Includes the amnesty signature which is always provided in new swaps.
    Partial {
        tx_partial_refund_encsig: bitcoin::EncryptedSignature,
        tx_refund_amnesty_sig: bitcoin::Signature,
    },
    /// Alice has signed both the partial and full refund transactions.
    /// Includes the amnesty signature which is always provided in new swaps.
    Full {
        tx_partial_refund_encsig: bitcoin::EncryptedSignature,
        // Serde rename keeps + untagged + flatten keeps this backwards compatible with old swaps in the database.
        #[serde(rename = "tx_refund_encsig")]
        tx_full_refund_encsig: bitcoin::EncryptedSignature,
        tx_refund_amnesty_sig: bitcoin::Signature,
    },
    /// Alice has only signed the full refund transaction.
    /// This is only used to maintain backwards compatibility for older swaps
    /// from before the partial refund protocol change.
    /// See [#675](https://github.com/eigenwallet/core/pull/675).
    Legacy {
        // Serde rename keeps + untagged + flatten keeps this backwards compatible with old swaps in the database.
        #[serde(rename = "tx_refund_encsig")]
        tx_full_refund_encsig: bitcoin::EncryptedSignature,
    },
}

/// Either a full refund or a partial refund
pub enum RefundType {
    Full,
    Partial {
        total_swap_amount: bitcoin::Amount,
        btc_amnesty_amount: bitcoin::Amount,
    },
}

impl fmt::Display for BobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BobState::Started { .. } => write!(f, "quote has been requested"),
            BobState::SwapSetupCompleted(..) => write!(f, "execution setup done"),
            BobState::BtcLockReadyToPublish { .. } => {
                write!(f, "btc lock ready to publish")
            }
            BobState::BtcLocked { .. } => write!(f, "btc is locked"),
            BobState::XmrLockTransactionCandidate { .. } => {
                write!(f, "xmr lock transaction candidate found")
            }
            BobState::XmrLockTransactionSeen { .. } => write!(f, "xmr lock transaction seen"),
            BobState::XmrLocked(..) => write!(f, "xmr is locked"),
            BobState::EncSigSent(..) => write!(f, "encrypted signature is sent"),
            BobState::BtcRedeemed(..) => write!(f, "btc is redeemed"),
            BobState::WaitingForCancelTimelockExpiration { .. } => {
                write!(f, "waiting for cancel timelock expiration")
            }
            BobState::CancelTimelockExpired(..) => write!(f, "cancel timelock is expired"),
            BobState::BtcCancelled(..) => write!(f, "btc is cancelled"),
            BobState::BtcRefundPublished { .. } => write!(f, "btc refund is published"),
            BobState::BtcEarlyRefundPublished { .. } => write!(f, "btc early refund is published"),
            BobState::BtcPartialRefundPublished { .. } => {
                write!(f, "btc partial refund is published")
            }
            BobState::BtcRefunded(..) => write!(f, "btc is refunded"),
            BobState::XmrRedeemed { .. } => write!(f, "xmr is redeemed"),
            BobState::BtcPunished { .. } => write!(f, "btc is punished"),
            BobState::BtcEarlyRefunded { .. } => write!(f, "btc is early refunded"),
            BobState::BtcPartiallyRefunded { .. } => write!(f, "btc is partially refunded"),
            BobState::BtcAmnestyPublished { .. } => write!(f, "btc amnesty is published"),
            BobState::BtcAmnestyConfirmed { .. } => write!(f, "btc amnesty is confirmed"),
            BobState::WaitingForRemainingRefundTimelockExpiration { .. } => {
                write!(f, "waiting for remaining refund timelock to expire")
            }
            BobState::RemainingRefundTimelockExpired { .. } => {
                write!(f, "remaining refund timelock expired")
            }
            BobState::BtcRefundBurnPublished { .. } => write!(f, "btc refund burn is published"),
            BobState::BtcRefundBurnt { .. } => write!(f, "btc refund is burnt"),
            BobState::BtcFinalAmnestyPublished { .. } => {
                write!(f, "btc final amnesty is published")
            }
            BobState::BtcFinalAmnestyConfirmed { .. } => {
                write!(f, "btc final amnesty is confirmed")
            }
            BobState::SafelyAborted => write!(f, "safely aborted"),
        }
    }
}

impl fmt::Display for RefundType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefundType::Full => write!(f, "full btc refund"),
            RefundType::Partial { .. } => write!(f, "partial btc refund"),
        }
    }
}

impl BobState {
    /// Fetch the expired timelocks for the swap.
    /// Depending on the State, there are no locks to expire.
    pub async fn expired_timelocks(
        &self,
        bitcoin_wallet: Arc<dyn bitcoin_wallet::BitcoinWallet>,
    ) -> Result<Option<ExpiredTimelocks>> {
        Ok(match self.clone() {
            BobState::Started { .. }
            | BobState::BtcLockReadyToPublish { .. }
            | BobState::SafelyAborted
            | BobState::SwapSetupCompleted(_) => None,
            BobState::BtcLocked { state3: state, .. }
            | BobState::XmrLockTransactionCandidate { state, .. } => {
                Some(state.expired_timelock(bitcoin_wallet.as_ref()).await?)
            }
            BobState::XmrLockTransactionSeen { state, .. }
            | BobState::WaitingForCancelTimelockExpiration { state, .. } => {
                Some(state.expired_timelock(bitcoin_wallet.as_ref()).await?)
            }
            BobState::XmrLocked(state) | BobState::EncSigSent(state) => {
                Some(state.expired_timelock(bitcoin_wallet.as_ref()).await?)
            }
            BobState::CancelTimelockExpired(state)
            | BobState::BtcCancelled(state)
            | BobState::BtcRefundPublished(state)
            | BobState::BtcEarlyRefundPublished(state)
            | BobState::BtcPartialRefundPublished(state)
            | BobState::BtcPartiallyRefunded(state)
            | BobState::BtcAmnestyPublished(state)
            | BobState::BtcAmnestyConfirmed(state)
            | BobState::WaitingForRemainingRefundTimelockExpiration(state)
            | BobState::RemainingRefundTimelockExpired(state)
            | BobState::BtcRefundBurnPublished(state)
            | BobState::BtcRefundBurnt(state)
            | BobState::BtcFinalAmnestyPublished(state) => {
                Some(state.expired_timelock(bitcoin_wallet.as_ref()).await?)
            }
            BobState::BtcPunished { .. } => Some(ExpiredTimelocks::Punish),
            BobState::BtcFinalAmnestyConfirmed(_) => Some(ExpiredTimelocks::RemainingRefund),
            BobState::BtcRefunded(_)
            | BobState::BtcEarlyRefunded { .. }
            | BobState::BtcRedeemed(_)
            | BobState::XmrRedeemed { .. } => None,
        })
    }
}

pub fn is_complete(state: &BobState) -> bool {
    matches!(
        state,
        BobState::BtcRefunded(..)
            | BobState::BtcEarlyRefunded { .. }
            | BobState::BtcAmnestyConfirmed { .. }
            | BobState::BtcFinalAmnestyConfirmed { .. }
            | BobState::XmrRedeemed { .. }
            | BobState::SafelyAborted
    )
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, PartialEq)]
pub struct State0 {
    swap_id: Uuid,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    v_b: monero::PrivateViewKey,
    dleq_proof_s_b: CrossCurveDLEQProof,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    remaining_refund_timelock: Option<RemainingRefundTimelock>,
    refund_address: bitcoin::Address,
    min_monero_confirmations: u64,
    tx_partial_refund_fee: Option<bitcoin::Amount>,
    tx_refund_amnesty_fee: Option<bitcoin::Amount>,
    tx_final_amnesty_fee: Option<bitcoin::Amount>,
    tx_refund_fee: bitcoin::Amount,
    tx_cancel_fee: bitcoin::Amount,
    tx_lock_fee: bitcoin::Amount,
}

impl State0 {
    #[allow(clippy::too_many_arguments)]
    pub fn new<R: RngCore + CryptoRng>(
        swap_id: Uuid,
        rng: &mut R,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        cancel_timelock: CancelTimelock,
        punish_timelock: PunishTimelock,
        remaining_refund_timelock: RemainingRefundTimelock,
        refund_address: bitcoin::Address,
        min_monero_confirmations: u64,
        tx_partial_refund_fee: bitcoin::Amount,
        tx_refund_amnesty_fee: bitcoin::Amount,
        tx_final_amnesty_fee: bitcoin::Amount,
        tx_refund_fee: bitcoin::Amount,
        tx_cancel_fee: bitcoin::Amount,
        tx_lock_fee: bitcoin::Amount,
    ) -> Self {
        let b = bitcoin::SecretKey::new_random(rng);

        let s_b = monero::Scalar::random(rng);
        let v_b = monero::PrivateViewKey::new_random(rng);

        let (dleq_proof_s_b, (S_b_bitcoin, S_b_monero)) =
            CROSS_CURVE_PROOF_SYSTEM.prove(&s_b.into_dalek_ng(), rng);

        Self {
            swap_id,
            b,
            s_b,
            v_b,
            S_b_bitcoin: bitcoin::PublicKey::from(S_b_bitcoin),
            S_b_monero: monero::PublicKey {
                point: S_b_monero.compress(),
            },
            btc,
            xmr,
            dleq_proof_s_b,
            cancel_timelock,
            punish_timelock,
            remaining_refund_timelock: Some(remaining_refund_timelock),
            refund_address,
            min_monero_confirmations,
            tx_partial_refund_fee: Some(tx_partial_refund_fee),
            tx_refund_amnesty_fee: Some(tx_refund_amnesty_fee),
            tx_final_amnesty_fee: Some(tx_final_amnesty_fee),
            tx_refund_fee,
            tx_cancel_fee,
            tx_lock_fee,
        }
    }

    pub fn next_message(&self) -> Result<Message0> {
        Ok(Message0 {
            swap_id: self.swap_id,
            B: self.b.public(),
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            dleq_proof_s_b: self.dleq_proof_s_b.clone(),
            v_b: self.v_b,
            refund_address: self.refund_address.clone(),
            tx_refund_fee: self.tx_refund_fee,
            tx_partial_refund_fee: self
                .tx_partial_refund_fee
                .context("tx_partial_refund_fee missing but required to setup swap")?,
            tx_refund_amnesty_fee: self
                .tx_refund_amnesty_fee
                .context("tx_refund_amnesty_fee missing but required to setup swap")?,
            tx_final_amnesty_fee: self
                .tx_final_amnesty_fee
                .context("tx_final_amnesty_fee missing but required to setup swap")?,
            tx_cancel_fee: self.tx_cancel_fee,
        })
    }

    pub async fn receive(
        self,
        wallet: &dyn bitcoin_wallet::BitcoinWallet,
        msg: Message1,
    ) -> Result<State1> {
        let valid = CROSS_CURVE_PROOF_SYSTEM.verify(
            &msg.dleq_proof_s_a,
            (
                msg.S_a_bitcoin.into(),
                msg.S_a_monero
                    .point
                    .decompress()
                    .ok_or_else(|| anyhow!("S_a is not a monero curve point"))?,
            ),
        );

        if !valid {
            bail!("Alice's dleq proof doesn't verify")
        }

        let tx_lock = swap_core::bitcoin::TxLock::new(
            wallet,
            self.btc,
            self.tx_lock_fee,
            msg.A,
            self.b.public(),
            self.refund_address.clone(),
        )
        .await?;
        let v = msg.v_a + self.v_b;

        Ok(State1 {
            A: msg.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: msg.S_a_monero,
            S_a_bitcoin: msg.S_a_bitcoin,
            v,
            xmr: self.xmr,
            btc_amnesty_amount: Some(msg.amnesty_amount),
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            remaining_refund_timelock: self.remaining_refund_timelock,
            refund_address: self.refund_address,
            redeem_address: msg.redeem_address,
            punish_address: msg.punish_address,
            tx_lock,
            min_monero_confirmations: self.min_monero_confirmations,
            tx_redeem_fee: msg.tx_redeem_fee,
            tx_refund_fee: self.tx_refund_fee,
            tx_partial_refund_fee: self.tx_partial_refund_fee,
            tx_refund_amnesty_fee: self.tx_refund_amnesty_fee,
            tx_refund_burn_fee: Some(msg.tx_refund_burn_fee),
            tx_final_amnesty_fee: self.tx_final_amnesty_fee,
            tx_punish_fee: msg.tx_punish_fee,
            tx_cancel_fee: self.tx_cancel_fee,
        })
    }
}

#[allow(non_snake_case)]
#[derive(Debug)]
pub struct State1 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    xmr: monero::Amount,
    btc_amnesty_amount: Option<bitcoin::Amount>,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    remaining_refund_timelock: Option<RemainingRefundTimelock>,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    min_monero_confirmations: u64,
    tx_partial_refund_fee: Option<bitcoin::Amount>,
    tx_refund_amnesty_fee: Option<bitcoin::Amount>,
    tx_refund_burn_fee: Option<bitcoin::Amount>,
    tx_final_amnesty_fee: Option<bitcoin::Amount>,
    tx_redeem_fee: bitcoin::Amount,
    tx_refund_fee: bitcoin::Amount,
    tx_punish_fee: bitcoin::Amount,
    tx_cancel_fee: bitcoin::Amount,
}

impl State1 {
    pub fn next_message(&self) -> Message2 {
        Message2 {
            tx_lock_psbt: self.tx_lock.clone().into(),
        }
    }

    pub fn receive(self, msg: Message3) -> Result<State2> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )?;

        // Depending on which signatures we get, verify and store them
        let refund_signatures = match (
            msg.tx_full_refund_encsig,
            msg.tx_partial_refund_encsig,
            msg.tx_refund_amnesty_sig,
        ) {
            // We got the encrypted signature for the full refund - awesome
            (Some(tx_full_refund_encsig), _, _) => {
                let tx_full_refund =
                    TxFullRefund::new(&tx_cancel, &self.refund_address, self.tx_refund_fee);
                bitcoin::verify_encsig(
                    self.A,
                    bitcoin::PublicKey::from(self.s_b.to_secpfun_scalar()),
                    &tx_full_refund.digest(),
                    &tx_full_refund_encsig,
                )
                .context("Couldn't verify Alice's signature on TxFullRefund")?;

                RefundSignatures::Legacy {
                    tx_full_refund_encsig,
                }
            }
            // We got the encrypted signatures for the partial refund path.
            (None, Some(tx_partial_refund_encsig), Some(tx_refund_amnesty_sig)) => {
                let tx_partial_refund = TxPartialRefund::new(
                    &tx_cancel,
                    &self.refund_address,
                    self.A,
                    self.b.public(),
                    self.btc_amnesty_amount
                        .context("Missing btc_amnesty_amount")?,
                    self.tx_partial_refund_fee
                        .context("Missing tx_partial_refund_fee")?,
                )?;
                bitcoin::verify_encsig(
                    self.A,
                    bitcoin::PublicKey::from(self.s_b.to_secpfun_scalar()),
                    &tx_partial_refund.digest(),
                    &tx_partial_refund_encsig,
                )?;

                let tx_refund_amnesty = TxRefundAmnesty::new(
                    &tx_partial_refund,
                    &self.refund_address,
                    self.tx_refund_amnesty_fee
                        .context("missing tx_refund_amnesty_fee")?,
                    self.remaining_refund_timelock
                        .context("missing remaining_refund_timelock")?,
                )?;
                bitcoin::verify_sig(&self.A, &tx_refund_amnesty.digest(), &tx_refund_amnesty_sig)?;

                RefundSignatures::Partial {
                    tx_partial_refund_encsig,
                    tx_refund_amnesty_sig,
                }
            }
            (_, _, _) => anyhow::bail!(
                "Alice sent us neither TxFullRefund encsig nor signatures for the partial refund path"
            ),
        };

        Ok(State2 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            xmr: self.xmr,
            btc_amnesty_amount: self.btc_amnesty_amount,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            remaining_refund_timelock: self.remaining_refund_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: msg.tx_cancel_sig,
            refund_signatures,
            min_monero_confirmations: self.min_monero_confirmations,
            tx_redeem_fee: self.tx_redeem_fee,
            tx_refund_fee: self.tx_refund_fee,
            tx_partial_refund_fee: self.tx_partial_refund_fee,
            tx_refund_amnesty_fee: self.tx_refund_amnesty_fee,
            tx_refund_burn_fee: self.tx_refund_burn_fee,
            tx_final_amnesty_fee: self.tx_final_amnesty_fee,
            tx_punish_fee: self.tx_punish_fee,
            tx_cancel_fee: self.tx_cancel_fee,
        })
    }
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct State2 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    pub xmr: monero::Amount,
    pub btc_amnesty_amount: Option<bitcoin::Amount>,
    pub cancel_timelock: CancelTimelock,
    pub punish_timelock: PunishTimelock,
    remaining_refund_timelock: Option<RemainingRefundTimelock>,
    #[serde(with = "address_serde")]
    pub refund_address: bitcoin::Address,
    #[serde(with = "address_serde")]
    redeem_address: bitcoin::Address,
    #[serde(with = "address_serde")]
    punish_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    /// This field was changed in [#675](https://github.com/eigenwallet/core/pull/675).
    /// It boils down to the same json except that it now may also contain a partial refund signature.
    #[serde(flatten)]
    refund_signatures: RefundSignatures,
    min_monero_confirmations: u64,
    tx_redeem_fee: bitcoin::Amount,
    tx_punish_fee: bitcoin::Amount,
    pub tx_refund_fee: bitcoin::Amount,
    pub tx_cancel_fee: bitcoin::Amount,
    tx_partial_refund_fee: Option<bitcoin::Amount>,
    tx_refund_amnesty_fee: Option<bitcoin::Amount>,
    tx_refund_burn_fee: Option<bitcoin::Amount>,
    tx_final_amnesty_fee: Option<bitcoin::Amount>,
}

impl State2 {
    pub fn next_message(&self) -> Result<Message4> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )
        .expect("valid cancel tx");

        let tx_cancel_sig = self.b.sign(tx_cancel.digest());

        let tx_punish = bitcoin::TxPunish::new(
            &tx_cancel,
            &self.punish_address,
            self.punish_timelock,
            self.tx_punish_fee,
        );
        let tx_punish_sig = self.b.sign(tx_punish.digest());

        let tx_early_refund =
            bitcoin::TxEarlyRefund::new(&self.tx_lock, &self.refund_address, self.tx_refund_fee);

        let tx_early_refund_sig = self.b.sign(tx_early_refund.digest());

        // We can only construct a valid TxRefundAmnesty/TxRefundBurn/TxFinalAmnesty when the amnesty amount
        // is greater than zero. Thus we only send our signatures for them if that is the case.
        // Alice accepts this because she sent us her signature for TxFullRefund already anyway.
        let (tx_refund_amnesty_sig, tx_refund_burn_sig, tx_final_amnesty_sig) =
            if self.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO) == bitcoin::Amount::ZERO {
                (None, None, None)
            } else {
                let tx_partial_refund = TxPartialRefund::new(
                    &tx_cancel,
                    &self.refund_address,
                    self.A,
                    self.b.public(),
                    self.btc_amnesty_amount
                        .context("missing btc_amnesty_amount")?,
                    self.tx_partial_refund_fee
                        .context("missing tx_partial_refund_fee")?,
                )
                .context("Couldn't construct TxPartialRefund")?;
                let tx_refund_amnesty = TxRefundAmnesty::new(
                    &tx_partial_refund,
                    &self.refund_address,
                    self.tx_refund_amnesty_fee
                        .context("Missing tx_refund_amnesty_fee")?,
                    self.remaining_refund_timelock
                        .context("missing remaining_refund_timelock")?,
                )?;
                let tx_refund_amnesty_sig = self.b.sign(tx_refund_amnesty.digest());

                let tx_refund_burn = TxRefundBurn::new(
                    &tx_partial_refund,
                    self.A,
                    self.b.public(),
                    self.tx_refund_burn_fee
                        .context("Missing tx_refund_burn_fee")?,
                )?;
                let tx_refund_burn_sig = self.b.sign(tx_refund_burn.digest());

                let tx_final_amnesty = TxFinalAmnesty::new(
                    &tx_refund_burn,
                    &self.refund_address,
                    self.tx_final_amnesty_fee
                        .context("Missing tx_final_amnesty_fee")?,
                );
                let tx_final_amnesty_sig = self.b.sign(tx_final_amnesty.digest());

                (
                    Some(tx_refund_amnesty_sig),
                    Some(tx_refund_burn_sig),
                    Some(tx_final_amnesty_sig),
                )
            };

        Ok(Message4 {
            tx_punish_sig,
            tx_cancel_sig,
            tx_early_refund_sig,
            tx_refund_amnesty_sig,
            tx_refund_burn_sig,
            tx_final_amnesty_sig,
        })
    }

    pub async fn lock_btc(self) -> Result<(State3, TxLock)> {
        Ok((
            State3 {
                A: self.A,
                b: self.b,
                s_b: self.s_b,
                S_a_monero: self.S_a_monero,
                S_a_bitcoin: self.S_a_bitcoin,
                v: self.v,
                xmr: self.xmr,
                btc_amnesty_amount: self.btc_amnesty_amount,
                cancel_timelock: self.cancel_timelock,
                punish_timelock: self.punish_timelock,
                remaining_refund_timelock: self.remaining_refund_timelock,
                refund_address: self.refund_address,
                redeem_address: self.redeem_address,
                tx_lock: self.tx_lock.clone(),
                tx_cancel_sig_a: self.tx_cancel_sig_a,
                refund_signatures: self.refund_signatures,
                min_monero_confirmations: self.min_monero_confirmations,
                tx_redeem_fee: self.tx_redeem_fee,
                tx_refund_fee: self.tx_refund_fee,
                tx_partial_refund_fee: self.tx_partial_refund_fee,
                tx_refund_amnesty_fee: self.tx_refund_amnesty_fee,
                tx_refund_burn_fee: self.tx_refund_burn_fee,
                tx_final_amnesty_fee: self.tx_final_amnesty_fee,
                tx_cancel_fee: self.tx_cancel_fee,
            },
            self.tx_lock,
        ))
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct State3 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    xmr: monero::Amount,
    btc_amnesty_amount: Option<bitcoin::Amount>,
    pub cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    remaining_refund_timelock: Option<RemainingRefundTimelock>,
    #[serde(with = "address_serde")]
    refund_address: bitcoin::Address,
    #[serde(with = "address_serde")]
    redeem_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    /// The (encrypted) signatures Alice sent us for the Bitcoin refund transaction(s).
    ///
    /// This field was changed in [#675](https://github.com/eigenwallet/core/pull/675).
    /// It boils down to the same json except that it now may also contain a partial refund signature.
    #[serde(flatten)]
    refund_signatures: RefundSignatures,
    min_monero_confirmations: u64,
    tx_redeem_fee: bitcoin::Amount,
    tx_refund_fee: bitcoin::Amount,
    tx_partial_refund_fee: Option<bitcoin::Amount>,
    tx_refund_amnesty_fee: Option<bitcoin::Amount>,
    tx_refund_burn_fee: Option<bitcoin::Amount>,
    tx_final_amnesty_fee: Option<bitcoin::Amount>,
    tx_cancel_fee: bitcoin::Amount,
}

impl State3 {
    pub fn xmr_view_keys(&self) -> (monero::PublicKey, monero::PrivateViewKey) {
        let S_b_monero = monero::PublicKey::from_private_key(&monero::PrivateKey::from_scalar(
            self.s_b.into_dalek_ng(),
        ));
        let S = self.S_a_monero + S_b_monero;

        (S, self.v)
    }

    pub fn xmr_amount(&self) -> monero::Amount {
        self.xmr
    }

    pub fn xmr_locked(
        self,
        monero_wallet_restore_blockheight: BlockHeight,
        lock_transfer_proof: TransferProofMaybeWithTxKey,
    ) -> State4 {
        State4 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            xmr: self.xmr,
            btc_amnesty_amount: self.btc_amnesty_amount,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            remaining_refund_timelock: self.remaining_refund_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            refund_signatures: self.refund_signatures,
            monero_wallet_restore_blockheight,
            lock_transfer_proof,
            tx_redeem_fee: self.tx_redeem_fee,
            tx_refund_fee: self.tx_refund_fee,
            tx_cancel_fee: self.tx_cancel_fee,
            tx_partial_refund_fee: self.tx_partial_refund_fee,
            tx_refund_amnesty_fee: self.tx_refund_amnesty_fee,
            tx_refund_burn_fee: self.tx_refund_burn_fee,
            tx_final_amnesty_fee: self.tx_final_amnesty_fee,
        }
    }

    pub fn cancel(&self, monero_wallet_restore_blockheight: BlockHeight) -> State6 {
        State6 {
            A: self.A,
            b: self.b.clone(),
            s_b: self.s_b,
            v: self.v,
            monero_wallet_restore_blockheight,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            remaining_refund_timelock: self.remaining_refund_timelock,
            refund_address: self.refund_address.clone(),
            tx_lock: self.tx_lock.clone(),
            tx_cancel_sig_a: self.tx_cancel_sig_a.clone(),
            refund_signatures: self.refund_signatures.clone(),
            tx_refund_fee: self.tx_refund_fee,
            tx_cancel_fee: self.tx_cancel_fee,
            tx_partial_refund_fee: self.tx_partial_refund_fee,
            tx_refund_amnesty_fee: self.tx_refund_amnesty_fee,
            tx_refund_burn_fee: self.tx_refund_burn_fee,
            tx_final_amnesty_fee: self.tx_final_amnesty_fee,
            xmr: self.xmr,
            btc_amnesty_amount: self.btc_amnesty_amount,
        }
    }

    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }

    pub async fn expired_timelock(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<ExpiredTimelocks> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )?;

        let tx_lock_status = bitcoin_wallet.status_of_script(&self.tx_lock).await?;
        let tx_cancel_status = bitcoin_wallet.status_of_script(&tx_cancel).await?;
        let tx_partial_refund_status =
            if let (Some(btc_amnesty_amount), Some(tx_partial_refund_fee)) =
                (self.btc_amnesty_amount, self.tx_partial_refund_fee)
            {
                let tx = TxPartialRefund::new(
                    &tx_cancel,
                    &self.refund_address,
                    self.A,
                    self.b.public(),
                    btc_amnesty_amount,
                    tx_partial_refund_fee,
                )?;
                Some(bitcoin_wallet.status_of_script(&tx).await?)
            } else {
                None
            };

        Ok(current_epoch(
            self.cancel_timelock,
            self.punish_timelock,
            self.remaining_refund_timelock,
            tx_lock_status,
            tx_cancel_status,
            tx_partial_refund_status,
        ))
    }

    pub fn construct_tx_early_refund(&self) -> bitcoin::TxEarlyRefund {
        bitcoin::TxEarlyRefund::new(&self.tx_lock, &self.refund_address, self.tx_refund_fee)
    }

    pub async fn check_for_tx_early_refund(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<Option<Arc<Transaction>>> {
        let tx_early_refund = self.construct_tx_early_refund();
        let tx = bitcoin_wallet
            .get_raw_transaction(tx_early_refund.txid())
            .await
            .context("Failed to check for existence of tx_early_refund")?;

        Ok(tx)
    }

    pub async fn is_tx_lock_published(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<bool> {
        let tx_lock = self.tx_lock.clone();
        let tx = bitcoin_wallet.get_raw_transaction(tx_lock.txid()).await?;

        Ok(tx.is_some())
    }
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct State4 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    xmr: monero::Amount,
    btc_amnesty_amount: Option<bitcoin::Amount>,
    pub cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    remaining_refund_timelock: Option<RemainingRefundTimelock>,
    #[serde(with = "address_serde")]
    refund_address: bitcoin::Address,
    #[serde(with = "address_serde")]
    redeem_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    /// This field was changed in [#675](https://github.com/eigenwallet/core/pull/675).
    /// It boils down to the same json except that it now may also contain a partial refund signature.
    #[serde(flatten)]
    refund_signatures: RefundSignatures,
    monero_wallet_restore_blockheight: BlockHeight,
    lock_transfer_proof: TransferProofMaybeWithTxKey,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    tx_redeem_fee: bitcoin::Amount,
    tx_refund_fee: bitcoin::Amount,
    tx_partial_refund_fee: Option<bitcoin::Amount>,
    tx_refund_amnesty_fee: Option<bitcoin::Amount>,
    tx_refund_burn_fee: Option<bitcoin::Amount>,
    tx_final_amnesty_fee: Option<bitcoin::Amount>,
    tx_cancel_fee: bitcoin::Amount,
}

impl State4 {
    pub async fn check_for_tx_redeem(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<Option<State5>> {
        let tx_redeem =
            bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address, self.tx_redeem_fee);
        let tx_redeem_encsig = self.b.encsign(self.S_a_bitcoin, tx_redeem.digest());

        let tx_redeem_candidate = bitcoin_wallet.get_raw_transaction(tx_redeem.txid()).await?;

        if let Some(tx_redeem_candidate) = tx_redeem_candidate {
            let tx_redeem_sig =
                tx_redeem.extract_signature_by_key(tx_redeem_candidate, self.b.public())?;
            let s_a = bitcoin::recover(self.S_a_bitcoin, tx_redeem_sig, tx_redeem_encsig)?;
            let s_a = monero::PrivateKey::from_scalar(
                monero::private_key_from_secp256k1_scalar(s_a.into()).into_dalek_ng(),
            );

            Ok(Some(State5 {
                s_a,
                s_b: self.s_b,
                v: self.v,
                xmr: self.xmr,
                btc_amnesty_amount: self.btc_amnesty_amount,
                tx_lock: self.tx_lock.clone(),
                monero_wallet_restore_blockheight: self.monero_wallet_restore_blockheight,
                lock_transfer_proof: self.lock_transfer_proof.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn tx_redeem_encsig(&self) -> bitcoin::EncryptedSignature {
        let tx_redeem =
            bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address, self.tx_redeem_fee);
        self.b.encsign(self.S_a_bitcoin, tx_redeem.digest())
    }

    pub async fn watch_for_redeem_btc(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<State5> {
        let tx_redeem =
            bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address, self.tx_redeem_fee);

        bitcoin_wallet
            .subscribe_to(Box::new(tx_redeem))
            .await
            .wait_until_seen()
            .await?;

        let state5 = self.check_for_tx_redeem(bitcoin_wallet).await?;

        state5.ok_or_else(|| {
            anyhow!("Bitcoin redeem transaction was not found in the chain even though we previously saw it in the mempool. Our Electrum server might have cleared its mempool?")
        })
    }

    pub async fn expired_timelock(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<ExpiredTimelocks> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )?;

        let tx_lock_status = bitcoin_wallet.status_of_script(&self.tx_lock).await?;
        let tx_cancel_status = bitcoin_wallet.status_of_script(&tx_cancel).await?;
        let tx_partial_refund_status =
            if let (Some(btc_amnesty_amount), Some(tx_partial_refund_fee)) =
                (self.btc_amnesty_amount, self.tx_partial_refund_fee)
            {
                let tx = TxPartialRefund::new(
                    &tx_cancel,
                    &self.refund_address,
                    self.A,
                    self.b.public(),
                    btc_amnesty_amount,
                    tx_partial_refund_fee,
                )?;
                Some(bitcoin_wallet.status_of_script(&tx).await?)
            } else {
                None
            };

        Ok(current_epoch(
            self.cancel_timelock,
            self.punish_timelock,
            self.remaining_refund_timelock,
            tx_lock_status,
            tx_cancel_status,
            tx_partial_refund_status,
        ))
    }

    pub fn cancel(self) -> State6 {
        State6 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            v: self.v,
            monero_wallet_restore_blockheight: self.monero_wallet_restore_blockheight,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            remaining_refund_timelock: self.remaining_refund_timelock,
            refund_address: self.refund_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            refund_signatures: self.refund_signatures,
            tx_refund_fee: self.tx_refund_fee,
            tx_cancel_fee: self.tx_cancel_fee,
            xmr: self.xmr,
            btc_amnesty_amount: self.btc_amnesty_amount,
            tx_partial_refund_fee: self.tx_partial_refund_fee,
            tx_refund_amnesty_fee: self.tx_refund_amnesty_fee,
            tx_refund_burn_fee: self.tx_refund_burn_fee,
            tx_final_amnesty_fee: self.tx_final_amnesty_fee,
        }
    }

    pub fn construct_tx_early_refund(&self) -> bitcoin::TxEarlyRefund {
        bitcoin::TxEarlyRefund::new(&self.tx_lock, &self.refund_address, self.tx_refund_fee)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct State5 {
    #[serde(with = "swap_serde::monero::private_key")]
    s_a: monero::PrivateKey,
    s_b: monero::Scalar,
    v: monero::PrivateViewKey,
    xmr: monero::Amount,
    btc_amnesty_amount: Option<bitcoin::Amount>,
    tx_lock: bitcoin::TxLock,
    pub monero_wallet_restore_blockheight: BlockHeight,
    pub lock_transfer_proof: TransferProofMaybeWithTxKey,
}

impl State5 {
    pub fn xmr_keys(&self) -> (monero::PrivateKey, monero::PrivateViewKey) {
        let s_b = monero::PrivateKey {
            scalar: self.s_b.into_dalek_ng(),
        };
        let s = self.s_a + s_b;

        (s, self.v)
    }

    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct State6 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    v: monero::PrivateViewKey,
    pub xmr: monero::Amount,
    /// How much of the locked Bitcoin will stay locked in case of a partial refund.
    /// May still be retrieve by publishing the `TxAmnesty` transaction.
    pub btc_amnesty_amount: Option<bitcoin::Amount>,
    pub monero_wallet_restore_blockheight: BlockHeight,
    pub cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    pub remaining_refund_timelock: Option<RemainingRefundTimelock>,
    #[serde(with = "address_serde")]
    refund_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    /// This field was changed in [#675](https://github.com/eigenwallet/core/pull/675).
    /// It boils down to the same json except that it now may also contain a partial refund signature.
    #[serde(flatten)]
    pub refund_signatures: RefundSignatures,
    pub tx_refund_fee: bitcoin::Amount,
    pub tx_cancel_fee: bitcoin::Amount,
    pub tx_partial_refund_fee: Option<bitcoin::Amount>,
    pub tx_refund_amnesty_fee: Option<bitcoin::Amount>,
    pub tx_refund_burn_fee: Option<bitcoin::Amount>,
    pub tx_final_amnesty_fee: Option<bitcoin::Amount>,
}

impl State6 {
    pub async fn expired_timelock(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<ExpiredTimelocks> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )?;

        let tx_lock_status = bitcoin_wallet.status_of_script(&self.tx_lock).await?;
        let tx_cancel_status = bitcoin_wallet.status_of_script(&tx_cancel).await?;
        // Only check partial refund status if we have the data to construct it
        // (old swaps won't have these fields)
        let tx_partial_refund_status =
            if let (Some(_), Some(_)) = (self.btc_amnesty_amount, self.tx_partial_refund_fee) {
                let tx = self.construct_tx_partial_refund()?;
                Some(bitcoin_wallet.status_of_script(&tx).await?)
            } else {
                None
            };

        Ok(current_epoch(
            self.cancel_timelock,
            self.punish_timelock,
            self.remaining_refund_timelock,
            tx_lock_status,
            tx_cancel_status,
            tx_partial_refund_status,
        ))
    }

    pub fn construct_tx_cancel(&self) -> Result<bitcoin::TxCancel> {
        bitcoin::TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )
    }

    pub async fn check_for_tx_cancel(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<Option<Arc<Transaction>>> {
        let tx_cancel = self.construct_tx_cancel()?;

        let tx = bitcoin_wallet
            .get_raw_transaction(tx_cancel.txid())
            .await
            .context("Failed to check for existence of tx_cancel")?;

        Ok(tx)
    }

    pub async fn submit_tx_cancel(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<(Txid, Subscription)> {
        let transaction = self
            .construct_tx_cancel()?
            .complete_as_bob(self.A, self.b.clone(), self.tx_cancel_sig_a.clone())
            .context("Failed to complete Bitcoin cancel transaction")?;

        let (tx_id, subscription) = bitcoin_wallet
            .ensure_broadcasted(transaction, "cancel")
            .await?;

        Ok((tx_id, subscription))
    }

    /// Construct the best refund transaction based on the refund signatures Alice has sent us.
    /// This is either `TxFullRefund` or `TxPartialRefund`.
    /// Returns the fully constructed and signed transaction along with the refund type.
    pub async fn construct_best_bitcoin_refund_tx(&self) -> Result<(Transaction, RefundType)> {
        if self.refund_signatures.tx_full_refund_encsig().is_some() {
            tracing::debug!("Have the full refund signature, constructing full Bitcoin refund");
            let tx_full_refund = self
                .signed_full_refund_transaction()
                .context("Couldn't construct TxFullRefund")?;

            return Ok((tx_full_refund, RefundType::Full));
        }

        if self.refund_signatures.tx_partial_refund_encsig().is_some() {
            tracing::debug!(
                "Don't have the full refund signature, constructing partial Bitcoin refund"
            );

            let tx_partial_refund = self
                .signed_partial_refund_transaction()
                .context("Couldn't construct TxPartialRefund")?;
            let total_swap_amount = self.tx_lock.lock_amount();
            let btc_amnesty_amount = self.btc_amnesty_amount.context("Missing Bitcoin amnesty amount even though we don't have the full refund signature")?;

            return Ok((
                tx_partial_refund,
                RefundType::Partial {
                    total_swap_amount,
                    btc_amnesty_amount,
                },
            ));
        }

        unreachable!("We always have either the partial or full refund encsig");
    }

    pub fn construct_tx_refund(&self) -> Result<bitcoin::TxFullRefund> {
        let tx_cancel = self.construct_tx_cancel()?;

        let tx_refund =
            bitcoin::TxFullRefund::new(&tx_cancel, &self.refund_address, self.tx_refund_fee);

        Ok(tx_refund)
    }

    pub fn signed_full_refund_transaction(&self) -> Result<Transaction> {
        let tx_full_refund_encsig = self.refund_signatures.tx_full_refund_encsig().context(
            "Can't sign full refund transaction because we don't have the necessary signature",
        )?;

        let tx_refund = self.construct_tx_refund()?;

        let adaptor = Adaptor::<HashTranscript<Sha256>, Deterministic<Sha256>>::default();

        let sig_b = self.b.sign(tx_refund.digest());
        let sig_a = adaptor.decrypt_signature(&self.s_b.to_secpfun_scalar(), tx_full_refund_encsig);

        let signed_tx_refund =
            tx_refund.add_signatures((self.A, sig_a), (self.b.public(), sig_b))?;

        Ok(signed_tx_refund)
    }

    pub fn construct_tx_partial_refund(&self) -> Result<bitcoin::TxPartialRefund> {
        let tx_cancel = self.construct_tx_cancel()?;
        bitcoin::TxPartialRefund::new(
            &tx_cancel,
            &self.refund_address,
            self.A,
            self.b.public(),
            self.btc_amnesty_amount
                .context("Can't construct TxPartialRefund because btc_amnesty_amount is missing")?,
            self.tx_partial_refund_fee.context(
                "Can't construct TxPartialRefund because tx_partial_refund_fee is missing",
            )?,
        )
    }

    pub fn signed_partial_refund_transaction(&self) -> Result<Transaction> {
        let tx_partial_refund_encsig = self
            .refund_signatures
            .tx_partial_refund_encsig()
            .context("Can't finalize TxPartialRefund because Alice's encsig is missing")?;

        let tx_partial_refund = self.construct_tx_partial_refund()?;

        let adaptor = Adaptor::<HashTranscript<Sha256>, Deterministic<Sha256>>::default();

        let sig_b = self.b.sign(tx_partial_refund.digest());
        let sig_a =
            adaptor.decrypt_signature(&self.s_b.to_secpfun_scalar(), tx_partial_refund_encsig);

        let signed_tx_partial_refund =
            tx_partial_refund.add_signatures((self.A, sig_a), (self.b.public(), sig_b))?;

        Ok(signed_tx_partial_refund)
    }

    pub fn signed_amnesty_transaction(&self) -> Result<Transaction> {
        let tx_amnesty = self.construct_tx_amnesty()?;

        let sig_a = self.refund_signatures.tx_refund_amnesty_sig().context(
            "Can't sign amnesty transaction because Alice's amnesty signature is missing",
        )?;
        let sig_b = self.b.sign(tx_amnesty.digest());

        let signed_tx_amnesty =
            tx_amnesty.add_signatures((self.A, sig_a), (self.b.public(), sig_b))?;

        Ok(signed_tx_amnesty)
    }

    pub fn construct_tx_amnesty(&self) -> Result<bitcoin::TxRefundAmnesty> {
        let tx_partial_refund = self.construct_tx_partial_refund()?;

        bitcoin::TxRefundAmnesty::new(
            &tx_partial_refund,
            &self.refund_address,
            self.tx_refund_amnesty_fee.context(
                "Can't construct TxRefundAmnesty because tx_refund_amnesty_fee is missing",
            )?,
            self.remaining_refund_timelock.context(
                "Can't construct TxRefundAmnesty because remaining_refund_timelock is missing",
            )?,
        )
    }

    pub fn construct_tx_refund_burn(&self) -> Result<bitcoin::TxRefundBurn> {
        let tx_partial_refund = self.construct_tx_partial_refund()?;
        bitcoin::TxRefundBurn::new(
            &tx_partial_refund,
            self.A,
            self.b.public(),
            self.tx_refund_burn_fee
                .context("Can't construct TxRefundBurn because tx_refund_burn_fee is missing")?,
        )
    }

    pub fn construct_tx_final_amnesty(&self) -> Result<bitcoin::TxFinalAmnesty> {
        let tx_refund_burn = self.construct_tx_refund_burn()?;
        Ok(bitcoin::TxFinalAmnesty::new(
            &tx_refund_burn,
            &self.refund_address,
            self.tx_final_amnesty_fee.context(
                "Can't construct TxFinalAmnesty because tx_final_amnesty_fee is missing",
            )?,
        ))
    }

    pub fn construct_tx_early_refund(&self) -> bitcoin::TxEarlyRefund {
        bitcoin::TxEarlyRefund::new(&self.tx_lock, &self.refund_address, self.tx_refund_fee)
    }

    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }

    pub fn attempt_cooperative_redeem(
        &self,
        s_a: monero::Scalar,
        lock_transfer_proof: TransferProofMaybeWithTxKey,
    ) -> State5 {
        // TODO: Validate `s_a` here!
        let s_a = monero::PrivateKey::from_scalar(s_a.into_dalek_ng());

        State5 {
            s_a,
            s_b: self.s_b,
            v: self.v,
            xmr: self.xmr,
            btc_amnesty_amount: self.btc_amnesty_amount,
            tx_lock: self.tx_lock.clone(),
            monero_wallet_restore_blockheight: self.monero_wallet_restore_blockheight,
            lock_transfer_proof,
        }
    }

    pub async fn check_for_tx_early_refund(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<Option<Arc<Transaction>>> {
        let tx_early_refund = self.construct_tx_early_refund();

        let tx = bitcoin_wallet
            .get_raw_transaction(tx_early_refund.txid())
            .await
            .context("Failed to check for existence of tx_early_refund")?;

        Ok(tx)
    }
}

impl RefundSignatures {
    pub fn from_possibly_full_refund_sig(
        partial_refund_encsig: bitcoin::EncryptedSignature,
        full_refund_encsig: Option<bitcoin::EncryptedSignature>,
        refund_amnesty_sig: bitcoin::Signature,
    ) -> Self {
        if let Some(full_refund_encsig) = full_refund_encsig {
            Self::Full {
                tx_partial_refund_encsig: partial_refund_encsig,
                tx_full_refund_encsig: full_refund_encsig,
                tx_refund_amnesty_sig: refund_amnesty_sig,
            }
        } else {
            Self::Partial {
                tx_partial_refund_encsig: partial_refund_encsig,
                tx_refund_amnesty_sig: refund_amnesty_sig,
            }
        }
    }

    pub fn tx_full_refund_encsig(&self) -> Option<bitcoin::EncryptedSignature> {
        match self {
            RefundSignatures::Partial { .. } => None,
            RefundSignatures::Full {
                tx_full_refund_encsig,
                ..
            } => Some(tx_full_refund_encsig.clone()),
            RefundSignatures::Legacy {
                tx_full_refund_encsig,
            } => Some(tx_full_refund_encsig.clone()),
        }
    }

    pub fn tx_partial_refund_encsig(&self) -> Option<bitcoin::EncryptedSignature> {
        match self {
            RefundSignatures::Partial {
                tx_partial_refund_encsig,
                ..
            } => Some(tx_partial_refund_encsig.clone()),
            RefundSignatures::Full {
                tx_partial_refund_encsig,
                ..
            } => Some(tx_partial_refund_encsig.clone()),
            RefundSignatures::Legacy { .. } => None,
        }
    }

    /// Returns Alice's signature for the amnesty transaction.
    /// Only available for new swaps (Partial/Full variants), not Legacy swaps.
    pub fn tx_refund_amnesty_sig(&self) -> Option<bitcoin::Signature> {
        match self {
            RefundSignatures::Partial {
                tx_refund_amnesty_sig,
                ..
            } => Some(tx_refund_amnesty_sig.clone()),
            RefundSignatures::Full {
                tx_refund_amnesty_sig,
                ..
            } => Some(tx_refund_amnesty_sig.clone()),
            RefundSignatures::Legacy { .. } => None,
        }
    }

    pub fn has_full_refund_encsig(&self) -> bool {
        self.tx_full_refund_encsig().is_some()
    }

    pub fn has_partial_refund_encsig(&self) -> bool {
        self.tx_partial_refund_encsig().is_some()
    }
}
