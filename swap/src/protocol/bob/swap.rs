use crate::cli::api::tauri_bindings::LockBitcoinDetails;
use crate::cli::api::tauri_bindings::{TauriEmitter, TauriHandle, TauriSwapProgressEvent};
use crate::cli::SwapEventLoopHandle;
use crate::common::retry;
use crate::monero;
use crate::monero::MoneroAddressPool;
use crate::network::cooperative_xmr_redeem_after_punish::Response::{Fullfilled, Rejected};
use crate::network::swap_setup::bob::NewSwap;
use crate::protocol::bob::common::{
    InfallibleVerifyXmrLockTransaction, InfallibleXmrRedeemable, RecvTransferProof,
    WaitForBtcRedeem, WaitForIncomingXmrLockTransaction, WaitForXmrLockTransactionConfirmation,
    XmrRedeemable,
};
use crate::protocol::bob::*;
use crate::protocol::{bob, Database};
use anyhow::{Context as AnyContext, Result, anyhow};
use std::sync::Arc;
use std::time::Duration;
use swap_core::bitcoin::{
    ExpiredTimelocks, TxCancel, TxFinalAmnesty, TxFullRefund, TxPartialRefund, TxRefundAmnesty,
};
use swap_core::monero::BlockHeight;
use swap_env::env;
use tokio::select;
use uuid::Uuid;

const PRE_BTC_LOCK_APPROVAL_TIMEOUT_SECS: u64 = 60 * 3;

/// Identifies states that have already processed the transfer proof.
/// This is used to be able to acknowledge the transfer proof multiple times (if it was already processed).
/// This is necessary because sometimes our acknowledgement might not reach Alice.
pub fn has_already_processed_transfer_proof(state: &BobState) -> bool {
    // This match statement MUST match all states which Bob can enter after receiving the transfer proof.
    // We do not match any of the cancel / refund states because in those, the swap cannot be successfull anymore.
    matches!(
        state,
        |BobState::XmrLockTransactionSeen { .. }| BobState::XmrLocked(..)
            | BobState::EncSigSent(..)
            | BobState::BtcRedeemed(..)
            | BobState::XmrRedeemed { .. }
    )
}

// Identifies states that should be run at most once before exiting.
// This is used to prevent infinite retry loops while still allowing manual resumption.
//
// Currently, this applies to the BtcPunished state:
// - We want to attempt recovery via cooperative XMR redeem once.
// - If unsuccessful, we exit to avoid an infinite retry loop.
// - The swap can still be manually resumed later and retried if desired.
//
// The same is true for the BtcRefundBurnt. 
pub fn is_run_at_most_once(state: &BobState) -> bool {
    matches!(state, BobState::BtcPunished { .. } | BobState::BtcRefundBurnt(..))
}

#[allow(clippy::too_many_arguments)]
pub async fn run(swap: bob::Swap) -> Result<BobState> {
    run_until(swap, is_complete).await
}

pub async fn run_until(
    mut swap: bob::Swap,
    is_target_state: fn(&BobState) -> bool,
) -> Result<BobState> {
    let mut current_state = swap.state.clone();

    while !is_target_state(&current_state) {
        let next_state = next_state(
            swap.id,
            current_state.clone(),
            &mut swap.event_loop_handle,
            swap.db.clone(),
            swap.bitcoin_wallet.clone(),
            swap.monero_wallet.clone(),
            swap.monero_receive_pool.clone(),
            swap.event_emitter.clone(),
            swap.env_config,
        )
        .await?;

        swap.db
            .insert_latest_state(swap.id, next_state.clone().into())
            .await?;

        if is_run_at_most_once(&current_state) && next_state == current_state {
            break;
        }

        current_state = next_state;
    }

    Ok(current_state)
}

// TODO: We have a lot of nested retry logic here which is not very nice.
// We retry inside the EventLoop and we also retry inside the state machine.
#[allow(clippy::too_many_arguments)]
async fn next_state(
    swap_id: Uuid,
    state: BobState,
    event_loop_handle: &mut SwapEventLoopHandle,
    db: Arc<dyn Database + Send + Sync>,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    monero_wallet: Arc<monero::Wallets>,
    monero_receive_pool: MoneroAddressPool,
    event_emitter: Option<TauriHandle>,
    env_config: env::Config,
) -> Result<BobState> {
    tracing::debug!(%state, "Advancing state");

    Ok(match state {
        BobState::Started {
            btc_amount,
            change_address,
            tx_lock_fee,
        } => {
            let tx_cancel_fee = bitcoin_wallet
                .estimate_fee(TxCancel::weight(), Some(btc_amount))
                .await?;
            let tx_refund_fee = bitcoin_wallet
                .estimate_fee(TxFullRefund::weight(), Some(btc_amount))
                .await?;

            // At this point we don't know how high btc_amnesty_amount is.
            // This means we don't know how large the amount of the partial refund and amnesty transactions will be.
            // We therefore specify the same upper limit on tx fees as for the other transactions, even though
            // the maximum fee percentage might be higher due to that.
            let tx_partial_refund_fee = bitcoin_wallet
                .estimate_fee(TxPartialRefund::weight(), Some(btc_amount))
                .await?;
            let tx_refund_amnesty_fee = bitcoin_wallet
                .estimate_fee(TxRefundAmnesty::weight(), Some(btc_amount))
                .await?;
            let tx_final_amnesty_fee = bitcoin_wallet
                .estimate_fee(TxFinalAmnesty::weight(), Some(btc_amount))
                .await?;

            // Emit an event to tauri that we are negotiating with the maker to lock the Bitcoin
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::SwapSetupInflight {
                    btc_lock_amount: btc_amount,
                },
            );

            let state2 = event_loop_handle
                .setup_swap(NewSwap {
                    swap_id,
                    btc: btc_amount,
                    tx_lock_fee,
                    tx_refund_fee,
                    tx_partial_refund_fee,
                    tx_refund_amnesty_fee,
                    tx_final_amnesty_fee,
                    tx_cancel_fee,
                    bitcoin_refund_address: change_address,
                })
                .await?;

            tracing::info!(%swap_id, "Starting new swap");

            BobState::SwapSetupCompleted(state2)
        }
        BobState::SwapSetupCompleted(state2) => {
            // Alice and Bob have exchanged all necessary signatures
            let xmr_receive_amount = state2.xmr;
            let btc_amnesty_amount = state2.btc_amnesty_amount.context("btc_amnesty_amount missing")?;

            // Sign the Bitcoin lock transaction
            let (state3, tx_lock) = state2.lock_btc().await?;
            let signed_tx = bitcoin_wallet
                .sign_and_finalize(tx_lock.clone().into())
                .await
                .context("Failed to sign Bitcoin lock transaction")?;

            let btc_network_fee = tx_lock.fee().context("Failed to get fee")?;
            let btc_lock_amount = signed_tx
                .output
                .first()
                .context("Failed to get lock amount")?
                .value;

            let details = LockBitcoinDetails {
                btc_lock_amount,
                btc_network_fee,
                btc_amnesty_amount,
                xmr_receive_amount,
                monero_receive_pool,
                swap_id,
                has_full_refund_signature: state3.refund_signatures.has_full_refund_encsig()
            };

            // We request approval before publishing the Bitcoin lock transaction,
            // as the exchange rate determined at this step might be different
            // from the one we previously displayed to the user.
            let approval_result = event_emitter
                .request_bitcoin_approval(details, PRE_BTC_LOCK_APPROVAL_TIMEOUT_SECS)
                .await;

            match approval_result {
                Ok(true) => {
                    tracing::debug!(
                        "User approved swap offer. Fetching current Monero blockheight."
                    );

                    // Record the current monero wallet block height so we don't have to scan from
                    // block 0 once we create the redeem wallet.
                    // This has to be done **before** the Bitcoin is locked in order to ensure that
                    // if Bob goes offline the recorded wallet-height is correct.
                    // If we only record this later, it can happen that Bob publishes the Bitcoin
                    // transaction, goes offline, while offline Alice publishes Monero.
                    // If the Monero transaction gets confirmed before Bob comes online again then
                    // Bob would record a wallet-height that is past the lock transaction height,
                    // which can lead to the wallet not detect the transaction.
                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::RetrievingMoneroBlockheight,
                    );

                    let monero_wallet_restore_blockheight = retry(
                        "Fetch current Monero blockheight",
                        || async {
                            monero_wallet
                                .direct_rpc_block_height()
                                .await
                                .map_err(backoff::Error::transient)
                        },
                        Duration::from_secs(120),
                        None,
                    )
                    .await
                    .context("Failed to fetch current Monero blockheight")?;

                    tracing::debug!(
                        %monero_wallet_restore_blockheight,
                        "Recording monero wallet restore blockheight",
                    );

                    BobState::BtcLockReadyToPublish {
                        btc_lock_tx_signed: signed_tx,
                        state3,
                        monero_wallet_restore_blockheight: BlockHeight {
                            height: monero_wallet_restore_blockheight,
                        },
                    }
                }
                Ok(false) => {
                    tracing::warn!("User denied or timed out on swap offer approval");

                    BobState::SafelyAborted
                }
                Err(err) => {
                    tracing::warn!(%err, "Failed to get user approval for swap offer. Assuming swap was aborted.");

                    BobState::SafelyAborted
                }
            }
        }
        // User has approved the swap
        // Bitcoin lock transaction has been signed
        // Monero restore height has been recorded
        BobState::BtcLockReadyToPublish {
            btc_lock_tx_signed,
            state3,
            monero_wallet_restore_blockheight,
        } => {
            event_emitter
                .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::BtcLockPublishInflight);

            retry(
                "Publish Bitcoin lock transaction",
                || async {
                    bitcoin_wallet
                        .ensure_broadcasted(btc_lock_tx_signed.clone(), "lock")
                        .await
                        .map_err(backoff::Error::transient)?;

                    Ok(())
                },
                Duration::from_secs(5 * 60),
                None,
            )
            .await
            .context("Failed to publish Bitcoin lock transaction")?;

            BobState::BtcLocked {
                state3,
                monero_wallet_restore_blockheight,
            }
        }
        BobState::BtcLocked {
            state3,
            monero_wallet_restore_blockheight,
        } => {
            tracing::info!("Waiting for Alice to lock Monero");

            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcLockTxInMempool {
                    btc_lock_txid: state3.tx_lock_id(),
                    btc_lock_confirmations: None,
                },
            );

            let (tx_early_refund_status, tx_lock_status): (
                bitcoin_wallet::Subscription,
                bitcoin_wallet::Subscription,
            ) = tokio::join!(
                bitcoin_wallet.subscribe_to(Box::new(state3.construct_tx_early_refund())),
                bitcoin_wallet.subscribe_to(Box::new(state3.tx_lock.clone()))
            );

            // Check if we have already buffered the XMR transfer proof
            if let Some(transfer_proof) = db
                .get_buffered_transfer_proof(swap_id)
                .await
                .context("Failed to get buffered transfer proof")?
            {
                tracing::debug!(txid = %transfer_proof.tx_hash(), "Found buffered transfer proof");

                return Ok(BobState::XmrLockTransactionCandidate {
                    state: state3,
                    lock_transfer_proof: transfer_proof.into(),
                    monero_wallet_restore_blockheight,
                });
            }

            let cancel_timelock_expires = tx_lock_status.wait_until(|status| {
                // Emit a tauri event on new confirmations
                // TODO: Extract this into some helper function?
                match status {
                    bitcoin_wallet::primitives::ScriptStatus::Confirmed(confirmed) => {
                        event_emitter.emit_swap_progress_event(
                            swap_id,
                            TauriSwapProgressEvent::BtcLockTxInMempool {
                                btc_lock_txid: state3.tx_lock_id(),
                                btc_lock_confirmations: Some(u64::from(confirmed.confirmations())),
                            },
                        );
                    }
                    bitcoin_wallet::primitives::ScriptStatus::InMempool => {
                        event_emitter.emit_swap_progress_event(
                            swap_id,
                            TauriSwapProgressEvent::BtcLockTxInMempool {
                                btc_lock_txid: state3.tx_lock_id(),
                                btc_lock_confirmations: Some(0),
                            },
                        );
                    }
                    bitcoin_wallet::primitives::ScriptStatus::Unseen
                    | bitcoin_wallet::primitives::ScriptStatus::Retrying => {
                        event_emitter.emit_swap_progress_event(
                            swap_id,
                            TauriSwapProgressEvent::BtcLockTxInMempool {
                                btc_lock_txid: state3.tx_lock_id(),
                                btc_lock_confirmations: None,
                            },
                        );
                    }
                }

                // Stop when the cancel timelock expires
                status.is_confirmed_with(state3.cancel_timelock)
            });

            let wait_for_incoming_xmr_lock_transaction = state3
                .wait_for_incoming_xmr_lock_transaction(
                    &monero_wallet,
                    swap_id,
                    monero_wallet_restore_blockheight,
                );

            // Wait until any of these things happens:
            // - We see the early refund transaction published by Alice
            // - Alice sends us the XMR transfer proof
            // - We detect the incoming Monero lock transaction on the view only wallet
            // - Cancel timelock expires
            select! {
                // Wait for Alice to publish the early refund transaction
                _ = tx_early_refund_status.wait_until_seen() => {
                    BobState::BtcEarlyRefundPublished(state3.cancel(monero_wallet_restore_blockheight))
                },
                // Wait for Alice to send us the transfer proof for the Monero she locked
                transfer_proof = state3.infallible_recv_transfer_proof(event_loop_handle) => {
                    tracing::info!(transfer_proof = ?transfer_proof, "Received Monero transfer proof from Alice");

                    BobState::XmrLockTransactionCandidate {
                        state: state3,
                        lock_transfer_proof: transfer_proof.into(),
                        monero_wallet_restore_blockheight
                    }
                },
                // Wait for Monero lock to be scanned
                incoming_xmr_lock_transaction = wait_for_incoming_xmr_lock_transaction => {
                    tracing::info!(txid = %incoming_xmr_lock_transaction, "Identified Monero lock transaction candidate during block scanning");

                    let lock_transfer_proof = monero::TransferProofMaybeWithTxKey::new_without_tx_key(incoming_xmr_lock_transaction);

                    BobState::XmrLockTransactionCandidate {
                        state: state3,
                        lock_transfer_proof,
                        monero_wallet_restore_blockheight
                    }
                },
                // Wait for the cancel timelock to expire
                result = cancel_timelock_expires => {
                    result?;
                    tracing::info!("Alice took too long to lock Monero, cancelling the swap");

                    let state4 = state3.cancel(monero_wallet_restore_blockheight);
                    BobState::CancelTimelockExpired(state4)
                },
            }
        }
        BobState::XmrLockTransactionCandidate {
            state,
            lock_transfer_proof,
            monero_wallet_restore_blockheight,
        } => {
            tracing::debug!(transfer_proof = %lock_transfer_proof.tx_hash(), "Validating Monero lock transaction candidate");

            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::VerifyingXmrLockTx {
                    xmr_lock_txid: lock_transfer_proof.tx_hash(),
                },
            );

            let (tx_early_refund_status, tx_lock_status): (
                bitcoin_wallet::Subscription,
                bitcoin_wallet::Subscription,
            ) = tokio::join!(
                bitcoin_wallet.subscribe_to(Box::new(state.construct_tx_early_refund())),
                bitcoin_wallet.subscribe_to(Box::new(state.tx_lock.clone()))
            );

            let xmr_lock_transaction_verification =
                state.clone().infallible_verify_xmr_lock_transaction(
                    monero_wallet.clone(),
                    lock_transfer_proof.tx_hash(),
                );

            select! {
                // Wait until we have verified the Monero lock transaction candidate
                is_valid_transfer = xmr_lock_transaction_verification => {
                    if is_valid_transfer {
                        tracing::info!(txid = %lock_transfer_proof.tx_hash(), "Monero lock transaction is valid");

                        return Ok(BobState::XmrLockTransactionSeen {
                            state,
                            lock_transfer_proof,
                            monero_wallet_restore_blockheight,
                        });
                    } else {
                        tracing::warn!(txid = %lock_transfer_proof.tx_hash(), "Monero lock transaction is invalid. It does not transfer the correct amount of Monero to the correct address.");

                        // TODO: We loose the transfer proof here.
                        // We might need it later on in case of a cooperative Monero redeem after punish.
                        // Currently Alice will transmit us the xmr_lock_txid during the cooperative redeem.
                        // but it'd still be good not to lose this.
                        return Ok(BobState::WaitingForCancelTimelockExpiration {
                            state: state.clone(),
                            monero_wallet_restore_blockheight,
                        });
                    }
                }
                // Wait for the cancel timelock to expire
                result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                    result?;
                    BobState::CancelTimelockExpired(state.cancel(monero_wallet_restore_blockheight))
                },
                // Wait for Alice to publish the early refund transaction
                // We could have an incorrect candidate.
                // Alice might publish the early refund transaction while we are verifying the candidate
                _ = tx_early_refund_status.wait_until_seen() => {
                    BobState::BtcEarlyRefundPublished(state.cancel(monero_wallet_restore_blockheight))
                },
            }
        }
        BobState::XmrLockTransactionSeen {
            state,
            lock_transfer_proof,
            monero_wallet_restore_blockheight,
        } => {
            tracing::info!(txid = %lock_transfer_proof.tx_hash(), "Waiting for Monero lock transaction to be fully confirmed");

            // Emit initial event showing transaction
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::XmrLockTxInMempool {
                    xmr_lock_txid: lock_transfer_proof.tx_hash(),
                    xmr_lock_tx_confirmations: None,
                    xmr_lock_tx_target_confirmations: env_config
                        .monero_double_spend_safe_confirmations,
                },
            );

            let (tx_lock_status, tx_early_refund_status): (
                bitcoin_wallet::Subscription,
                bitcoin_wallet::Subscription,
            ) = tokio::join!(
                bitcoin_wallet.subscribe_to(Box::new(state.tx_lock.clone())),
                bitcoin_wallet.subscribe_to(Box::new(state.construct_tx_early_refund()))
            );

            let event_emitter_for_callback = event_emitter.clone();

            let wait_for_confirmation = state.infallible_wait_for_xmr_lock_confirmation(
                &*monero_wallet,
                lock_transfer_proof.tx_hash(),
                env_config.monero_double_spend_safe_confirmations,
                Some(
                    move |(
                        xmr_lock_txid,
                        xmr_lock_tx_confirmations,
                        xmr_lock_tx_target_confirmations,
                    )| {
                        event_emitter_for_callback.emit_swap_progress_event(
                            swap_id,
                            TauriSwapProgressEvent::XmrLockTxInMempool {
                                xmr_lock_txid,
                                xmr_lock_tx_confirmations: Some(xmr_lock_tx_confirmations),
                                xmr_lock_tx_target_confirmations,
                            },
                        );
                    },
                ),
            );

            select! {
                // Wait for the Monero lock transaction to be fully confirmed
                _ = wait_for_confirmation => {
                    BobState::XmrLocked(
                        state.xmr_locked(monero_wallet_restore_blockheight, lock_transfer_proof.clone())
                    )
                }
                // Wait for the cancel timelock to expire
                result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                    result?;
                    BobState::CancelTimelockExpired(state.cancel(monero_wallet_restore_blockheight))
                },
                // Wait for Alice to publish the early refund transaction
                // There is really no reason at all for Alice to ever do an early refund
                // after she has locked her Monero because she won't be able to refund her
                // Monero without our Bitcoin refund transaction
                // However, theoretically it's possible so we check for it
                _ = tx_early_refund_status.wait_until_seen() => {
                    BobState::BtcEarlyRefundPublished(state.cancel(monero_wallet_restore_blockheight))
                },
            }
        }
        BobState::XmrLocked(state) => {
            tracing::info!("Monero lock transaction is fully confirmed. Sending encrypted signature to Alice to allow her to redeem the Bitcoin.");

            event_emitter
                .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::PreflightEncSig);

            // TODO: Can we already send the encrypted signature to Alice here while we checking for tx_redeem?
            let bitcoin_wallet_for_retry = bitcoin_wallet.clone();
            let (redeem_state, expired_timelocks) = retry(
                "Checking Bitcoin redeem transaction and cancel timelock status before sending encrypted signature",
                || {
                    let bitcoin_wallet = bitcoin_wallet_for_retry.clone();
                    let state_for_attempt = state.clone();

                    async move {
                        let (redeem_state, expired_timelocks) = tokio::join!(
                            state_for_attempt.check_for_tx_redeem(&*bitcoin_wallet),
                            state_for_attempt.expired_timelock(&*bitcoin_wallet),
                        );

                        // In case we send the encrypted signature to Alice, but she doesn't give us a confirmation
                        // We need to check if she still published the Bitcoin redeem transaction
                        // Otherwise we risk staying stuck in "XmrLocked"
                        let redeem_state = redeem_state
                            .context("Failed to check for existence of tx_redeem before sending encrypted signature")
                            .map_err(backoff::Error::transient)?;

                        // We do not want to race tx_refund against tx_redeem
                        // we therefore never send the encrypted signature if the cancel timelock has expired
                        let expired_timelocks = expired_timelocks
                            .context("Failed to check for expired timelocks before sending encrypted signature")
                            .map_err(backoff::Error::transient)?;

                        Ok::<_, backoff::Error<anyhow::Error>>((
                            redeem_state,
                            expired_timelocks,
                        ))
                    }
                },
                None,
                None,
            )
            .await?;

            // It is important that we check for tx_redeem BEFORE checking for the timelock
            // because do not want to race tx_refund against tx_redeem and we prefer
            // successful redeem over a refund (obviously)
            if let Some(state5) = redeem_state {
                return Ok(BobState::BtcRedeemed(state5));
            }

            // Check whether we can cancel the swap and do so if possible.
            if expired_timelocks.cancel_timelock_expired() {
                return Ok(BobState::CancelTimelockExpired(state.cancel()));
            }

            let (tx_lock_status, tx_early_refund_status): (
                bitcoin_wallet::Subscription,
                bitcoin_wallet::Subscription,
            ) = tokio::join!(
                bitcoin_wallet.subscribe_to(Box::new(state.tx_lock.clone())),
                bitcoin_wallet.subscribe_to(Box::new(state.construct_tx_early_refund()))
            );

            event_emitter.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::InflightEncSig);

            // Alice has locked her Monero
            // Bob sends Alice the encrypted signature which allows her to sign and broadcast the Bitcoin redeem transaction
            select! {
                // Wait for the confirmation from Alice that she has received the encrypted signature
                _ = event_loop_handle.send_encrypted_signature(state.tx_redeem_encsig()) => {
                    BobState::EncSigSent(state)
                },
                // Wait for the Bitcoin redeem transaction to be published
                state5 = state.infallible_wait_for_btc_redeem(&*bitcoin_wallet) => {
                    BobState::BtcRedeemed(state5)
                },
                // Wait for the cancel timelock to expire
                result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                    result?;
                    BobState::CancelTimelockExpired(state.cancel())
                }
                // Wait for Alice to publish the early refund transaction
                // There is really no reason at all for Alice to ever refund the Bitcoin
                // after she has locked her Monero because she won't be able to refund her
                // Monero without our Bitcoin refund transaction
                // However, theoretically it's possible so we check for it
                _ = tx_early_refund_status.wait_until_seen() => {
                    BobState::BtcEarlyRefundPublished(state.cancel())
                },
            }
        }
        BobState::EncSigSent(state) => {
            event_emitter
                .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::EncryptedSignatureSent);

            let bitcoin_wallet_for_retry = bitcoin_wallet.clone();

            // TODO: Extract into an infallible trait in ./common.rs
            let redeem_state = retry(
                "Checking for Bitcoin redeem transaction after sending encrypted signature",
                || {
                    let bitcoin_wallet = bitcoin_wallet_for_retry.clone();
                    let state_for_attempt = state.clone();

                    async move {
                        // We need to make sure that Alice did not publish the redeem transaction while we were offline
                        // Even if the cancel timelock expired, if Alice published the redeem transaction while we were away we cannot miss it
                        // If we do we cannot refund and will never be able to leave the "CancelTimelockExpired" state
                        let redeem_state = state_for_attempt
                            .check_for_tx_redeem(&*bitcoin_wallet)
                            .await
                            .context("Failed to check for existence of tx_redeem after sending encrypted signature")
                            .map_err(backoff::Error::transient)?;

                        Ok::<_, backoff::Error<anyhow::Error>>(redeem_state)
                    }
                },
                None,
                None,
            )
            .await?;

            // It is important that we check for tx_redeem BEFORE checking for the timelock
            // because we do not want to race tx_refund against tx_redeem and we prefer
            // successful redeem over a refund
            if let Some(state5) = redeem_state {
                return Ok(BobState::BtcRedeemed(state5));
            }

            let (tx_lock_status, tx_early_refund_status): (
                bitcoin_wallet::Subscription,
                bitcoin_wallet::Subscription,
            ) = tokio::join!(
                bitcoin_wallet.subscribe_to(Box::new(state.tx_lock.clone())),
                bitcoin_wallet.subscribe_to(Box::new(state.construct_tx_early_refund()))
            );

            select! {
                // Wait for Alice to redeem the Bitcoin
                // We can then extract the key and redeem our Monero
                state5 = state.infallible_wait_for_btc_redeem(&*bitcoin_wallet) => {
                    BobState::BtcRedeemed(state5)
                },
                // Wait for the cancel timelock to expire
                result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                    result?;
                    BobState::CancelTimelockExpired(state.cancel())
                }
                // Wait for Alice to publish the early refund transaction
                // There is really no reason at all for Alice to ever refund the Bitcoin
                // after she has locked her Monero because she won't be able to refund her
                // Monero without our Bitcoin refund transaction
                // However, theoretically it's possible so we check for it
                _ = tx_early_refund_status.wait_until_seen() => {
                    BobState::BtcEarlyRefundPublished(state.cancel())
                },
            }
        }
        BobState::BtcRedeemed(state) => {
            // Now we wait for the full 10 confirmations on the Monero lock transaction
            // because we simply cannot spend it otherwise
            let event_emitter_for_callback = event_emitter.clone();

            state
                .infallible_wait_for_xmr_lock_confirmation(
                    &*monero_wallet,
                    state.lock_transfer_proof.tx_hash(),
                    env_config.monero_finality_confirmations,
                    Some(
                        move |(
                            xmr_lock_txid,
                            xmr_lock_tx_confirmations,
                            xmr_lock_tx_target_confirmations,
                        )| {
                            event_emitter_for_callback.emit_swap_progress_event(
                                swap_id,
                                TauriSwapProgressEvent::WaitingForXmrConfirmationsBeforeRedeem {
                                    xmr_lock_txid,
                                    xmr_lock_tx_confirmations,
                                    xmr_lock_tx_target_confirmations,
                                },
                            );
                        },
                    ),
                )
                .await
                .context("Failed to wait for Monero lock transaction to be confirmed")?;

            event_emitter
                .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::RedeemingMonero);

            let xmr_redeem_txid = state
                .infallible_redeem_xmr(&*monero_wallet, swap_id, monero_receive_pool.clone())
                .await;

            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::XmrRedeemInMempool {
                    xmr_redeem_txids: vec![xmr_redeem_txid],
                    xmr_receive_pool: monero_receive_pool.clone(),
                },
            );

            BobState::XmrRedeemed {
                tx_lock_id: state.tx_lock_id(),
            }
        }
        BobState::WaitingForCancelTimelockExpiration {
            state,
            monero_wallet_restore_blockheight,
        } => {
            tracing::info!("Waiting for cancel timelock to expire");

            // TODO: Also emit the confirmations and target here?
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::WaitingForCancelTimelockExpiration,
            );

            let (tx_lock_status, tx_early_refund_status): (
                bitcoin_wallet::Subscription,
                bitcoin_wallet::Subscription,
            ) = tokio::join!(
                bitcoin_wallet.subscribe_to(Box::new(state.tx_lock.clone())),
                bitcoin_wallet.subscribe_to(Box::new(state.construct_tx_early_refund()))
            );

            select! {
                // Wait for the cancel timelock to expire
                result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                    result?;
                    BobState::CancelTimelockExpired(state.cancel(monero_wallet_restore_blockheight))
                }
                // Wait for Alice to publish the early refund transaction
                _ = tx_early_refund_status.wait_until_seen() => {
                    BobState::BtcEarlyRefundPublished(state.cancel(monero_wallet_restore_blockheight))
                },
            }
        }
        BobState::CancelTimelockExpired(state6) => {
            event_emitter
                .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::CancelTimelockExpired);

            let bitcoin_wallet_for_retry = bitcoin_wallet.clone();
            let state6_for_retry = state6.clone();
            retry(
                "Check for tx_redeem, tx_early_refund and tx_cancel then publish tx_cancel if necessary",
                || {
                    let bitcoin_wallet = bitcoin_wallet_for_retry.clone();
                    let state6 = state6_for_retry.clone();
                    async move {

                    // TODO: Uncomment this once we have the required data in State6
                    // First we check if tx_redeem is present on the chain
                    // 
                    // We may have sent the enc sig close to the timelock expiration,
                    // never received the confirmation and now the cancel timelock has expired.
                    //
                    // Alice may still have received the enc sig even if we are in this state
                    // if state6.check_for_tx_redeem(&*bitcoin_wallet).await.map_err(backoff::Error::transient)?.is_some() {
                    //     return Ok(BobState::BtcRedeemed(state6));
                    // }

                    // TODO: Do these in parallel to speed up

                    // Check if tx_early_refund is present on the chain, if it is then there 
                    if state6.check_for_tx_early_refund(&*bitcoin_wallet).await.context("Failed to check for existence of tx_early_refund before cancelling").map_err(backoff::Error::transient)?.is_some() {
                        return Ok(BobState::BtcEarlyRefundPublished(state6.clone()));
                    }

                    // Then we check if tx_cancel is present on the chain
                    if state6.check_for_tx_cancel(&*bitcoin_wallet).await.context("Failed to check for existence of tx_cancel before cancelling").map_err(backoff::Error::transient)?.is_some() {
                        return Ok(BobState::BtcCancelled(state6.clone()));
                    }

                    // If none of the above are present, we publish tx_cancel
                    state6.submit_tx_cancel(&*bitcoin_wallet).await.context("Failed to submit tx_cancel after ensuring both tx_early_refund and tx_cancel are not present").map_err(backoff::Error::transient)?;

                    Ok(BobState::BtcCancelled(state6))
                    }
                },
                None,
                None,
            )
            .await
            .expect("we never stop retrying to check for tx_redeem, tx_early_refund and tx_cancel then publishing tx_cancel if necessary")
        }
        BobState::BtcCancelled(state) => {
            // TODO: We should differentiate between BtcCancelPublished and BtcCancelled (confirmed)
            let btc_cancel_txid = state.construct_tx_cancel()?.txid();

            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcCancelled { btc_cancel_txid },
            );

            let bitcoin_wallet_for_retry = bitcoin_wallet.clone();
            let state_for_retry = state.clone();

            retry(
                "Check timelocks and try to refund",
                || {
                    let bitcoin_wallet = bitcoin_wallet_for_retry.clone();
                    let state = state_for_retry.clone();
                    async move {
                    match state.expired_timelock(&*bitcoin_wallet).await.map_err(backoff::Error::transient)? {
                        ExpiredTimelocks::None { .. } => {
                            Err(backoff::Error::Permanent(anyhow::anyhow!(
                                "Internal error: canceled state reached before cancel timelock was expired"
                            )))
                        }
                        ExpiredTimelocks::Cancel { .. } => {
                            // Publish the best Bitcoin refund transaction we can sign:
                            //  - either full refund, if alice sent use that signature (prioritized)
                            //  - or just partial refund.
                            tracing::debug!("Attempting to refund Bitcoin");
                            
                            if state.refund_signatures.has_full_refund_encsig() {
                                let full_refund_tx = state.signed_full_refund_transaction().context("Couldn't construct full refund Bitcoin transaction")?;
                                tracing::debug!("Have full refund signature, attempting full refund");
                                bitcoin_wallet.ensure_broadcasted(full_refund_tx, "full refund")
                                    .await
                                    .context("Couldn't ensure broadcast of Bitcoin full refund transaction")
                                    .map_err(backoff::Error::transient)?;

                                Ok(BobState::BtcRefundPublished(state.clone()))
                            } else if state.refund_signatures.has_partial_refund_encsig() {
                                let partial_refund_tx = state.signed_partial_refund_transaction().context("Couldn't construct partial refund Bitcoin transaction")?;
                                tracing::debug!("Don't have full refund signature, attempting partial refund");
                                bitcoin_wallet.ensure_broadcasted(partial_refund_tx, "partial refund")
                                    .await
                                    .context("Couldn't ensure broadcast of Bitcoin partial refund transaction")
                                    .map_err(backoff::Error::transient)?;
                                
                                Ok(BobState::BtcPartialRefundPublished(state.clone()))
                            } else {
                                Err(backoff::Error::permanent(anyhow!("Unreachable - We have neither partial nor full refund signatures")))
                            }
                        }
                        ExpiredTimelocks::Punish => {
                            let tx_lock_id = state.tx_lock_id();

                            Ok(BobState::BtcPunished {
                                tx_lock_id,
                                state,
                            })
                        }
                        ExpiredTimelocks::WaitingForRemainingRefund { blocks_left } => {
                            // TxPartialRefund has been published, waiting for remaining_refund_timelock
                            // This is unusual from BtcCancelled state - means we published partial refund but crashed
                            // Retry until timelock expires
                            tracing::debug!("Partial refund published, waiting {} blocks for amnesty timelock", blocks_left);
                            Err(backoff::Error::transient(anyhow::anyhow!(
                                "Waiting for remaining refund timelock to expire. Blocks left: {}",
                                blocks_left
                            )))
                        }
                        ExpiredTimelocks::RemainingRefund => {
                            // TxPartialRefund was published and timelock expired - publish TxRefundAmnesty
                            // Transition to BtcPartiallyRefunded which handles amnesty publication
                            tracing::info!("Remaining refund timelock expired, can publish amnesty transaction");
                            Ok(BobState::BtcPartiallyRefunded(state))
                        }
                    }
                    }
                },
                None,
                None,
            )
            .await
            .expect("we never stop retrying to refund")
        }
        BobState::BtcRefundPublished(state) => {
            // Emit a Tauri event
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcRefundPublished {
                    btc_refund_txid: state.signed_full_refund_transaction()?.compute_txid(),
                },
            );

            // Watch for the refund transaction to be confirmed by its txid
            let tx_refund = state.construct_tx_refund()?;
            let tx_early_refund = state.construct_tx_early_refund();

            let (tx_refund_status, tx_early_refund_status): (
                bitcoin_wallet::Subscription,
                bitcoin_wallet::Subscription,
            ) = tokio::join!(
                bitcoin_wallet.subscribe_to(Box::new(tx_refund.clone())),
                bitcoin_wallet.subscribe_to(Box::new(tx_early_refund.clone())),
            );

            // Either of these two refund transactions could have been published
            // They are mutually exclusive since they spend the same UTXO
            // We wait for either of them to be confirmed, then transition into
            // BtcRefunded state with the txid of the confirmed transaction
            select! {
                // Wait for the refund transaction to be confirmed
                // TODO: Publish the tx_refund transaction anyway
                _ = tx_refund_status.wait_until_final() => {
                    let tx_refund_txid = tx_refund.txid();

                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::BtcRefunded { btc_refund_txid: tx_refund_txid },
                    );

                    BobState::BtcRefunded(state)
                },
                // Wait for the early refund transaction to be confirmed
                _ = tx_early_refund_status.wait_until_final() => {
                    let tx_early_refund_txid = tx_early_refund.txid();

                    tracing::info!(%tx_early_refund_txid, "Alice refunded us our Bitcoin early");

                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::BtcRefunded { btc_refund_txid: tx_early_refund_txid },
                    );

                    BobState::BtcEarlyRefunded(state)
                },
            }
        }
        BobState::BtcEarlyRefundPublished(state) => {
            let tx_early_refund_tx = state.construct_tx_early_refund();
            let tx_early_refund_txid = tx_early_refund_tx.txid();

            tracing::info!(%tx_early_refund_txid, "Alice has refunded us our Bitcoin early");

            // Emit Tauri event
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcEarlyRefundPublished {
                    btc_early_refund_txid: tx_early_refund_txid,
                },
            );

            // Wait for confirmations
            let (tx_lock_status, tx_early_refund_status): (
                bitcoin_wallet::Subscription,
                bitcoin_wallet::Subscription,
            ) = tokio::join!(
                bitcoin_wallet.subscribe_to(Box::new(state.tx_lock.clone())),
                bitcoin_wallet.subscribe_to(Box::new(tx_early_refund_tx.clone())),
            );

            select! {
                // The early refund transaction has been published but we cannot guarantee
                // that it will be confirmed before the cancel timelock expires
                result = tx_early_refund_status.wait_until_final() => {
                    result?;

                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::BtcRefunded { btc_refund_txid: tx_early_refund_txid },
                    );

                    BobState::BtcEarlyRefunded(state)
                },
                // We cannot guarantee that tx_early_refund will be confirmed before the cancel timelock expires
                // Once it expires we will also publish the cancel and refund transactions
                // We will then race to see which one (tx_early_refund or tx_refund) is confirmed first
                // Both transactions refund the Bitcoin to our refund address
                _ = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                    BobState::CancelTimelockExpired(state)
                },
            }
        }
        BobState::BtcPartialRefundPublished(state)=> {
            // 1. Emit a Tauri event
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcPartialRefundPublished {
                    btc_partial_refund_txid: state.construct_tx_partial_refund()?.txid(),
                    btc_lock_amount: state.tx_lock.lock_amount(),
                    btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
                },
            );

            // TxEarlyRefund might still get published+confirmed before the PartialRefund gets confirmed
            // 2. Wait for either refund transaction to be confirmed
            
            let tx_partial_refund = state.construct_tx_partial_refund()?;
            let tx_early_refund = state.construct_tx_early_refund();

            let (tx_partial_refund_status, tx_early_refund_status) = tokio::join!(
                bitcoin_wallet.subscribe_to(Box::new(tx_partial_refund.clone())),
                bitcoin_wallet.subscribe_to(Box::new(tx_early_refund.clone())),
            );

            select!{
                _ = tx_partial_refund_status.wait_until_final() => {
                    tracing::info!("TxPartialRefund has been confirmed");
                    BobState::BtcPartiallyRefunded(state)
                }
                _ = tx_early_refund_status.wait_until_final() => {
                    tracing::info!("TxEarlyRefund has been confirmed");
                    BobState::BtcEarlyRefunded(state)
                }
            }
        }
        BobState::BtcPartiallyRefunded(state) => {
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcPartiallyRefunded {
                    btc_partial_refund_txid: state.construct_tx_partial_refund()?.txid(),
                    btc_lock_amount: state.tx_lock.lock_amount(),
                    btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
                },
            );

            // Transition to waiting state where we race remaining_refund_timelock
            // against Alice potentially publishing TxRefundBurn
            BobState::WaitingForRemainingRefundTimelockExpiration(state)
        }
        BobState::BtcRefunded(state) => {
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcRefunded {
                    btc_refund_txid: state.signed_full_refund_transaction()?.compute_txid(),
                },
            );

            BobState::BtcRefunded(state)
        }
        BobState::BtcAmnestyPublished(state) => {
            // Here we just wait for the amnesty transaction to be confirmed
            let tx_amnesty = state.construct_tx_amnesty().context("Couldn't construct Bitcoin amnesty transaction")?;

            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcAmnestyPublished {
                    btc_amnesty_txid: tx_amnesty.txid(),
                    btc_lock_amount: state.tx_lock.lock_amount(),
                    btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
                },
            );

            let subscription = bitcoin_wallet.subscribe_to(Box::new(tx_amnesty.clone())).await;

            retry("Waiting for Bitcoin amnesty transaction to be published by Alice", || async {
                subscription.clone()
                    .wait_until_final()
                    .await
                    .context("Failed to wait for Bitcoin amnesty transaction to be confirmed")
                    .map_err(backoff::Error::transient)?;

                event_emitter.emit_swap_progress_event(
                    swap_id,
                    TauriSwapProgressEvent::BtcAmnestyReceived {
                        btc_amnesty_txid: state.construct_tx_amnesty()?.txid(),
                        btc_lock_amount: state.tx_lock.lock_amount(),
                        btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
                    },
                );

                Ok(BobState::BtcAmnestyConfirmed(state.clone()))
            }, None, None)
            .await
            .context("Failed to wait for Bitcoin amnesty transaction to be confirmed")?
        }
        BobState::BtcPunished { state, tx_lock_id } => {
            tracing::info!("You have been punished for not refunding in time");
            event_emitter.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::BtcPunished);
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::AttemptingCooperativeRedeem,
            );

            tracing::info!("Attempting to cooperatively redeem XMR after being punished");
            let response = event_loop_handle.request_cooperative_xmr_redeem().await;

            match response {
                Ok(Fullfilled {
                    s_a,
                    lock_transfer_proof,
                    ..
                }) => {
                    tracing::info!(
                        "Alice has accepted our request to cooperatively redeem the XMR"
                    );

                    // TODO: Somehow validate this key!
                    let state5 = state.attempt_cooperative_redeem(s_a, lock_transfer_proof.into());

                    // TODO: Extract this into an infallible function with a trait
                    // TODO: This is duplicated in the transition from BtcRedeemed to XmrRedeemed
                    // TODO: We should transition into BtcRedeemed here. We should rename BtcRedeemed to something like "XmrRedeemable"
                    let event_emitter_for_callback = event_emitter.clone();

                    state5
                        .infallible_wait_for_xmr_lock_confirmation(
                            &*monero_wallet,
                            state5.lock_transfer_proof.tx_hash(),
                            10,
                            Some(
                                move |(
                                    xmr_lock_txid,
                                    xmr_lock_tx_confirmations,
                                    xmr_lock_tx_target_confirmations,
                                )| {
                                    event_emitter_for_callback.emit_swap_progress_event(
                                    swap_id,
                                    TauriSwapProgressEvent::WaitingForXmrConfirmationsBeforeRedeem {
                                        xmr_lock_txid,
                                        xmr_lock_tx_confirmations,
                                        xmr_lock_tx_target_confirmations,
                                    },
                                );
                                },
                            ),
                        )
                        .await
                        .context("Failed to wait for Monero lock transaction to be confirmed")?;

                    match retry(
                        "Redeeming Monero",
                        || async {
                            state5
                                .clone()
                                .redeem_xmr(&monero_wallet, swap_id, monero_receive_pool.clone())
                                .await
                                .map_err(backoff::Error::transient)
                        },
                        // TODO: Once we validate the key, make this infallible
                        Some(Duration::from_secs(5 * 60)),
                        None,
                    )
                    .await
                    .context("Failed to redeem Monero")
                    {
                        Ok(xmr_redeem_txid) => {
                            event_emitter.emit_swap_progress_event(
                                swap_id,
                                TauriSwapProgressEvent::XmrRedeemInMempool {
                                    xmr_redeem_txids: vec![xmr_redeem_txid],
                                    xmr_receive_pool: monero_receive_pool.clone(),
                                },
                            );

                            return Ok(BobState::XmrRedeemed { tx_lock_id });
                        }
                        Err(error) => {
                            event_emitter.emit_swap_progress_event(
                                swap_id,
                                TauriSwapProgressEvent::CooperativeRedeemRejected {
                                    reason: error.to_string(),
                                },
                            );

                            let err: std::result::Result<_, anyhow::Error> =
                                Err(error).context("Failed to redeem XMR with revealed XMR key");

                            return err;
                        }
                    }
                }
                Ok(Rejected { reason, .. }) => {
                    let err = Err(reason.clone())
                        .context("Alice rejected our request for cooperative XMR redeem");

                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::CooperativeRedeemRejected {
                            reason: reason.to_string(),
                        },
                    );

                    tracing::error!(
                        %reason,
                        "Alice rejected our request for cooperative XMR redeem"
                    );

                    return err;
                }
                Err(error) => {
                    tracing::error!(
                        %error,
                        "Failed to request cooperative XMR redeem from Alice"
                    );

                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::CooperativeRedeemRejected {
                            reason: error.to_string(),
                        },
                    );

                    return Err(error)
                        .context("Failed to request cooperative XMR redeem from Alice");
                }
            };
        }
        BobState::BtcEarlyRefunded(state) => {
            event_emitter.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::BtcEarlyRefunded {
                btc_early_refund_txid: state.construct_tx_early_refund().txid(),
            });
            BobState::BtcEarlyRefunded(state)
        },
        BobState::BtcAmnestyConfirmed(state) => {
            event_emitter.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::BtcAmnestyReceived {
                btc_amnesty_txid: state.construct_tx_amnesty()?.txid(),
                btc_lock_amount: state.tx_lock.lock_amount(),
                btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
            });
            BobState::BtcAmnestyConfirmed(state)
        },
        BobState::WaitingForRemainingRefundTimelockExpiration(state) => {
            // Race between:
            // - Remaining refund timelock expiring (so we can publish TxRefundAmnesty)
            // - Alice publishing TxRefundBurn (burns the amnesty output)
            let tx_partial_refund = state.construct_tx_partial_refund()?;
            let tx_refund_burn = state.construct_tx_refund_burn()?;

            let remaining_refund_timelock = state.remaining_refund_timelock.context(
                "Can't wait for remaining refund timelock because remaining_refund_timelock is missing",
            )?;

            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::WaitingForEarnestDepositTimelockExpiration {
                    btc_partial_refund_txid: tx_partial_refund.txid(),
                    btc_lock_amount: state.tx_lock.lock_amount(),
                    btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
                    target_blocks: remaining_refund_timelock.into(),
                    blocks_until_expiry: remaining_refund_timelock.into(),
                },
            );

            let (tx_partial_refund_status, tx_refund_burn_status) = tokio::join!(
                bitcoin_wallet.subscribe_to(Box::new(tx_partial_refund.clone())),
                bitcoin_wallet.subscribe_to(Box::new(tx_refund_burn)),
            );

            // Emit a tauri event everytime the TxPartialRefund status changes so we can
            // show an estimate when we will be able to claim the remaining bitcoin
            let timelock_expired_future = tx_partial_refund_status.wait_until(|status| {
                event_emitter.emit_swap_progress_event(
                    swap_id,
                    TauriSwapProgressEvent::WaitingForEarnestDepositTimelockExpiration {
                        btc_partial_refund_txid: tx_partial_refund.txid(),
                        btc_lock_amount: state.tx_lock.lock_amount(),
                        btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
                        target_blocks: remaining_refund_timelock.into(),
                        blocks_until_expiry: status.confirmations(),
                    },
                );

                status.is_confirmed_with(remaining_refund_timelock.0)
            });

            select! {
                // Wait for remaining_refund_timelock confirmations on tx_partial_refund
                result =  timelock_expired_future => {
                    result?;
                    tracing::info!("Remaining refund timelock expired, can now publish TxRefundAmnesty");
                    BobState::RemainingRefundTimelockExpired(state)
                }
                // Watch for Alice publishing TxRefundBurn
                _ = tx_refund_burn_status.wait_until_seen() => {
                    tracing::info!("Alice published TxRefundBurn, amnesty output is being burnt");
                    BobState::BtcRefundBurnPublished(state)
                }
            }
        }
        BobState::RemainingRefundTimelockExpired(state) => {
            // TODO: We should retry this and the check
            // First check if TxRefundBurn was seen (we may have missed it while offline)
            let tx_refund_burn = state.construct_tx_refund_burn()?;
            let tx_refund_burn_status = bitcoin_wallet.status_of_script(&tx_refund_burn).await?;

            if tx_refund_burn_status.has_been_seen() {
                tracing::info!("TxRefundBurn was already published, transitioning to BtcRefundBurnPublished");
                return Ok(BobState::BtcRefundBurnPublished(state));
            }

            // TxRefundBurn not published, we can publish TxRefundAmnesty
            // Alice always sends the amnesty signature in swap setup
            let transaction = state.signed_amnesty_transaction()
                .context("Couldn't construct Bitcoin amnesty transaction")?;
            bitcoin_wallet.ensure_broadcasted(transaction, "amnesty")
                .await
                .context("Couldn't ensure broadcast of Bitcoin amnesty transaction")?;
            BobState::BtcAmnestyPublished(state)
        }
        BobState::BtcRefundBurnPublished(state) => {
            // Wait for TxRefundBurn confirmation
            let tx_refund_burn = state.construct_tx_refund_burn()?;
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcRefundBurnPublished {
                    btc_refund_burn_txid: tx_refund_burn.txid(),
                    btc_lock_amount: state.tx_lock.lock_amount(),
                    btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
                },
            );
            let subscription = bitcoin_wallet.subscribe_to(Box::new(tx_refund_burn)).await;

            subscription.wait_until_final().await?;
            tracing::info!("TxRefundBurn confirmed, amnesty output is burnt");
            BobState::BtcRefundBurnt(state)
        }
        BobState::BtcRefundBurnt(state) => {
            // Watch for Alice publishing TxFinalAmnesty
            // Alice may grant final amnesty after burning our refund
            // However, we don't expect Alice to publish the tx at once, if at all.
            // Thus we only check once, and then stop the swap.
            // User's can still manually resume the swap to check again.
            let tx_refund_burn = state.construct_tx_refund_burn()?;
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcRefundBurnt {
                    btc_refund_burn_txid: tx_refund_burn.txid(),
                    btc_lock_amount: state.tx_lock.lock_amount(),
                    btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
                },
            );

            let tx_final_amnesty = state.construct_tx_final_amnesty()?;

            let final_amnesty_status = bitcoin_wallet.status_of_script(&tx_final_amnesty).await.context("Failed to check TxFinalAmnesty status")?;

            if final_amnesty_status.has_been_seen() {
                BobState::BtcFinalAmnestyPublished(state)
            } else {
                BobState::BtcRefundBurnt(state)
            }
        }
        BobState::BtcFinalAmnestyPublished(state) => {
            // Wait for TxFinalAmnesty confirmation
            let tx_final_amnesty = state.construct_tx_final_amnesty()?;
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcFinalAmnestyPublished {
                    btc_final_amnesty_txid: tx_final_amnesty.txid(),
                    btc_lock_amount: state.tx_lock.lock_amount(),
                    btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
                },
            );
            let subscription = bitcoin_wallet.subscribe_to(Box::new(tx_final_amnesty)).await;

            subscription.wait_until_final().await?;
            tracing::info!("TxFinalAmnesty confirmed, received burnt funds back");
            BobState::BtcFinalAmnestyConfirmed(state)
        }
        BobState::BtcFinalAmnestyConfirmed(state) => {
            // Terminal state - we received the burnt funds back
            let tx_final_amnesty = state.construct_tx_final_amnesty()?;
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcFinalAmnestyConfirmed {
                    btc_final_amnesty_txid: tx_final_amnesty.txid(),
                    btc_lock_amount: state.tx_lock.lock_amount(),
                    btc_amnesty_amount: state.btc_amnesty_amount.unwrap_or(bitcoin::Amount::ZERO),
                },
            );
            BobState::BtcFinalAmnestyConfirmed(state)
        }
        BobState::SafelyAborted => BobState::SafelyAborted,
        BobState::XmrRedeemed { tx_lock_id } => {
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::XmrRedeemInMempool {
                    // We don't have the txids of the redeem transaction here, so we can't emit them
                    // We return an empty array instead
                    xmr_redeem_txids: vec![],
                    xmr_receive_pool: monero_receive_pool.clone(),
                },
            );
            BobState::XmrRedeemed { tx_lock_id }
        }
    })
}
