use zeroize::Zeroizing;

use monero_oxide_wallet::block::{Block, BlockHeader};
use monero_oxide_wallet::ed25519::{Point, Scalar};
use monero_oxide_wallet::interface::{ProvidesTransactions, ScannableBlock, TransactionsError};
use monero_oxide_wallet::transaction::{Input, Pruned, Timelock, Transaction, TransactionPrefix};
use monero_oxide_wallet::{Scanner, ViewPair};

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("Transaction error: {0}")]
    Transaction(#[from] TransactionsError),
    #[error("Scan error: {0}")]
    Scan(#[from] monero_oxide_wallet::ScanError),
    #[error("Failed to create view pair: {0}")]
    ViewPair(#[from] monero_oxide_wallet::ViewPairError),
}

/// Verify that a transaction sends the expected amount to the given view pair.
///
/// This function fetches the transaction from the RPC, scans it using the view pair,
/// and returns `true` if the transaction contains any single output to this view pair with the expected amount.
///
/// # Arguments
/// * `provider` - A provider implementing the `ProvidesTransactions` trait
/// * `tx_id` - The transaction ID (hash)
/// * `public_spend_key` - The public spend key of the receiving wallet
/// * `private_view_key` - The private view key of the receiving wallet
/// * `expected_amount` - The expected amount in piconero
///
/// # Returns
/// * `Ok(true)` if the transaction contains any single output to this view pair with the expected amount
/// * `Ok(false)` if the amounts don't match or no outputs were found
/// * `Err(...)` if there was an error fetching or scanning the transaction
///
/// Note: This doesn't register any subaddresses which means it will only detect outputs that are sent to the primary addres of the wallet.
pub async fn verify_transfer<P: ProvidesTransactions>(
    provider: &P,
    tx_id: [u8; 32],
    public_spend_key: Point,
    private_view_key: Zeroizing<Scalar>,
    expected_amount: u64,
) -> Result<bool, VerifyError> {
    // Fetch the transaction
    let tx: Transaction<Pruned> = provider.pruned_transaction(tx_id).await?;

    // Create the view pair
    let view_pair = ViewPair::new(public_spend_key, private_view_key)?;

    // Create a scanner
    let mut scanner = Scanner::new(view_pair);

    // Create a fake ScannableBlock containing with just this transaction.
    // The output_index_for_first_ringct_output is garbage (0) but we don't care
    // since we're only verifying amounts, not spending.
    let scannable_block = create_scannable_block_for_tx(tx_id, tx);

    // Scan the block
    let outputs = scanner.scan(scannable_block)?;

    // Check if any of the outputs have the expected amount
    let has_expected_amount_output = outputs
        .ignore_additional_timelock()
        .iter()
        .map(|output| output.commitment().amount)
        .any(|amount| amount == expected_amount);

    Ok(has_expected_amount_output)
}

/// Create a fake ScannableBlock containing a single transaction.
///
/// This is a workaround since monero-oxide's Scanner only scans blocks, not individual
/// transactions.
fn create_scannable_block_for_tx(tx_id: [u8; 32], tx: Transaction<Pruned>) -> ScannableBlock {
    // Fake miner transaction
    let miner_tx = Transaction::V1 {
        prefix: TransactionPrefix {
            additional_timelock: Timelock::None,
            inputs: vec![Input::Gen(0)],
            outputs: vec![],
            extra: vec![],
        },
        signatures: Vec::new(),
    };

    // Fake block header
    let header = BlockHeader {
        hardfork_version: 16,
        hardfork_signal: 0,
        timestamp: 0,
        previous: [0u8; 32],
        nonce: 0,
    };

    // Create the block with our transaction (and the fake miner transaction)
    let block = Block::new(header, miner_tx, vec![tx_id])
        .expect("Block creation should succeed with valid miner tx");

    ScannableBlock {
        block,
        transactions: vec![tx],
        output_index_for_first_ringct_output: Some(0),
    }
}
