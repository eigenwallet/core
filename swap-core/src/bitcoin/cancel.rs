#![allow(non_snake_case)]

use crate::bitcoin::{self, CancelTimelock, PunishTimelock};
use crate::bitcoin::{
    Address, Amount, PublicKey, Transaction, TxLock, build_shared_output_descriptor,
};
use ::bitcoin::Weight;
use ::bitcoin::sighash::SighashCache;
use ::bitcoin::transaction::Version;
use ::bitcoin::{
    EcdsaSighashType, OutPoint, ScriptBuf, Sequence, TxIn, TxOut, Txid,
    locktime::absolute::LockTime as PackedLockTime, secp256k1, sighash::SegwitV0Sighash as Sighash,
};
use anyhow::Result;
use bdk_wallet::miniscript::Descriptor;
use bitcoin_wallet::primitives::Watchable;
use ecdsa_fun::Signature;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Transaction format agreed during swap setup. Missing fields in stored swaps
/// mean Legacy: changing their format would invalidate the exchanged signatures.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum CancelRefundVersion {
    #[default]
    Legacy,
    Truc,
}

#[derive(Debug)]
pub struct TxCancel {
    pub(super) inner: Transaction,
    digest: Sighash,
    pub(in crate::bitcoin) output_descriptor: Descriptor<::bitcoin::PublicKey>,
    lock_output_descriptor: Descriptor<::bitcoin::PublicKey>,
}

impl TxCancel {
    pub fn new(
        tx_lock: &TxLock,
        cancel_timelock: CancelTimelock,
        A: PublicKey,
        B: PublicKey,
        spending_fee: Amount,
        version: CancelRefundVersion,
    ) -> Result<Self> {
        let cancel_output_descriptor = build_shared_output_descriptor(A.0, B.0)?;

        let tx_in = TxIn {
            previous_output: tx_lock.as_outpoint(),
            script_sig: Default::default(),
            sequence: Sequence(cancel_timelock.0),
            witness: Default::default(),
        };

        let tx_out = TxOut {
            value: tx_lock.lock_amount() - spending_fee,
            script_pubkey: cancel_output_descriptor.script_pubkey(),
        };

        let transaction = Transaction {
            version: match version {
                CancelRefundVersion::Legacy => Version::TWO,
                CancelRefundVersion::Truc => Version(3),
            },
            lock_time: PackedLockTime::from_height(0).expect("0 to be below lock time threshold"),
            input: vec![tx_in],
            output: vec![tx_out],
        };

        let digest = SighashCache::new(&transaction)
            .p2wsh_signature_hash(
                0, // Only one input: lock_input (lock transaction)
                &tx_lock.output_descriptor.script_code().expect("scriptcode"),
                tx_lock.lock_amount(),
                EcdsaSighashType::All,
            )
            .expect("sighash");

        Ok(Self {
            inner: transaction,
            digest,
            output_descriptor: cancel_output_descriptor,
            lock_output_descriptor: tx_lock.output_descriptor.clone(),
        })
    }

    pub fn txid(&self) -> Txid {
        self.inner.compute_txid()
    }

    pub fn digest(&self) -> Sighash {
        self.digest
    }

    pub fn amount(&self) -> Amount {
        self.inner.output[0].value
    }

    pub fn as_outpoint(&self) -> OutPoint {
        OutPoint::new(self.inner.compute_txid(), 0)
    }

    pub fn complete_as_alice(
        self,
        a: bitcoin::SecretKey,
        B: bitcoin::PublicKey,
        tx_cancel_sig_B: bitcoin::Signature,
    ) -> Result<Transaction> {
        let sig_a = a.sign(self.digest());
        let sig_b = tx_cancel_sig_B;

        let tx_cancel = self
            .add_signatures((a.public(), sig_a), (B, sig_b))
            .expect("sig_{a,b} to be valid signatures for tx_cancel");

        Ok(tx_cancel)
    }

    pub fn complete_as_bob(
        self,
        A: bitcoin::PublicKey,
        b: bitcoin::SecretKey,
        tx_cancel_sig_A: bitcoin::Signature,
    ) -> Result<Transaction> {
        let sig_a = tx_cancel_sig_A;
        let sig_b = b.sign(self.digest());

        let tx_cancel = self
            .add_signatures((A, sig_a), (b.public(), sig_b))
            .expect("sig_{a,b} to be valid signatures for tx_cancel");

        Ok(tx_cancel)
    }

    fn add_signatures(
        self,
        (A, sig_a): (PublicKey, Signature),
        (B, sig_b): (PublicKey, Signature),
    ) -> Result<Transaction> {
        let satisfier = {
            let mut satisfier = HashMap::with_capacity(2);

            let A = ::bitcoin::PublicKey {
                compressed: true,
                inner: secp256k1::PublicKey::from_slice(&A.0.to_bytes())?,
            };
            let B = ::bitcoin::PublicKey {
                compressed: true,
                inner: secp256k1::PublicKey::from_slice(&B.0.to_bytes())?,
            };

            // The order in which these are inserted doesn't matter
            let sig_a = secp256k1::ecdsa::Signature::from_compact(&sig_a.to_bytes())?;
            let sig_b = secp256k1::ecdsa::Signature::from_compact(&sig_b.to_bytes())?;
            satisfier.insert(
                A,
                ::bitcoin::ecdsa::Signature {
                    signature: sig_a,
                    sighash_type: EcdsaSighashType::All,
                },
            );
            satisfier.insert(
                B,
                ::bitcoin::ecdsa::Signature {
                    signature: sig_b,
                    sighash_type: EcdsaSighashType::All,
                },
            );

            satisfier
        };

        let mut tx_cancel = self.inner;
        self.lock_output_descriptor
            .satisfy(&mut tx_cancel.input[0], satisfier)?;

        Ok(tx_cancel)
    }

    pub fn build_spend_transaction(
        &self,
        spend_address: &Address,
        sequence: Option<PunishTimelock>,
        spending_fee: Amount,
    ) -> Transaction {
        let previous_output = self.as_outpoint();

        // Punish keeps its own delay and v2 format. Refund must wait until
        // cancel confirms, so its fees cannot help mine an unconfirmed cancel.
        let (sequence, version) = if let Some(timelock) = sequence {
            (Sequence(timelock.0), Version::TWO)
        } else {
            (self.refund_sequence(), self.inner.version)
        };
        let tx_in = TxIn {
            previous_output,
            script_sig: Default::default(),
            sequence,
            witness: Default::default(),
        };

        let tx_out = TxOut {
            value: self.amount() - spending_fee,
            script_pubkey: spend_address.script_pubkey(),
        };

        Transaction {
            version,
            lock_time: PackedLockTime::from_height(0).expect("0 to be below lock time threshold"),
            input: vec![tx_in],
            output: vec![tx_out],
        }
    }

    pub fn build_refund_with_amnesty_transaction(
        &self,
        refund_address: &Address,
        amnesty_descriptor: &Descriptor<::bitcoin::PublicKey>,
        amnesty_amount: Amount,
        spending_fee: Amount,
    ) -> Transaction {
        let previous_output = self.as_outpoint();

        let tx_in = TxIn {
            previous_output,
            script_sig: Default::default(),
            sequence: self.refund_sequence(),
            witness: Default::default(),
        };

        assert!(self.amount() > (amnesty_amount + spending_fee));
        let refund_amount = self.amount() - amnesty_amount - spending_fee;

        let tx_out_refund = TxOut {
            value: refund_amount,
            script_pubkey: refund_address.script_pubkey(),
        };

        let tx_out_amnesty = TxOut {
            value: amnesty_amount,
            script_pubkey: amnesty_descriptor.script_pubkey(),
        };

        Transaction {
            version: self.inner.version,
            lock_time: PackedLockTime::from_height(0).expect("0 to be below lock time threshold"),
            input: vec![tx_in],
            output: vec![tx_out_refund, tx_out_amnesty],
        }
    }

    fn refund_sequence(&self) -> Sequence {
        if self.inner.version == Version(3) {
            return Sequence::from_height(1);
        }
        Sequence::MAX
    }

    pub fn weight() -> Weight {
        Weight::from_wu(596)
    }
}

impl Watchable for TxCancel {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> ScriptBuf {
        self.output_descriptor.script_pubkey()
    }
}
