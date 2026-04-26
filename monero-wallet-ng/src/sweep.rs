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
use monero_oxide::ed25519::Scalar;
use monero_oxide::ringct::RctType;
use monero_oxide_wallet::address::MoneroAddress;
use monero_oxide_wallet::send::{Change, SendError, SignableTransaction};
use monero_oxide_wallet::transaction::{NotPruned, Transaction};
use monero_oxide_wallet::{OutputWithDecoys, Scanner, ViewPair, ViewPairError};

use crate::rpc::{ProvidesTransactionStatus, TransactionStatus, TransactionStatusError};
use crate::util::public_key;
use crate::{MAX_FEE_PER_WEIGHT, RING_LEN};

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

/// An error while building and signing a sweep transaction.
///
/// Everything [`build_sweep_transaction`] can fail with
#[derive(Debug, thiserror::Error)]
pub enum BuildSweepError {
    #[error(transparent)]
    Destinations(#[from] DestinationsError),
    #[error("Necessary fee {fee} exceeds input amount {input}")]
    FeeExceedsInput { fee: u64, input: u64 },
    #[error("Final fee {actual} differs from probed fee {probed}")]
    FeeMismatch { probed: u64, actual: u64 },
    #[error("Failed to build transaction: {0}")]
    Send(#[from] SendError),
}

/// An error while sweeping a transaction.
///
/// Everything [`sweep_tx_to`] can fail with: build errors, transaction-status
/// lookup failures, and the I/O, scanning, and publishing errors that come
/// from interacting with the daemon.
#[derive(Debug, thiserror::Error)]
pub enum SweepError {
    #[error(transparent)]
    Build(#[from] BuildSweepError),
    #[error("Failed to create view pair: {0}")]
    ViewPair(#[from] ViewPairError),
    #[error("Scan error: {0}")]
    Scan(#[from] monero_oxide_wallet::ScanError),
    #[error("Failed to look up transaction status: {0}")]
    StatusLookup(#[from] TransactionStatusError),
    #[error("Transaction {} is unknown to the daemon", hex::encode(.tx_id))]
    TransactionNotFound { tx_id: [u8; 32] },
    #[error(
        "Transaction {} is still in the mempool; cannot sweep until it is mined",
        hex::encode(.tx_id)
    )]
    TransactionInMempool { tx_id: [u8; 32] },
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

/// The outcome of a successful sweep: the published transaction and its hash.
#[derive(Debug, Clone)]
pub struct SweepResult {
    /// The 32-byte hash of the published transaction.
    pub tx_hash: [u8; 32],
    /// The signed transaction that was published.
    pub tx: Transaction<NotPruned>,
}

/// Convenience wrapper around [`sweep_tx_to`] for the single-destination case.
///
/// Equivalent to `sweep_tx_to(..., vec![(destination, 1.0)])`.
pub async fn sweep_tx_to_single<P>(
    provider: P,
    private_spend_key: Zeroizing<Scalar>,
    private_view_key: Zeroizing<Scalar>,
    tx_id: [u8; 32],
    destination: MoneroAddress,
) -> Result<SweepResult, SweepError>
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
    sweep_tx_to(
        provider,
        private_spend_key,
        private_view_key,
        tx_id,
        vec![(destination, 1.0)],
    )
    .await
}

/// Locate `tx_id` on-chain and sweep it largest by the private key spendable output across `destinations`.
///
/// Looks up which block contains `tx_id` via the provider, scans that block
/// for outputs belonging to `tx_id`, selects the largest output,
/// and sweeps it across `destinations` split by ratio.
pub async fn sweep_tx_to<P>(
    provider: P,
    private_spend_key: Zeroizing<Scalar>,
    private_view_key: Zeroizing<Scalar>,
    tx_id: [u8; 32],
    destinations: Vec<(MoneroAddress, f64)>,
) -> Result<SweepResult, SweepError>
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
    if destinations.is_empty() {
        return Err(BuildSweepError::from(DestinationsError::Empty).into());
    }

    // Locate the block that contains the transaction
    let block_height = match provider.transaction_status(tx_id).await? {
        TransactionStatus::InBlock { block_height } => block_height as usize,
        TransactionStatus::InPool => return Err(SweepError::TransactionInMempool { tx_id }),
        TransactionStatus::Unknown => return Err(SweepError::TransactionNotFound { tx_id }),
    };

    // Scanner for finding sweepable outputs
    let mut scanner = {
        let public_spend_key = public_key(&private_spend_key);
        let view_pair = ViewPair::new(public_spend_key, private_view_key.clone())?;

        Scanner::new(view_pair)
    };

    // Find the largest output belonging to the transaction
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
        .fee_rate(FeePriority::Normal, MAX_FEE_PER_WEIGHT)
        .await?;

    // Generate a random outgoing view key
    let mut outgoing_view_key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(outgoing_view_key.as_mut());

    let signed = build_sweep_transaction(
        input,
        &destinations,
        fee_rate,
        outgoing_view_key,
        &private_spend_key,
    )?;
    let tx_hash = signed.hash();
    provider.publish_transaction(&signed).await?;

    Ok(SweepResult {
        tx_hash,
        tx: signed,
    })
}

/// Build and sign a sweep transaction that spends `input` across `destinations`.
///
/// Probe-builds an identically-shaped tx with zero-value payments to read
/// `necessary_fee`. The fee depends only on the number of inputs/outputs and
/// the fee rate, not on the amount values.
///
/// Then it distributes `amount - fee` across destinations by ratio.
/// There is no change slot.
///
/// For len(destinations) = 1 an extra 0-amount payment to a freshly-derived burner address is
/// appended to satisfy Monero's consensus-level 2-output minimum
/// (<https://github.com/monero-project/monero/issues/5399>). This matches
/// wallet2's approach (`wallet2.cpp::create_transactions_from`).
///
/// The burner's keys are derived from `outgoing_view_key`.
fn build_sweep_transaction(
    input: OutputWithDecoys,
    destinations: &[(MoneroAddress, f64)],
    fee_rate: FeeRate,
    outgoing_view_key: Zeroizing<[u8; 32]>,
    private_spend_key: &Zeroizing<Scalar>,
) -> Result<Transaction<NotPruned>, BuildSweepError> {
    const TX_TYPE: RctType = RctType::ClsagBulletproofPlus;

    let amount = input.commitment().amount;

    // If there is only one destination, add a single burner payment address
    let burner = if destinations.len() == 1 {
        Some(burner_address(
            destinations[0].0.network(),
            &outgoing_view_key,
        ))
    } else {
        None
    };

    // Probe the fee by building an identically-shaped tx with zero-value
    // payments. The fee depends only on input/output count and fee rate, not
    // on amount values, so zero-valued payments give the same fee as the real
    // ones will.
    let probed_necessary_fee = {
        let mut probe_payments = distribute(0, destinations)?;

        // Add the burner address
        if let Some(burner) = burner {
            probe_payments.push((burner, 0));
        }

        let probe = SignableTransaction::new(
            TX_TYPE,
            outgoing_view_key.clone(),
            vec![input.clone()],
            probe_payments,
            Change::fingerprintable(None),
            vec![],
            fee_rate,
        )?;

        probe.necessary_fee()
    };

    // How much is left to distribute after paying the necessary fee?
    let distributable =
        amount
            .checked_sub(probed_necessary_fee)
            .ok_or(BuildSweepError::FeeExceedsInput {
                fee: probed_necessary_fee,
                input: amount,
            })?;

    let tx = {
        // Distribute the distributable amount across the destinations
        let mut payments = distribute(distributable, destinations)?;

        // Add the burner address
        if let Some(burner) = burner {
            payments.push((burner, 0));
        }

        SignableTransaction::new(
            TX_TYPE,
            outgoing_view_key,
            vec![input],
            payments,
            // We don't need a change because we spread the
            // entire input amount across the destinations
            Change::fingerprintable(None),
            vec![],
            fee_rate,
        )
    }?;

    let signed = tx.sign(&mut OsRng, private_spend_key)?;

    // Assert that we did not accidentally pay more than the necessary fee
    // If the fee is higher than what is necessary, we made a mistake in the distribution.
    {
        let actual_fee = {
            let Transaction::V2 {
                proofs: Some(proofs),
                ..
            } = &signed
            else {
                unreachable!("sweep transactions are RingCT v2 transactions");
            };
            proofs.base.fee
        };

        if actual_fee != probed_necessary_fee {
            return Err(BuildSweepError::FeeMismatch {
                probed: probed_necessary_fee,
                actual: actual_fee,
            });
        }
    }

    Ok(signed)
}

/// Deterministically derive a burner address from `outgoing_view_key`.
///
/// The keys are discarded after this call — nobody retains them, so the
/// 0-amount output to this address is unspendable in practice.
fn burner_address(
    network: monero_oxide_wallet::address::Network,
    outgoing_view_key: &[u8; 32],
) -> MoneroAddress {
    let mut spend_seed = [0u8; 32 + 18];
    spend_seed[..32].copy_from_slice(outgoing_view_key);
    spend_seed[32..].copy_from_slice(b"sweep-burner-spend");
    let mut view_seed = [0u8; 32 + 17];
    view_seed[..32].copy_from_slice(outgoing_view_key);
    view_seed[32..].copy_from_slice(b"sweep-burner-view");

    let spend = Scalar::hash(spend_seed);
    let view = Scalar::hash(view_seed);

    MoneroAddress::new(
        network,
        monero_oxide_wallet::address::AddressType::Legacy,
        public_key(&spend),
        public_key(&view),
    )
}

/// Distribute `total` piconero across `destinations` by ratio (ratios summing to ~1.0).
///
/// The first n-1 destinations get `floor(total * ratio)` and the last destination
/// absorbs the remainder so the sum of allocated amounts is exactly `total`.
fn distribute(
    total: u64,
    destinations: &[(MoneroAddress, f64)],
) -> Result<Vec<(MoneroAddress, u64)>, DestinationsError> {
    if destinations.is_empty() {
        return Err(DestinationsError::Empty);
    }

    // Assert that the ratios sum to 1.0
    const RATIO_SUM_TOLERANCE: f64 = 1e-6;
    let sum: f64 = destinations.iter().map(|(_, r)| *r).sum();
    if (sum - 1.0).abs() > RATIO_SUM_TOLERANCE {
        return Err(DestinationsError::RatiosDontSumToOne { sum });
    }

    // First n-1 destinations
    let mut allocations = Vec::with_capacity(destinations.len());
    let mut assigned: u64 = 0;
    for (addr, ratio) in &destinations[..destinations.len() - 1] {
        let amount = ((total as f64) * ratio).floor() as u64;
        allocations.push((*addr, amount));
        assigned = assigned
            .checked_add(amount)
            .ok_or(DestinationsError::Overflow)?;
    }

    // Last destination gets the remainder
    let remainder = total
        .checked_sub(assigned)
        .ok_or(DestinationsError::Overflow)?;
    let (last_addr, _) = destinations[destinations.len() - 1];
    allocations.push((last_addr, remainder));

    Ok(allocations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monero_oxide::ed25519::Point;
    use monero_oxide_wallet::address::{AddressType, Network};

    /// A distinct dummy mainnet legacy address derived from a `seed`.
    /// Addresses are not compared by value in any test; we only need them to
    /// be structurally valid and differ between slots.
    fn addr(seed: u8) -> MoneroAddress {
        use curve25519_dalek::Scalar as DalekScalar;
        let spend_scalar = DalekScalar::from(u64::from(seed).wrapping_add(1));
        let view_scalar = DalekScalar::from(u64::from(seed).wrapping_add(101));
        let spend =
            Point::from(curve25519_dalek::constants::ED25519_BASEPOINT_POINT * spend_scalar);
        let view = Point::from(curve25519_dalek::constants::ED25519_BASEPOINT_POINT * view_scalar);
        MoneroAddress::new(Network::Mainnet, AddressType::Legacy, spend, view)
    }

    #[test]
    fn single_destination_gets_everything() {
        let a = addr(0);
        let out = distribute(1_000, &[(a, 1.0)]).unwrap();
        assert_eq!(out, vec![(a, 1_000)]);
    }

    #[test]
    fn even_split_divides_evenly() {
        let (a, b) = (addr(0), addr(1));
        let out = distribute(1_000, &[(a, 0.5), (b, 0.5)]).unwrap();
        assert_eq!(out, vec![(a, 500), (b, 500)]);
    }

    #[test]
    fn ratios_must_sum_to_one() {
        let (a, b) = (addr(0), addr(1));
        let err = distribute(1_000, &[(a, 0.5), (b, 0.4)]).unwrap_err();
        assert!(matches!(err, DestinationsError::RatiosDontSumToOne { .. }));
    }

    #[test]
    fn empty_destinations_rejected() {
        let err = distribute(1_000, &[]).unwrap_err();
        assert!(matches!(err, DestinationsError::Empty));
    }

    #[test]
    fn zero_total_assigns_zero_to_every_destination() {
        let dests = [
            (addr(0), 0.25),
            (addr(1), 0.25),
            (addr(2), 0.25),
            (addr(3), 0.25),
        ];
        let out = distribute(0, &dests).unwrap();
        assert_eq!(
            out,
            vec![(addr(0), 0), (addr(1), 0), (addr(2), 0), (addr(3), 0)]
        );
    }

    #[test]
    fn remainder_absorbed_by_last_destination() {
        // 10 * 1/3 floors to 3 for the first two slots; last slot absorbs
        // 10 - 3 - 3 = 4 so the sum equals total exactly.
        let third = 1.0 / 3.0;
        let (a, b, c) = (addr(0), addr(1), addr(2));
        let out = distribute(10, &[(a, third), (b, third), (c, third)]).unwrap();
        assert_eq!(out, vec![(a, 3), (b, 3), (c, 4)]);
    }
}
