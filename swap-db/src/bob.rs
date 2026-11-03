use serde::{Deserialize, Serialize};
use std::fmt;
use swap_core::monero::{BlockHeight, TransferProofMaybeWithTxKey};
use swap_machine::bob;
use swap_machine::bob::BobState;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Bob {
    Started {
        btc_amount: bitcoin::Amount,
        #[serde(with = "swap_serde::bitcoin::address_serde")]
        change_address: bitcoin::Address,
        tx_lock_fee: bitcoin::Amount,
        /// `None` means the swap has no Apollo transaction, also for rows written before Apollo.
        #[serde(default)]
        tx_apollo_fee: Option<bitcoin::Amount>,
    },
    ExecutionSetupDone {
        state2: bob::State2,
    },
    BtcLockReadyToPublish {
        btc_lock_tx_signed: bitcoin::Transaction,
        /// Absent in rows written before Apollo existed.
        #[serde(default)]
        apollo_tx: Option<bitcoin::Transaction>,
        state3: bob::State3,
        monero_wallet_restore_blockheight: BlockHeight,
        #[serde(default)]
        publish_deadline: Option<i64>,
        #[serde(default)]
        broadcast_attempted: bool,
    },
    BtcLocked {
        state3: bob::State3,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    XmrLockProofReceived {
        state: bob::State3,
        lock_transfer_proof: TransferProofMaybeWithTxKey,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    XmrLockTransactionSeen {
        state: bob::State3,
        lock_transfer_proof: TransferProofMaybeWithTxKey,
        monero_wallet_restore_blockheight: BlockHeight,
        #[serde(default)]
        hermes_amount: Option<swap_core::monero::Amount>,
    },
    XmrLocked {
        state4: bob::State4,
    },
    EncSigReadyToBeSent {
        state4: bob::State4,
        hermes: bob::HermesProgress,
        #[serde(default)]
        p2p_sent: bool,
    },
    EncSigSent {
        state4: bob::State4,
        #[serde(default, with = "swap_serde::monero::transaction::option")]
        hermes_tx: Option<monero_oxide_wallet::transaction::Transaction>,
    },
    BtcPunished {
        state: bob::State6,
        tx_lock_id: bitcoin::Txid,
    },
    BtcRedeemed(bob::State5),
    XmrRedeemConstructed {
        state: bob::State5,
        #[serde(with = "swap_serde::monero::transaction")]
        xmr_redeem_tx: monero_oxide_wallet::transaction::Transaction,
    },
    XmrRedeemPublished {
        state: bob::State5,
        #[serde(with = "swap_serde::monero::transaction")]
        xmr_redeem_tx: monero_oxide_wallet::transaction::Transaction,
    },
    WaitingForCancelTimelockExpiration {
        state: bob::State3,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    CancelTimelockExpired(bob::State6),
    BtcCancelPublished(bob::State6),
    BtcCancelled(bob::State6),
    BtcRefundPublished(bob::State6),
    BtcEarlyRefundPublished(bob::State6),
    BtcPartialRefundPublished(bob::State6),
    BtcPartiallyRefunded(bob::State6),
    WaitingForReclaimTimelockExpiration(bob::State6),
    ReclaimTimelockExpired(bob::State6),
    BtcReclaimPublished(bob::State6),
    BtcWithholdPublished(bob::State6),
    BtcWithheld(bob::State6),
    BtcMercyPublished(bob::State6),
    Done(BobEndState),
}

#[derive(Clone, strum::Display, Debug, Deserialize, Serialize, PartialEq)]
pub enum BobEndState {
    SafelyAborted,
    XmrRedeemed { tx_lock_id: bitcoin::Txid },
    BtcRefunded(Box<bob::State6>),
    BtcEarlyRefunded(Box<bob::State6>),
    BtcReclaimConfirmed(Box<bob::State6>),
    BtcMercyConfirmed(Box<bob::State6>),
}

impl From<BobState> for Bob {
    fn from(bob_state: BobState) -> Self {
        match bob_state {
            BobState::Started {
                btc_amount,
                change_address,
                tx_lock_fee,
                tx_apollo_fee,
            } => Bob::Started {
                btc_amount,
                change_address,
                tx_lock_fee,
                tx_apollo_fee,
            },
            BobState::SwapSetupCompleted(state2) => Bob::ExecutionSetupDone { state2 },
            BobState::BtcLockReadyToPublish {
                btc_lock_tx_signed,
                apollo_tx,
                state3,
                monero_wallet_restore_blockheight,
                publish_deadline,
                broadcast_attempted,
            } => Bob::BtcLockReadyToPublish {
                btc_lock_tx_signed,
                apollo_tx,
                state3,
                monero_wallet_restore_blockheight,
                publish_deadline,
                broadcast_attempted,
            },
            BobState::BtcLocked {
                state3,
                monero_wallet_restore_blockheight,
            } => Bob::BtcLocked {
                state3,
                monero_wallet_restore_blockheight,
            },
            BobState::XmrLockTransactionCandidate {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
            } => Bob::XmrLockProofReceived {
                state,
                lock_transfer_proof: lock_transfer_proof.into(),
                monero_wallet_restore_blockheight,
            },
            BobState::XmrLockTransactionSeen {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
                hermes_amount,
            } => Bob::XmrLockTransactionSeen {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
                hermes_amount,
            },
            BobState::XmrLocked(state4) => Bob::XmrLocked { state4 },
            BobState::EncSigReadyToBeSent {
                state,
                hermes,
                p2p_sent,
            } => Bob::EncSigReadyToBeSent {
                state4: state,
                hermes,
                p2p_sent,
            },
            BobState::EncSigSent { state, hermes_tx } => Bob::EncSigSent {
                state4: state,
                hermes_tx,
            },
            BobState::BtcRedeemed(state5) => Bob::BtcRedeemed(state5),
            BobState::XmrRedeemConstructed {
                state,
                xmr_redeem_tx,
            } => Bob::XmrRedeemConstructed {
                state,
                xmr_redeem_tx,
            },
            BobState::XmrRedeemPublished {
                state,
                xmr_redeem_tx,
            } => Bob::XmrRedeemPublished {
                state,
                xmr_redeem_tx,
            },
            BobState::WaitingForCancelTimelockExpiration {
                state,
                monero_wallet_restore_blockheight,
            } => Bob::WaitingForCancelTimelockExpiration {
                state,
                monero_wallet_restore_blockheight,
            },
            BobState::CancelTimelockExpired(state6) => Bob::CancelTimelockExpired(state6),
            BobState::BtcCancelPublished(state6) => Bob::BtcCancelPublished(state6),
            BobState::BtcCancelled(state6) => Bob::BtcCancelled(state6),
            BobState::BtcRefundPublished(state6) => Bob::BtcRefundPublished(state6),
            BobState::BtcEarlyRefundPublished(state6) => Bob::BtcEarlyRefundPublished(state6),
            BobState::BtcPartialRefundPublished(state6) => Bob::BtcPartialRefundPublished(state6),
            BobState::BtcPunished { state, tx_lock_id } => Bob::BtcPunished { state, tx_lock_id },
            BobState::BtcRefunded(state6) => Bob::Done(BobEndState::BtcRefunded(Box::new(state6))),
            BobState::XmrRedeemed { tx_lock_id } => {
                Bob::Done(BobEndState::XmrRedeemed { tx_lock_id })
            }
            BobState::BtcEarlyRefunded(state6) => {
                Bob::Done(BobEndState::BtcEarlyRefunded(Box::new(state6)))
            }
            BobState::BtcPartiallyRefunded(state6) => Bob::BtcPartiallyRefunded(state6),
            BobState::BtcReclaimPublished(state6) => Bob::BtcReclaimPublished(state6),
            BobState::BtcReclaimConfirmed(state6) => {
                Bob::Done(BobEndState::BtcReclaimConfirmed(Box::new(state6)))
            }
            BobState::WaitingForReclaimTimelockExpiration(state6) => {
                Bob::WaitingForReclaimTimelockExpiration(state6)
            }
            BobState::ReclaimTimelockExpired(state6) => Bob::ReclaimTimelockExpired(state6),
            BobState::BtcWithholdPublished(state6) => Bob::BtcWithholdPublished(state6),
            BobState::BtcWithheld(state6) => Bob::BtcWithheld(state6),
            BobState::BtcMercyPublished(state6) => Bob::BtcMercyPublished(state6),
            BobState::BtcMercyConfirmed(state6) => {
                Bob::Done(BobEndState::BtcMercyConfirmed(Box::new(state6)))
            }
            BobState::SafelyAborted => Bob::Done(BobEndState::SafelyAborted),
        }
    }
}

impl From<Bob> for BobState {
    fn from(db_state: Bob) -> Self {
        match db_state {
            Bob::Started {
                btc_amount,
                change_address,
                tx_lock_fee,
                tx_apollo_fee,
            } => BobState::Started {
                btc_amount,
                change_address,
                tx_lock_fee,
                tx_apollo_fee,
            },
            Bob::ExecutionSetupDone { state2 } => BobState::SwapSetupCompleted(state2),
            Bob::BtcLockReadyToPublish {
                btc_lock_tx_signed,
                apollo_tx,
                state3,
                monero_wallet_restore_blockheight,
                publish_deadline,
                broadcast_attempted,
            } => BobState::BtcLockReadyToPublish {
                btc_lock_tx_signed,
                apollo_tx,
                state3,
                monero_wallet_restore_blockheight,
                publish_deadline,
                broadcast_attempted,
            },
            Bob::BtcLocked {
                state3,
                monero_wallet_restore_blockheight,
            } => BobState::BtcLocked {
                state3,
                monero_wallet_restore_blockheight,
            },
            Bob::XmrLockProofReceived {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
            } => BobState::XmrLockTransactionCandidate {
                state,
                lock_transfer_proof: lock_transfer_proof.into(),
                monero_wallet_restore_blockheight,
            },
            Bob::XmrLockTransactionSeen {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
                hermes_amount,
            } => BobState::XmrLockTransactionSeen {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
                hermes_amount,
            },
            Bob::XmrLocked { state4 } => BobState::XmrLocked(state4),
            Bob::EncSigReadyToBeSent {
                state4,
                hermes,
                p2p_sent,
            } => BobState::EncSigReadyToBeSent {
                state: state4,
                hermes,
                p2p_sent,
            },
            Bob::EncSigSent { state4, hermes_tx } => BobState::EncSigSent {
                state: state4,
                hermes_tx,
            },
            Bob::BtcRedeemed(state5) => BobState::BtcRedeemed(state5),
            Bob::XmrRedeemConstructed {
                state,
                xmr_redeem_tx,
            } => BobState::XmrRedeemConstructed {
                state,
                xmr_redeem_tx,
            },
            Bob::XmrRedeemPublished {
                state,
                xmr_redeem_tx,
            } => BobState::XmrRedeemPublished {
                state,
                xmr_redeem_tx,
            },
            Bob::WaitingForCancelTimelockExpiration {
                state,
                monero_wallet_restore_blockheight,
            } => BobState::WaitingForCancelTimelockExpiration {
                state,
                monero_wallet_restore_blockheight,
            },
            Bob::CancelTimelockExpired(state6) => BobState::CancelTimelockExpired(state6),
            Bob::BtcCancelPublished(state6) => BobState::BtcCancelPublished(state6),
            Bob::BtcCancelled(state6) => BobState::BtcCancelled(state6),
            Bob::BtcRefundPublished(state6) => BobState::BtcRefundPublished(state6),
            Bob::BtcPartialRefundPublished(state6) => BobState::BtcPartialRefundPublished(state6),
            Bob::BtcPartiallyRefunded(state6) => BobState::BtcPartiallyRefunded(state6),
            Bob::BtcReclaimPublished(state6) => BobState::BtcReclaimPublished(state6),
            Bob::BtcEarlyRefundPublished(state6) => BobState::BtcEarlyRefundPublished(state6),
            Bob::BtcPunished { state, tx_lock_id } => BobState::BtcPunished { state, tx_lock_id },
            Bob::WaitingForReclaimTimelockExpiration(state6) => {
                BobState::WaitingForReclaimTimelockExpiration(state6)
            }
            Bob::ReclaimTimelockExpired(state6) => BobState::ReclaimTimelockExpired(state6),
            Bob::BtcWithholdPublished(state6) => BobState::BtcWithholdPublished(state6),
            Bob::BtcWithheld(state6) => BobState::BtcWithheld(state6),
            Bob::BtcMercyPublished(state6) => BobState::BtcMercyPublished(state6),
            Bob::Done(end_state) => match end_state {
                BobEndState::SafelyAborted => BobState::SafelyAborted,
                BobEndState::XmrRedeemed { tx_lock_id } => BobState::XmrRedeemed { tx_lock_id },
                BobEndState::BtcRefunded(state6) => BobState::BtcRefunded(*state6),
                BobEndState::BtcEarlyRefunded(state6) => BobState::BtcEarlyRefunded(*state6),
                BobEndState::BtcReclaimConfirmed(state6) => BobState::BtcReclaimConfirmed(*state6),
                BobEndState::BtcMercyConfirmed(state6) => BobState::BtcMercyConfirmed(*state6),
            },
        }
    }
}

impl fmt::Display for Bob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bob::Started { .. } => write!(f, "Started"),
            Bob::ExecutionSetupDone { .. } => f.write_str("Execution setup done"),
            Bob::BtcLockReadyToPublish { .. } => f.write_str("Bitcoin lock ready to publish"),
            Bob::BtcLocked { .. } => f.write_str("Bitcoin locked"),
            Bob::XmrLockProofReceived { .. } => {
                f.write_str("XMR lock transaction transfer proof received")
            }
            Bob::XmrLockTransactionSeen { .. } => f.write_str("XMR lock transaction seen"),
            Bob::XmrLocked { .. } => f.write_str("Monero locked"),
            Bob::EncSigReadyToBeSent { .. } => f.write_str("Encrypted signature ready to be sent"),
            Bob::WaitingForCancelTimelockExpiration { .. } => {
                f.write_str("Waiting for cancel timelock expiration")
            }
            Bob::CancelTimelockExpired(_) => f.write_str("Cancel timelock is expired"),
            Bob::BtcCancelPublished(_) => f.write_str("Bitcoin cancel published"),
            Bob::BtcCancelled(_) => f.write_str("Bitcoin refundable"),
            Bob::BtcRefundPublished { .. } => f.write_str("Bitcoin refund published"),
            Bob::BtcEarlyRefundPublished { .. } => f.write_str("Bitcoin early refund published"),
            Bob::BtcPartialRefundPublished { .. } => {
                f.write_str("Bitcoin partially refund published")
            }
            Bob::BtcRedeemed(_) => f.write_str("Monero redeemable"),
            Bob::XmrRedeemConstructed { .. } => {
                f.write_str("Monero redeem transaction constructed")
            }
            Bob::XmrRedeemPublished { .. } => f.write_str("Monero redeem transaction published"),
            Bob::Done(end_state) => write!(f, "Done: {}", end_state),
            Bob::EncSigSent { .. } => f.write_str("Encrypted signature sent"),
            Bob::BtcPunished { .. } => f.write_str("Bitcoin punished"),
            Bob::BtcPartiallyRefunded { .. } => f.write_str("Bitcoin partially refunded"),
            Bob::BtcReclaimPublished { .. } => f.write_str("Bitcoin reclaim transaction published"),
            Bob::WaitingForReclaimTimelockExpiration { .. } => {
                f.write_str("Waiting for reclaim timelock to expire")
            }
            Bob::ReclaimTimelockExpired { .. } => f.write_str("Reclaim timelock expired"),
            Bob::BtcWithholdPublished { .. } => f.write_str("Bitcoin withhold published"),
            Bob::BtcWithheld { .. } => f.write_str("Bitcoin withheld"),
            Bob::BtcMercyPublished { .. } => f.write_str("Bitcoin mercy published"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDRESS: &str = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";

    #[test]
    fn started_row_without_tx_apollo_fee_still_deserializes() {
        let old_row = format!(
            r#"{{"Started":{{"btc_amount":100000,"change_address":"{ADDRESS}","tx_lock_fee":500}}}}"#
        );
        let Bob::Started {
            btc_amount,
            tx_lock_fee,
            tx_apollo_fee,
            ..
        } = serde_json::from_str(&old_row).unwrap()
        else {
            panic!("expected Bob::Started");
        };
        assert_eq!(btc_amount, bitcoin::Amount::from_sat(100_000));
        assert_eq!(tx_lock_fee, bitcoin::Amount::from_sat(500));
        assert_eq!(tx_apollo_fee, None);

        let new_row = old_row.replace(
            r#""tx_lock_fee":500"#,
            r#""tx_lock_fee":500,"tx_apollo_fee":700"#,
        );
        let Bob::Started { tx_apollo_fee, .. } = serde_json::from_str(&new_row).unwrap() else {
            panic!("expected Bob::Started");
        };
        assert_eq!(tx_apollo_fee, Some(bitcoin::Amount::from_sat(700)));
    }

    #[test]
    fn xmr_redeemed_row_with_the_old_apollo_field_still_deserializes() {
        let txid = "0000000000000000000000000000000000000000000000000000000000000001";
        let old_row =
            format!(r#"{{"Done":{{"XmrRedeemed":{{"tx_lock_id":"{txid}","apollo":"None"}}}}}}"#);
        let Bob::Done(BobEndState::XmrRedeemed { tx_lock_id }) =
            serde_json::from_str(&old_row).unwrap()
        else {
            panic!("expected Bob::Done(XmrRedeemed)");
        };
        assert_eq!(tx_lock_id.to_string(), txid);
    }

    /// Rows of the released version have none of `apollo_tx`, `publish_deadline` and
    /// `broadcast_attempted`.
    #[test]
    fn released_btc_lock_ready_to_publish_row_still_decodes() {
        let state = Bob::BtcLockReadyToPublish {
            btc_lock_tx_signed: bitcoin::Transaction {
                version: bitcoin::transaction::Version::TWO,
                lock_time: bitcoin::absolute::LockTime::ZERO,
                input: vec![],
                output: vec![],
            },
            apollo_tx: None,
            state3: bob::apollo_test_support::state3(),
            monero_wallet_restore_blockheight: BlockHeight { height: 7 },
            publish_deadline: None,
            broadcast_attempted: false,
        };
        let mut row = serde_json::to_value(&state).unwrap();
        let fields = row["BtcLockReadyToPublish"].as_object_mut().unwrap();
        for field in ["apollo_tx", "publish_deadline", "broadcast_attempted"] {
            fields.remove(field).unwrap();
        }

        assert_eq!(serde_json::from_value::<Bob>(row).unwrap(), state);
    }
}
