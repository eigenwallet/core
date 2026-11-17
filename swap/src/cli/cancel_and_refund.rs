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

const REDEEM_LOOKUP_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn cancel_and_refund(
    swap_id: Uuid,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<BobState> {
    match cancel(swap_id, bitcoin_wallet.clone(), db.clone()).await {
        Ok((_, state)) => {
            wait_for_cancel_tx_confirmation(swap_id, state, bitcoin_wallet.clone(), db.clone())
                .await?;
        }
        Err(err) => {
            tracing::warn!(%err, "Could not cancel swap. Attempting to refund anyway");
        }
    }

    refund(swap_id, bitcoin_wallet, db).await
}

async fn wait_for_cancel_tx_confirmation(
    swap_id: Uuid,
    state: BobState,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<()> {
    let BobState::BtcCancelPublished(state6) = state else {
        return Ok(());
    };

    let tx_cancel = state6.construct_tx_cancel()?;
    let txid = tx_cancel.txid();
    tracing::info!(%txid, "Waiting for cancel transaction to be confirmed");

    // Alice's redeem conflicts with `TxCancel`, so `TxCancel` never confirms after a redeem.
    // A redeem in the mempool is enough. This future only ends if it finds the redeem.
    let alice_redeemed = bail_once_alice_redeem_available(swap_id, &state6, bitcoin_wallet.as_ref());

    tokio::select! {
        result = async {
            bitcoin_wallet
                .subscribe_to(Box::new(tx_cancel))
                .await
                .wait_until_final()
                .await
                .context("Failed waiting for TxCancel confirmation")
        } => result?,
        result = alice_redeemed => result?,
    }

    db.insert_latest_state(swap_id, BobState::BtcCancelled(state6).into())
        .await?;

    Ok(())
}

/// Waits until Alice's redeem is seen, then fails with [`AliceAlreadyRedeemed`] once its transaction is available.
///
/// Never returns `Ok`: without a redeem the caller's other branch decides.
async fn bail_once_alice_redeem_available(
    swap_id: Uuid,
    state6: &State6,
    bitcoin_wallet: &dyn BitcoinWallet,
) -> Result<()> {
    state6.wait_until_redeem_seen(bitcoin_wallet).await?;
    loop {
        bail_if_alice_redeemed(swap_id, state6, bitcoin_wallet).await?;
        // The redeem was seen but its raw transaction is not available yet, so retry the lookup.
        tokio::time::sleep(REDEEM_LOOKUP_RETRY_INTERVAL).await;
    }
}

/// Alice has redeemed the Bitcoin of the swap, so it cannot be cancelled or refunded.
#[derive(Debug, thiserror::Error)]
#[error(
    "Alice has already redeemed the Bitcoin of swap {swap_id}, so it cannot be cancelled or refunded. Resume the swap to claim the Monero"
)]
pub struct AliceAlreadyRedeemed {
    pub swap_id: Uuid,
}

/// Fails with [`AliceAlreadyRedeemed`] if Alice has redeemed the Bitcoin.
///
/// The redeem spends the lock, so `TxCancel` and `TxRefund` never confirm and waiting for them blocks forever.
/// Resume the swap to claim the Monero.
async fn bail_if_alice_redeemed(
    swap_id: Uuid,
    state6: &State6,
    bitcoin_wallet: &dyn BitcoinWallet,
) -> Result<()> {
    let redeemed = state6
        .check_for_tx_redeem(bitcoin_wallet)
        .await
        .context("Failed to check whether Alice redeemed the Bitcoin")?;

    if redeemed.is_some() {
        return Err(AliceAlreadyRedeemed { swap_id }.into());
    }

    Ok(())
}

/// The state from which a swap in `state` can be cancelled.
async fn state_to_cancel(swap_id: Uuid, state: BobState) -> Result<State6> {
    Ok(match state {
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
        BobState::EncSigReadyToBeSent { state, .. } => state.cancel(),
        BobState::EncSigSent { state, .. } => state.cancel(),
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
        | BobState::XmrRedeemConstructed { .. }
        | BobState::XmrRedeemPublished { .. }
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
    })
}

pub async fn cancel(
    swap_id: Uuid,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<(Txid, BobState)> {
    let state = db.get_state(swap_id).await?.try_into()?;
    let state6 = state_to_cancel(swap_id, state).await?;

    bail_if_alice_redeemed(swap_id, &state6, bitcoin_wallet.as_ref()).await?;

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
    let (state6, refund_type, subscription) =
        match publish_refund(swap_id, bitcoin_wallet.clone(), db.clone()).await? {
            RefundPublication::Reclaim(state6) => {
                return reclaim(swap_id, state6, bitcoin_wallet, db)
                    .await
                    .context("Couldn't reclaim anti-spam deposit");
            }
            RefundPublication::Published {
                state6,
                refund_type,
                subscription,
            } => (state6, refund_type, subscription),
        };

    // Wait for the transaction to be confirmed
    tracing::info!("Waiting for refund transaction to be confirmed...");
    subscription.wait_until_final().await?;

    record_refund_confirmed(swap_id, state6, refund_type, db).await
}

/// Takes the next step towards refunding the swap, without waiting for any
/// transaction to confirm, so the caller does not hold the swap lock while one
/// is pending. Publishes cancel until it is final, then publishes the refund,
/// then records the refund once it is final. After a partial refund, it does
/// the same for the reclaim. Call it again to take the next step.
pub async fn advance_refund(
    swap_id: Uuid,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<BobState> {
    let state = db.get_state(swap_id).await?.try_into()?;
    let finality = bitcoin_wallet.finality_confirmations();

    // A refund or reclaim we published is recorded once it is final, whatever
    // the timelocks say by then
    match &state {
        BobState::BtcRefundPublished(state6) | BobState::BtcPartialRefundPublished(state6) => {
            let (refund_tx, refund_type) = state6.construct_best_bitcoin_refund_tx()?;
            if bitcoin_wallet
                .status_of_script(&refund_tx)
                .await?
                .is_confirmed_with(finality)
            {
                return record_refund_confirmed(swap_id, state6.clone(), refund_type, db).await;
            }
            // Publish the refund again in case it was dropped. This skips the timelock
            // checks of `publish_refund`: the refund is valid until Alice punishes.
            if bitcoin_wallet
                .status_of_script(&state6.construct_tx_cancel()?)
                .await?
                .is_confirmed_with(finality)
            {
                bitcoin_wallet
                    .ensure_broadcasted(refund_tx, &refund_type.to_string())
                    .await?;
                return Ok(state.clone());
            }
        }
        BobState::BtcReclaimPublished(state6) => {
            return advance_reclaim(swap_id, state6.clone(), bitcoin_wallet, db).await;
        }
        _ => {}
    }

    let state6 = state_to_cancel(swap_id, state).await?;

    // Alice's early refund spends the lock, so cancel can never be published. A recovered
    // swap is restored as `BtcLocked`, so record the finished refund here instead of retrying forever.
    // This re-implements parts of the swap state machine, which is a sign that the watcher
    // should be abandoned at some point.
    if bitcoin_wallet
        .status_of_script(&state6.construct_tx_early_refund())
        .await
        .context("Failed to check whether Alice refunded the Bitcoin early")?
        .is_confirmed_with(finality)
    {
        let state = BobState::BtcEarlyRefunded(state6);
        db.insert_latest_state(swap_id, state.clone().into())
            .await?;
        return Ok(state);
    }

    let tx_cancel_status = bitcoin_wallet
        .status_of_script(&state6.construct_tx_cancel()?)
        .await?;
    if !tx_cancel_status.is_confirmed_with(finality) {
        let (_txid, state) = cancel(swap_id, bitcoin_wallet, db).await?;
        return Ok(state);
    }

    let (state6, refund_type) =
        match publish_refund(swap_id, bitcoin_wallet.clone(), db.clone()).await? {
            RefundPublication::Reclaim(state6) => {
                return advance_reclaim(swap_id, state6, bitcoin_wallet, db).await;
            }
            RefundPublication::Published {
                state6,
                refund_type,
                ..
            } => (state6, refund_type),
        };

    let (refund_tx, _) = state6.construct_best_bitcoin_refund_tx()?;
    if !bitcoin_wallet
        .status_of_script(&refund_tx)
        .await?
        .is_confirmed_with(finality)
    {
        return Ok(refund_published_state(state6, &refund_type));
    }

    record_refund_confirmed(swap_id, state6, refund_type, db).await
}

enum RefundPublication {
    /// The partial refund is done, so the anti-spam deposit is reclaimed next
    Reclaim(State6),
    Published {
        state6: State6,
        refund_type: RefundType,
        subscription: bitcoin_wallet::Subscription,
    },
}

/// Publishes the best refund we have, unless the partial refund is already
/// done and the anti-spam deposit is to be reclaimed instead.
async fn publish_refund(
    swap_id: Uuid,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<RefundPublication> {
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
        BobState::EncSigReadyToBeSent { state, .. } => state.cancel(),
        BobState::EncSigSent { state, .. } => state.cancel(),
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
        | BobState::XmrRedeemConstructed { .. }
        | BobState::XmrRedeemPublished { .. }
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

    bail_if_alice_redeemed(swap_id, &state6, bitcoin_wallet.as_ref()).await?;

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
            return Ok(RefundPublication::Reclaim(state6));
        }
    }

    // A v3 refund is only valid after cancel has confirmed (BIP68). Only a
    // cancel that was seen can confirm: `cancel_and_refund` also refunds after
    // cancel failed, e.g. because Alice already redeemed.
    if state6.cancel_refund_version() == swap_core::bitcoin::CancelRefundVersion::Truc {
        let tx_cancel = state6.construct_tx_cancel()?;
        if !bitcoin_wallet
            .status_of_script(&tx_cancel)
            .await?
            .has_been_seen()
        {
            bail!("Cannot refund because the cancel transaction was not published");
        }
        bitcoin_wallet
            .subscribe_to(Box::new(tx_cancel))
            .await
            .wait_until_confirmed_with(1_u32)
            .await
            .context("Failed waiting for cancel confirmation before refund")?;
    }

    let (refund_tx, refund_type) = state6.construct_best_bitcoin_refund_tx()?;

    tracing::info!("Best possible refund available: {refund_type}");
    tracing::info!("Attempting to publish Bitcoin refund transaction");

    let (_txid, subscription) = bitcoin_wallet
        .ensure_broadcasted(refund_tx, &refund_type.to_string())
        .await?;

    db.insert_latest_state(
        swap_id,
        refund_published_state(state6.clone(), &refund_type).into(),
    )
    .await?;

    Ok(RefundPublication::Published {
        state6,
        refund_type,
        subscription,
    })
}

fn refund_published_state(state6: State6, refund_type: &RefundType) -> BobState {
    match refund_type {
        RefundType::Full => BobState::BtcRefundPublished(state6),
        RefundType::Partial { .. } => BobState::BtcPartialRefundPublished(state6),
    }
}

async fn record_refund_confirmed(
    swap_id: Uuid,
    state6: State6,
    refund_type: RefundType,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<BobState> {
    let confirmed_state = match refund_type {
        RefundType::Full => BobState::BtcRefunded(state6),
        RefundType::Partial { .. } => BobState::BtcPartiallyRefunded(state6),
    };

    db.insert_latest_state(swap_id, confirmed_state.clone().into())
        .await?;

    Ok(confirmed_state)
}

/// Takes the next step towards reclaiming the anti-spam deposit, without
/// waiting for any transaction to confirm: records Alice's withhold or our
/// reclaim once final, and otherwise publishes the reclaim.
async fn advance_reclaim(
    swap_id: Uuid,
    state6: State6,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<BobState> {
    let finality = bitcoin_wallet.finality_confirmations();

    if bitcoin_wallet
        .status_of_script(&state6.construct_tx_withhold()?)
        .await?
        .is_confirmed_with(finality)
    {
        tracing::info!("Alice confirmed TxWithhold, anti-spam deposit is burnt");
        let state = BobState::BtcWithheld(state6);
        db.insert_latest_state(swap_id, state.clone().into())
            .await?;
        return Ok(state);
    }

    let tx_reclaim = state6
        .signed_amnesty_transaction()
        .context("Couldn't construct signed TxReclaim")?;
    if bitcoin_wallet
        .status_of_script(&tx_reclaim)
        .await?
        .is_confirmed_with(finality)
    {
        tracing::info!("TxReclaim confirmed, anti-spam deposit reclaimed");
        let state = BobState::BtcReclaimConfirmed(state6);
        db.insert_latest_state(swap_id, state.clone().into())
            .await?;
        return Ok(state);
    }

    let remaining_refund_timelock = state6
        .remaining_refund_timelock
        .context("Can't reclaim because remaining_refund_timelock is missing")?;
    let tx_partial_refund_status = bitcoin_wallet
        .status_of_script(&state6.construct_tx_partial_refund()?)
        .await?;
    // The reclaim spends the partial refund, which a reorg may have dropped
    if !tx_partial_refund_status.has_been_seen() {
        bitcoin_wallet
            .ensure_broadcasted(
                state6.signed_partial_refund_transaction()?,
                "partial btc refund",
            )
            .await?;
    }
    if !tx_partial_refund_status.is_confirmed_with(remaining_refund_timelock) {
        bail!("Cannot reclaim the anti-spam deposit before the remaining refund timelock expired");
    }

    bitcoin_wallet
        .ensure_broadcasted(tx_reclaim, "reclaim")
        .await
        .context("Couldn't broadcast TxReclaim")?;
    let state = BobState::BtcReclaimPublished(state6);
    db.insert_latest_state(swap_id, state.clone().into())
        .await?;
    Ok(state)
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
