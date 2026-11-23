//! the funder: scanner + state + transfer wired into one wallet that
//! pays exact amounts.
//!
//! this is the facade the harness wants: hold some keys, mine or
//! receive funds, then pay exact piconero amounts to exact addresses
//! and keep the change. everything below the facade is the pieces
//! that already exist — the state module for accounting and unlock
//! rules, the transfer module for construction — plus a scan loop of
//! my own.
//!
//! why my own scan loop instead of [`crate::scanner`]: the scanner
//! subscription hands over outputs but not the height they landed
//! at, and the state needs that height for every unlock rule. my
//! loop knows the height of every block it scans, which also means
//! real reorg handling: i remember the hash of every block i scanned
//! and, when the chain stops agreeing, walk back to the exact height
//! where i diverged and re-take everything above it.
//!
//! coinbase outputs come in with their consensus unlock time
//! (height + 60) sitting in the additional timelock, which unlocks
//! at the exact same height as the state's coinbase window, so i
//! let the timelock do the talking and mark nothing as coinbase.
//!
//! funding is single flight (one `fund` at a time, mutex guarded)
//! and scanning pauses while a transfer is being built, since
//! construction reads the state under its lock across the decoy
//! fetches. publish and confirmation waiting happen without the
//! data lock so scanning picks back up right away.
//!
//! spends are read off the chain, not remembered: every scanned
//! transaction's input key images get matched against the key images
//! of my own outputs, and a hit means that output left me at that
//! height. that is what makes reorgs self healing — a rolled back
//! spend reverts with the block, and the rescan re-marks it when the
//! tx lands on the new chain. my own in-flight transfers stay
//! tracked only so the loop knows when the chain has finished
//! marking them and can give up on ones that cannot land.
//!
//! honest windows that remain: after a reorg undoes a confirmation
//! there is a gap before the tx lands again where its inputs look
//! available — spending them there makes a conflicting tx (the
//! daemon will refuse one of the two, money is never miscounted).
//! a transfer whose tx got evicted from the mempool without landing
//! keeps its inputs pending until the funder is dropped; there is no
//! abandon API. and the scanner only watches the base address, so
//! change must come back to the base address, not a subaddress.
//! dropping the funder stops the scan loop (the shutdown watch
//! closes).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, watch};
use zeroize::Zeroizing;

use monero_interface::{
    ProvidesBlockchainMeta, ProvidesDecoys, ProvidesFeeRates, ProvidesScannableBlocks,
    PublishTransaction,
};
use monero_oxide::ed25519::{Point, Scalar};
use monero_oxide::transaction::Input;
use monero_oxide_wallet::Scanner;
use monero_oxide_wallet::address::MoneroAddress;
use monero_oxide_wallet::{ViewPair, ViewPairError, WalletOutput};

use crate::confirmations;
use crate::rpc::{ProvidesAdjustedTime, ProvidesTransactionStatus, TransactionStatus};
use crate::state::{OutputId, ReceivedOutput, StateError, WalletState};
use crate::transfer::{self, Transfer, TransferError};
use crate::util::public_key;

/// how often the scan loop polls the daemon for new blocks.
const SCAN_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// how many blocks the scan loop fetches per batch.
const SCAN_BATCH_SIZE: usize = 10;

/// how long `fund` waits for the transfer to land in a block before
/// giving up. the tx may still confirm after this; the inputs stay
/// pending either way.
const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(120);

/// how long `refresh` gives the scan loop to reach the daemon's tip.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(60);

/// the ways funding can fail.
#[derive(Debug, thiserror::Error)]
pub enum FunderError {
    #[error("could not create the view pair: {0}")]
    ViewPair(#[from] ViewPairError),
    #[error("input selection or bookkeeping: {0}")]
    State(#[from] StateError),
    #[error("building the transfer: {0}")]
    Transfer(#[from] TransferError),
    #[error("daemon rpc failed: {0}")]
    Interface(#[from] monero_interface::InterfaceError),
    #[error("the daemon rejected the transfer: {0}")]
    Publish(#[from] monero_interface::PublishTransactionError),
    #[error("transaction status lookup failed: {0}")]
    TransactionStatus(#[from] crate::rpc::TransactionStatusError),
    #[error(
        "the transfer did not confirm (or lost its confirmation to a reorg) \
         within {}s; it may still land, the inputs stay pending",
        CONFIRMATION_TIMEOUT.as_secs()
    )]
    ConfirmationTimeout,
    #[error(
        "the scan did not reach the daemon's tip within {}s: \
         scanned to {scanned_to}, tip {tip}",
        REFRESH_TIMEOUT.as_secs()
    )]
    RefreshTimeout { scanned_to: u64, tip: u64 },
}

/// one of my own transfers, in flight until its spend is confirmed.
///
/// the scan loop uses these to finish the bookkeeping a reorg undid:
/// a rolled back confirmation puts the inputs back to pending, and
/// the loop re-confirms when the tx lands on the new chain.
#[derive(Debug, Clone)]
struct TrackedTransfer {
    tx_hash: [u8; 32],
    input_ids: Vec<OutputId>,
}

/// state, scan data and transfer tracking under one lock, so the
/// scan loop and `fund` take turns instead of deadlocking each
/// other.
#[derive(Debug, Default)]
struct WalletData {
    state: WalletState,
    /// the next height the scan loop will look at. one past the last
    /// block it actually scanned.
    scanned_tip: u64,
    outputs: HashMap<OutputId, WalletOutput>,
    /// key image -> output, for spotting my own outputs being spent
    /// by scanned transactions
    key_images: HashMap<[u8; 32], OutputId>,
    transfers: Vec<TrackedTransfer>,
}

impl<P> Drop for Funder<P> {
    fn drop(&mut self) {
        // tell the scan loop to stop; a mid-poll loop exits at its
        // next select
        let _ = self.shutdown.send(true);
    }
}

/// a wallet that funds exact amounts.
pub struct Funder<P> {
    provider: P,
    private_spend_key: Zeroizing<Scalar>,
    data: Arc<Mutex<WalletData>>,
    /// one fund at a time; scanning is not blocked by this.
    fund_lock: Arc<Mutex<()>>,
    /// dropped when the funder drops; stops the scan loop.
    shutdown: watch::Sender<bool>,
}

impl<P> Funder<P>
where
    P: ProvidesScannableBlocks
        + ProvidesBlockchainMeta
        + ProvidesDecoys
        + ProvidesFeeRates
        + ProvidesTransactionStatus
        + ProvidesAdjustedTime
        + PublishTransaction
        + Clone
        + Send
        + Sync
        + 'static,
{
    /// watch a wallet from `restore_height`, scanning in the
    /// background.
    ///
    /// scanning starts immediately and stops when the funder is
    /// dropped. must be called inside a tokio runtime.
    pub fn new(
        provider: P,
        private_spend_key: Zeroizing<Scalar>,
        private_view_key: Zeroizing<Scalar>,
        restore_height: usize,
    ) -> Result<Self, FunderError> {
        let view_pair = ViewPair::new(public_key(&private_spend_key), private_view_key.clone())?;
        let scanner = Scanner::new(view_pair);

        let data = Arc::new(Mutex::new(WalletData::default()));
        let fund_lock = Arc::new(Mutex::new(()));
        let (shutdown, shutdown_receiver) = watch::channel(false);

        tokio::spawn(scan_loop(
            provider.clone(),
            scanner,
            Arc::clone(&data),
            private_spend_key.clone(),
            restore_height,
            shutdown_receiver,
        ));

        Ok(Self {
            provider,
            private_spend_key,
            data,
            fund_lock,
            shutdown,
        })
    }

    /// everything received and not spent, locked or not.
    pub async fn balance(&self) -> Result<u64, FunderError> {
        Ok(self.data.lock().await.state.balance()?)
    }

    /// everything you could spend right now, at the daemon's current
    /// tip and adjusted time.
    pub async fn unlocked_balance(&self) -> Result<u64, FunderError> {
        let (tip, adjusted_time) = tokio::join!(
            self.provider.latest_block_number(),
            self.provider.adjusted_time()
        );
        let tip = tip?;
        let adjusted_time = adjusted_time?;

        let inclusion_height = tip as u64 + 1;
        Ok(self
            .data
            .lock()
            .await
            .state
            .unlocked_balance(inclusion_height, adjusted_time)?)
    }

    /// wait until the scan loop has seen every block the daemon has.
    /// the wallet2 `wait_until_synced` equivalent: a balance read
    /// before this returns is a balance from the past.
    pub async fn refresh(&self) -> Result<(), FunderError> {
        let deadline = tokio::time::Instant::now() + REFRESH_TIMEOUT;
        loop {
            let tip = self.provider.latest_block_number().await? as u64;
            let scanned_to = self.data.lock().await.scanned_tip;
            if scanned_to > tip {
                return Ok(());
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(FunderError::RefreshTimeout { scanned_to, tip });
            }

            tokio::time::sleep(SCAN_POLL_INTERVAL).await;
        }
    }

    /// pay exact amounts, change coming back to `change_address`,
    /// and wait for the transfer to land in a block.
    ///
    /// one fund at a time. the returned [`Transfer`] describes what
    /// happened: inputs spent, fee, change. if the daemon rejects the
    /// transfer the inputs are released again; if the confirmation
    /// times out, or a reorg eats it after confirming, they STAY
    /// pending, because the tx may still land and spending them
    /// twice would be a double spend. the scan loop finishes the
    /// bookkeeping either way.
    pub async fn fund(
        &self,
        destinations: Vec<(MoneroAddress, u64)>,
        change_address: MoneroAddress,
    ) -> Result<Transfer, FunderError> {
        let _guard = self.fund_lock.lock().await;

        let (tip, adjusted_time) = tokio::join!(
            self.provider.latest_block_number(),
            self.provider.adjusted_time()
        );
        let tip = tip?;
        let adjusted_time = adjusted_time?;
        let inclusion_height = tip as u64 + 1;

        let transfer = {
            let data = self.data.lock().await;
            transfer::construct_transfer(
                &self.provider,
                &self.private_spend_key,
                &data.outputs,
                &data.state,
                destinations,
                change_address,
                inclusion_height,
                adjusted_time,
                None,
            )
            .await?
        };

        let tx_hash = transfer.transaction.hash();

        // pending before the daemon ever sees the tx, tracked so the
        // scan loop can finish the job if a reorg interferes
        {
            let mut data = self.data.lock().await;
            data.state.mark_pending(&transfer.input_ids)?;
            data.transfers.push(TrackedTransfer {
                tx_hash,
                input_ids: transfer.input_ids.clone(),
            });
        }

        if let Err(publish_error) = self
            .provider
            .publish_transaction(&transfer.transaction)
            .await
        {
            // rejected: this tx will never land, the inputs are free
            // again. a reorg may have rolled some of them away
            // between selection and now, and releasing the survivors
            // is still right — the tx is never landing either way. if
            // the release itself fails that is the weirder failure
            // and the one to surface, but both get logged.
            let release_result = {
                let mut data = self.data.lock().await;
                let survivors: Vec<OutputId> = transfer
                    .input_ids
                    .iter()
                    .filter(|id| data.state.output(id).is_some())
                    .copied()
                    .collect();
                let release = data.state.release_pending(&survivors);
                data.transfers.retain(|tracked| tracked.tx_hash != tx_hash);
                release
            };
            if let Err(release_error) = release_result {
                tracing::error!(
                    publish = ?publish_error,
                    release = ?release_error,
                    "publish rejected and releasing the inputs failed too"
                );
                return Err(release_error.into());
            }

            return Err(publish_error.into());
        }

        // the wait, with a timeout that can actually fire:
        // wait_until_confirmed only ends early when the subscription
        // closes, which cannot happen while i hold it
        let confirmed = tokio::time::timeout(
            CONFIRMATION_TIMEOUT,
            confirmations::subscribe(self.provider.clone(), tx_hash, SCAN_POLL_INTERVAL)
                .wait_until_confirmed(1),
        )
        .await;
        match confirmed {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => return Err(FunderError::ConfirmationTimeout),
        }

        let TransactionStatus::InBlock { .. } = self.provider.transaction_status(tx_hash).await?
        else {
            // confirmed, then a reorg evicted it before i could
            // double check. it is still in flight and the scan loop
            // will finish this when it lands again.
            return Err(FunderError::ConfirmationTimeout);
        };

        // the scan loop records the spend when it scans the block the
        // tx landed in, and stops tracking the transfer once every
        // input shows spent. fund returning only means the chain has
        // the tx, the bookkeeping lands with the next scan batch.
        Ok(transfer)
    }
}

/// scan blocks from `next_height`, feed everything belonging to the
/// wallet into the shared data, and roll back to the exact
/// divergence height on reorgs.
///
/// the loop is self healing: RPC failures are logged and retried on
/// the next poll, and a block that fails to scan is retried rather
/// than skipped — a block i cannot scan is a block i cannot afford
/// to miss. it exits when the shutdown watch closes, ie when every
/// funder sharing the data is gone.
async fn scan_loop<P>(
    provider: P,
    mut scanner: Scanner,
    data: Arc<Mutex<WalletData>>,
    private_spend_key: Zeroizing<Scalar>,
    restore_height: usize,
    mut shutdown: watch::Receiver<bool>,
) where
    P: ProvidesScannableBlocks + ProvidesBlockchainMeta + Send + Sync,
{
    let mut next_height = restore_height;
    // the hash of every block i scanned, by height, for finding the
    // exact divergence point on a reorg
    let mut scanned_hashes: HashMap<u64, [u8; 32]> = HashMap::new();

    loop {
        let tip = match provider.latest_block_number().await {
            Ok(tip) => tip,
            Err(e) => {
                tracing::warn!(error = ?e, "funder scan: tip lookup failed");
                if !poll_sleep(&mut shutdown).await {
                    return;
                }
                continue;
            }
        };

        // my own transfers: drop the finished ones (the chain marked
        // every input spent) and release the dead ones (a reorg
        // rolled an input away entirely, the tx can never land)
        reconcile_trackers(&data).await;

        // the tip went backwards: a reorg rolled the chain back
        if tip + 1 < next_height {
            let from = tip as u64;
            match divergence_height(&provider, &scanned_hashes, from).await {
                Ok(divergence) => {
                    tracing::warn!(
                        divergence,
                        "funder scan: reorg rolled the chain back, rescanning"
                    );
                    apply_rollback(&data, &mut scanned_hashes, divergence).await;
                    next_height = divergence as usize;
                }
                Err(e) => {
                    // an unresolved reorg means i cannot say which
                    // of my blocks are still on this chain, so i
                    // cannot claim to be synced to anything either.
                    // the walk retries next poll and the rescan
                    // restores a real number
                    tracing::warn!(error = ?e, "funder scan: divergence walk failed, retrying");
                    data.lock().await.scanned_tip = 0;
                }
            }
            continue;
        }

        if next_height > tip {
            // caught up, but the chain can still be swapped under me
            // at the same height with no new block to give it away.
            // re-check the last block i scanned: if the daemon's
            // version differs, that is a reorg
            if next_height > 0 {
                let last = next_height as u64 - 1;
                let still_agrees = match provider.scannable_block_by_number(next_height - 1).await {
                    Ok(fresh) => scanned_hashes
                        .get(&last)
                        .is_none_or(|scanned| fresh.block.hash() == *scanned),
                    Err(e) => {
                        tracing::warn!(error = ?e, height = last, "funder scan: idle recheck failed");
                        true
                    }
                };

                if !still_agrees {
                    let from = last;
                    match divergence_height(&provider, &scanned_hashes, from).await {
                        Ok(divergence) => {
                            tracing::warn!(
                                divergence,
                                "funder scan: idle recheck found a reorg, rescanning"
                            );
                            apply_rollback(&data, &mut scanned_hashes, divergence).await;
                            next_height = divergence as usize;
                        }
                        Err(e) => {
                            tracing::warn!(error = ?e, "funder scan: divergence walk failed, retrying");
                            data.lock().await.scanned_tip = 0;
                        }
                    }
                    continue;
                }
            }

            if !poll_sleep(&mut shutdown).await {
                return;
            }
            continue;
        }

        let end = (next_height + SCAN_BATCH_SIZE - 1).min(tip);
        let blocks = match provider
            .contiguous_scannable_blocks(next_height..=end)
            .await
        {
            Ok(blocks) => blocks,
            Err(e) => {
                tracing::warn!(error = ?e, from = next_height, to = end, "funder scan: block fetch failed");
                if !poll_sleep(&mut shutdown).await {
                    return;
                }
                continue;
            }
        };

        for block in blocks {
            let height = block.block.number();
            let hash = block.block.hash();

            // the chain stopped agreeing with what i scanned before:
            // the divergence is at or below the block before this one
            if height > 0
                && scanned_hashes
                    .get(&(height as u64 - 1))
                    .is_some_and(|previous| block.block.header.previous != *previous)
            {
                let from = height as u64 - 1;
                match divergence_height(&provider, &scanned_hashes, from).await {
                    Ok(divergence) => {
                        tracing::warn!(
                            height,
                            divergence,
                            "funder scan: previous hash mismatch, rescanning"
                        );
                        apply_rollback(&data, &mut scanned_hashes, divergence).await;
                        next_height = divergence as usize;
                    }
                    Err(e) => {
                        // same as above: unresolved reorg, no sync
                        // claim until the walk gets through
                        tracing::warn!(error = ?e, "funder scan: divergence walk failed, retrying");
                        data.lock().await.scanned_tip = 0;
                    }
                }
                break;
            }

            // spends come off the chain: every input key image in
            // this block gets matched against my own outputs, and a
            // hit means that output left me right here
            let spent_here = {
                let input_key_images: Vec<[u8; 32]> = block
                    .transactions
                    .iter()
                    .flat_map(|transaction| transaction.prefix().inputs.iter())
                    .filter_map(|input| match input {
                        Input::ToKey { key_image, .. } => Some(key_image.to_bytes()),
                        Input::Gen(_) => None,
                    })
                    .collect();
                let data = data.lock().await;
                input_key_images
                    .iter()
                    .filter_map(|key_image| data.key_images.get(key_image).copied())
                    .collect::<Vec<OutputId>>()
            };

            match scanner.scan(block) {
                Ok(scanned) => {
                    // locked outputs included: coinbase money arrives
                    // with its unlock time and the state tracks windows
                    let mut data = data.lock().await;
                    for output in scanned.ignore_additional_timelock() {
                        let id = transfer::output_id(&output);
                        data.state.receive(ReceivedOutput {
                            id,
                            amount_pico: output.commitment().amount,
                            received_at_height: height as u64,
                            is_coinbase: false,
                            additional_timelock: output.additional_timelock(),
                        });
                        data.key_images
                            .insert(key_image(&private_spend_key, &output), id);
                        data.outputs.insert(id, output);
                    }

                    if !spent_here.is_empty()
                        && let Err(e) = data.state.confirm_spent(&spent_here, height as u64)
                    {
                        // re-confirming at a different height means
                        // the chain moved under me in a way the
                        // rollback pass did not catch. loud, not
                        // silent
                        tracing::warn!(
                            error = ?e,
                            height,
                            "funder scan: recording an on-chain spend"
                        );
                    }
                }
                Err(e) => {
                    // do not advance past a block i failed to scan:
                    // next poll refetches it. better a loud retry loop
                    // than quietly never seeing those funds
                    tracing::warn!(error = ?e, height, "funder scan: scan failed, retrying this block");
                    break;
                }
            }

            scanned_hashes.insert(height as u64, hash);
            next_height = height + 1;
        }

        data.lock().await.scanned_tip = next_height as u64;
    }
}

/// sleep for one poll interval, or report that shutdown was signaled.
async fn poll_sleep(shutdown: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        changed = shutdown.changed() => changed.is_err(),
        _ = tokio::time::sleep(SCAN_POLL_INTERVAL) => true,
    }
}

/// walk down from `from` until the stored hash matches the daemon's
/// block, and return the first height above the agreement — the
/// height everything at or above which is orphaned.
///
/// below what i ever scanned there is nothing to compare, so the
/// daemon is trusted from there.
async fn divergence_height<P>(
    provider: &P,
    scanned_hashes: &HashMap<u64, [u8; 32]>,
    from: u64,
) -> Result<u64, monero_interface::InterfaceError>
where
    P: ProvidesScannableBlocks + Sync,
{
    let mut height = from;

    loop {
        let Some(stored) = scanned_hashes.get(&height) else {
            return Ok(height + 1);
        };

        let fresh = provider.scannable_block_by_number(height as usize).await?;
        if fresh.block.hash() == *stored {
            return Ok(height + 1);
        }

        if height == 0 {
            return Ok(0);
        }
        height -= 1;
    }
}

/// finish the bookkeeping of my own transfers: anything confirmed
/// on the current chain gets its spend recorded and stops being
/// tracked.
/// my own transfers, checked against what the chain has already
/// said:
///
/// - every input spent: the transfer finished, stop tracking.
/// - an input rolled away entirely: the tx spends an output this
///   chain no longer has, so it cannot land as things stand and the
///   surviving inputs go back to available. (a deeper reorg could
///   resurrect the rolled-away input and let the old tx land after
///   all, conflicting with anything new i spent the survivors on —
///   a liveness risk, not an accounting one: the chain still decides
///   what is spent.)
/// - anything else: in flight, leave it alone.
async fn reconcile_trackers(data: &Arc<Mutex<WalletData>>) {
    let mut data = data.lock().await;
    let WalletData {
        state, transfers, ..
    } = &mut *data;

    let mut dead = Vec::new();
    transfers.retain(|tracked| {
        let mut any_missing = false;
        let mut all_spent = true;
        for id in &tracked.input_ids {
            match state.is_spent(id) {
                None => any_missing = true,
                Some(spent) => all_spent &= spent,
            }
        }

        if any_missing {
            dead.push(tracked.input_ids.clone());
            false
        } else {
            !all_spent
        }
    });

    for input_ids in dead {
        // release only what still exists: the state refuses the whole
        // call if any id is unknown, and dead means at least one is
        // gone. it is a no-op for inputs that are not pending (one of
        // them may have been spent by some other tx), which is fine
        let survivors: Vec<OutputId> = input_ids
            .iter()
            .filter(|id| state.output(id).is_some())
            .copied()
            .collect();
        if let Err(e) = state.release_pending(&survivors) {
            tracing::warn!(error = ?e, "funder scan: releasing a dead transfer's inputs");
        }
    }
}

/// the key image for an output i own: the one-time private key
/// times the hash of the one-time public key. same recipe oxide's
/// signer uses, so what the scan matches is exactly what a signed
/// spend of ours would carry.
fn key_image(spend_key: &Scalar, output: &WalletOutput) -> [u8; 32] {
    let spend: curve25519_dalek::Scalar = (*spend_key).into();
    let offset: curve25519_dalek::Scalar = output.key_offset().into();
    let input_key = spend + offset;
    let hashed: curve25519_dalek::EdwardsPoint =
        Point::biased_hash(output.key().compress().to_bytes()).into();
    Point::from(input_key * hashed).compress().to_bytes()
}

/// roll the shared data back to `divergence`: outputs received at or
/// above it are gone, spends that happened at or above it never
/// happened, my own in-flight transfers go back to pending, the
/// scan data loses everything the state lost.
async fn apply_rollback(
    data: &Arc<Mutex<WalletData>>,
    scanned_hashes: &mut HashMap<u64, [u8; 32]>,
    divergence: u64,
) {
    let mut data = data.lock().await;
    let WalletData {
        state,
        scanned_tip,
        outputs,
        key_images,
        transfers,
    } = &mut *data;

    state.rollback_to(divergence);

    // my own in-flight transfers: the ones whose inputs all survived
    // go back to pending until the tx lands on the new chain (the
    // rescan re-marks the spend off the key images when it does).
    // the ones that lost an input cannot land as things stand — the
    // tx spends an output this chain no longer has — so the
    // survivors go free and tracking stops. (a deeper reorg could
    // resurrect the lost input; worst case is a conflicting tx, the
    // accounting stays chain-derived.)
    let mut dead = Vec::new();
    transfers.retain(|tracked| {
        if tracked
            .input_ids
            .iter()
            .all(|id| state.output(id).is_some())
        {
            true
        } else {
            dead.push(tracked.input_ids.clone());
            false
        }
    });
    for tracked in &*transfers {
        if let Err(e) = state.mark_pending(&tracked.input_ids) {
            // unreachable after the retain above; if i am wrong about
            // that i want to hear about it
            tracing::error!(error = ?e, "funder scan: re-pending after rollback failed");
        }
    }
    for input_ids in dead {
        // release only what still exists, same as above: the state
        // refuses the whole call on an unknown id and dead means at
        // least one is gone
        let survivors: Vec<OutputId> = input_ids
            .iter()
            .filter(|id| state.output(id).is_some())
            .copied()
            .collect();
        if let Err(e) = state.release_pending(&survivors) {
            tracing::warn!(error = ?e, "funder scan: releasing a dead transfer's inputs");
        }
    }

    // the scan data loses whatever the state lost
    outputs.retain(|id, _| state.output(id).is_some());
    key_images.retain(|_, id| state.output(id).is_some());
    scanned_hashes.retain(|height, _| *height < divergence);
    *scanned_tip = divergence;
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::sync::Mutex as StdMutex;

    use monero_interface::ProvidesScannableBlocks;
    use monero_oxide::block::{Block, BlockHeader};
    use monero_oxide::ed25519::Point;
    use monero_oxide::transaction::{Input, Pruned, Timelock, Transaction, TransactionPrefix};
    use monero_oxide_wallet::transaction::NotPruned;

    // the same pinned keys and captured transaction as the empty
    // wallet tests: a real output those keys can scan
    const SPEND_KEY: &str = "ccf0ea10e1ea64354f42fa710c2b318e581969cf49046d809d1f0aadb3fc7a02";
    const VIEW_KEY: &str = "a28b4b2085592881df94ee95da332c16b5bb773eb8bb74730208cbb236c73806";
    const RECEIVED_TX: &str = "020001020003060101cf60390bb71aa15eb24037772012d59dc68cb4b6211e1c93206db09a6c346261020002ee8ca293511571c0005e1c144e49d09b8ff03046dbafb3e064a34cb9fc1994b600029e2e5cd08c8681dbcf2ce66071467e835f7e86613fbfed3c4fb170127b94e1072c01d3ce2a622c6e06ed465f81017dd6188c3a6e3d8e65a846f9c98416da0e150a82020901c553d35e54111bd001e0bbcbf289d701ce90e309ead2b487ec1d4d8af5d649543eb99a7620f6b54e532898527be29704f050e6f06de61e5967b2ddd506b4d6d36546065d6aae156ac7bec18c99580c07867fb98cb29853edbafec91af2df605c12f9aaa81a9165625afb6649f5a652012c5ba6612351140e1fb4a8463cc765d0a9bb7d999ba35750f365c5285d77230b76c7a612784f4845812a2899f2ca6a304fee61362db59b263115c27d2ce78af6b1d9e939c1f4036c7707851f41abe6458cf1c748353e593469ebf43536a939f7";

    fn spend_scalar() -> Zeroizing<Scalar> {
        let spend_key = Scalar::read(&mut hex::decode(SPEND_KEY).unwrap().as_slice()).unwrap();
        Zeroizing::new(spend_key)
    }

    fn view_pair_keys() -> (Point, Zeroizing<Scalar>) {
        let spend_key = Scalar::read(&mut hex::decode(SPEND_KEY).unwrap().as_slice()).unwrap();
        let view_key = Scalar::read(&mut hex::decode(VIEW_KEY).unwrap().as_slice()).unwrap();
        (public_key(&spend_key), Zeroizing::new(view_key))
    }

    fn received_transaction() -> (Transaction<Pruned>, [u8; 32]) {
        let tx =
            Transaction::<Pruned>::read(&mut hex::decode(RECEIVED_TX).unwrap().as_slice()).unwrap();
        (tx, [1; 32])
    }

    /// transactions by height, each with its tx id
    type MockTransactions = BTreeMap<usize, Vec<(Transaction<Pruned>, [u8; 32])>>;

    /// a controllable chain: transactions by height, and a tip.
    #[derive(Default)]
    struct MockChain {
        latest: usize,
        transactions: MockTransactions,
    }

    #[derive(Clone, Default)]
    struct MockProvider {
        chain: Arc<StdMutex<MockChain>>,
        /// when set, block-by-number lookups fail: the daemon is
        /// unreachable for the divergence walk
        fail_by_number: Arc<StdMutex<bool>>,
    }

    fn mock_chain_blocks(
        chain: &MockChain,
        range: std::ops::RangeInclusive<usize>,
    ) -> Vec<monero_interface::ScannableBlock> {
        let mut previous = [0; 32];
        let mut requested = Vec::new();

        for height in 0..=*range.end() {
            let transactions = chain.transactions.get(&height).cloned().unwrap_or_default();
            let (txids, transactions) = transactions
                .into_iter()
                .map(|(tx, tx_id)| (tx_id, tx))
                .unzip();
            let miner_transaction = Transaction::V1 {
                prefix: TransactionPrefix {
                    additional_timelock: Timelock::None,
                    inputs: vec![Input::Gen(height)],
                    outputs: vec![],
                    extra: vec![],
                },
                signatures: Vec::new(),
            };
            let block = Block::new(
                BlockHeader {
                    hardfork_version: crate::HARDFORK_VERSION,
                    hardfork_signal: 0,
                    timestamp: 0,
                    previous,
                    nonce: 0,
                },
                miner_transaction,
                txids,
            )
            .unwrap();
            previous = block.hash();

            if range.contains(&height) {
                requested.push(monero_interface::ScannableBlock {
                    block,
                    transactions,
                    output_index_for_first_ringct_output: Some(0),
                });
            }
        }

        requested
    }

    impl ProvidesBlockchainMeta for MockProvider {
        async fn latest_block_number(&self) -> Result<usize, monero_interface::InterfaceError> {
            Ok(self.chain.lock().unwrap().latest)
        }
    }

    impl ProvidesScannableBlocks for MockProvider {
        async fn contiguous_scannable_blocks(
            &self,
            range: std::ops::RangeInclusive<usize>,
        ) -> Result<Vec<monero_interface::ScannableBlock>, monero_interface::InterfaceError>
        {
            Ok(mock_chain_blocks(&self.chain.lock().unwrap(), range))
        }

        async fn scannable_block(
            &self,
            _hash: [u8; 32],
        ) -> Result<monero_interface::ScannableBlock, monero_interface::InterfaceError> {
            panic!("unexpected block-by-hash lookup")
        }

        async fn scannable_block_by_number(
            &self,
            number: usize,
        ) -> Result<monero_interface::ScannableBlock, monero_interface::InterfaceError> {
            if *self.fail_by_number.lock().unwrap() {
                return Err(monero_interface::InterfaceError::InternalError(
                    "block by number failed".to_owned(),
                ));
            }
            Ok(
                mock_chain_blocks(&self.chain.lock().unwrap(), number..=number)
                    .pop()
                    .unwrap(),
            )
        }
    }

    impl ProvidesTransactionStatus for MockProvider {
        async fn transaction_status(
            &self,
            _tx_id: [u8; 32],
        ) -> Result<TransactionStatus, crate::rpc::TransactionStatusError> {
            // the reorg test drives no transfers of my own
            Ok(TransactionStatus::Unknown)
        }
    }

    /// poll until the balance reads `expected`, or give up.
    async fn wait_for_balance(data: &Arc<Mutex<WalletData>>, expected: u64) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            let balance = data.lock().await.state.balance();
            if balance == Ok(expected) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    /// poll until the wallet reads empty: zero balance, nothing in
    /// the scan data.
    async fn wait_for_drain(data: &Arc<Mutex<WalletData>>) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            let drained = {
                let data = data.lock().await;
                matches!(data.state.balance(), Ok(0)) && data.outputs.is_empty()
            };
            if drained {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    #[tokio::test]
    async fn scan_loop_rolls_back_to_the_divergence_and_rescans() {
        let provider = MockProvider::default();
        let (public_spend_key, private_view_key) = view_pair_keys();
        let scanner =
            Scanner::new(ViewPair::new(public_spend_key, private_view_key.clone()).unwrap());

        let data = Arc::new(Mutex::new(WalletData::default()));
        let (shutdown, shutdown_receiver) = watch::channel(false);

        // the output lands at height 5, the chain runs to 10
        {
            let mut chain = provider.chain.lock().unwrap();
            chain.transactions.insert(5, vec![received_transaction()]);
            chain.latest = 10;
        }

        let expected_amount = {
            // scan the transaction once to learn what it pays
            let blocks = provider.contiguous_scannable_blocks(5..=5).await.unwrap();
            let mut scan_scanner =
                Scanner::new(ViewPair::new(public_spend_key, private_view_key).unwrap());
            let scanned = scan_scanner
                .scan(blocks.into_iter().next().unwrap())
                .unwrap();
            scanned
                .ignore_additional_timelock()
                .iter()
                .map(|output| output.commitment().amount)
                .sum()
        };
        assert!(expected_amount > 0);

        tokio::spawn(scan_loop(
            provider.clone(),
            scanner,
            Arc::clone(&data),
            spend_scalar(),
            0,
            shutdown_receiver,
        ));

        // the output shows up
        assert!(
            wait_for_balance(&data, expected_amount).await,
            "output never scanned in"
        );

        // reorg: the transaction vanishes from height 5 and the
        // chain extends. everything from 5 up is a different chain
        // now
        {
            let mut chain = provider.chain.lock().unwrap();
            chain.transactions.remove(&5);
            chain.latest = 11;
        }

        // the output is gone, and the scan data went with it
        assert!(
            wait_for_drain(&data).await,
            "reorg did not roll the output back"
        );

        // reorg again: the transaction is back on the new chain
        {
            let mut chain = provider.chain.lock().unwrap();
            chain.transactions.insert(5, vec![received_transaction()]);
            chain.latest = 12;
        }

        assert!(
            wait_for_balance(&data, expected_amount).await,
            "rescan did not pick the output back up"
        );

        // a shallower reorg above the output must not touch it
        {
            let mut chain = provider.chain.lock().unwrap();
            chain.latest = 9;
        }

        tokio::time::sleep(Duration::from_millis(750)).await;
        assert_eq!(
            data.lock().await.state.balance(),
            Ok(expected_amount),
            "a reorg above the output must not roll it back"
        );

        shutdown.send(true).unwrap();
    }

    /// a compile-time witness that Transfer stays nameable.
    #[allow(dead_code)]
    fn _transfer_witness(transfer: Transfer) -> Transfer {
        let _: &Transaction<NotPruned> = &transfer.transaction;
        transfer
    }

    #[tokio::test]
    async fn scan_loop_marks_spends_off_the_chain_and_a_reorg_takes_them_back() {
        let provider = MockProvider::default();
        let (public_spend_key, private_view_key) = view_pair_keys();
        let spend_key = spend_scalar();
        let scanner =
            Scanner::new(ViewPair::new(public_spend_key, private_view_key.clone()).unwrap());

        let data = Arc::new(Mutex::new(WalletData::default()));
        let (shutdown, shutdown_receiver) = watch::channel(false);

        // learn the outputs and their key images from a direct scan
        // (the pinned transaction pays two outputs to these keys)
        let (expected_amount, key_images) = {
            provider
                .chain
                .lock()
                .unwrap()
                .transactions
                .insert(5, vec![received_transaction()]);
            let blocks = provider.contiguous_scannable_blocks(5..=5).await.unwrap();
            let mut probe_scanner =
                Scanner::new(ViewPair::new(public_spend_key, private_view_key.clone()).unwrap());
            let scanned = probe_scanner
                .scan(blocks.into_iter().next().unwrap())
                .unwrap();
            let outputs = scanned.ignore_additional_timelock();
            let amount: u64 = outputs
                .iter()
                .map(|output| output.commitment().amount)
                .sum();
            let key_images = outputs
                .iter()
                .map(|output| key_image(&spend_key, output))
                .collect::<Vec<_>>();
            (amount, key_images)
        };
        assert!(expected_amount > 0);

        // a transaction spending my outputs: each input carries one
        // of my key images. the scan loop only reads inputs, so a
        // real shape with no signatures is enough to drive it
        let spend_transaction: Transaction<Pruned> = Transaction::V1 {
            prefix: TransactionPrefix {
                additional_timelock: Timelock::None,
                inputs: key_images
                    .into_iter()
                    .map(|key_image| Input::ToKey {
                        amount: Some(0),
                        key_offsets: vec![],
                        key_image: key_image.into(),
                    })
                    .collect(),
                outputs: vec![],
                extra: vec![],
            },
            signatures: (),
        };

        // the spend lands at height 12. the tip starts at 9, inside
        // the first batch, so the loop scans the receive and then
        // idles; extending the tip later lets the spend land as its
        // own observable step
        {
            let mut chain = provider.chain.lock().unwrap();
            chain
                .transactions
                .insert(12, vec![(spend_transaction.clone(), [7; 32])]);
            chain.latest = 9;
        }

        tokio::spawn(scan_loop(
            provider.clone(),
            scanner,
            Arc::clone(&data),
            spend_key,
            0,
            shutdown_receiver,
        ));

        // the chain says the output left me. the wallet starts
        // empty at zero, so first wait for the money to arrive, then
        // wait for the spend to take it away again
        assert!(
            wait_for_balance(&data, expected_amount).await,
            "the output never scanned in"
        );
        // extend the tip so the spend block becomes scannable
        provider.chain.lock().unwrap().latest = 13;
        assert!(
            wait_for_balance(&data, 0).await,
            "the on-chain spend never marked the output spent"
        );

        // reorg the spend away
        {
            let mut chain = provider.chain.lock().unwrap();
            chain.transactions.remove(&12);
            chain.latest = 14;
        }

        // the spend reverted with the block
        assert!(
            wait_for_balance(&data, expected_amount).await,
            "the reorg did not give the spent output back"
        );

        // the spend is back on the new chain
        {
            let mut chain = provider.chain.lock().unwrap();
            chain
                .transactions
                .insert(12, vec![(spend_transaction, [7; 32])]);
            chain.latest = 15;
        }

        assert!(
            wait_for_balance(&data, 0).await,
            "the rescan did not re-mark the spend"
        );

        shutdown.send(true).unwrap();
    }

    fn output_id_at(byte: u8, index: u64) -> OutputId {
        OutputId {
            transaction: [byte; 32],
            index,
        }
    }

    fn received(id: OutputId, amount: u64, height: u64) -> ReceivedOutput {
        ReceivedOutput {
            id,
            amount_pico: amount,
            received_at_height: height,
            is_coinbase: false,
            additional_timelock: Timelock::None,
        }
    }

    /// a reorg that kills one input of an in-flight transfer must
    /// release the survivor, not strand it pending forever
    #[tokio::test]
    async fn rollback_releases_the_survivors_of_a_dead_transfer() {
        let data = Arc::new(Mutex::new(WalletData::default()));

        let first = output_id_at(1, 0);
        let second = output_id_at(2, 0);

        {
            let mut data = data.lock().await;
            data.state.receive(received(first, 100, 100));
            data.state.receive(received(second, 200, 200));
            data.state.mark_pending(&[first, second]).unwrap();
            data.transfers.push(TrackedTransfer {
                tx_hash: [9; 32],
                input_ids: vec![first, second],
            });
        }

        // the divergence eats the second output's block
        let mut scanned_hashes = HashMap::new();
        scanned_hashes.insert(100, [1; 32]);
        scanned_hashes.insert(200, [2; 32]);
        apply_rollback(&data, &mut scanned_hashes, 150).await;

        let data = data.lock().await;
        // the survivor is spendable again, not stranded pending
        assert_eq!(data.state.balance(), Ok(100));
        assert_eq!(data.state.pending_balance(), Ok(0));
        // the dead transfer is not tracked anymore
        assert!(data.transfers.is_empty());
        // the rolled-away output is gone from the scan data
        assert!(data.state.output(&second).is_none());
    }

    /// a reorg that swaps the chain at the same height, with no new
    /// block to give it away, is caught by the idle recheck
    #[tokio::test]
    async fn idle_recheck_catches_a_same_height_reorg() {
        let provider = MockProvider::default();
        let (public_spend_key, private_view_key) = view_pair_keys();
        let scanner =
            Scanner::new(ViewPair::new(public_spend_key, private_view_key.clone()).unwrap());

        let data = Arc::new(Mutex::new(WalletData::default()));
        let (shutdown, shutdown_receiver) = watch::channel(false);

        {
            let mut chain = provider.chain.lock().unwrap();
            chain.transactions.insert(5, vec![received_transaction()]);
            chain.latest = 10;
        }

        let expected_amount = {
            let blocks = provider.contiguous_scannable_blocks(5..=5).await.unwrap();
            let mut probe_scanner =
                Scanner::new(ViewPair::new(public_spend_key, private_view_key).unwrap());
            let scanned = probe_scanner
                .scan(blocks.into_iter().next().unwrap())
                .unwrap();
            scanned
                .ignore_additional_timelock()
                .iter()
                .map(|output| output.commitment().amount)
                .sum()
        };

        tokio::spawn(scan_loop(
            provider.clone(),
            scanner,
            Arc::clone(&data),
            spend_scalar(),
            0,
            shutdown_receiver,
        ));

        assert!(
            wait_for_balance(&data, expected_amount).await,
            "output never scanned in"
        );
        // let the loop go idle: next poll runs the idle recheck
        tokio::time::sleep(Duration::from_millis(750)).await;

        // the chain is swapped at the same height: the transaction
        // vanishes, the tip does not move
        provider.chain.lock().unwrap().transactions.remove(&5);

        // the idle recheck notices and the rollback takes effect
        assert!(
            wait_for_drain(&data).await,
            "the same-height reorg was never detected"
        );

        shutdown.send(true).unwrap();
    }

    /// while a reorg is detected but the divergence walk cannot run,
    /// the wallet must not claim to be synced
    #[tokio::test]
    async fn unresolved_reorg_retracts_the_sync_claim() {
        let provider = MockProvider::default();
        let (public_spend_key, private_view_key) = view_pair_keys();
        let scanner =
            Scanner::new(ViewPair::new(public_spend_key, private_view_key.clone()).unwrap());

        let data = Arc::new(Mutex::new(WalletData::default()));
        let (shutdown, shutdown_receiver) = watch::channel(false);

        {
            let mut chain = provider.chain.lock().unwrap();
            chain.transactions.insert(5, vec![received_transaction()]);
            chain.latest = 10;
        }

        let expected_amount = {
            let blocks = provider.contiguous_scannable_blocks(5..=5).await.unwrap();
            let mut probe_scanner =
                Scanner::new(ViewPair::new(public_spend_key, private_view_key).unwrap());
            let scanned = probe_scanner
                .scan(blocks.into_iter().next().unwrap())
                .unwrap();
            scanned
                .ignore_additional_timelock()
                .iter()
                .map(|output| output.commitment().amount)
                .sum()
        };

        tokio::spawn(scan_loop(
            provider.clone(),
            scanner,
            Arc::clone(&data),
            spend_scalar(),
            0,
            shutdown_receiver,
        ));

        // wait until the loop claims to be synced past the tip
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while data.lock().await.scanned_tip <= 10 {
            assert!(std::time::Instant::now() < deadline, "never synced");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // a reorg rolls the tip back while the daemon cannot answer
        // block-by-number lookups: the divergence walk cannot run
        *provider.fail_by_number.lock().unwrap() = true;
        provider.chain.lock().unwrap().latest = 8;

        // the sync claim must be retracted: scanned_tip goes to zero
        // while the reorg is unresolved
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while data.lock().await.scanned_tip != 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "sync claim was not retracted during the unresolved reorg"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // the daemon comes back: the walk gets through, the rollback
        // happens and the scan claim recovers with the rescan
        *provider.fail_by_number.lock().unwrap() = false;

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while data.lock().await.scanned_tip <= 8 {
            assert!(
                std::time::Instant::now() < deadline,
                "sync claim never recovered after the walk succeeded"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // the output below the divergence survived it all
        assert_eq!(data.lock().await.state.balance(), Ok(expected_amount));

        shutdown.send(true).unwrap();
    }

    /// same contract through reconcile: a transfer with an input the
    /// state never saw (rolled away entirely) releases its survivors
    #[tokio::test]
    async fn reconcile_releases_the_survivors_of_a_dead_transfer() {
        let data = Arc::new(Mutex::new(WalletData::default()));

        let first = output_id_at(1, 0);
        let phantom = output_id_at(3, 0);

        {
            let mut data = data.lock().await;
            data.state.receive(received(first, 100, 100));
            data.state.mark_pending(&[first]).unwrap();
            data.transfers.push(TrackedTransfer {
                tx_hash: [9; 32],
                input_ids: vec![first, phantom],
            });
        }

        reconcile_trackers(&data).await;

        let data = data.lock().await;
        assert_eq!(data.state.balance(), Ok(100));
        assert_eq!(data.state.pending_balance(), Ok(0));
        assert!(data.transfers.is_empty());
    }
}
