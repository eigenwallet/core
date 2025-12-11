//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use crate::asb::{EventLoopHandle, LatestRate};
use crate::common::retry;
use crate::monero;
use crate::monero::TransferProof;
use crate::protocol::alice::{AliceState, Swap, TipConfig};
use ::bitcoin::consensus::encode::serialize_hex;
use anyhow::{bail, Context, Result};
use bitcoin_wallet::BitcoinWallet;
use rust_decimal::Decimal;
use swap_core::bitcoin::ExpiredTimelocks;
use swap_env::env::Config;
use swap_machine::alice::State3;
use tokio::select;
use tokio::time::timeout;
use uuid::Uuid;

pub async fn run<LR>(swap: Swap, rate_service: LR) -> Result<AliceState>
where
    LR: LatestRate + Clone,
{
    run_until(swap, |_| false, rate_service).await
}

#[tracing::instrument(name = "swap", skip(swap,exit_early,rate_service), fields(id = %swap.swap_id), err)]
pub async fn run_until<LR>(
    mut swap: Swap,
    exit_early: fn(&AliceState) -> bool,
    rate_service: LR,
) -> Result<AliceState>
where
    LR: LatestRate + Clone,
{
    let mut current_state = swap.state;

    while !swap_machine::alice::is_complete(&current_state) && !exit_early(&current_state) {
        current_state = next_state(
            swap.swap_id,
            current_state,
            &mut swap.event_loop_handle,
            swap.bitcoin_wallet.clone(),
            swap.monero_wallet.clone(),
            &swap.env_config,
            swap.developer_tip.clone(),
            rate_service.clone(),
        )
        .await?;

        swap.db
            .insert_latest_state(swap.swap_id, current_state.clone().into())
            .await?;
    }

    Ok(current_state)
}

async fn next_state<LR>(
    swap_id: Uuid,
    state: AliceState,
    event_loop_handle: &mut EventLoopHandle,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    monero_wallet: Arc<monero::Wallets>,
    env_config: &Config,
    developer_tip: TipConfig,
    mut rate_service: LR,
) -> Result<AliceState>
where
    LR: LatestRate,
{
    let rate = rate_service
        .latest_rate()
        .map_or("NaN".to_string(), |rate| format!("{}", rate));

    tracing::info!(%state, %rate, "Advancing state");

    Ok(match state {
        AliceState::Started { state3 } => {
            let tx_lock_status = bitcoin_wallet
                .subscribe_to(Box::new(state3.tx_lock.clone()))
                .await;

            match timeout(
                env_config.bitcoin_lock_mempool_timeout,
                tx_lock_status.wait_until_seen(),
            )
            .await
            {
                Err(_) => {
                    tracing::info!(
                        minutes = %env_config.bitcoin_lock_mempool_timeout.as_secs_f64() / 60.0,
                        "TxLock lock was not seen in mempool in time. Alice might have denied our offer.",
                    );
                    AliceState::SafelyAborted
                }
                Ok(res) => {
                    res?;
                    AliceState::BtcLockTransactionSeen { state3 }
                }
            }
        }
        AliceState::BtcLockTransactionSeen { state3 } => {
            let tx_lock_status = bitcoin_wallet
                .subscribe_to(Box::new(state3.tx_lock.clone()))
                .await;

            match timeout(
                env_config.bitcoin_lock_confirmed_timeout,
                tx_lock_status.wait_until_final(),
            )
            .await
            {
                Err(_) => {
                    tracing::info!(
                        confirmations_needed = %env_config.bitcoin_finality_confirmations,
                        minutes = %env_config.bitcoin_lock_confirmed_timeout.as_secs_f64() / 60.0,
                        "TxLock lock did not get enough confirmations in time",
                    );

                    AliceState::BtcEarlyRefundable { state3 }
                }
                Ok(res) => {
                    res?;
                    AliceState::BtcLocked { state3 }
                }
            }
        }
        AliceState::BtcLocked { state3 } => {
            // Sometimes locking the Monero can fail e.g due to the daemon not being fully synced
            // We will retry indefinitely to lock the Monero funds, until either:
            // - the cancel timelock expires
            // - we do not manage to lock the Monero funds within the timeout
            let backoff = backoff::ExponentialBackoffBuilder::new()
                .with_max_elapsed_time(Some(env_config.monero_lock_retry_timeout))
                .with_max_interval(Duration::from_secs(30))
                .build();

            let transfer_proof = backoff::future::retry_notify(
                backoff,
                || async {
                    // We check the status of the Bitcoin lock transaction
                    // If the swap is cancelled, there is no need to lock the Monero funds anymore
                    // because there is no way for the swap to succeed.
                    if !matches!(
                        state3
                            .expired_timelocks(&*bitcoin_wallet)
                            .await
                            .context("Failed to check for expired timelocks before locking Monero")
                            .map_err(backoff::Error::transient)?,
                        ExpiredTimelocks::None { .. }
                    ) {
                        return Ok(None);
                    }

                    // Record the current monero wallet block height so we don't have to scan from
                    // block 0 for scenarios where we create a refund wallet.
                    let monero_wallet_restore_blockheight = monero_wallet
                        .blockchain_height()
                        .await
                        .context("Failed to get Monero wallet block height")
                        .map_err(backoff::Error::transient)?;

                    let (lock_address, amount) = state3
                        .lock_xmr_transfer_request()
                        .address_and_amount(env_config.monero_network);

                    let destinations =
                        build_transfer_destinations(lock_address, amount, developer_tip.clone())?;

                    // Lock the Monero
                    let receipt = monero_wallet
                        .main_wallet()
                        .await
                        .transfer_multi_destination(&destinations)
                        .await
                        .map_err(|e| tracing::error!(err=%e, "Failed to lock Monero"))
                        .ok();

                    let Some(receipt) = receipt else {
                        return Err(backoff::Error::transient(anyhow::anyhow!(
                            "Failed to lock Monero"
                        )));
                    };

                    let tx_key = receipt.tx_keys.get(&lock_address).expect("monero-sys guarantees that the address has a valid tx key or the tx isn't published");

                    Ok(Some((
                        monero_wallet_restore_blockheight,
                        TransferProof::new(
                            monero::TxHash(receipt.txid),
                            *tx_key,
                        ),
                    )))
                },
                |e, wait_time: Duration| {
                    tracing::warn!(
                        swap_id = %swap_id,
                        error = ?e,
                        "Failed to lock Monero. We will retry in {} seconds",
                        wait_time.as_secs()
                    )
                },
            )
            .await;

            match transfer_proof {
                // If the transfer was successful, we transition to the next state
                Ok(Some((monero_wallet_restore_blockheight, transfer_proof))) => {
                    AliceState::XmrLockTransactionSent {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    }
                }
                // If we were not able to lock the Monero funds before the timelock expired,
                // we can safely abort the swap because we did not lock any funds
                // We do not do an early refund because Bob can refund himself (timelock expired)
                Ok(None) => {
                    tracing::info!(
                        swap_id = %swap_id,
                        "We did not manage to lock the Monero funds before the timelock expired. Aborting swap."
                    );

                    AliceState::SafelyAborted
                }
                Err(e) => {
                    tracing::error!(
                        swap_id = %swap_id,
                        error = ?e,
                        "Failed to lock Monero within {} seconds. We will do an early refund of the Bitcoin. We didn't lock any Monero funds so this is safe.",
                        env_config.monero_lock_retry_timeout.as_secs()
                    );

                    AliceState::BtcEarlyRefundable { state3 }
                }
            }
        }
        AliceState::BtcEarlyRefundable { state3 } => {
            if let Some(tx_early_refund) = state3.signed_early_refund_transaction() {
                let tx_early_refund = tx_early_refund?;
                let tx_early_refund_txid = tx_early_refund.compute_txid();

                // Bob might cancel the swap and refund for himself. We won't need to early refund anymore.
                let tx_cancel_status = bitcoin_wallet
                    .subscribe_to(Box::new(state3.tx_cancel()))
                    .await;

                let backoff = backoff::ExponentialBackoffBuilder::new()
                    // We give up after 6 hours
                    // (Most likely Bob the a Replace-by-Fee on the tx_lock transaction)
                    .with_max_elapsed_time(Some(Duration::from_secs(6 * 60 * 60)))
                    // We wait a while between retries
                    .with_max_interval(Duration::from_secs(10 * 60))
                    .build();

                // Concurrently retry to broadcast the early refund transaction
                // and wait for the cancel transaction to be broadcasted.
                tokio::select! {
                    // If Bob cancels the swap, he can refund himself.
                    // Nothing for us to do anymore.
                    result = tx_cancel_status.wait_until_seen() => {
                        result?;
                        AliceState::SafelyAborted
                    }

                    // Retry repeatedly to broadcast tx_early_refund
                    result = async {
                        backoff::future::retry_notify(backoff, || async {
                            bitcoin_wallet.broadcast(tx_early_refund.clone(), "early_refund").await.map_err(backoff::Error::transient)
                        }, |e, wait_time: Duration| {
                            tracing::warn!(
                                %tx_early_refund_txid,
                                error = ?e,
                                "Failed to broadcast early refund transaction. We will retry in {} seconds",
                                wait_time.as_secs()
                            )
                        })
                        .await
                    } => {
                        match result {
                            Ok((_txid, _subscription)) => {
                                tracing::info!(
                                    %tx_early_refund_txid,
                                    "Refunded Bitcoin early for Bob"
                                );

                                AliceState::BtcEarlyRefunded(state3)
                            }
                            Err(e) => {
                                tracing::error!(
                                    %tx_early_refund_txid,
                                    error = ?e,
                                    "Failed to broadcast early refund transaction after retries exhausted. Bob will have to wait for the timelock to expire then refund himself."
                                );
                                AliceState::SafelyAborted
                            }
                        }
                    }
                }
            } else {
                // We do not have Bob's signature for the early refund transaction
                // Therefore we cannot do an early refund.
                // We abort the swap on our side.
                // Bob will have to wait for the timelock to expire then refund himself.
                AliceState::SafelyAborted
            }
        }
        AliceState::XmrLockTransactionSent {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => match state3.expired_timelocks(&*bitcoin_wallet).await? {
            ExpiredTimelocks::None { .. } => {
                tracing::info!("Locked Monero, waiting for confirmations");
                monero_wallet
                    .wait_until_confirmed(
                        state3.lock_xmr_watch_request(transfer_proof.clone(), 1),
                        Some(|(confirmations, target_confirmations)| {
                            tracing::debug!(
                                %confirmations,
                                %target_confirmations,
                                "Monero lock tx got new confirmation"
                            )
                        }),
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to wait until Monero transaction was confirmed ({})",
                            transfer_proof.tx_hash()
                        )
                    })?;

                AliceState::XmrLocked {
                    monero_wallet_restore_blockheight,
                    transfer_proof,
                    state3,
                }
            }
            _ => AliceState::CancelTimelockExpired {
                monero_wallet_restore_blockheight,
                transfer_proof,
                state3,
            },
        },
        AliceState::XmrLocked {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            let tx_lock_status = bitcoin_wallet
                .subscribe_to(Box::new(state3.tx_lock.clone()))
                .await;

            tokio::select! {
                result = event_loop_handle.send_transfer_proof(transfer_proof.clone()) => {
                   result?;

                   AliceState::XmrLockTransferProofSent {
                       monero_wallet_restore_blockheight,
                       transfer_proof,
                       state3,
                   }
                },
                // If we send Bob the transfer proof, but for whatever reason we do not receive an acknoledgement from him
                // we would be stuck in this state forever until the timelock expires.
                //
                // By listening for the encrypted signature here we can still proceed to the next state
                // even if Bob does not respond with an acknoledgement but sends us the encrypted signature immediately.
                enc_sig = event_loop_handle.recv_encrypted_signature() => {
                    tracing::info!("Received encrypted signature");

                    AliceState::EncSigLearned {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        encrypted_signature: Box::new(enc_sig?),
                        state3,
                    }
                }
                result = tx_lock_status.wait_until_confirmed_with(state3.cancel_timelock) => {
                    result?;
                    AliceState::CancelTimelockExpired {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    }
                }
            }
        }
        AliceState::XmrLockTransferProofSent {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            let tx_lock_status_subscription = bitcoin_wallet
                .subscribe_to(Box::new(state3.tx_lock.clone()))
                .await;

            select! {
                biased; // make sure the cancel timelock expiry future is polled first
                result = tx_lock_status_subscription.wait_until_confirmed_with(state3.cancel_timelock) => {
                    result?;
                    AliceState::CancelTimelockExpired {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    }
                }
                enc_sig = event_loop_handle.recv_encrypted_signature() => {
                    tracing::info!("Received encrypted signature");

                    AliceState::EncSigLearned {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        encrypted_signature: Box::new(enc_sig?),
                        state3,
                    }
                }
            }
        }
        AliceState::EncSigLearned {
            monero_wallet_restore_blockheight,
            transfer_proof,
            encrypted_signature,
            state3,
        } => {
            // Try to sign the Bitcoin redeem transactions
            let tx_redeem = match state3.signed_redeem_transaction(*encrypted_signature) {
                Ok(tx_redeem) => tx_redeem,
                // If we cannot sign the transaction there must be something wrong
                // We just wait for the cancel timelock to expire and then refund
                Err(error) => {
                    tracing::error!("Failed to construct redeem transaction: {:#}, we will wait for the cancel timelock expiration to refund", error);

                    return Ok(AliceState::WaitingForCancelTimelockExpiration {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    });
                }
            };

            // Retry indefinitely to publish the redeem transaction, until the cancel timelock expires
            // Publishing the redeem transaction might fail on the first try due to any number of reasons
            let backoff = backoff::ExponentialBackoffBuilder::new()
                .with_max_elapsed_time(None)
                .with_max_interval(Duration::from_secs(60))
                .build();

            match backoff::future::retry_notify(backoff.clone(), || async {
                let tx_lock_status = bitcoin_wallet
                    .status_of_script(&state3.tx_lock.clone())
                    .await?;

                // If the cancel timelock is expired, it it not safe to publish the Bitcoin redeem transaction anymore
                //
                // TODO: In practice this should be redundant because the logic above will trigger for a superset of the cases where this is true
                if tx_lock_status.is_confirmed_with(state3.cancel_timelock) {
                    return Ok(None);
                }

                // We can only redeem the Bitcoin if we are fairly sure that our Bitcoin redeem transaction
                // will be confirmed before the cancel timelock expires
                //
                // We make an assumption that it will take at most `env_config.bitcoin_blocks_till_confirmed_upper_bound_assumption` blocks
                // until our transaction is included in a block. If this assumption is not satisfied, we will not publish the transaction.
                //
                // We will instead wait for the cancel timelock to expire and then refund.
                if tx_lock_status.blocks_left_until(state3.cancel_timelock) < env_config.bitcoin_blocks_till_confirmed_upper_bound_assumption {
                    return Ok(None);
                }

                bitcoin_wallet
                    .broadcast(tx_redeem.clone(), "redeem")
                    .await
                    .map(Some)
                    .map_err(backoff::Error::transient)
            }, |e, wait_time: Duration| {
                tracing::warn!(
                    swap_id = %swap_id,
                    error = ?e,
                    "Failed to broadcast Bitcoin redeem transaction. We will retry in {} seconds",
                    wait_time.as_secs()
                )
            })
            .await
            .expect("We should never run out of retries while publishing the Bitcoin redeem transaction")
            {
                // We successfully published the redeem transaction
                // We wait until we see the transaction in the mempool before transitioning to the next state
                Some((txid, subscription)) => match subscription.wait_until_seen().await {
                    Ok(_) => AliceState::BtcRedeemTransactionPublished { state3, transfer_proof },
                    // TODO: No need to bail here, we should just retry?
                    Err(e) => {
                        // We extract the txid and the hex representation of the transaction
                        // this'll allow the user to manually re-publish the transaction
                        let tx_hex = serialize_hex(&tx_redeem);

                        bail!("Waiting for Bitcoin redeem transaction to be in mempool failed with {}! The redeem transaction was published, but it is not ensured that the transaction was included! You might be screwed. You can try to manually re-publish the transaction (TxID: {}, Tx Hex: {})", e, txid, tx_hex)
                    }
                },

                // It is not safe to publish the Bitcoin redeem transaction anymore
                // We wait for the cancel timelock to expire and then refund
                None => {
                    tracing::error!("We were unable to publish the Bitcoin redeem transaction before the timelock expired.");

                    AliceState::WaitingForCancelTimelockExpiration {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    }
                }
            }
        }
        AliceState::BtcRedeemTransactionPublished { state3, .. } => {
            let subscription = bitcoin_wallet
                .subscribe_to(Box::new(state3.tx_redeem()))
                .await;

            match subscription.wait_until_final().await {
                Ok(_) => AliceState::BtcRedeemed,
                Err(e) => {
                    bail!("The Bitcoin redeem transaction was seen in mempool, but waiting for finality timed out with {}. Manual investigation might be needed to ensure that the transaction was included.", e)
                }
            }
        }
        AliceState::WaitingForCancelTimelockExpiration {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            let tx_lock_status_subscription = bitcoin_wallet
                .subscribe_to(Box::new(state3.tx_lock.clone()))
                .await;

            // TODO: Retry here
            tx_lock_status_subscription
                .wait_until_confirmed_with(state3.cancel_timelock)
                .await?;

            AliceState::CancelTimelockExpired {
                monero_wallet_restore_blockheight,
                transfer_proof,
                state3,
            }
        }
        AliceState::CancelTimelockExpired {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            let backoff = backoff::ExponentialBackoffBuilder::new()
                .with_max_elapsed_time(None)
                // No need to be super agressive here
                .with_max_interval(Duration::from_secs(60 * 10))
                .build();

            backoff::future::retry_notify::<_, anyhow::Error, _, _, _, _>(
                backoff,
                || async {
                    if state3
                        .check_for_tx_cancel(&*bitcoin_wallet)
                        .await
                        .context("Failed to check for existence of Bitcoin cancel transaction on chain")
                        .map_err(backoff::Error::transient)?
                        .is_some()
                    {
                        return Ok(());
                    }

                    state3
                        .submit_tx_cancel(&*bitcoin_wallet)
                        .await
                        .context("Failed to submit cancel transaction")
                        .map_err(backoff::Error::transient)?;

                    Ok(())
                },
                |e: anyhow::Error, wait_time: Duration| {
                    tracing::warn!(
                        swap_id = %swap_id,
                        error = ?e,
                        "Failed to ensure cancel transaction is published. We will retry in {} seconds",
                        wait_time.as_secs()
                    )
                },
            )
            .await
            .expect("We should never run out of retries while ensuring the cancel transaction is published");

            AliceState::BtcCancelled {
                monero_wallet_restore_blockheight,
                transfer_proof,
                state3,
            }
        }
        AliceState::BtcCancelled {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            let tx_cancel_status = bitcoin_wallet
                .subscribe_to(Box::new(state3.tx_cancel()))
                .await;

            // We wait for either TxFullRefund or TxPartialRefund to be published
            // - both allow us to extract the Monero refund key.
            // Otherwise we punish, once that timelock expired.

            // TODO: should we retry here?
            select! {
                spend_key = state3.watch_for_btc_tx_full_refund(&*bitcoin_wallet) => {
                    let spend_key = spend_key?;

                    AliceState::BtcRefunded {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        spend_key,
                        state3,
                    }
                }
                spend_key = state3.watch_for_btc_tx_partial_refund(&*bitcoin_wallet) => {
                    let spend_key = spend_key?;

                    AliceState::BtcRefunded {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        spend_key,
                        state3,
                    }
                }
                result = tx_cancel_status.wait_until_confirmed_with(state3.punish_timelock) => {
                    result?;

                    AliceState::BtcPunishable {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    }
                }
            }
        }
        AliceState::BtcRefunded {
            transfer_proof,
            spend_key,
            state3,
            monero_wallet_restore_blockheight,
        } => AliceState::XmrRefundable {
            monero_wallet_restore_blockheight,
            transfer_proof,
            spend_key,
            state3,
        },
        AliceState::BtcPartiallyRefunded {
            transfer_proof,
            spend_key,
            state3,
            monero_wallet_restore_blockheight,
        } => {
            let should_grant_amnesty = true;

            // TODO: Publish amnesty transaction/send amnesty tx sig / decide against it

            AliceState::XmrRefundable {
                monero_wallet_restore_blockheight,
                transfer_proof,
                spend_key,
                state3,
            }
        }
        AliceState::XmrRefundable {
            monero_wallet_restore_blockheight,
            transfer_proof,
            spend_key,
            state3,
        } => {
            retry(
                "Refund Monero",
                || async {
                    state3
                        .refund_xmr(
                            monero_wallet.clone(),
                            swap_id,
                            spend_key,
                            transfer_proof.clone(),
                        )
                        .await
                        .map_err(backoff::Error::transient)
                },
                None,
                Duration::from_secs(60),
            )
            .await
            .expect("We should never run out of retries while refunding Monero");

            AliceState::XmrRefunded
        }
        AliceState::BtcPunishable {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            retry(
                "Punish Bitcoin",
                || async {
                    // Before punishing, we explicitly check for the refund transaction as we prefer refunds over punishments
                    let spend_key_from_btc_refund = state3.refund_btc(&*bitcoin_wallet).await.context("Failed to check for existence of Bitcoin refund transaction before punishing").map_err(backoff::Error::transient)?;

                    // If we find the Bitcoin refund transaction, we go ahead and refund the Monero
                    if let Some(spend_key_from_btc_refund) = spend_key_from_btc_refund {
                        return Ok::<AliceState, backoff::Error<anyhow::Error>>(AliceState::BtcRefunded {
                            monero_wallet_restore_blockheight,
                            transfer_proof: transfer_proof.clone(),
                            spend_key: spend_key_from_btc_refund,
                            state3: state3.clone(),
                        });
                    }

                    state3.punish_btc(&*bitcoin_wallet).await.context("Failed to construct and publish Bitcoin punish transaction").map_err(backoff::Error::transient)?;

                    Ok::<AliceState, backoff::Error<anyhow::Error>>(AliceState::BtcPunished {
                        state3: state3.clone(),
                        transfer_proof: transfer_proof.clone(),
                    })
                },
                None,
                // We can take our time when punishing
                Duration::from_secs(60 * 5),
            )
            .await
            .expect("We should never run out of retries while publishing the punish transaction")
        }
        AliceState::XmrRefunded => AliceState::XmrRefunded,
        AliceState::BtcRedeemed => AliceState::BtcRedeemed,
        AliceState::BtcPunished {
            state3,
            transfer_proof,
        } => AliceState::BtcPunished {
            state3,
            transfer_proof,
        },
        AliceState::BtcEarlyRefunded(state3) => AliceState::BtcEarlyRefunded(state3),
        AliceState::SafelyAborted => AliceState::SafelyAborted,
    })
}

#[allow(async_fn_in_trait)]
pub trait XmrRefundable {
    async fn refund_xmr(
        &self,
        monero_wallet: Arc<monero::Wallets>,
        swap_id: Uuid,
        spend_key: monero::PrivateKey,
        transfer_proof: TransferProof,
    ) -> Result<()>;
}

impl XmrRefundable for State3 {
    async fn refund_xmr(
        &self,
        monero_wallet: Arc<monero::Wallets>,
        swap_id: Uuid,
        spend_key: monero::PrivateKey,
        transfer_proof: TransferProof,
    ) -> Result<()> {
        let view_key = self.v;

        // Ensure that the XMR to be refunded are spendable by awaiting 10 confirmations
        // on the lock transaction.
        tracing::info!("Waiting for Monero lock transaction to be confirmed");
        let transfer_proof_2 = transfer_proof.clone();
        monero_wallet
            .wait_until_confirmed(
                self.lock_xmr_watch_request(transfer_proof_2, 10),
                Some(move |(confirmations, target_confirmations)| {
                    tracing::debug!(
                        %confirmations,
                        %target_confirmations,
                        "Monero lock transaction got a confirmation"
                    );
                }),
            )
            .await
            .context("Failed to wait for Monero lock transaction to be confirmed")?;

        tracing::info!("Refunding Monero");

        tracing::debug!(%swap_id, "Opening temporary Monero wallet from keys");
        let swap_wallet = monero_wallet
            .swap_wallet(swap_id, spend_key, view_key, transfer_proof.tx_hash())
            .await
            .context(format!("Failed to open/create swap wallet `{}`", swap_id))?;

        // Update blockheight to ensure that the wallet knows the funds are unlocked
        tracing::debug!(%swap_id, "Updating temporary Monero wallet's blockheight");
        let _ = swap_wallet
            .blockchain_height()
            .await
            .context("Couldn't get Monero blockheight")?;

        tracing::debug!(%swap_id, "Sweeping Monero to redeem address");
        let main_address = monero_wallet.main_wallet().await.main_address().await?;

        swap_wallet
            .sweep(&main_address)
            .await
            .context("Failed to sweep Monero to redeem address")?;

        Ok(())
    }
}

impl XmrRefundable for Box<State3> {
    async fn refund_xmr(
        &self,
        monero_wallet: Arc<monero::Wallets>,
        swap_id: Uuid,
        spend_key: monero::PrivateKey,
        transfer_proof: TransferProof,
    ) -> Result<()> {
        (**self)
            .refund_xmr(monero_wallet, swap_id, spend_key, transfer_proof)
            .await
    }
}

/// Build transfer destinations for the Monero lock transaction, optionally including a developer tip.
///
/// If the tip.ratio > 0 and the effective tip is >= MIN_USEFUL_TIP_AMOUNT_PICONERO:
///     returns two destinations: one for the lock output, one for the tip output
///
/// Otherwise:
///     returns one destination: for the lock output
fn build_transfer_destinations(
    lock_address: ::monero::Address,
    lock_amount: ::monero::Amount,
    tip: TipConfig,
) -> anyhow::Result<Vec<(::monero::Address, ::monero::Amount)>> {
    use rust_decimal::prelude::ToPrimitive;

    // If the effective tip is less than this amount, we do not include the tip output
    // Any values below `MIN_USEFUL_TIP_AMOUNT_PICONERO` are clamped to zero
    //
    // At $300/XMR, this is around one cent
    const MIN_USEFUL_TIP_AMOUNT_PICONERO: u64 = 30_000_000;

    // TODO: Move this code into the impl of TipConfig
    let tip_amount_piconero = tip
        .ratio
        .saturating_mul(Decimal::from(lock_amount.as_pico()))
        .floor()
        .to_u64()
        .context("Developer tip amount should not overflow")?;

    if tip_amount_piconero >= MIN_USEFUL_TIP_AMOUNT_PICONERO {
        let tip_amount = ::monero::Amount::from_pico(tip_amount_piconero);

        Ok(vec![(lock_address, lock_amount), (tip.address, tip_amount)])
    } else {
        Ok(vec![(lock_address, lock_amount)])
    }
}

/// This function is used to check if Alice is in a state where it is clear that she has already received the encrypted signature from Bob.
/// This allows us to acknowledge the encrypted signature multiple times
/// If our acknowledgement does not reach Bob, he might send the encrypted signature again.
pub(crate) fn has_already_processed_enc_sig(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::EncSigLearned { .. }
            | AliceState::BtcRedeemTransactionPublished { .. }
            | AliceState::BtcRedeemed
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_build_transfer_destinations_without_tip() {
        todo!("implement once unit tests compile again")
    }

    #[test]
    fn test_build_transfer_destinations_with_tip() {
        todo!("implement once unit tests compile again")
    }

    #[test]
    fn test_build_transfer_destinations_with_small_tip() {
        todo!("implement once unit tests compile again")
    }

    #[test]
    fn test_build_transfer_destinations_with_zero_tip() {
        todo!("implement once unit tests compile again")
    }

    #[test]
    fn test_build_transfer_destinations_with_fractional_tip() {
        todo!("implement once unit tests compile again")
    }
}
