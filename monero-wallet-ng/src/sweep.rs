//! Sweep funds from a wallet to an external address.
//!
//! Scans a single block for outputs belonging to a wallet, finds the largest output,
//! and sweeps it across a set of destinations split by ratio.

use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use monero_interface::{
    FeePriority, FeeRate, ProvidesBlockchainMeta, ProvidesDecoys, ProvidesFeeRates,
    ProvidesScannableBlocks, PublishTransaction,
};
use monero_oxide::ed25519::{Point, Scalar};
use monero_oxide::ringct::RctType;
use monero_oxide_wallet::address::MoneroAddress;
use monero_oxide_wallet::send::{Change, SendError, SignableTransaction};
use monero_oxide_wallet::{OutputWithDecoys, Scanner, ViewPair, ViewPairError};

use crate::RING_LEN;
use crate::rpc::{ProvidesTransactionStatus, TransactionStatus, TransactionStatusError};

fn public_key(private_key: &Scalar) -> Point {
    Point::from(curve25519_dalek::constants::ED25519_BASEPOINT_POINT * (*private_key).into())
}

const RATIO_SUM_TOLERANCE: f64 = 1e-6;

/// The caller-supplied set of destinations is invalid.
#[derive(Debug, thiserror::Error)]
pub enum DestinationsError {
    #[error("No destinations provided")]
    Empty,
    #[error("Ratios must sum to 1 (got {sum})")]
    RatiosDontSumToOne { sum: f64 },
    #[error("More destinations ({destinations}) than piconero to distribute ({total})")]
    TooMany { total: u64, destinations: usize },
    #[error("Overflow while computing distribution")]
    Overflow,
}

/// An error while building an unsigned sweep transaction.
///
/// Everything [`build_sweep_transaction`] can fail with
#[derive(Debug, thiserror::Error)]
pub enum BuildSweepError {
    #[error(transparent)]
    Destinations(#[from] DestinationsError),
    #[error("Necessary fee {fee} exceeds input amount {input}")]
    FeeExceedsInput { fee: u64, input: u64 },
    #[error("Failed to build transaction: {0}")]
    Send(#[from] SendError),
}

/// An error while sweeping a known block.
///
/// Everything [`sweep_tx_given_block_to`] can fail with: build errors plus
/// the I/O, scanning, and publishing errors that come from interacting with
/// the daemon.
#[derive(Debug, thiserror::Error)]
pub enum SweepError {
    #[error(transparent)]
    Build(#[from] BuildSweepError),
    #[error("Failed to create view pair: {0}")]
    ViewPair(#[from] ViewPairError),
    #[error("Scan error: {0}")]
    Scan(#[from] monero_oxide_wallet::ScanError),
    #[error("Block at height {block_height} does not exist")]
    BlockNotFound { block_height: usize },
    #[error(
        "No outputs belonging to the provided view-pair were found in transaction {}",
        hex::encode(.tx_id)
    )]
    NoOutputsInTransaction { tx_id: [u8; 32] },
    #[error("Fee error: {0}")]
    Fee(#[from] monero_interface::FeeError),
    #[error("Decoy selection error: {0}")]
    Decoys(#[from] monero_interface::TransactionsError),
    #[error("Publish error: {0}")]
    Publish(#[from] monero_interface::PublishTransactionError),
    #[error("Interface error: {0}")]
    Interface(#[from] monero_interface::InterfaceError),
}

/// An error while looking up a transaction and sweeping it.
///
/// Everything [`sweep_tx_to`] can fail with: the lookup failure modes plus
/// anything [`SweepError`] can represent.
#[derive(Debug, thiserror::Error)]
pub enum SweepTxError {
    #[error(transparent)]
    Sweep(#[from] SweepError),
    #[error("Failed to look up transaction status: {0}")]
    StatusLookup(#[from] TransactionStatusError),
    #[error("Transaction {} is unknown to the daemon", hex::encode(.tx_id))]
    TransactionNotFound { tx_id: [u8; 32] },
    #[error(
        "Transaction {} is still in the mempool; cannot sweep until it is mined",
        hex::encode(.tx_id)
    )]
    TransactionInMempool { tx_id: [u8; 32] },
}

/// Build an unsigned sweep transaction that spends `input` across `destinations`.
///
/// Pure: same inputs (including `outgoing_view_key`) produce the same
/// `SignableTransaction`. Performs no I/O, no randomness, no network access.
///
/// - n = 1: splits the input into two halves to satisfy Monero's consensus-level
///   2-output minimum (<https://github.com/monero-project/monero/issues/5399>).
///   One payment of `amount / 2` goes to the destination and the remainder
///   (minus fee) is routed to the same destination via the change slot, using
///   a fingerprintable change address since we do not hold the view key.
/// - n >= 2: probe-builds an identically-shaped tx with zero-value payments to
///   read `necessary_fee` (the fee depends only on the number of inputs/outputs
///   and the fee rate, not on the amount values), distributes `amount - fee`
///   across destinations by ratio (mirroring `monero-sys::FfiWallet::sweep_multi`),
///   and encodes every destination as an explicit payment. There is no change
///   slot: `Change::fingerprintable(None)` shunts the leftover (`input -
///   sum(payments)`) to the fee, which by construction equals `necessary_fee`.
fn build_sweep_transaction(
    input: OutputWithDecoys,
    destinations: &[(MoneroAddress, f64)],
    fee_rate: FeeRate,
    outgoing_view_key: Zeroizing<[u8; 32]>,
) -> Result<SignableTransaction, BuildSweepError> {
    let amount = input.commitment().amount;

    let (payments, change) = if destinations.len() == 1 {
        let (addr, _) = destinations[0];
        (
            vec![(addr, amount / 2)],
            Change::fingerprintable(Some(addr)),
        )
    } else {
        // Probe the fee by building an identically-shaped tx with zero-value
        // payments. The fee depends only on input/output count and fee rate,
        // not amount values, so zero-valued payments give the same fee as
        // the real ones will. `Change::fingerprintable(None)` shunts any
        // leftover to the fee; with all-zero payments the "leftover" is
        // `input - 0 = input`, which `validate` accepts since `input >=
        // necessary_fee` for any realistic sweep.
        let probe_payments: Vec<(MoneroAddress, u64)> =
            destinations.iter().map(|(addr, _)| (*addr, 0u64)).collect();
        let probe = SignableTransaction::new(
            RctType::ClsagBulletproofPlus,
            outgoing_view_key.clone(),
            vec![input.clone()],
            probe_payments,
            Change::fingerprintable(None),
            vec![],
            fee_rate,
        )?;
        let necessary_fee = probe.necessary_fee();

        let distributable =
            amount
                .checked_sub(necessary_fee)
                .ok_or(BuildSweepError::FeeExceedsInput {
                    fee: necessary_fee,
                    input: amount,
                })?;

        let ratios: Vec<f64> = destinations.iter().map(|(_, r)| *r).collect();
        let amounts = distribute(distributable, &ratios)?;

        // All destinations are explicit payments summing to `distributable
        // = amount - necessary_fee`. With `Change::fingerprintable(None)`,
        // monero-oxide shunts `input - sum(payments) = necessary_fee` into
        // the fee, so the paid fee is exactly what the probe reported.
        let payments: Vec<(MoneroAddress, u64)> = destinations
            .iter()
            .zip(amounts.iter())
            .map(|((addr, _), amount)| (*addr, *amount))
            .collect();
        (payments, Change::fingerprintable(None))
    };

    Ok(SignableTransaction::new(
        RctType::ClsagBulletproofPlus,
        outgoing_view_key,
        vec![input],
        payments,
        change,
        vec![],
        fee_rate,
    )?)
}

/// Locate `tx_id` on-chain and sweep its output across `destinations`.
///
/// Looks up which block contains `tx_id` via the provider and then delegates
/// to [`sweep_tx_given_block_to`]. Returns a [`SweepTxError`] if the daemon
/// does not know about the transaction or if it is still in the mempool.
pub async fn sweep_tx_to<P>(
    provider: P,
    private_spend_key: Zeroizing<Scalar>,
    private_view_key: Zeroizing<Scalar>,
    tx_id: [u8; 32],
    destinations: Vec<(MoneroAddress, f64)>,
    max_fee_per_weight: u64,
) -> Result<[u8; 32], SweepTxError>
where
    P: ProvidesScannableBlocks
        + ProvidesBlockchainMeta
        + ProvidesDecoys
        + ProvidesFeeRates
        + PublishTransaction
        + ProvidesTransactionStatus
        + Send
        + Sync,
{
    let block_height = match provider.transaction_status(tx_id).await? {
        TransactionStatus::InBlock { block_height } => block_height as usize,
        TransactionStatus::InPool => return Err(SweepTxError::TransactionInMempool { tx_id }),
        TransactionStatus::Unknown => return Err(SweepTxError::TransactionNotFound { tx_id }),
    };

    Ok(sweep_tx_given_block_to(
        provider,
        private_spend_key,
        private_view_key,
        tx_id,
        block_height,
        destinations,
        max_fee_per_weight,
    )
    .await?)
}

/// Sweep the largest output belonging to the caller's view-pair from
/// transaction `tx_id` at `block_height` across `destinations`, split by the
/// associated ratios.
///
/// Only outputs from `tx_id` itself are considered — any other outputs in the
/// same block that happen to also belong to the view-pair are ignored.
///
/// The caller is responsible for locating the block; see [`sweep_tx_to`] for
/// a wrapper that does the lookup via the daemon RPC.
pub async fn sweep_tx_given_block_to<P>(
    provider: P,
    private_spend_key: Zeroizing<Scalar>,
    private_view_key: Zeroizing<Scalar>,
    tx_id: [u8; 32],
    block_height: usize,
    destinations: Vec<(MoneroAddress, f64)>,
    max_fee_per_weight: u64,
) -> Result<[u8; 32], SweepError>
where
    P: ProvidesScannableBlocks
        + ProvidesBlockchainMeta
        + ProvidesDecoys
        + ProvidesFeeRates
        + PublishTransaction
        + Send
        + Sync,
{
    if destinations.is_empty() {
        return Err(BuildSweepError::from(DestinationsError::Empty).into());
    }

    // Scanner for finding sweepable outputs
    let mut scanner = {
        let public_spend_key = public_key(&private_spend_key);
        let view_pair = ViewPair::new(public_spend_key, private_view_key.clone())?;

        Scanner::new(view_pair)
    };

    // Find the largest output belonging to `tx_id`.
    let largest_output = {
        let blocks = provider
            .contiguous_scannable_blocks(block_height..=block_height)
            .await?;
        let block = blocks
            .into_iter()
            .next()
            .ok_or(SweepError::BlockNotFound { block_height })?;
        let outputs = scanner.scan(block)?.not_additionally_locked();

        outputs
            .into_iter()
            .filter(|o| o.transaction() == tx_id)
            .max_by_key(|o| o.commitment().amount)
            .ok_or(SweepError::NoOutputsInTransaction { tx_id })?
    };

    // Generate decoys for the input
    let block_number = provider.latest_block_number().await?;
    let input = OutputWithDecoys::new(
        &mut OsRng,
        &provider,
        RING_LEN,
        block_number,
        largest_output,
    )
    .await?;

    // Get the fee rate
    let fee_rate = provider
        .fee_rate(FeePriority::Normal, max_fee_per_weight)
        .await?;

    // Generate a random outgoing view key
    let mut outgoing_view_key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(outgoing_view_key.as_mut());

    let tx = build_sweep_transaction(input, &destinations, fee_rate, outgoing_view_key)?;

    let signed = tx
        .sign(&mut OsRng, &private_spend_key)
        .map_err(BuildSweepError::from)?;
    let tx_hash = signed.hash();
    provider.publish_transaction(&signed).await?;

    Ok(tx_hash)
}

/// Distribute `total` piconero across `ratios` (summing to ~1.0).
///
/// Mirrors `FfiWallet::distribute` from `monero-sys`: the first n-1 slots are
/// `floor(total * ratio)` and the last slot absorbs the remainder so the sum
/// is exactly `total`.
fn distribute(total: u64, ratios: &[f64]) -> Result<Vec<u64>, DestinationsError> {
    if ratios.is_empty() {
        return Err(DestinationsError::Empty);
    }

    // Assert that the ratios sum to 1.0
    let sum: f64 = ratios.iter().sum();
    if (sum - 1.0).abs() > RATIO_SUM_TOLERANCE {
        return Err(DestinationsError::RatiosDontSumToOne { sum });
    }

    // Check if the total is enough to cover at least one piconero per output
    if total < ratios.len() as u64 {
        return Err(DestinationsError::TooMany {
            total,
            destinations: ratios.len(),
        });
    }

    // First n-1 destinations
    let mut amounts = Vec::with_capacity(ratios.len());
    let mut assigned: u64 = 0;
    for &r in &ratios[..ratios.len() - 1] {
        let amount = ((total as f64) * r).floor() as u64;
        amounts.push(amount);
        assigned = assigned
            .checked_add(amount)
            .ok_or(DestinationsError::Overflow)?;
    }

    // Last destination gets the remainder
    let remainder = total
        .checked_sub(assigned)
        .ok_or(DestinationsError::Overflow)?;
    amounts.push(remainder);

    Ok(amounts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_destination_gets_everything() {
        let amounts = distribute(1_000, &[1.0]).unwrap();
        assert_eq!(amounts, vec![1_000]);
    }

    #[test]
    fn even_split_divides_evenly() {
        let amounts = distribute(1_000, &[0.5, 0.5]).unwrap();
        assert_eq!(amounts, vec![500, 500]);
    }

    #[test]
    fn ratios_must_sum_to_one() {
        let err = distribute(1_000, &[0.5, 0.4]).unwrap_err();
        assert!(matches!(
            err,
            DestinationsError::RatiosDontSumToOne { .. }
        ));
    }

    #[test]
    fn empty_destinations_rejected() {
        let err = distribute(1_000, &[]).unwrap_err();
        assert!(matches!(err, DestinationsError::Empty));
    }

    #[test]
    fn more_destinations_than_piconero_rejected() {
        let err = distribute(2, &[0.25, 0.25, 0.25, 0.25]).unwrap_err();
        assert!(matches!(err, DestinationsError::TooMany { .. }));
    }

    #[test]
    fn remainder_absorbed_by_last_destination() {
        // 10 * 1/3 floors to 3 for the first two slots; last slot absorbs
        // 10 - 3 - 3 = 4 so the sum equals total exactly.
        let third = 1.0 / 3.0;
        let amounts = distribute(10, &[third, third, third]).unwrap();
        assert_eq!(amounts, vec![3, 3, 4]);
    }
}
