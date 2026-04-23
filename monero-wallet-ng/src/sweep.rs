//! Sweep funds from a wallet to an external address.
//!
//! Scans a single block for outputs belonging to a wallet, finds the largest output,
//! and sweeps it across a set of destinations split by ratio.

use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use monero_interface::{
    FeePriority, ProvidesBlockchainMeta, ProvidesDecoys, ProvidesFeeRates, ProvidesScannableBlocks,
    PublishTransaction,
};
use monero_oxide::ed25519::{Point, Scalar};
use monero_oxide::ringct::RctType;
use monero_oxide_wallet::address::MoneroAddress;
use monero_oxide_wallet::send::{Change, SendError, SignableTransaction};
use monero_oxide_wallet::{OutputWithDecoys, Scanner, ViewPair, ViewPairError};

fn public_key(private_key: &Scalar) -> Point {
    Point::from(curve25519_dalek::constants::ED25519_BASEPOINT_POINT * (*private_key).into())
}

const RING_LEN: u8 = 16;
const RATIO_SUM_TOLERANCE: f64 = 1e-6;

#[derive(Debug, thiserror::Error)]
pub enum SweepError {
    #[error("Failed to create view pair: {0}")]
    ViewPair(#[from] ViewPairError),
    #[error("Scan error: {0}")]
    Scan(#[from] monero_oxide_wallet::ScanError),
    #[error("No outputs found to sweep")]
    NoOutputs,
    #[error("Transaction error: {0}")]
    Transaction(#[from] SendError),
    #[error("Fee error: {0}")]
    Fee(#[from] monero_interface::FeeError),
    #[error("Decoy selection error: {0}")]
    Decoys(#[from] monero_interface::TransactionsError),
    #[error("Publish error: {0}")]
    Publish(#[from] monero_interface::PublishTransactionError),
    #[error("Interface error: {0}")]
    Interface(#[from] monero_interface::InterfaceError),
    #[error("Invalid destinations: {0}")]
    InvalidDestinations(String),
}

/// Distribute `total` piconero across `ratios` (summing to ~1.0).
///
/// Mirrors `FfiWallet::distribute` from `monero-sys`: the first n-1 slots are
/// `floor(total * ratio)` and the last slot absorbs the remainder so the sum
/// is exactly `total`.
fn distribute(total: u64, ratios: &[f64]) -> Result<Vec<u64>, SweepError> {
    if ratios.is_empty() {
        return Err(SweepError::InvalidDestinations(
            "no destinations".to_string(),
        ));
    }
    let sum: f64 = ratios.iter().sum();
    if (sum - 1.0).abs() > RATIO_SUM_TOLERANCE {
        return Err(SweepError::InvalidDestinations(format!(
            "ratios must sum to 1 (got {})",
            sum
        )));
    }
    if total < ratios.len() as u64 {
        return Err(SweepError::InvalidDestinations(format!(
            "more destinations than piconero to distribute ({} < {})",
            total,
            ratios.len()
        )));
    }

    let mut amounts = Vec::with_capacity(ratios.len());
    let mut assigned: u64 = 0;
    for &r in &ratios[..ratios.len() - 1] {
        let amount = ((total as f64) * r).floor() as u64;
        amounts.push(amount);
        assigned = assigned
            .checked_add(amount)
            .ok_or_else(|| SweepError::InvalidDestinations("overflow".to_string()))?;
    }
    let remainder = total.checked_sub(assigned).ok_or_else(|| {
        SweepError::InvalidDestinations(format!(
            "underflow: total {} < assigned {}",
            total, assigned
        ))
    })?;
    amounts.push(remainder);
    Ok(amounts)
}

pub async fn sweep<P>(
    provider: P,
    private_spend_key: Zeroizing<Scalar>,
    private_view_key: Zeroizing<Scalar>,
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
        return Err(SweepError::InvalidDestinations(
            "no destinations".to_string(),
        ));
    }

    let public_spend_key = public_key(&private_spend_key);
    let view_pair = ViewPair::new(public_spend_key, private_view_key.clone())?;
    let mut scanner = Scanner::new(view_pair);

    // Scan the given block and find the largest output
    let blocks = provider
        .contiguous_scannable_blocks(block_height..=block_height)
        .await?;
    let block = blocks.into_iter().next().ok_or(SweepError::NoOutputs)?;
    let outputs = scanner.scan(block)?.not_additionally_locked();
    let largest_output = outputs
        .into_iter()
        .max_by_key(|o| o.commitment().amount)
        .ok_or(SweepError::NoOutputs)?;

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
    let amount = input.commitment().amount;

    // Get the fee rate
    let fee_rate = provider
        .fee_rate(FeePriority::Normal, max_fee_per_weight)
        .await?;

    // Generate a random outgoing view key
    let mut outgoing_view_key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(outgoing_view_key.as_mut());

    let (payments, change) = if destinations.len() == 1 {
        // Single destination, 100% — split into two halves, with change going
        // back to the same destination. The Monero protocol enforces a
        // minimum of 2 outputs per transaction at consensus level:
        // https://github.com/monero-project/monero/issues/5399
        //
        // We create one payment output for the destination address with half
        // the amount of the output we are spending. Additionally, we set the
        // destination address as the change address such that `monero-oxide`
        // will sweep anything that is left over after subtracting the fee.
        //
        // As we do not have the view key for the destination address, we
        // must use a fingerprintable change address.
        let (addr, _) = destinations[0];
        (
            vec![(addr, amount / 2)],
            Change::fingerprintable(Some(addr)),
        )
    } else {
        // Multi-destination: split `amount - fee` across n destinations by
        // ratio. Mirrors `monero-sys::FfiWallet::sweep_multi`: the last
        // destination absorbs the remainder (and the fee, via the Change
        // slot).
        //
        // Probe the fee by building an identically-shaped tx with zero-value
        // payments. The fee depends only on the number of inputs/outputs and
        // the fee rate, not on the amount values — `weight_and_necessary_fee`
        // uses shimmed `[0; 8]` encrypted amounts regardless of the real
        // values. Validate() passes because `in_amount >= 0 + necessary_fee`
        // for any realistic sweep.
        let probe_payments: Vec<(MoneroAddress, u64)> = destinations
            .iter()
            .take(destinations.len() - 1)
            .map(|(addr, _)| (*addr, 0u64))
            .collect();
        let last_addr = destinations[destinations.len() - 1].0;
        let probe = SignableTransaction::new(
            RctType::ClsagBulletproofPlus,
            outgoing_view_key.clone(),
            vec![input.clone()],
            probe_payments,
            Change::fingerprintable(Some(last_addr)),
            vec![],
            fee_rate,
        )?;
        let necessary_fee = probe.necessary_fee();

        let distributable = amount.checked_sub(necessary_fee).ok_or_else(|| {
            SweepError::InvalidDestinations(format!(
                "fee {} exceeds input {}",
                necessary_fee, amount
            ))
        })?;

        let ratios: Vec<f64> = destinations.iter().map(|(_, r)| *r).collect();
        let amounts = distribute(distributable, &ratios)?;

        // First n-1 destinations become explicit payments; the last
        // destination becomes the change slot. `monero-oxide` will set the
        // change amount to `input - sum(payments) - fee` which equals exactly
        // `amounts[n-1]` (the remainder from `distribute`).
        let payments: Vec<(MoneroAddress, u64)> = destinations
            .iter()
            .take(destinations.len() - 1)
            .zip(amounts.iter())
            .map(|((addr, _), amount)| (*addr, *amount))
            .collect();
        (payments, Change::fingerprintable(Some(last_addr)))
    };

    let tx = SignableTransaction::new(
        RctType::ClsagBulletproofPlus,
        outgoing_view_key,
        vec![input],
        payments,
        change,
        vec![],
        fee_rate,
    )?;

    let signed = tx.sign(&mut OsRng, &private_spend_key)?;
    let tx_hash = signed.hash();
    provider.publish_transaction(&signed).await?;

    Ok(tx_hash)
}
