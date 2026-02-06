#![allow(non_snake_case)]

use crate::common::{CROSS_CURVE_PROOF_SYSTEM, Message0, Message1, Message2, Message3, Message4};
use anyhow::{Context, Result, bail};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sigma_fun::ext::dl_secp256k1_ed25519_eq::CrossCurveDLEQProof;
use std::fmt::{self, Debug};
use std::sync::Arc;
use swap_core::bitcoin::{
    CancelTimelock, ExpiredTimelocks, PunishTimelock, RemainingRefundTimelock, Transaction,
    TxCancel, TxEarlyRefund, TxFullRefund, TxMercy, TxPartialRefund, TxPunish, TxReclaim, TxRedeem,
    TxWithhold, Txid, current_epoch,
};
use swap_core::compat::{IntoDalek, IntoDalekNg, IntoMoneroOxide};
use swap_core::monero::ScalarExt;
use swap_core::monero::primitives::{AmountExt, BlockHeight, TransferProof, TransferRequest};
use swap_core::monero::{self, Scalar};
use swap_env::env::Config;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub enum AliceState {
    Started {
        state3: Box<State3>,
    },
    BtcLockTransactionSeen {
        state3: Box<State3>,
    },
    BtcLocked {
        state3: Box<State3>,
    },
    BtcEarlyRefundable {
        state3: Box<State3>,
    },
    XmrLockTransactionSent {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    XmrLocked {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    XmrLockTransferProofSent {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    EncSigLearned {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        encrypted_signature: Box<swap_core::bitcoin::EncryptedSignature>,
        state3: Box<State3>,
    },
    BtcRedeemTransactionPublished {
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    BtcRedeemed,
    BtcCancelled {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    BtcEarlyRefunded(Box<State3>),
    // We enter the refund states regardless of whether or not the refund
    // transaction was confirmed because we do not care. We can extract the key
    // we need to refund ourself regardless.
    BtcRefunded {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        spend_key: monero_oxide_ext::PrivateKey,
        state3: Box<State3>,
    },
    BtcPartiallyRefunded {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        spend_key: monero::PrivateKey,
        state3: Box<State3>,
    },
    XmrRefundable {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        spend_key: monero::PrivateKey,
        state3: Box<State3>,
    },
    // TODO: save redeem transaction id
    XmrRefunded {
        state3: Option<Box<State3>>,
    },
    BtcWithholdPublished {
        state3: Box<State3>,
    },
    BtcWithholdConfirmed {
        state3: Box<State3>,
    },
    /// Operator has decided to grant final amnesty to Bob.
    /// This state will publish TxFinalAmnesty and transition to BtcRefundFinalAmnestyPublished.
    BtcMercyGranted {
        state3: Box<State3>,
    },
    BtcMercyPublished {
        state3: Box<State3>,
    },
    BtcMercyConfirmed {
        state3: Box<State3>,
    },
    WaitingForCancelTimelockExpiration {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    CancelTimelockExpired {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    BtcPunishable {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    BtcPunished {
        state3: Box<State3>,
        transfer_proof: TransferProof,
    },
    SafelyAborted,
}

pub fn is_complete(state: &AliceState) -> bool {
    match state {
        // XmrRefunded is only complete if we don't need to publish TxRefundBurn
        AliceState::XmrRefunded { state3 } => match state3 {
            Some(s3) if s3.should_publish_tx_refund_burn == Some(true) => false,
            _ => true,
        },
        AliceState::BtcRedeemed
        | AliceState::BtcPunished { .. }
        | AliceState::SafelyAborted
        | AliceState::BtcEarlyRefunded(_)
        | AliceState::BtcWithholdConfirmed { .. }
        | AliceState::BtcMercyConfirmed { .. } => true,
        _ => false,
    }
}

impl fmt::Display for AliceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AliceState::Started { .. } => write!(f, "started"),
            AliceState::BtcLockTransactionSeen { .. } => {
                write!(f, "bitcoin lock transaction in mempool")
            }
            AliceState::BtcLocked { .. } => write!(f, "btc is locked"),
            AliceState::XmrLockTransactionSent { .. } => write!(f, "xmr lock transaction sent"),
            AliceState::XmrLocked { .. } => write!(f, "xmr is locked"),
            AliceState::XmrLockTransferProofSent { .. } => {
                write!(f, "xmr lock transfer proof sent")
            }
            AliceState::EncSigLearned { .. } => write!(f, "encrypted signature is learned"),
            AliceState::BtcRedeemTransactionPublished { .. } => {
                write!(f, "bitcoin redeem transaction published")
            }
            AliceState::BtcRedeemed => write!(f, "btc is redeemed"),
            AliceState::BtcCancelled { .. } => write!(f, "btc is cancelled"),
            AliceState::BtcRefunded { .. } => write!(f, "btc is refunded"),
            AliceState::BtcPunished { .. } => write!(f, "btc is punished"),
            AliceState::SafelyAborted => write!(f, "safely aborted"),
            AliceState::BtcPunishable { .. } => write!(f, "btc is punishable"),
            AliceState::XmrRefunded { .. } => write!(f, "xmr is refunded"),
            AliceState::BtcWithholdPublished { .. } => write!(f, "btc withhold published"),
            AliceState::BtcWithholdConfirmed { .. } => write!(f, "btc withheld"),
            AliceState::BtcMercyGranted { .. } => write!(f, "btc mercy granted"),
            AliceState::BtcMercyPublished { .. } => {
                write!(f, "btc mercy published")
            }
            AliceState::BtcMercyConfirmed { .. } => {
                write!(f, "btc mercy confirmed")
            }
            AliceState::WaitingForCancelTimelockExpiration { .. } => {
                write!(f, "waiting for cancel timelock expiration")
            }
            AliceState::CancelTimelockExpired { .. } => write!(f, "cancel timelock is expired"),
            AliceState::BtcEarlyRefundable { .. } => write!(f, "btc is early refundable"),
            AliceState::BtcEarlyRefunded(_) => write!(f, "btc is early refunded"),
            AliceState::BtcPartiallyRefunded { .. } => write!(f, "btc is partially refunded"),
            AliceState::XmrRefundable { .. } => write!(f, "xmr is refundable"),
        }
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, PartialEq)]
pub struct State0 {
    a: swap_core::bitcoin::SecretKey,
    s_a: swap_core::monero::Scalar,
    v_a: monero::PrivateViewKey,
    S_a_monero: monero_oxide_ext::PublicKey,
    S_a_bitcoin: swap_core::bitcoin::PublicKey,
    dleq_proof_s_a: CrossCurveDLEQProof,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    btc_amnesty_amount: Option<bitcoin::Amount>,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    remaining_refund_timelock: Option<RemainingRefundTimelock>,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_redeem_fee: bitcoin::Amount,
    tx_punish_fee: bitcoin::Amount,
    tx_refund_burn_fee: Option<bitcoin::Amount>,
    should_publish_tx_refund_burn: Option<bool>,
}

impl State0 {
    #[allow(clippy::too_many_arguments)]
    pub fn new<R>(
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        btc_amnesty_amount: bitcoin::Amount,
        env_config: Config,
        redeem_address: bitcoin::Address,
        punish_address: bitcoin::Address,
        tx_redeem_fee: bitcoin::Amount,
        tx_punish_fee: bitcoin::Amount,
        tx_refund_burn_fee: bitcoin::Amount,
        should_publish_tx_refund_burn: bool,
        rng: &mut R,
    ) -> Self
    where
        R: RngCore + CryptoRng,
    {
        let a = swap_core::bitcoin::SecretKey::new_random(rng);
        let v_a = monero::PrivateViewKey::new_random(rng);

        let s_a = swap_core::monero::Scalar::random(rng);
        let (dleq_proof_s_a, (S_a_bitcoin, S_a_monero)) =
            CROSS_CURVE_PROOF_SYSTEM.prove(&s_a.into_dalek_ng(), rng);

        Self {
            a,
            s_a,
            v_a,
            S_a_bitcoin: S_a_bitcoin.into(),
            S_a_monero: S_a_monero.compress().into(),
            dleq_proof_s_a,
            btc_amnesty_amount: Some(btc_amnesty_amount),
            redeem_address,
            punish_address,
            btc,
            xmr,
            cancel_timelock: env_config.bitcoin_cancel_timelock.into(),
            punish_timelock: env_config.bitcoin_punish_timelock.into(),
            remaining_refund_timelock: Some(env_config.bitcoin_remaining_refund_timelock.into()),
            tx_redeem_fee,
            tx_punish_fee,
            tx_refund_burn_fee: Some(tx_refund_burn_fee),
            should_publish_tx_refund_burn: Some(should_publish_tx_refund_burn),
        }
    }

    pub fn receive(self, msg: Message0) -> Result<(Uuid, State1)> {
        let valid = CROSS_CURVE_PROOF_SYSTEM.verify(
            &msg.dleq_proof_s_b,
            (msg.S_b_bitcoin.into(), msg.S_b_monero.decompress_ng()),
        );

        if !valid {
            bail!("Bob's dleq proof doesn't verify")
        }

        let amnesty_amount = self
            .btc_amnesty_amount
            .context("btc_amnesty_amount missing for new swap")?;
        let tx_refund_burn_fee = self
            .tx_refund_burn_fee
            .context("tx_refund_burn_fee missing for new swap")?;

        crate::common::sanity_check_amnesty_amount(
            self.btc,
            amnesty_amount,
            msg.tx_partial_refund_fee,
            msg.tx_refund_amnesty_fee,
            tx_refund_burn_fee,
            msg.tx_final_amnesty_fee,
        )?;

        let v = self.v_a + msg.v_b;

        Ok((
            msg.swap_id,
            State1 {
                a: self.a,
                B: msg.B,
                s_a: self.s_a,
                S_a_monero: self.S_a_monero,
                S_a_bitcoin: self.S_a_bitcoin,
                S_b_monero: msg.S_b_monero,
                S_b_bitcoin: msg.S_b_bitcoin,
                v,
                v_a: self.v_a,
                dleq_proof_s_a: self.dleq_proof_s_a,
                btc: self.btc,
                xmr: self.xmr,
                btc_amnesty_amount: self.btc_amnesty_amount,
                cancel_timelock: self.cancel_timelock,
                punish_timelock: self.punish_timelock,
                remaining_refund_timelock: self.remaining_refund_timelock,
                refund_address: msg.refund_address,
                redeem_address: self.redeem_address,
                punish_address: self.punish_address,
                tx_redeem_fee: self.tx_redeem_fee,
                tx_punish_fee: self.tx_punish_fee,
                tx_refund_fee: msg.tx_refund_fee,
                tx_partial_refund_fee: Some(msg.tx_partial_refund_fee),
                tx_refund_amnesty_fee: Some(msg.tx_refund_amnesty_fee),
                tx_refund_burn_fee: self.tx_refund_burn_fee,
                tx_final_amnesty_fee: Some(msg.tx_final_amnesty_fee),
                tx_cancel_fee: msg.tx_cancel_fee,
                should_publish_tx_refund_burn: self.should_publish_tx_refund_burn,
            },
        ))
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Debug)]
pub struct State1 {
    a: swap_core::bitcoin::SecretKey,
    B: swap_core::bitcoin::PublicKey,
    s_a: swap_core::monero::Scalar,
    S_a_monero: monero_oxide_ext::PublicKey,
    S_a_bitcoin: swap_core::bitcoin::PublicKey,
    S_b_monero: monero_oxide_ext::PublicKey,
    S_b_bitcoin: swap_core::bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    v_a: monero::PrivateViewKey,
    dleq_proof_s_a: CrossCurveDLEQProof,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    btc_amnesty_amount: Option<bitcoin::Amount>,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    remaining_refund_timelock: Option<RemainingRefundTimelock>,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_redeem_fee: bitcoin::Amount,
    tx_punish_fee: bitcoin::Amount,
    tx_refund_fee: bitcoin::Amount,
    tx_partial_refund_fee: Option<bitcoin::Amount>,
    tx_refund_amnesty_fee: Option<bitcoin::Amount>,
    tx_refund_burn_fee: Option<bitcoin::Amount>,
    tx_final_amnesty_fee: Option<bitcoin::Amount>,
    tx_cancel_fee: bitcoin::Amount,
    should_publish_tx_refund_burn: Option<bool>,
}

impl State1 {
    pub fn next_message(&self) -> Result<Message1> {
        Ok(Message1 {
            A: self.a.public(),
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            dleq_proof_s_a: self.dleq_proof_s_a.clone(),
            v_a: self.v_a,
            redeem_address: self.redeem_address.clone(),
            punish_address: self.punish_address.clone(),
            tx_redeem_fee: self.tx_redeem_fee,
            tx_punish_fee: self.tx_punish_fee,
            amnesty_amount: self
                .btc_amnesty_amount
                .context("Missing btc_amesty_amount for new swap that should have it")?,
            tx_refund_burn_fee: self
                .tx_refund_burn_fee
                .context("Missing tx_refund_burn_fee for new swap that should have it")?,
        })
    }

    pub fn receive(self, msg: Message2) -> Result<State2> {
        let tx_lock = swap_core::bitcoin::TxLock::from_psbt(
            msg.tx_lock_psbt,
            self.a.public(),
            self.B,
            self.btc,
        )
        .context("Failed to re-construct TxLock from received PSBT")?;

        Ok(State2 {
            a: self.a,
            B: self.B,
            s_a: self.s_a,
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            btc_amnesty_amount: self.btc_amnesty_amount,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            remaining_refund_timelock: self.remaining_refund_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock,
            tx_redeem_fee: self.tx_redeem_fee,
            tx_punish_fee: self.tx_punish_fee,
            tx_refund_fee: self.tx_refund_fee,
            tx_partial_refund_fee: self.tx_partial_refund_fee,
            tx_refund_amnesty_fee: self.tx_refund_amnesty_fee,
            tx_refund_burn_fee: self.tx_refund_burn_fee,
            tx_final_amnesty_fee: self.tx_final_amnesty_fee,
            tx_cancel_fee: self.tx_cancel_fee,
            should_publish_tx_refund_burn: self.should_publish_tx_refund_burn,
        })
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Debug)]
pub struct State2 {
    a: swap_core::bitcoin::SecretKey,
    B: swap_core::bitcoin::PublicKey,
    s_a: swap_core::monero::Scalar,
    S_b_monero: monero_oxide_ext::PublicKey,
    S_b_bitcoin: swap_core::bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    btc_amnesty_amount: Option<bitcoin::Amount>,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    remaining_refund_timelock: Option<RemainingRefundTimelock>,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: swap_core::bitcoin::TxLock,
    tx_redeem_fee: bitcoin::Amount,
    tx_punish_fee: bitcoin::Amount,
    tx_refund_fee: bitcoin::Amount,
    tx_partial_refund_fee: Option<bitcoin::Amount>,
    tx_refund_amnesty_fee: Option<bitcoin::Amount>,
    tx_refund_burn_fee: Option<bitcoin::Amount>,
    tx_final_amnesty_fee: Option<bitcoin::Amount>,
    tx_cancel_fee: bitcoin::Amount,
    should_publish_tx_refund_burn: Option<bool>,
}

impl State2 {
    pub fn next_message(&self) -> Result<Message3> {
        let tx_cancel = swap_core::bitcoin::TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.a.public(),
            self.B,
            self.tx_cancel_fee,
        )
        .expect("valid cancel tx");

        let tx_cancel_sig = self.a.sign(tx_cancel.digest());

        // When the amnesty output is zero, we can't construct the tx partial refund transaction
        // due to an integer underflow.
        // We only send the cancel and full refund signatures.
        if self.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO) == bitcoin::Amount::ZERO {
            let tx_full_refund =
                TxFullRefund::new(&tx_cancel, &self.refund_address, self.tx_refund_fee);
            let tx_refund_encsig = self.a.encsign(self.S_b_bitcoin, tx_full_refund.digest());

            return Ok(Message3 {
                tx_cancel_sig,
                tx_partial_refund_encsig: None,
                tx_full_refund_encsig: Some(tx_refund_encsig),
                tx_refund_amnesty_sig: None,
            });
        }

        let tx_partial_refund = swap_core::bitcoin::TxPartialRefund::new(
            &tx_cancel,
            &self.refund_address,
            self.a.public(),
            self.B,
            self.btc_amnesty_amount
                .context("Missing btc_amnesty_amount for new swap that should have it")?,
            self.tx_refund_fee,
        )?;
        // Alice encsigns the partial refund transaction(bitcoin) digest with Bob's monero
        // pubkey(S_b). The partial refund transaction spends the output of
        // tx_lock_bitcoin to Bob's refund address (except for the amnesty output).
        // recover(encsign(a, S_b, d), sign(a, d), S_b) = s_b where d is a digest, (a,
        // A) is alice's keypair and (s_b, S_b) is bob's keypair.
        let tx_partial_refund_encsig = self.a.encsign(self.S_b_bitcoin, tx_partial_refund.digest());

        // Construct and sign TxRefundAmnesty
        let tx_refund_amnesty = swap_core::bitcoin::TxReclaim::new(
            &tx_partial_refund,
            &self.refund_address,
            self.tx_refund_amnesty_fee
                .context("Missing tx_refund_amnesty_fee for new swap")?,
            self.remaining_refund_timelock
                .context("Missing remaining_refund_timelock for new swap")?,
        )?;
        let tx_refund_amnesty_sig = self.a.sign(tx_refund_amnesty.digest());

        Ok(Message3 {
            tx_cancel_sig,
            tx_partial_refund_encsig: Some(tx_partial_refund_encsig),
            tx_refund_amnesty_sig: Some(tx_refund_amnesty_sig),
            tx_full_refund_encsig: None,
        })
    }

    pub fn receive(self, msg: Message4) -> Result<State3> {
        // Create the TxCancel transaction ourself
        let tx_cancel = swap_core::bitcoin::TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.a.public(),
            self.B,
            self.tx_cancel_fee,
        )?;

        // Check if the provided signature by Bob is valid for the transaction
        swap_core::bitcoin::verify_sig(&self.B, &tx_cancel.digest(), &msg.tx_cancel_sig)
            .context("Failed to verify cancel transaction")?;

        // Create the TxPunish transaction ourself
        let tx_punish = swap_core::bitcoin::TxPunish::new(
            &tx_cancel,
            &self.punish_address,
            self.punish_timelock,
            self.tx_punish_fee,
        );

        // Check if the provided signature by Bob is valid for the transaction
        swap_core::bitcoin::verify_sig(&self.B, &tx_punish.digest(), &msg.tx_punish_sig)
            .context("Failed to verify punish transaction")?;

        // Create the TxEarlyRefund transaction ourself
        let tx_early_refund = swap_core::bitcoin::TxEarlyRefund::new(
            &self.tx_lock,
            &self.refund_address,
            self.tx_refund_fee,
        );

        // Check if the provided signature by Bob is valid for the transaction
        swap_core::bitcoin::verify_sig(
            &self.B,
            &tx_early_refund.digest(),
            &msg.tx_early_refund_sig,
        )
        .context("Failed to verify early refund transaction")?;

        // When the bitcoin amnesty amount is zero, we can't construct the transactions for the partial refund path.
        // We sent Bob the encsig for the full refund path already, so we don't
        // care about the partial refund path signatures of Bob anyway.
        // We just save `None`.
        if self.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO) == bitcoin::Amount::ZERO {
            return Ok(State3 {
                a: self.a,
                B: self.B,
                s_a: self.s_a,
                S_b_monero: self.S_b_monero,
                S_b_bitcoin: self.S_b_bitcoin,
                v: self.v,
                btc: self.btc,
                xmr: self.xmr,
                btc_amnesty_amount: self.btc_amnesty_amount,
                cancel_timelock: self.cancel_timelock,
                punish_timelock: self.punish_timelock,
                remaining_refund_timelock: self.remaining_refund_timelock,
                refund_address: self.refund_address,
                redeem_address: self.redeem_address,
                punish_address: self.punish_address,
                tx_lock: self.tx_lock,
                tx_punish_sig_bob: msg.tx_punish_sig,
                tx_cancel_sig_bob: msg.tx_cancel_sig,
                tx_early_refund_sig_bob: msg.tx_early_refund_sig.into(),
                tx_refund_amnesty_sig_bob: None,
                tx_redeem_fee: self.tx_redeem_fee,
                tx_punish_fee: self.tx_punish_fee,
                tx_refund_fee: self.tx_refund_fee,
                tx_partial_refund_fee: self.tx_partial_refund_fee,
                tx_refund_amnesty_fee: self.tx_refund_amnesty_fee,
                tx_refund_burn_fee: self.tx_refund_burn_fee,
                tx_final_amnesty_fee: self.tx_final_amnesty_fee,
                tx_cancel_fee: self.tx_cancel_fee,
                tx_refund_burn_sig_bob: None,
                tx_final_amnesty_sig_bob: None,
                should_publish_tx_refund_burn: self.should_publish_tx_refund_burn,
            });
        }

        // Create TxRefundAmnesty ourself
        let tx_partial_refund = TxPartialRefund::new(
            &tx_cancel,
            &self.refund_address,
            self.a.public(),
            self.B,
            self.btc_amnesty_amount
                .context("missing btc_amnesty_amount")?,
            self.tx_partial_refund_fee
                .context("missing tx_partial_refund_fee")?,
        )
        .context("Couldn't construct TxPartialRefund")?;
        let tx_refund_amnesty = TxReclaim::new(
            &tx_partial_refund,
            &self.refund_address,
            self.tx_refund_amnesty_fee
                .context("missing tx_refund_amnesty_fee")?,
            self.remaining_refund_timelock
                .context("missing remaining_refund_timelock")?,
        )?;

        // Check if the provided signature by Bob is valid for the transaction
        let tx_refund_amnesty_sig = msg
            .tx_refund_amnesty_sig
            .as_ref()
            .context("Missing tx_refund_amnesty_sig from Bob")?;
        swap_core::bitcoin::verify_sig(&self.B, &tx_refund_amnesty.digest(), tx_refund_amnesty_sig)
            .context("Failed to verify refund amnesty transaction")?;

        // Create TxRefundBurn ourself
        let tx_refund_burn = TxWithhold::new(
            &tx_partial_refund,
            self.a.public(),
            self.B,
            self.tx_refund_burn_fee
                .context("missing tx_refund_burn_fee")?,
        )?;

        // Check if the provided signature by Bob is valid for the transaction
        let tx_refund_burn_sig = msg
            .tx_refund_burn_sig
            .as_ref()
            .context("Missing tx_refund_burn_sig from Bob")?;
        swap_core::bitcoin::verify_sig(&self.B, &tx_refund_burn.digest(), tx_refund_burn_sig)
            .context("Failed to verify refund burn transaction")?;

        // Create TxFinalAmnesty ourself
        let tx_final_amnesty = TxMercy::new(
            &tx_refund_burn,
            &self.refund_address,
            self.tx_final_amnesty_fee
                .context("missing tx_final_amnesty_fee")?,
        );

        // Check if the provided signature by Bob is valid for the transaction
        let tx_final_amnesty_sig = msg
            .tx_final_amnesty_sig
            .as_ref()
            .context("Missing tx_final_amnesty_sig from Bob")?;
        swap_core::bitcoin::verify_sig(&self.B, &tx_final_amnesty.digest(), tx_final_amnesty_sig)
            .context("Failed to verify final amnesty transaction")?;

        Ok(State3 {
            a: self.a,
            B: self.B,
            s_a: self.s_a,
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            btc_amnesty_amount: self.btc_amnesty_amount,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            remaining_refund_timelock: self.remaining_refund_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_punish_sig_bob: msg.tx_punish_sig,
            tx_cancel_sig_bob: msg.tx_cancel_sig,
            tx_early_refund_sig_bob: msg.tx_early_refund_sig.into(),
            tx_refund_amnesty_sig_bob: msg.tx_refund_amnesty_sig.into(),
            tx_redeem_fee: self.tx_redeem_fee,
            tx_punish_fee: self.tx_punish_fee,
            tx_refund_fee: self.tx_refund_fee,
            tx_partial_refund_fee: self.tx_partial_refund_fee,
            tx_refund_amnesty_fee: self.tx_refund_amnesty_fee,
            tx_refund_burn_fee: self.tx_refund_burn_fee,
            tx_final_amnesty_fee: self.tx_final_amnesty_fee,
            tx_cancel_fee: self.tx_cancel_fee,
            tx_refund_burn_sig_bob: msg.tx_refund_burn_sig,
            tx_final_amnesty_sig_bob: msg.tx_final_amnesty_sig,
            should_publish_tx_refund_burn: self.should_publish_tx_refund_burn,
        })
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct State3 {
    a: swap_core::bitcoin::SecretKey,
    B: swap_core::bitcoin::PublicKey,
    #[serde(with = "swap_serde::monero::scalar")]
    pub s_a: swap_core::monero::Scalar,
    S_b_monero: monero_oxide_ext::PublicKey,
    S_b_bitcoin: swap_core::bitcoin::PublicKey,
    pub v: monero::PrivateViewKey,
    pub btc: bitcoin::Amount,
    pub xmr: monero::Amount,
    pub btc_amnesty_amount: Option<bitcoin::Amount>,
    pub cancel_timelock: CancelTimelock,
    pub punish_timelock: PunishTimelock,
    #[serde(default)]
    remaining_refund_timelock: Option<RemainingRefundTimelock>,
    #[serde(with = "swap_serde::bitcoin::address_serde")]
    refund_address: bitcoin::Address,
    #[serde(with = "swap_serde::bitcoin::address_serde")]
    redeem_address: bitcoin::Address,
    #[serde(with = "swap_serde::bitcoin::address_serde")]
    punish_address: bitcoin::Address,
    pub tx_lock: swap_core::bitcoin::TxLock,
    tx_punish_sig_bob: swap_core::bitcoin::Signature,
    tx_cancel_sig_bob: swap_core::bitcoin::Signature,
    /// This field was added in this pull request:
    /// https://github.com/eigenwallet/core/pull/344
    ///
    /// Previously this did not exist. To avoid deserialization failing for
    /// older swaps we default it to None.
    ///
    /// The signature is not essential for the protocol to work. It is used optionally
    /// to allow Alice to refund the Bitcoin early. If it is not present, Bob will have
    /// to wait for the timelock to expire.
    #[serde(default)]
    tx_early_refund_sig_bob: Option<swap_core::bitcoin::Signature>,
    /// This field was added in PR [#675](https://github.com/eigenwallet/core/pull/344).
    /// It is optional to maintain backwards compatibility with old swaps in the database.
    /// Bob must send this to us during swap setup, in order for us to publish TxRefundAmnesty
    /// in case of a refund. Otherwise Bob will only be partially refunded.
    #[serde(default)]
    tx_refund_amnesty_sig_bob: Option<swap_core::bitcoin::Signature>,
    tx_redeem_fee: bitcoin::Amount,
    pub tx_punish_fee: bitcoin::Amount,
    pub tx_refund_fee: bitcoin::Amount,
    #[serde(default)]
    pub tx_partial_refund_fee: Option<bitcoin::Amount>,
    #[serde(default)]
    pub tx_refund_amnesty_fee: Option<bitcoin::Amount>,
    #[serde(default)]
    pub tx_refund_burn_fee: Option<bitcoin::Amount>,
    #[serde(default)]
    pub tx_final_amnesty_fee: Option<bitcoin::Amount>,
    pub tx_cancel_fee: bitcoin::Amount,
    #[serde(default)]
    tx_refund_burn_sig_bob: Option<swap_core::bitcoin::Signature>,
    #[serde(default)]
    tx_final_amnesty_sig_bob: Option<swap_core::bitcoin::Signature>,
    /// Whether Alice should publish TxRefundBurn to deny Bob's amnesty.
    /// None = no decision yet (legacy swaps or awaiting controller input)
    /// Some(false) = don't burn (default for new swaps)
    /// Some(true) = burn the amnesty output
    #[serde(default)]
    pub should_publish_tx_refund_burn: Option<bool>,
}

impl State3 {
    pub async fn expired_timelocks(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<ExpiredTimelocks> {
        let tx_cancel = self.tx_cancel();

        let tx_lock_status = bitcoin_wallet.status_of_script(&self.tx_lock).await?;
        let tx_cancel_status = bitcoin_wallet.status_of_script(&tx_cancel).await?;
        // Only check partial refund status if we have the data to construct it
        // (old swaps won't have these fields)
        let tx_partial_refund_status =
            if let (Some(_), Some(_)) = (self.btc_amnesty_amount, self.tx_partial_refund_fee) {
                let tx = self.tx_partial_refund()?;
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

    pub fn lock_xmr_transfer_request(&self) -> TransferRequest {
        let S_a = monero_oxide_ext::PublicKey::from_private_key(&monero_oxide_ext::PrivateKey {
            scalar: self.s_a,
        });

        let public_spend_key = S_a + self.S_b_monero;
        let public_view_key = self.v.public();

        TransferRequest {
            public_spend_key,
            public_view_key,
            amount: self.xmr.into(),
        }
    }

    pub fn tx_cancel(&self) -> TxCancel {
        TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.a.public(),
            self.B,
            self.tx_cancel_fee,
        )
        .expect("valid cancel tx")
    }

    pub fn tx_refund(&self) -> TxFullRefund {
        swap_core::bitcoin::TxFullRefund::new(
            &self.tx_cancel(),
            &self.refund_address,
            self.tx_refund_fee,
        )
    }

    pub fn tx_partial_refund(&self) -> Result<TxPartialRefund> {
        swap_core::bitcoin::TxPartialRefund::new(
            &self.tx_cancel(),
            &self.refund_address,
            self.a.public(),
            self.B,
            self.btc_amnesty_amount
                .context("Missing btc_amnesty_amount")?,
            self.tx_partial_refund_fee
                .context("Missing tx_partial_refund_fee")?,
        )
    }

    pub fn tx_redeem(&self) -> TxRedeem {
        TxRedeem::new(&self.tx_lock, &self.redeem_address, self.tx_redeem_fee)
    }

    pub fn tx_early_refund(&self) -> TxEarlyRefund {
        swap_core::bitcoin::TxEarlyRefund::new(
            &self.tx_lock,
            &self.refund_address,
            self.tx_refund_fee,
        )
    }

    pub fn extract_monero_private_key_from_refund(
        &self,
        signed_refund_tx: Arc<bitcoin::Transaction>,
    ) -> Result<monero_oxide_ext::PrivateKey> {
        Ok(monero_oxide_ext::PrivateKey::from_scalar(
            self.tx_refund()
                .extract_monero_private_key(
                    signed_refund_tx,
                    self.s_a.into_dalek(),
                    self.a.clone(),
                    self.S_b_bitcoin,
                )?
                .into_monero_oxide(),
        ))
    }

    pub fn extract_monero_private_key_from_partial_refund(
        &self,
        signed_partial_refund_tx: Arc<bitcoin::Transaction>,
    ) -> Result<monero::PrivateKey> {
        Ok(monero::PrivateKey::from_scalar(Scalar::from(
            self.tx_partial_refund()?.extract_monero_private_key(
                signed_partial_refund_tx,
                self.s_a.into(),
                self.a.clone(),
                self.S_b_bitcoin,
            )?,
        )))
    }

    pub async fn check_for_tx_cancel(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<Option<Arc<Transaction>>> {
        let tx_cancel = self.tx_cancel();
        let tx = bitcoin_wallet
            .get_raw_transaction(tx_cancel.txid())
            .await
            .context("Failed to check for existence of tx_cancel")?;

        Ok(tx)
    }

    pub async fn fetch_tx_refund(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<Option<Arc<Transaction>>> {
        let tx_refund = self.tx_refund();
        let tx = bitcoin_wallet
            .get_raw_transaction(tx_refund.txid())
            .await
            .context("Failed to fetch Bitcoin refund transaction")?;

        Ok(tx)
    }

    pub async fn submit_tx_cancel(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<Txid> {
        let transaction = self.signed_cancel_transaction()?;
        let (tx_id, _) = bitcoin_wallet
            .ensure_broadcasted(transaction, "cancel")
            .await?;
        Ok(tx_id)
    }

    pub fn signed_bitcoin_amnesty_transaction(&self) -> Result<Transaction> {
        let tx_partial_refund = self.tx_partial_refund()?;
        let tx_amnesty = TxReclaim::new(
            &tx_partial_refund,
            &self.refund_address,
            self.tx_refund_amnesty_fee
                .context("Missing tx_refund_amnesty_fee")?,
            self.remaining_refund_timelock
                .context("Missing remaining_refund_timelock")?,
        )?;

        tx_amnesty.complete_as_alice(
            self.a.clone(),
            self.B,
            self.tx_refund_amnesty_sig_bob
                .clone()
                .context("missing Bob's signature for TxRefundAmnesty")?,
        )
    }

    /// Check if we have Bob's signature for TxRefundBurn.
    pub fn has_tx_refund_burn_sig(&self) -> bool {
        self.tx_refund_burn_sig_bob.is_some()
    }

    /// Construct TxRefundBurn from tx_partial_refund output.
    pub fn tx_refund_burn(&self) -> Result<TxWithhold> {
        TxWithhold::new(
            &self.tx_partial_refund()?,
            self.a.public(),
            self.B,
            self.tx_refund_burn_fee
                .context("Missing tx_refund_burn_fee")?,
        )
    }

    /// Construct signed TxRefundBurn using Alice's key and Bob's presigned signature.
    pub fn signed_refund_burn_transaction(&self) -> Result<Transaction> {
        let tx_refund_burn = self.tx_refund_burn()?;

        tx_refund_burn.complete_as_alice(
            self.a.clone(),
            self.B,
            self.tx_refund_burn_sig_bob
                .clone()
                .context("missing Bob's signature for TxRefundBurn")?,
        )
    }

    /// Construct TxFinalAmnesty from tx_refund_burn output.
    pub fn tx_final_amnesty(&self) -> Result<TxMercy> {
        Ok(TxMercy::new(
            &self.tx_refund_burn()?,
            &self.refund_address,
            self.tx_final_amnesty_fee
                .context("Missing tx_final_amnesty_fee")?,
        ))
    }

    /// Construct signed TxFinalAmnesty using Alice's key and Bob's presigned signature.
    pub fn signed_final_amnesty_transaction(&self) -> Result<Transaction> {
        let tx_final_amnesty = self.tx_final_amnesty()?;

        tx_final_amnesty.complete_as_alice(
            self.a.clone(),
            self.B,
            self.tx_final_amnesty_sig_bob
                .clone()
                .context("missing Bob's signature for TxFinalAmnesty")?,
        )
    }

    pub async fn punish_btc(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<Txid> {
        let signed_tx_punish = self.signed_punish_transaction()?;

        let (txid, subscription) = bitcoin_wallet
            .ensure_broadcasted(signed_tx_punish, "punish")
            .await?;
        subscription.wait_until_final().await?;

        Ok(txid)
    }

    pub fn signed_redeem_transaction(
        &self,
        sig: swap_core::bitcoin::EncryptedSignature,
    ) -> Result<bitcoin::Transaction> {
        swap_core::bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address, self.tx_redeem_fee)
            .complete(sig, self.a.clone(), self.s_a.to_secpfun_scalar(), self.B)
            .context("Failed to complete Bitcoin redeem transaction")
    }

    pub fn signed_cancel_transaction(&self) -> Result<bitcoin::Transaction> {
        self.tx_cancel()
            .complete_as_alice(self.a.clone(), self.B, self.tx_cancel_sig_bob.clone())
            .context("Failed to complete Bitcoin cancel transaction")
    }

    pub fn signed_punish_transaction(&self) -> Result<bitcoin::Transaction> {
        self.tx_punish()
            .complete(self.tx_punish_sig_bob.clone(), self.a.clone(), self.B)
            .context("Failed to complete Bitcoin punish transaction")
    }

    /// Construct tx_early_refund, sign it with Bob's signature and our own.
    /// If we do not have a Bob's signature stored, we return None.
    pub fn signed_early_refund_transaction(&self) -> Option<Result<bitcoin::Transaction>> {
        let tx_early_refund = self.tx_early_refund();

        if let Some(signature) = &self.tx_early_refund_sig_bob {
            let tx = tx_early_refund
                .complete(signature.clone(), self.a.clone(), self.B)
                .context("Failed to complete Bitcoin early refund transaction");

            Some(tx)
        } else {
            None
        }
    }

    fn tx_punish(&self) -> TxPunish {
        swap_core::bitcoin::TxPunish::new(
            &self.tx_cancel(),
            &self.punish_address,
            self.punish_timelock,
            self.tx_punish_fee,
        )
    }

    pub async fn refund_btc(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<Option<monero_oxide_ext::PrivateKey>> {
        let refund_tx = bitcoin_wallet
            .get_raw_transaction(self.tx_refund().txid())
            .await?;
        let partial_refund_tx = bitcoin_wallet
            .get_raw_transaction(self.tx_partial_refund()?.txid())
            .await?;

        match (refund_tx, partial_refund_tx) {
            (Some(refund_tx), _) => {
                let spend_key = self.extract_monero_private_key_from_refund(refund_tx)?;
                Ok(Some(spend_key))
            }
            (_, Some(partial_refund_tx)) => {
                let spend_key =
                    self.extract_monero_private_key_from_partial_refund(partial_refund_tx)?;
                Ok(Some(spend_key))
            }
            (None, None) => Ok(None),
        }
    }

    pub async fn watch_for_btc_tx_full_refund(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<monero_oxide_ext::PrivateKey> {
        let tx_refund_status = bitcoin_wallet
            .subscribe_to(Box::new(self.tx_refund()))
            .await;

        tx_refund_status
            .wait_until_seen()
            .await
            .context("Failed to monitor refund transaction")?;

        self.refund_btc(bitcoin_wallet).await?.context(
            "Bitcoin refund transaction not found even though we saw it in the mempool previously",
        )
    }

    pub async fn watch_for_btc_tx_partial_refund(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> Result<monero::PrivateKey> {
        let tx_refund_status = bitcoin_wallet
            .subscribe_to(Box::new(self.tx_partial_refund()?))
            .await;

        tx_refund_status
            .wait_until_seen()
            .await
            .context("Failed to monitor refund transaction")?;

        self.refund_btc(bitcoin_wallet).await?.context(
            "Bitcoin refund transaction not found even though we saw it in the mempool previously",
        )
    }
}

pub trait ReservesMonero {
    fn reserved_monero(&self) -> monero::Amount;
}

impl ReservesMonero for AliceState {
    /// Returns the Monero amount we need to reserve for this swap
    /// i.e funds we should not use for other things
    fn reserved_monero(&self) -> monero::Amount {
        match self {
            // We haven't seen proof yet that Bob has locked the Bitcoin
            // We must assume he will not lock the Bitcoin to avoid being
            // susceptible to a DoS attack
            AliceState::Started { .. } => monero::Amount::ZERO,
            // These are the only states where we have to assume we will have to lock
            // our Monero, and we haven't done so yet.
            AliceState::BtcLockTransactionSeen { state3 } | AliceState::BtcLocked { state3 } => {
                // We reserve as much Monero as we need for the output of the lock transaction
                // and as we need for the network fee
                state3.xmr.min_conservative_balance_to_spend()
            }
            // For all other states we either have already locked the Monero
            // or we can be sure that we don't have to lock our Monero in the future
            _ => monero::Amount::ZERO,
        }
    }
}
