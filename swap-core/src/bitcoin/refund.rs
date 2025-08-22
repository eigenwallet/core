use crate::bitcoin;
use crate::bitcoin::{
    verify_sig, Address, Amount, EmptyWitnessStack, NoInputs, NotThreeWitnesses, PublicKey,
    TooManyInputs, Transaction, TxCancel,
};
use ::bitcoin::sighash::SighashCache;
use ::bitcoin::{secp256k1, ScriptBuf, Weight};
use ::bitcoin::{sighash::SegwitV0Sighash as Sighash, EcdsaSighashType, Txid};
use anyhow::{bail, Context, Result};
use bdk_wallet::miniscript::Descriptor;
use bitcoin_wallet::primitives::Watchable;
use ecdsa_fun::Signature;
use std::collections::HashMap;
use std::sync::Arc;

use super::extract_ecdsa_sig;

#[derive(Debug, Clone)]
pub struct TxRefund {
    inner: Transaction,
    digest: Sighash,
    cancel_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    watch_script: ScriptBuf,
}

impl TxRefund {
    pub fn new(tx_cancel: &TxCancel, refund_address: &Address, spending_fee: Amount) -> Self {
        let tx_refund = tx_cancel.build_spend_transaction(refund_address, None, spending_fee);

        let digest = SighashCache::new(&tx_refund)
            .p2wsh_signature_hash(
                0, // Only one input: cancel transaction
                &tx_cancel
                    .output_descriptor
                    .script_code()
                    .expect("scriptcode"),
                tx_cancel.amount(),
                EcdsaSighashType::All,
            )
            .expect("sighash");

        Self {
            inner: tx_refund,
            digest,
            cancel_output_descriptor: tx_cancel.output_descriptor.clone(),
            watch_script: refund_address.script_pubkey(),
        }
    }

    pub fn txid(&self) -> Txid {
        self.inner.compute_txid()
    }

    pub fn digest(&self) -> Sighash {
        self.digest
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

        let mut tx_refund = self.inner;
        self.cancel_output_descriptor
            .satisfy(&mut tx_refund.input[0], satisfier)?;

        Ok(tx_refund)
    }

    pub fn extract_monero_private_key(
        &self,
        published_refund_tx: Arc<bitcoin::Transaction>,
        s_a: curve25519_dalek::scalar::Scalar,
        a: bitcoin::SecretKey,
        S_b_bitcoin: bitcoin::PublicKey,
    ) -> Result<curve25519_dalek::scalar::Scalar> {
        let tx_refund_sig = self
            .extract_signature_by_key(published_refund_tx, a.public())
            .context("Failed to extract signature from Bitcoin refund tx")?;
        let tx_refund_encsig = a.encsign(S_b_bitcoin, self.digest());

        let s_b = bitcoin::recover(S_b_bitcoin, tx_refund_sig, tx_refund_encsig)
            .context("Failed to recover Monero secret key from Bitcoin signature")?;

        // To convert a secp256k1 scalar to a curve25519 scalar, we need to reverse the bytes
        // because a secp256k1 scalar is big endian, whereas a curve25519 scalar is little endian
        let mut bytes = s_b.to_bytes();
        bytes.reverse();
        let s_b = curve25519_dalek::scalar::Scalar::from_bytes_mod_order(bytes);

        let spend_key = s_a + s_b;

        Ok(spend_key)
    }

    fn extract_signature_by_key(
        &self,
        candidate_transaction: Arc<Transaction>,
        B: PublicKey,
    ) -> Result<Signature> {
        let input = match candidate_transaction.input.as_slice() {
            [input] => input,
            [] => bail!(NoInputs),
            inputs => bail!(TooManyInputs(inputs.len())),
        };

        let sigs = match input.witness.to_vec().as_slice() {
            [sig_1, sig_2, _script] => [sig_1, sig_2]
                .into_iter()
                .map(|sig| extract_ecdsa_sig(sig))
                .collect::<Result<Vec<_>, _>>(),
            [] => bail!(EmptyWitnessStack),
            witnesses => bail!(NotThreeWitnesses(witnesses.len())),
        }?;

        let sig = sigs
            .into_iter()
            .find(|sig| verify_sig(&B, &self.digest(), sig).is_ok())
            .context("Neither signature on witness stack verifies against B")?;

        Ok(sig)
    }

    pub fn weight() -> Weight {
        Weight::from_wu(548)
    }
}

impl Watchable for TxRefund {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> ScriptBuf {
        self.watch_script.clone()
    }
}
