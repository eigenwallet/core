use crate::monero::BlockHeight;
use crate::protocol::Database;
use crate::protocol::bob::BobState;
use anyhow::{Context, Result, bail};
use bitcoin::Txid;
use bitcoin_wallet::BitcoinWallet;
use std::sync::Arc;
use swap_core::bitcoin::ExpiredTimelocks;
use swap_machine::bob::{RefundType, State6};
use uuid::Uuid;

pub async fn cancel_and_refund(
    swap_id: Uuid,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<BobState> {
    if let Err(err) = cancel(swap_id, bitcoin_wallet.clone(), db.clone()).await {
        tracing::warn!(%err, "Could not cancel swap. Attempting to refund anyway");
    };

    let state = match refund(swap_id, bitcoin_wallet, db).await {
        Ok(s) => s,
        Err(e) => bail!(e),
    };

    Ok(state)
}

pub async fn cancel(
    swap_id: Uuid,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<(Txid, BobState)> {
    let state = db.get_state(swap_id).await?.try_into()?;

    let state6 = match state {
        BobState::SwapSetupCompleted(state2) => {
            // This is only useful if we **lost** the [`BtcLocked`] state (e.g due to a manual deletion of crash)
            let state3 = state2.lock_btc().await?.0;
            let assumed_tx_lock_id = state3.tx_lock_id();

            tracing::warn!(%assumed_tx_lock_id, "We are trying to refund despite being in state SwapSetupCompleted. We have not locked the Bitcoin. We will attempt this but it is unlikely to work.");

            // We do not know the block height, so we set it to 0
            state3.cancel(BlockHeight { height: 0 })
        }
        // Refunding in this state is not possible but we still allow it
        // because this function is only used for recovery purposes.
        // We can try to do a refund here so we do.
        BobState::BtcLockReadyToPublish {
            state3,
            monero_wallet_restore_blockheight,
            ..
        } => state3.cancel(monero_wallet_restore_blockheight),
        BobState::BtcLocked {
            state3,
            monero_wallet_restore_blockheight,
        } => state3.cancel(monero_wallet_restore_blockheight),
        BobState::XmrLockTransactionCandidate {
            state,
            monero_wallet_restore_blockheight,
            ..
        } => state.cancel(monero_wallet_restore_blockheight),
        BobState::XmrLocked(state4) => state4.cancel(),
        BobState::EncSigSent(state4) => state4.cancel(),
        BobState::WaitingForCancelTimelockExpiration {
            state,
            monero_wallet_restore_blockheight,
        } => state.cancel(monero_wallet_restore_blockheight),
        BobState::CancelTimelockExpired(state6) => state6,
        BobState::BtcRefunded(state6) => state6,
        BobState::BtcCancelPublished(state6) => state6,
        BobState::BtcCancelled(state6) => state6,
        BobState::BtcRefundPublished(state6) => state6,
        BobState::BtcEarlyRefundPublished(state6) => state6,
        BobState::BtcPartialRefundPublished(state6) => state6,
        BobState::BtcPartiallyRefunded(state6) => state6,
        BobState::BtcReclaimConfirmed(state6) => state6,
        BobState::BtcReclaimPublished(state6) => state6,
        BobState::WaitingForReclaimTimelockExpiration(state6) => state6,
        BobState::ReclaimTimelockExpired(state6) => state6,
        BobState::BtcWithholdPublished(state6) => state6,
        BobState::BtcMercyPublished(state6) => state6,

        BobState::Started { .. }
        | BobState::BtcRedeemed(_)
        | BobState::XmrRedeemed { .. }
        | BobState::BtcPunished { .. }
        | BobState::BtcEarlyRefunded { .. }
        | BobState::BtcWithheld { .. }
        | BobState::BtcMercyConfirmed { .. }
        | BobState::SafelyAborted => bail!(
            "Cannot cancel swap {} because it is in state {} which is not cancellable.",
            swap_id,
            state
        ),
        BobState::XmrLockTransactionSeen {
            state,
            monero_wallet_restore_blockheight,
            ..
        } => state.cancel(monero_wallet_restore_blockheight),
    };

    tracing::info!(%swap_id, "Attempting to manually cancel swap");

    // Attempt to just publish the cancel transaction
    match state6.submit_tx_cancel(bitcoin_wallet.as_ref()).await {
        Ok((txid, _)) => {
            let state = BobState::BtcCancelPublished(state6);
            db.insert_latest_state(swap_id, state.clone().into())
                .await?;
            Ok((txid, state))
        }

        // If we fail to submit the cancel transaction it can have one of two reasons:
        // 1. The cancel timelock hasn't expired yet
        // 2. The cancel transaction has already been published by Alice
        Err(err) => {
            // Check if Alice has already published the cancel transaction while we were absent
            if let Some(tx) = state6.check_for_tx_cancel(bitcoin_wallet.as_ref()).await? {
                let state = BobState::BtcCancelPublished(state6);
                db.insert_latest_state(swap_id, state.clone().into())
                    .await?;
                tracing::info!("Alice has already cancelled the swap");

                return Ok((tx.compute_txid(), state));
            }

            // The cancel transaction has not been published yet and we failed to publish it ourselves
            // Here we try to figure out why
            match state6.expired_timelock(bitcoin_wallet.as_ref()).await {
                // We cannot cancel because Alice has already cancelled and punished afterwards
                Ok(ExpiredTimelocks::Punish { .. }) => {
                    let state = BobState::BtcPunished {
                        state: state6.clone(),
                        tx_lock_id: state6.tx_lock_id(),
                    };
                    db.insert_latest_state(swap_id, state.clone().into())
                        .await?;
                    tracing::info!("You have been punished for not refunding in time");
                    bail!(err.context("Cannot cancel swap because we have already been punished"));
                }
                // We cannot cancel because the cancel timelock has not expired yet
                Ok(ExpiredTimelocks::None { blocks_left }) => {
                    bail!(err.context(
                        format!(
                            "Cannot cancel swap because the cancel timelock has not expired yet. Blocks left: {}",
                            blocks_left
                        )
                    ));
                }
                Ok(ExpiredTimelocks::Cancel { .. }) => {
                    bail!(err.context("Failed to cancel swap even though cancel timelock has expired. This is unexpected."));
                }
                Ok(ExpiredTimelocks::WaitingForRemainingRefund { blocks_left }) => {
                    bail!(err.context(
                        format!(
                            "Cannot cancel swap because partial refund is already in progress. Waiting {} blocks for amnesty timelock.",
                            blocks_left
                        )
                    ));
                }
                Ok(ExpiredTimelocks::RemainingRefund) => {
                    bail!(err.context("Cannot cancel swap because we are in the partial refund phase. TxReclaim can be published."));
                }
                Err(timelock_err) => {
                    bail!(
                        err.context(timelock_err)
                            .context("Failed to cancel swap and could not check timelock status")
                    );
                }
            }
        }
    }
}

pub async fn refund(
    swap_id: Uuid,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<BobState> {
    let state = db.get_state(swap_id).await?.try_into()?;

    let state6 = match state {
        BobState::BtcLocked {
            state3,
            monero_wallet_restore_blockheight,
        } => state3.cancel(monero_wallet_restore_blockheight),
        BobState::BtcLockReadyToPublish {
            state3,
            monero_wallet_restore_blockheight,
            ..
        } => state3.cancel(monero_wallet_restore_blockheight),
        BobState::XmrLockTransactionCandidate {
            state,
            monero_wallet_restore_blockheight,
            ..
        } => state.cancel(monero_wallet_restore_blockheight),
        BobState::XmrLockTransactionSeen {
            state,
            monero_wallet_restore_blockheight,
            ..
        } => state.cancel(monero_wallet_restore_blockheight),
        BobState::XmrLocked(state4) => state4.cancel(),
        BobState::EncSigSent(state4) => state4.cancel(),
        BobState::WaitingForCancelTimelockExpiration {
            state,
            monero_wallet_restore_blockheight,
        } => state.cancel(monero_wallet_restore_blockheight),
        BobState::CancelTimelockExpired(state6) => state6,
        BobState::BtcCancelPublished(state6) => state6,
        BobState::BtcCancelled(state6) => state6,
        BobState::BtcRefunded(state6) => state6,
        BobState::BtcRefundPublished(state6) => state6,
        BobState::BtcEarlyRefundPublished(state6) => state6,
        BobState::BtcPartialRefundPublished(state6) => state6,
        BobState::BtcPartiallyRefunded(state6) => state6,
        BobState::BtcReclaimPublished(state6) => state6,
        BobState::BtcReclaimConfirmed(state6) => state6,
        BobState::WaitingForReclaimTimelockExpiration(state6) => state6,
        BobState::ReclaimTimelockExpired(state6) => state6,
        BobState::BtcWithholdPublished(state6) => state6,
        BobState::BtcMercyPublished(state6) => state6,
        BobState::Started { .. }
        | BobState::SwapSetupCompleted(_)
        | BobState::BtcRedeemed(_)
        | BobState::BtcEarlyRefunded { .. }
        | BobState::XmrRedeemed { .. }
        | BobState::BtcPunished { .. }
        | BobState::BtcWithheld { .. }
        | BobState::BtcMercyConfirmed { .. }
        | BobState::SafelyAborted => bail!(
            "Cannot refund swap {} because it is in state {} which is not refundable.",
            swap_id,
            state
        ),
    };

    tracing::info!(%swap_id, "Checking timelocks before attempting to manually refund swap");

    match state6.expired_timelock(bitcoin_wallet.as_ref()).await? {
        ExpiredTimelocks::None { blocks_left } => {
            // Cancel timelock isn't even expired -> no refund possible
            bail!(
                "Cannot refund swap because the cancel timelock has not expired yet. Blocks left: {}",
                blocks_left
            );
        }
        ExpiredTimelocks::Cancel { .. } => {
            // Refund possible
        }
        ExpiredTimelocks::Punish { .. } => {
            // We have been punished -> can't refund Bitcoin.
            // Only option left is cooperative redeem which is out of scope for this function
            let state = BobState::BtcPunished {
                state: state6.clone(),
                tx_lock_id: state6.tx_lock_id(),
            };
            db.insert_latest_state(swap_id, state.into()).await?;
            bail!(
                "Cannot refund swap because we have already been punished. Resume the swap to attempt cooperative redeem."
            );
        }
        ExpiredTimelocks::WaitingForRemainingRefund { .. } | ExpiredTimelocks::RemainingRefund => {
            // This means we already published TxPartialRefund, so we try to reclaim the
            // deposit
            tracing::info!(
                "TxPartialRefund was already published, attempting to reclaim the remaining Bitcoin (anti-spam deposit)"
            );
            return reclaim(swap_id, state6, bitcoin_wallet, db)
                .await
                .context("Couldn't reclaim anti-spam deposit");
        }
    }

    let (refund_tx, refund_type) = state6.construct_best_bitcoin_refund_tx()?;

    tracing::info!("Best possible refund available: {refund_type}");
    tracing::info!("Attempting to publish Bitcoin refund transaction");

    let (_txid, subscription) = bitcoin_wallet
        .ensure_broadcasted(refund_tx, &refund_type.to_string())
        .await?;

    // First save the "published" state
    let published_state = match &refund_type {
        RefundType::Full => BobState::BtcRefundPublished(state6.clone()),
        RefundType::Partial { .. } => BobState::BtcPartialRefundPublished(state6.clone()),
    };

    db.insert_latest_state(swap_id, published_state.into())
        .await?;

    // Wait for the transaction to be confirmed
    tracing::info!("Waiting for refund transaction to be confirmed...");
    subscription.wait_until_final().await?;

    // Now save and return the confirmed state
    let confirmed_state = match refund_type {
        RefundType::Full => BobState::BtcRefunded(state6),
        RefundType::Partial { .. } => BobState::BtcPartiallyRefunded(state6),
    };

    db.insert_latest_state(swap_id, confirmed_state.clone().into())
        .await?;

    Ok(confirmed_state)
}

/// On the partial refund path we need to attempt to reclaim the remaining
/// Bitcoin (anti-spam deposit) after completing the partial refund.
/// Waits for the remaining-refund timelock to expire, then publishes TxReclaim.
/// Races against Alice confirming TxWithhold — whichever confirms first wins.
async fn reclaim(
    swap_id: Uuid,
    state6: State6,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<BobState> {
    let remaining_refund_timelock = state6
        .remaining_refund_timelock
        .context("Can't reclaim because remaining_refund_timelock is missing")?;

    let tx_partial_refund = state6.construct_tx_partial_refund()?;
    let tx_withhold = state6.construct_tx_withhold()?;

    let partial_refund_sub = bitcoin_wallet
        .subscribe_to(Box::new(tx_partial_refund))
        .await;
    let withhold_sub = bitcoin_wallet.subscribe_to(Box::new(tx_withhold)).await;

    let state6_for_withhold = state6.clone();
    let db_for_reclaim = db.clone();

    let reclaim_future = async {
        partial_refund_sub
            .wait_until_confirmed_with(remaining_refund_timelock)
            .await
            .context("Failed waiting for remaining refund timelock to expire")?;

        tracing::info!("Remaining refund timelock expired, publishing TxReclaim");

        let tx_reclaim = state6
            .signed_amnesty_transaction()
            .context("Couldn't construct signed TxReclaim")?;
        let (_txid, sub) = bitcoin_wallet
            .ensure_broadcasted(tx_reclaim, "reclaim")
            .await
            .context("Couldn't broadcast TxReclaim")?;
        db_for_reclaim
            .insert_latest_state(
                swap_id,
                BobState::BtcReclaimPublished(state6.clone()).into(),
            )
            .await?;

        sub.wait_until_final()
            .await
            .context("Failed waiting for TxReclaim confirmation")?;
        tracing::info!("TxReclaim confirmed, anti-spam deposit reclaimed");
        anyhow::Ok(BobState::BtcReclaimConfirmed(state6))
    };

    let withhold_future = async {
        withhold_sub
            .wait_until_final()
            .await
            .context("Failed waiting for TxWithhold confirmation")?;
        tracing::info!("Alice confirmed TxWithhold, anti-spam deposit is burnt");
        anyhow::Ok(BobState::BtcWithheld(state6_for_withhold))
    };

    let state = tokio::select! {
        result = reclaim_future => result?,
        result = withhold_future => result?,
    };

    db.insert_latest_state(swap_id, state.clone().into())
        .await?;
    Ok(state)
}
