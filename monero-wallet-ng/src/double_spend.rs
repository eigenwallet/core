//! Locate sufficiently confirmed conflicting inputs without wallet-output scanning.

use anyhow::{Context, Result, ensure};
use monero_interface::{ProvidesBlockchainMeta, ProvidesScannableBlocks, ScannableBlock};
use monero_oxide_wallet::transaction::Input;

const BLOCKS_PER_BATCH: usize = 10;

/// Scans from `restore_height` to the latest sufficiently confirmed block for a conflict; this can take a long time.
/// The caller must first establish that an input is blockchain-spent using the trusted daemon.
/// Absence from the searched range does not establish finality and returns false.
pub async fn has_confirmed_conflict<P>(
    provider: &P,
    original_tx: [u8; 32],
    key_images: &[[u8; 32]],
    restore_height: usize,
    required_confirmations: usize,
) -> Result<bool>
where
    P: ProvidesBlockchainMeta + ProvidesScannableBlocks,
{
    ensure!(
        required_confirmations > 0,
        "Rebuild confirmations must be positive"
    );
    let tip = provider.latest_block_number().await?;
    ensure!(
        restore_height <= tip,
        "Conflict-search restore height {restore_height} exceeds Monero chain tip {tip}"
    );
    let Some(last_eligible_height) = tip.checked_sub(required_confirmations - 1) else {
        // Too few blocks have elapsed for any conflict to have the required confirmations.
        return Ok(false);
    };

    let mut start = restore_height;
    let mut previous_hash = None;
    while start <= last_eligible_height {
        let end = start
            .saturating_add(BLOCKS_PER_BATCH - 1)
            .min(last_eligible_height);
        let blocks = provider.contiguous_scannable_blocks(start..=end).await?;
        ensure!(
            blocks.len() == end - start + 1,
            "Incomplete conflict-search block batch"
        );
        for (offset, block) in blocks.into_iter().enumerate() {
            let height = start + offset;
            ensure!(
                block.block.number() == height,
                "Unexpected conflict-search block height"
            );
            if let Some(previous_hash) = previous_hash {
                ensure!(
                    block.block.header.previous == previous_hash,
                    "Monero chain changed between conflict-search blocks"
                );
            }
            previous_hash = Some(block.block.hash());
            if !contains_conflict(&block, original_tx, key_images)? {
                continue;
            }

            // The search may take time: verify both depth and canonical identity again.
            let current_tip = provider.latest_block_number().await?;
            let current_depth = current_tip
                .checked_sub(height)
                .context("Monero chain moved below the conflicting block during verification")?;
            ensure!(
                current_depth >= required_confirmations - 1,
                "Monero conflicting spend lost the required confirmation depth during verification"
            );
            let canonical = provider
                .scannable_block_by_number(height)
                .await
                .context("Failed to recheck conflicting spend's canonical block")?;
            ensure!(
                canonical.block.hash() == block.block.hash(),
                "Monero conflicting spend's block changed during verification"
            );
            return Ok(true);
        }
        if end == last_eligible_height {
            break;
        }
        start = end + 1;
    }
    if let Some(last_hash) = previous_hash {
        let canonical = provider
            .scannable_block_by_number(last_eligible_height)
            .await
            .context("Failed to recheck conflict-search checkpoint")?;
        ensure!(
            canonical.block.hash() == last_hash,
            "Monero conflict-search checkpoint changed during verification"
        );
    }
    Ok(false)
}

fn contains_conflict(
    block: &ScannableBlock,
    original_tx: [u8; 32],
    key_images: &[[u8; 32]],
) -> Result<bool> {
    ensure!(
        block.block.transactions.len() == block.transactions.len(),
        "Incomplete conflict-search transaction list"
    );
    Ok(block.block.transactions.iter().zip(&block.transactions).any(|(hash, tx)| {
        *hash != original_tx && tx.prefix().inputs.iter().any(|input| {
            matches!(input, Input::ToKey { key_image, .. } if key_images.contains(&key_image.to_bytes()))
        })
    }))
}
