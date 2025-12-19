use crate::bitcoin::partial_refund::TxPartialRefund;
use crate::bitcoin::{self, Address, Amount, PublicKey, Transaction};
use ::bitcoin::sighash::SighashCache;
use ::bitcoin::{EcdsaSighashType, Txid, sighash::SegwitV0Sighash as Sighash};
use ::bitcoin::{ScriptBuf, Weight, secp256k1};
use anyhow::{Context, Result};
use bdk_wallet::miniscript::Descriptor;
use bitcoin_wallet::primitives::Watchable;
use ecdsa_fun::Signature;
use std::collections::HashMap;

use super::timelocks::RemainingRefundTimelock;

#[derive(Debug, Clone)]
pub struct TxRefundAmnesty {
    inner: Transaction,
    digest: Sighash,
    amensty_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    watch_script: ScriptBuf,
}

impl TxRefundAmnesty {
    pub fn new(
        tx_refund: &TxPartialRefund,
        refund_address: &Address,
        spending_fee: Amount,
        remaining_refund_timelock: RemainingRefundTimelock,
    ) -> Result<Self> {
        let tx_refund_amnesty = tx_refund
            .build_amnesty_spend_transaction(
                refund_address,
                spending_fee,
                remaining_refund_timelock,
            )
            .context("Couldn't build tx refund amnesty")?;

        let digest = SighashCache::new(&tx_refund_amnesty)
            .p2wsh_signature_hash(
                0, // Only one input: amnesty box from tx_refund
                &tx_refund
                    .amnesty_output_descriptor
                    .script_code()
                    .expect("scriptcode"),
                tx_refund.amnesty_amount(),
                EcdsaSighashType::All,
            )
            .expect("sighash");

        Ok(Self {
            inner: tx_refund_amnesty,
            digest,
            amensty_output_descriptor: tx_refund.amnesty_output_descriptor.clone(),
            watch_script: refund_address.script_pubkey(),
        })
    }

    pub fn txid(&self) -> Txid {
        self.inner.compute_txid()
    }

    pub fn digest(&self) -> Sighash {
        self.digest
    }

    pub fn complete_as_alice(
        &self,
        s_a: bitcoin::SecretKey,
        B: bitcoin::PublicKey,
        sig_b: Signature,
    ) -> Result<Transaction> {
        let digest = self.digest();
        let sig_a = s_a.sign(digest);

        self.clone()
            .add_signatures((s_a.public(), sig_a), (B, sig_b))
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

        let mut tx_refund = self.inner;
        self.amensty_output_descriptor
            .satisfy(&mut tx_refund.input[0], satisfier)?;

        Ok(tx_refund)
    }

    pub fn weight() -> Weight {
        Weight::from_wu(548)
    }
}

impl Watchable for TxRefundAmnesty {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> ScriptBuf {
        self.watch_script.clone()
    }
}
