//! The part of [`State3`] that Bob writes on-chain in the Apollo transaction.
//!
//! The backup does not contain the lock transaction. Bob finds it on chain,
//! because the Apollo transaction spends the dedicated Apollo output (vout 1) of the lock transaction.

use super::{RefundSignatures, State3, SwapSecretKeys};
use ::bitcoin::TxOut;
use anyhow::{Context, Result, ensure};
use ecdsa_fun::Signature;
use serde::{Deserialize, Serialize};
use swap_core::bitcoin::{
    self, CancelTimelock, PunishTimelock, RemainingRefundTimelock, Transaction, TxLock,
};
use swap_core::monero::{self, ScalarExt};
use swap_serde::bitcoin::address_serde;

#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State3Apollo {
    A: bitcoin::PublicKey,
    S_a_monero: monero_oxide_ext::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    xmr: monero::Amount,
    btc_amnesty_amount: Option<bitcoin::Amount>,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    remaining_refund_timelock: Option<RemainingRefundTimelock>,
    #[serde(with = "address_serde")]
    refund_address: bitcoin::Address,
    #[serde(with = "address_serde")]
    redeem_address: bitcoin::Address,
    tx_cancel_sig_a: Signature,
    refund_signatures: ApolloRefundSignatures,
    tx_redeem_fee: bitcoin::Amount,
    tx_refund_fee: bitcoin::Amount,
    tx_partial_refund_fee: Option<bitcoin::Amount>,
    tx_reclaim_fee: Option<bitcoin::Amount>,
    tx_withhold_fee: Option<bitcoin::Amount>,
    tx_mercy_fee: Option<bitcoin::Amount>,
    tx_punish_fee: bitcoin::Amount,
    tx_cancel_fee: bitcoin::Amount,
    cancel_refund_version: bitcoin::CancelRefundVersion,
    #[serde(with = "address_serde")]
    punish_address: bitcoin::Address,
}

/// Replaces [`RefundSignatures`], which is `untagged` for database compatibility.
/// Non-self-describing binary formats cannot decode an untagged enum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum ApolloRefundSignatures {
    Partial {
        tx_partial_refund_encsig: bitcoin::EncryptedSignature,
        tx_reclaim_sig: bitcoin::Signature,
    },
    Full {
        tx_partial_refund_encsig: bitcoin::EncryptedSignature,
        tx_full_refund_encsig: bitcoin::EncryptedSignature,
        tx_reclaim_sig: bitcoin::Signature,
    },
    Legacy {
        tx_full_refund_encsig: bitcoin::EncryptedSignature,
    },
}

impl From<RefundSignatures> for ApolloRefundSignatures {
    fn from(signatures: RefundSignatures) -> Self {
        match signatures {
            RefundSignatures::Partial {
                tx_partial_refund_encsig,
                tx_reclaim_sig,
            } => Self::Partial {
                tx_partial_refund_encsig,
                tx_reclaim_sig,
            },
            RefundSignatures::Full {
                tx_partial_refund_encsig,
                tx_full_refund_encsig,
                tx_reclaim_sig,
            } => Self::Full {
                tx_partial_refund_encsig,
                tx_full_refund_encsig,
                tx_reclaim_sig,
            },
            RefundSignatures::Legacy {
                tx_full_refund_encsig,
            } => Self::Legacy {
                tx_full_refund_encsig,
            },
        }
    }
}

impl From<ApolloRefundSignatures> for RefundSignatures {
    fn from(signatures: ApolloRefundSignatures) -> Self {
        match signatures {
            ApolloRefundSignatures::Partial {
                tx_partial_refund_encsig,
                tx_reclaim_sig,
            } => Self::Partial {
                tx_partial_refund_encsig,
                tx_reclaim_sig,
            },
            ApolloRefundSignatures::Full {
                tx_partial_refund_encsig,
                tx_full_refund_encsig,
                tx_reclaim_sig,
            } => Self::Full {
                tx_partial_refund_encsig,
                tx_full_refund_encsig,
                tx_reclaim_sig,
            },
            ApolloRefundSignatures::Legacy {
                tx_full_refund_encsig,
            } => Self::Legacy {
                tx_full_refund_encsig,
            },
        }
    }
}

impl State3 {
    pub fn apollo(&self, secret_keys: &SwapSecretKeys) -> Result<State3Apollo> {
        ensure!(
            self.b == secret_keys.b && self.s_b == secret_keys.s_b,
            "Swap secrets do not match the seed-derived keys, cannot omit them from the backup"
        );

        Ok(State3Apollo {
            A: self.A,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            xmr: self.xmr,
            btc_amnesty_amount: self.btc_amnesty_amount,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            remaining_refund_timelock: self.remaining_refund_timelock,
            refund_address: self.refund_address.clone(),
            redeem_address: self.redeem_address.clone(),
            tx_cancel_sig_a: self.tx_cancel_sig_a.clone(),
            refund_signatures: self.refund_signatures.clone().into(),
            tx_redeem_fee: self.tx_redeem_fee,
            tx_refund_fee: self.tx_refund_fee,
            tx_partial_refund_fee: self.tx_partial_refund_fee,
            tx_reclaim_fee: self.tx_reclaim_fee,
            tx_withhold_fee: self.tx_withhold_fee,
            tx_mercy_fee: self.tx_mercy_fee,
            tx_punish_fee: self.tx_punish_fee,
            tx_cancel_fee: self.tx_cancel_fee,
            cancel_refund_version: self.cancel_refund_version,
            punish_address: self.punish_address.clone(),
        })
    }

    /// Rebuilds [`State3`] from the backup and the published lock transaction.
    ///
    /// `tx_lock_prevouts` are the outputs that the lock transaction spends, in input order.
    pub fn from_apollo(
        backup: State3Apollo,
        secret_keys: SwapSecretKeys,
        tx_lock: Transaction,
        tx_lock_prevouts: Vec<TxOut>,
        apollo_output_vout: u32,
    ) -> Result<Self> {
        let tx_lock = TxLock::from_published_transaction(
            tx_lock,
            tx_lock_prevouts,
            backup.A,
            secret_keys.b.public(),
            apollo_output_vout,
        )
        .context("Failed to rebuild the lock transaction from the Apollo")?;

        Ok(Self {
            A: backup.A,
            b: secret_keys.b,
            s_b: secret_keys.s_b,
            S_a_monero: backup.S_a_monero,
            S_a_bitcoin: backup.S_a_bitcoin,
            v: backup.v,
            xmr: backup.xmr,
            btc_amnesty_amount: backup.btc_amnesty_amount,
            cancel_timelock: backup.cancel_timelock,
            punish_timelock: backup.punish_timelock,
            remaining_refund_timelock: backup.remaining_refund_timelock,
            refund_address: backup.refund_address,
            redeem_address: backup.redeem_address,
            tx_lock,
            tx_cancel_sig_a: backup.tx_cancel_sig_a,
            refund_signatures: backup.refund_signatures.into(),
            tx_redeem_fee: backup.tx_redeem_fee,
            tx_refund_fee: backup.tx_refund_fee,
            tx_partial_refund_fee: backup.tx_partial_refund_fee,
            tx_reclaim_fee: backup.tx_reclaim_fee,
            tx_withhold_fee: backup.tx_withhold_fee,
            tx_mercy_fee: backup.tx_mercy_fee,
            tx_punish_fee: backup.tx_punish_fee,
            tx_cancel_fee: backup.tx_cancel_fee,
            cancel_refund_version: backup.cancel_refund_version,
            punish_address: backup.punish_address,
        })
    }
}

impl State3 {
    /// Checks every signature of Alice against the keys of this state, offline.
    ///
    /// The backup is only authenticated by the seed-derived encryption key.
    /// A wrong derived `s_b` or `b` would otherwise only show up when Bob needs the refund.
    pub fn verify_alice_signatures(&self) -> Result<()> {
        let encryption_key = bitcoin::PublicKey::from(self.s_b.to_secpfun_scalar());

        let tx_cancel = bitcoin::TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
            self.cancel_refund_version,
        )?;
        bitcoin::verify_sig(&self.A, &tx_cancel.digest(), &self.tx_cancel_sig_a)
            .context("Alice's signature on TxCancel does not verify")?;

        if let Some(encsig) = self.refund_signatures.tx_full_refund_encsig() {
            let tx_full_refund =
                bitcoin::TxFullRefund::new(&tx_cancel, &self.refund_address, self.tx_refund_fee);
            bitcoin::verify_encsig(self.A, encryption_key, &tx_full_refund.digest(), &encsig)
                .context("Alice's encrypted signature on TxFullRefund does not verify")?;
        }

        let Some(encsig) = self.refund_signatures.tx_partial_refund_encsig() else {
            return Ok(());
        };
        let tx_partial_refund = bitcoin::TxPartialRefund::new(
            &tx_cancel,
            &self.refund_address,
            self.A,
            self.b.public(),
            self.btc_amnesty_amount
                .context("Missing btc_amnesty_amount")?,
            self.tx_partial_refund_fee
                .context("Missing tx_partial_refund_fee")?,
        )?;
        bitcoin::verify_encsig(self.A, encryption_key, &tx_partial_refund.digest(), &encsig)
            .context("Alice's encrypted signature on TxPartialRefund does not verify")?;

        let sig = self
            .refund_signatures
            .tx_reclaim_sig()
            .context("Missing Alice's signature on TxReclaim")?;
        let tx_reclaim = bitcoin::TxReclaim::new(
            &tx_partial_refund,
            &self.refund_address,
            self.tx_reclaim_fee.context("Missing tx_reclaim_fee")?,
            self.remaining_refund_timelock
                .context("Missing remaining_refund_timelock")?,
        )?;
        bitcoin::verify_sig(&self.A, &tx_reclaim.digest(), &sig)
            .context("Alice's signature on TxReclaim does not verify")?;

        Ok(())
    }
}

/// Fully deterministic [`State3Apollo`] values for format and size tests.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;
    use ::bitcoin::hashes::Hash;
    use ::bitcoin::key::{TweakedPublicKey, XOnlyPublicKey};
    use ::bitcoin::sighash::SegwitV0Sighash;
    use ::bitcoin::{Network, hashes::sha256};
    use std::str::FromStr;
    use swap_core::bitcoin::{Amount, Scalar};

    fn scalar(n: u8) -> Scalar {
        Scalar::from_bytes_mod_order([n; 32]).non_zero().unwrap()
    }

    fn monero_private_key(n: u8) -> monero_oxide_ext::PrivateKey {
        let mut bytes = [0; 32];
        bytes[0] = n;
        monero_oxide_ext::PrivateKey::from_slice(&bytes).unwrap()
    }

    fn digest() -> SegwitV0Sighash {
        SegwitV0Sighash::from_byte_array(sha256::Hash::hash(b"apollo").to_byte_array())
    }

    /// Returns a Taproot address on mainnet (62 characters).
    /// With `largest`, returns the longest bech32m form: a 40-byte witness v16 program on regtest (76 characters).
    fn segwit_address(largest: bool) -> bitcoin::Address {
        if !largest {
            let x_only = XOnlyPublicKey::from_str(
                "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
            )
            .unwrap();
            let key = TweakedPublicKey::dangerous_assume_tweaked(x_only);
            return ::bitcoin::Address::p2tr_tweaked(key, Network::Bitcoin);
        }
        let program =
            ::bitcoin::WitnessProgram::new(::bitcoin::WitnessVersion::V16, &[7; 40]).unwrap();
        ::bitcoin::Address::from_witness_program(program, Network::Regtest)
    }

    /// A [`State3`] with a synthetic lock transaction, rebuilt from [`state3_apollo`].
    pub fn state3() -> State3 {
        use ::bitcoin::{OutPoint, ScriptBuf, Sequence, TxIn, TxOut, absolute::LockTime};

        let backup = state3_apollo(false);
        let secret_keys = SwapSecretKeys::derive([0; 32], uuid::Uuid::nil());
        let descriptor =
            bitcoin::build_shared_output_descriptor(backup.A.into(), secret_keys.b.public().into())
                .unwrap();
        let output = |script_pubkey| TxOut {
            value: ::bitcoin::Amount::from_sat(10_000),
            script_pubkey,
        };
        let tx_lock = Transaction {
            version: ::bitcoin::transaction::Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Default::default(),
            }],
            output: vec![
                output(descriptor.script_pubkey()),
                output(segwit_address(false).script_pubkey()),
            ],
        };

        State3::from_apollo(
            backup,
            secret_keys,
            tx_lock,
            vec![output(ScriptBuf::new())],
            1,
        )
        .unwrap()
    }

    /// With `largest`, every optional field is set, the refund signatures are full, and the addresses are the longest (regtest witness v16).
    /// Otherwise the addresses are mainnet, the refund signatures are partial, and the optional fields are mixed.
    pub fn state3_apollo(largest: bool) -> State3Apollo {
        let encsig_key = bitcoin::PublicKey::from(scalar(3));
        let encsig = |n: u8| bitcoin::SecretKey::from(scalar(n)).encsign(encsig_key, digest());
        let tx_reclaim_sig = bitcoin::SecretKey::from(scalar(2)).sign(digest());
        let optional =
            |sat: u64, in_ordinary: bool| (largest || in_ordinary).then(|| Amount::from_sat(sat));

        State3Apollo {
            A: bitcoin::PublicKey::from(scalar(1)),
            S_a_monero: monero_oxide_ext::PublicKey::from_private_key(&monero_private_key(4)),
            S_a_bitcoin: bitcoin::PublicKey::from(scalar(5)),
            v: monero::PrivateViewKey(monero_private_key(6)),
            xmr: monero::Amount::from_pico(123_456_789_012_345),
            btc_amnesty_amount: Some(Amount::from_sat(10_000)),
            cancel_timelock: CancelTimelock::new(144),
            punish_timelock: PunishTimelock::new(288),
            remaining_refund_timelock: Some(RemainingRefundTimelock::new(36)),
            refund_address: segwit_address(largest),
            redeem_address: segwit_address(largest),
            tx_cancel_sig_a: tx_reclaim_sig.clone(),
            refund_signatures: if largest {
                ApolloRefundSignatures::Full {
                    tx_partial_refund_encsig: encsig(7),
                    tx_full_refund_encsig: encsig(8),
                    tx_reclaim_sig,
                }
            } else {
                ApolloRefundSignatures::Partial {
                    tx_partial_refund_encsig: encsig(7),
                    tx_reclaim_sig,
                }
            },
            tx_redeem_fee: Amount::from_sat(1_001),
            tx_refund_fee: Amount::from_sat(1_002),
            tx_partial_refund_fee: optional(1_004, true),
            tx_reclaim_fee: optional(1_005, true),
            tx_withhold_fee: optional(1_006, false),
            tx_mercy_fee: optional(1_007, false),
            tx_punish_fee: Amount::from_sat(1_008),
            tx_cancel_fee: Amount::from_sat(1_009),
            cancel_refund_version: bitcoin::CancelRefundVersion::Truc,
            punish_address: segwit_address(largest),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bob::BobState;
    use swap_core::monero::BlockHeight;

    #[test]
    fn redeem_candidate_that_reveals_no_key_is_skipped() {
        let state = test_support::state3().cancel(BlockHeight { height: 7 });
        let candidate = Transaction {
            version: ::bitcoin::transaction::Version::TWO,
            lock_time: ::bitcoin::absolute::LockTime::ZERO,
            input: vec![],
            output: vec![],
        };

        assert!(
            state
                .s_a_from_redeem_candidate(std::sync::Arc::new(candidate))
                .is_none()
        );
    }

    /// Rows of the released version lack `apollo_tx`, `publish_deadline` and `broadcast_attempted`.
    #[test]
    fn released_btc_lock_ready_to_publish_row_still_decodes() {
        let state = BobState::BtcLockReadyToPublish {
            btc_lock_tx_signed: Transaction {
                version: ::bitcoin::transaction::Version::TWO,
                lock_time: ::bitcoin::absolute::LockTime::ZERO,
                input: vec![],
                output: vec![],
            },
            apollo_tx: None,
            state3: test_support::state3(),
            monero_wallet_restore_blockheight: BlockHeight { height: 7 },
            publish_deadline: None,
            broadcast_attempted: false,
        };
        let mut row = serde_json::to_value(&state).unwrap();
        let fields = row["BtcLockReadyToPublish"].as_object_mut().unwrap();
        for field in ["apollo_tx", "publish_deadline", "broadcast_attempted"] {
            fields.remove(field).unwrap();
        }

        assert_eq!(serde_json::from_value::<BobState>(row).unwrap(), state);
    }
}
