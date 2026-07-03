use monero_interface::{InterfaceError, ProvidesBlockchainMeta, ProvidesScannableBlocks};
use monero_oxide::ed25519::{Point, Scalar};
use monero_oxide_wallet::{ScanError, Scanner, ViewPair, ViewPairError};
use zeroize::Zeroizing;

use crate::retry::with_retry;
use crate::rpc::{MempoolTransactionsError, ProvidesMempoolTransactions};
use crate::util::create_scannable_block_for_tx;

const BLOCKS_PER_BATCH: usize = 10;
const MEMPOOL_TXS_PER_BATCH: usize = 100;

#[derive(Debug, thiserror::Error)]
pub enum EmptyError {
    #[error(
        "Invalid block range: start height {start_height} is greater than end height {end_height}"
    )]
    InvalidRange {
        start_height: usize,
        end_height: usize,
    },
    #[error("Interface error: {0}")]
    Interface(#[from] InterfaceError),
    #[error("Mempool transaction error: {0}")]
    Mempool(#[from] MempoolTransactionsError),
    #[error("Scan error: {0}")]
    Scan(#[from] ScanError),
    #[error("Failed to create view pair: {0}")]
    ViewPair(#[from] ViewPairError),
}

pub async fn has_received_outputs<P>(
    provider: &P,
    public_spend_key: Point,
    private_view_key: Zeroizing<Scalar>,
    start_height: usize,
    inner_retry: Option<backoff::ExponentialBackoff>,
) -> Result<bool, EmptyError>
where
    P: ProvidesBlockchainMeta + ProvidesScannableBlocks + ProvidesMempoolTransactions,
{
    let end_height = with_retry(
        inner_retry.clone(),
        "Received-output latest-block-number lookup",
        || async { provider.latest_block_number().await },
    )
    .await?;

    if start_height > end_height {
        return Err(EmptyError::InvalidRange {
            start_height,
            end_height,
        });
    }

    let view_pair = ViewPair::new(public_spend_key, private_view_key)?;
    let mut scanner = Scanner::new(view_pair);
    let mut next_height = start_height;

    while next_height <= end_height {
        let end = next_height
            .saturating_add(BLOCKS_PER_BATCH.saturating_sub(1))
            .min(end_height);

        let blocks = with_retry(
            inner_retry.clone(),
            "Received-output scannable-block fetch",
            || async {
                provider
                    .contiguous_scannable_blocks(next_height..=end)
                    .await
            },
        )
        .await?;

        for block in blocks {
            if !scanner.scan(block)?.ignore_additional_timelock().is_empty() {
                return Ok(true);
            }
        }

        if end == end_height {
            break;
        }

        next_height = end + 1;
    }

    let mempool_tx_hashes = with_retry(
        inner_retry.clone(),
        "Received-output mempool transaction hash fetch",
        || async { provider.mempool_transaction_hashes().await },
    )
    .await?;

    for batch in mempool_tx_hashes.chunks(MEMPOOL_TXS_PER_BATCH) {
        let mempool_txs = with_retry(
            inner_retry.clone(),
            "Received-output mempool transaction fetch",
            || async { provider.mempool_transactions(batch).await },
        )
        .await?;

        for mempool_tx in mempool_txs {
            let block = create_scannable_block_for_tx(mempool_tx.tx_id, mempool_tx.tx);
            if !scanner.scan(block)?.ignore_additional_timelock().is_empty() {
                return Ok(true);
            }
        }
    }

    Ok(false)
}
