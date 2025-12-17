#![allow(non_snake_case)]

use crate::bitcoin::partial_refund::TxPartialRefund;
use crate::bitcoin::{self, build_shared_output_descriptor, Address, Amount, PublicKey, Transaction};
use ::bitcoin::sighash::SighashCache;
use ::bitcoin::{EcdsaSighashType, Txid, sighash::SegwitV0Sighash as Sighash};
use ::bitcoin::{OutPoint, ScriptBuf, Weight, secp256k1};
use anyhow::{Context, Result};
use bdk_wallet::miniscript::Descriptor;
use bitcoin_wallet::primitives::Watchable;
use ecdsa_fun::Signature;
use std::collections::HashMap;

/// TxRefundBurn spends the amnesty output of TxPartialRefund and sends it to
/// a new 2-of-2 multisig. This allows Alice to "burn" the amnesty (prevent Bob
/// from claiming it via TxRefundAmnesty) while still allowing a later refund
/// via TxFinalAmnesty if Alice cooperates.
///
/// Unlike TxRefundAmnesty, this transaction has no timelock - Alice can publish
/// it immediately after TxPartialRefund is confirmed.
#[derive(Debug, Clone)]
pub struct TxRefundBurn {
    inner: Transaction,
    digest: Sighash,
    amnesty_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    pub(in crate::bitcoin) burn_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    watch_script: ScriptBuf,
}

impl TxRefundBurn {
    pub fn new(
        tx_partial_refund: &TxPartialRefund,
        A: PublicKey,
        B: PublicKey,
        spending_fee: Amount,
    ) -> Result<Self> {
        // TODO: Handle case where fee >= amnesty_amount more gracefully
        // For now, assert to catch this during development
        assert!(
            tx_partial_refund.amnesty_amount() > spending_fee,
            "TxRefundBurn fee ({}) must be less than amnesty amount ({})",
            spending_fee,
            tx_partial_refund.amnesty_amount()
        );

        let burn_output_descriptor = build_shared_output_descriptor(A.0, B.0)?;

        let tx_refund_burn = tx_partial_refund.build_burn_spend_transaction(
            &burn_output_descriptor,
            spending_fee,
        );

        let digest = SighashCache::new(&tx_refund_burn)
            .p2wsh_signature_hash(
                0, // Only one input: amnesty output from tx_partial_refund
                &tx_partial_refund
                    .amnesty_output_descriptor
                    .script_code()
                    .expect("scriptcode"),
                tx_partial_refund.amnesty_amount(),
                EcdsaSighashType::All,
            )
            .expect("sighash");

        let watch_script = burn_output_descriptor.script_pubkey();

        Ok(Self {
            inner: tx_refund_burn,
            digest,
            amnesty_output_descriptor: tx_partial_refund.amnesty_output_descriptor.clone(),
            burn_output_descriptor,
            watch_script,
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
        OutPoint::new(self.txid(), 0)
    }

    pub fn complete_as_alice(
        &self,
        a: bitcoin::SecretKey,
        B: bitcoin::PublicKey,
        sig_b: Signature,
    ) -> Result<Transaction> {
        let sig_a = a.sign(self.digest());

        self.clone()
            .add_signatures((a.public(), sig_a), (B, sig_b))
            .context("Couldn't add signatures to transaction")
    }

    pub fn add_signatures(
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

            let sig_a = secp256k1::ecdsa::Signature::from_compact(&sig_a.to_bytes())?;
            let sig_b = secp256k1::ecdsa::Signature::from_compact(&sig_b.to_bytes())?;

            // The order in which these are inserted doesn't matter
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

        let mut tx = self.inner;
        self.amnesty_output_descriptor
            .satisfy(&mut tx.input[0], satisfier)?;

        Ok(tx)
    }

    /// Build a transaction that spends the burn output to a destination address.
    /// Used by TxFinalAmnesty to send the funds back to Bob's refund address.
    pub fn build_spend_transaction(
        &self,
        destination: &Address,
        spending_fee: Amount,
    ) -> Transaction {
        use ::bitcoin::{
            Sequence, TxIn, TxOut, locktime::absolute::LockTime as PackedLockTime,
            transaction::Version,
        };

        // TODO: Handle case where fee >= burn amount more gracefully
        // For now, assert to catch this during development
        assert!(
            self.amount() > spending_fee,
            "TxFinalAmnesty fee ({}) must be less than burn amount ({})",
            spending_fee,
            self.amount()
        );

        let tx_in = TxIn {
            previous_output: self.as_outpoint(),
            script_sig: Default::default(),
            sequence: Sequence(0xFFFF_FFFF), // No timelock
            witness: Default::default(),
        };

        let tx_out = TxOut {
            value: self.amount() - spending_fee,
            script_pubkey: destination.script_pubkey(),
        };

        Transaction {
            version: Version(2),
            lock_time: PackedLockTime::from_height(0).expect("0 to be below lock time threshold"),
            input: vec![tx_in],
            output: vec![tx_out],
        }
    }

    pub fn weight() -> Weight {
        Weight::from_wu(548)
    }
}

impl Watchable for TxRefundBurn {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> ScriptBuf {
        self.watch_script.clone()
    }
}
