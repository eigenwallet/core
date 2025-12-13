//! Scanner utilities for view-only wallets.
//!
//! This module provides a helper similar to `confirmations::subscribe`: it spawns
//! background tasks which (1) fetch scannable blocks from an RPC provider and
//! (2) scan them using a `ViewPair`, emitting `WalletOutput`s as they are found.

use std::time::Duration;

use monero_interface::{ProvidesBlockchainMeta, ProvidesScannableBlocks, ScannableBlock};
use monero_oxide::ed25519::{Point, Scalar};
use monero_oxide_wallet::{GuaranteedViewPair, ViewPair, ViewPairError, WalletOutput};
use zeroize::Zeroizing;

/// A subscription to the scanner.
///
/// The `outputs` receiver yields outputs as they are discovered.
#[derive(Debug)]
pub struct Subscription {
    pub outputs: tokio::sync::mpsc::UnboundedReceiver<WalletOutput>,
    pub restore_height: usize,
}

/// Error returned when waiting for a subscription condition.
#[derive(Debug, thiserror::Error)]
#[error("Subscription closed before matching output was found")]
pub struct SubscriptionClosed;

impl Subscription {
    /// Wait until an output matching the predicate is found.
    ///
    /// This consumes outputs from the subscription until one matches the predicate.
    /// Non-matching outputs are discarded.
    ///
    /// # Returns
    /// * `Ok(output)` when a matching output is found
    /// * `Err(SubscriptionClosed)` if the background task stopped before finding a match
    pub async fn wait_until(
        &mut self,
        mut predicate: impl FnMut(&WalletOutput) -> bool,
    ) -> Result<WalletOutput, SubscriptionClosed> {
        loop {
            let output = self.outputs.recv().await.ok_or(SubscriptionClosed)?;

            if predicate(&output) {
                return Ok(output);
            }
        }
    }
}

#[derive(Debug)]
struct BlockAtHeight {
    height: usize,
    block: ScannableBlock,
}

// How many blocks to fetch per batch
const BLOCKS_PER_BATCH: usize = 10;

// How many blocks to queue up before blocking the fetcher
const BLOCK_QUEUE_SIZE: usize = BLOCKS_PER_BATCH * 5;

/// Spawn a scanner which catches up from `restore_height` and then follows the chain tip.
///
/// The returned subscription yields `WalletOutput`s as they are discovered.
/// The background tasks automatically stop when the `Subscription` is dropped.
pub fn scanner<P>(
    provider: P,
    public_spend_key: Point,
    private_view_key: Zeroizing<Scalar>,
    restore_height: usize,
    poll_interval: Duration,
) -> Result<Subscription, ViewPairError>
where
    P: ProvidesScannableBlocks + ProvidesBlockchainMeta + Send + 'static,
{
    let view_pair = GuaranteedViewPair::new(public_spend_key, private_view_key)?;

    let (outputs_sender, outputs) = tokio::sync::mpsc::unbounded_channel();
    let (blocks_sender, blocks_receiver) =
        tokio::sync::mpsc::channel::<BlockAtHeight>(BLOCK_QUEUE_SIZE);

    // We do not need to keep the task handles around.
    // The tasks will kill themselves once all subscribers are dropped.
    tokio::spawn(fetcher::run(
        provider,
        restore_height,
        poll_interval,
        blocks_sender,
    ));

    tokio::spawn(scanner::run(view_pair, blocks_receiver, outputs_sender));

    Ok(Subscription {
        outputs,
        restore_height,
    })
}

mod fetcher {
    use std::time::Duration;

    use monero_interface::{ProvidesBlockchainMeta, ProvidesScannableBlocks, ScannableBlock};

    use super::{BlockAtHeight, BLOCKS_PER_BATCH};
    use crate::retry::Backoff;

    pub(super) async fn run<P>(
        provider: P,
        restore_height: usize,
        poll_interval: Duration,
        blocks_sender: tokio::sync::mpsc::Sender<BlockAtHeight>,
    ) where
        P: ProvidesScannableBlocks + ProvidesBlockchainMeta + Send + 'static,
    {
        let mut backoff = Backoff::new();
        let mut next_height = restore_height;

        while !blocks_sender.is_closed() {
            let tip = match provider.latest_block_number().await {
                Ok(tip) => tip,
                Err(err) => {
                    backoff
                        .sleep_on_error(&err, "Failed to fetch latest block height")
                        .await;
                    continue;
                }
            };

            if next_height > tip {
                tokio::time::sleep(poll_interval).await;
                continue;
            }

            next_height =
                match fetch_until_tip(&provider, next_height, tip, &blocks_sender, &mut backoff)
                    .await
                {
                    Some(next) => next,
                    None => return,
                };
        }
    }

    async fn fetch_until_tip<P>(
        provider: &P,
        mut next_height: usize,
        tip: usize,
        blocks_sender: &tokio::sync::mpsc::Sender<BlockAtHeight>,
        backoff: &mut Backoff,
    ) -> Option<usize>
    where
        P: ProvidesScannableBlocks,
    {
        while !blocks_sender.is_closed() {
            let Some((start, end)) = batch_range(next_height, tip) else {
                return Some(next_height);
            };

            let blocks = match provider.contiguous_scannable_blocks(start..=end).await {
                Ok(blocks) => blocks,
                Err(err) => {
                    backoff
                        .sleep_on_error(&err, "Failed to fetch scannable blocks")
                        .await;
                    continue;
                }
            };

            tracing::trace!(blocks = blocks.len(), start, end, "Fetched blocks");

            if send_blocks(blocks_sender, start, blocks).await.is_none() {
                return None;
            }

            next_height = end.saturating_add(1);
        }

        None
    }

    fn batch_range(next_height: usize, tip: usize) -> Option<(usize, usize)> {
        if next_height > tip {
            return None;
        }

        let end = next_height
            .saturating_add(BLOCKS_PER_BATCH.saturating_sub(1))
            .min(tip);

        Some((next_height, end))
    }

    async fn send_blocks(
        sender: &tokio::sync::mpsc::Sender<BlockAtHeight>,
        start_height: usize,
        blocks: Vec<ScannableBlock>,
    ) -> Option<()> {
        for (i, block) in blocks.into_iter().enumerate() {
            let height = start_height.saturating_add(i);
            let msg = BlockAtHeight { height, block };

            if sender.send(msg).await.is_err() {
                return None;
            }
        }

        Some(())
    }
}

mod scanner {
    use monero_oxide_wallet::{GuaranteedScanner, GuaranteedViewPair, WalletOutput};

    use super::BlockAtHeight;

    pub(super) async fn run(
        view_pair: GuaranteedViewPair,
        mut blocks_receiver: tokio::sync::mpsc::Receiver<BlockAtHeight>,
        outputs_sender: tokio::sync::mpsc::UnboundedSender<WalletOutput>,
    ) {
        let mut scanner = GuaranteedScanner::new(view_pair);

        while !outputs_sender.is_closed() {
            let Some(BlockAtHeight { height, block }) = blocks_receiver.recv().await else {
                return;
            };

            let outputs = match scanner.scan(block) {
                Ok(outputs) => outputs,
                Err(err) => {
                    tracing::warn!(error = ?err, height, "Failed to scan block");
                    return;
                }
            };

            let outputs = outputs.ignore_additional_timelock();

            tracing::trace!(found_outputs = outputs.len(), height, "Scanned block");

            if send_outputs(&outputs_sender, outputs).is_none() {
                return;
            }
        }
    }

    fn send_outputs(
        sender: &tokio::sync::mpsc::UnboundedSender<WalletOutput>,
        outputs: Vec<WalletOutput>,
    ) -> Option<()> {
        for output in outputs {
            if sender.send(output).is_err() {
                return None;
            }
        }

        Some(())
    }
}
