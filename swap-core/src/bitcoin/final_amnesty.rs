#![allow(non_snake_case)]

use crate::bitcoin::refund_burn::TxRefundBurn;
use crate::bitcoin::{self, Address, Amount, PublicKey, Transaction};
use ::bitcoin::sighash::SighashCache;
use ::bitcoin::{EcdsaSighashType, Txid, sighash::SegwitV0Sighash as Sighash};
use ::bitcoin::{ScriptBuf, Weight, secp256k1};
use anyhow::{Context, Result};
use bdk_wallet::miniscript::Descriptor;
use bitcoin_wallet::primitives::Watchable;
use ecdsa_fun::Signature;
use std::collections::HashMap;

/// TxFinalAmnesty spends the burn output of TxRefundBurn and sends it to
/// Bob's refund address. This allows Alice to voluntarily refund Bob even
/// after she has "burnt" the amnesty output.
///
/// This transaction is presigned by Bob during swap setup, but Alice keeps
/// her signature private until she decides to cooperate (e.g., if Bob contacts
/// her to request the refund).
#[derive(Debug, Clone)]
pub struct TxFinalAmnesty {
    inner: Transaction,
    digest: Sighash,
    burn_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    watch_script: ScriptBuf,
}

impl TxFinalAmnesty {
    pub fn new(
        tx_refund_burn: &TxRefundBurn,
        refund_address: &Address,
        spending_fee: Amount,
    ) -> Self {
        // TODO: Handle case where fee >= burn amount more gracefully
        assert!(
            tx_refund_burn.amount() > spending_fee,
            "TxFinalAmnesty fee ({}) must be less than burn amount ({})",
            spending_fee,
            tx_refund_burn.amount()
        );

        let tx_final_amnesty = tx_refund_burn.build_spend_transaction(refund_address, spending_fee);

        let digest = SighashCache::new(&tx_final_amnesty)
            .p2wsh_signature_hash(
                0, // Only one input: burn output from tx_refund_burn
                &tx_refund_burn
                    .burn_output_descriptor
                    .script_code()
                    .expect("scriptcode"),
                tx_refund_burn.amount(),
                EcdsaSighashType::All,
            )
            .expect("sighash");

        Self {
            inner: tx_final_amnesty,
            digest,
            burn_output_descriptor: tx_refund_burn.burn_output_descriptor.clone(),
            watch_script: refund_address.script_pubkey(),
        }
    }

    pub fn txid(&self) -> Txid {
        self.inner.compute_txid()
    }

    pub fn digest(&self) -> Sighash {
        self.digest
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
        self.burn_output_descriptor
            .satisfy(&mut tx.input[0], satisfier)?;

        Ok(tx)
    }

    // TODO: calculate actual weight
    pub fn weight() -> Weight {
        Weight::from_wu(548)
    }
}

impl Watchable for TxFinalAmnesty {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> ScriptBuf {
        self.watch_script.clone()
    }
}
