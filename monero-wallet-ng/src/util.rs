use monero_oxide::ed25519::{Point, Scalar};
use monero_oxide_wallet::block::{Block, BlockHeader};
use monero_oxide_wallet::interface::ScannableBlock;
use monero_oxide_wallet::transaction::{
    Input, NotPruned, Pruned, Timelock, Transaction, TransactionPrefix,
};

use crate::HARDFORK_VERSION;

/// Derive the Ed25519 public key for a private scalar.
pub fn public_key(private_key: &Scalar) -> Point {
    Point::from(curve25519_dalek::constants::ED25519_BASEPOINT_POINT * (*private_key).into())
}

#[derive(Debug, thiserror::Error)]
pub enum TransactionFromHexError {
    #[error("Transaction blob was not valid hex")]
    Hex(#[from] hex::FromHexError),
    #[error("Failed to deserialize transaction blob")]
    Read(#[from] std::io::Error),
}

pub fn transaction_from_hex(
    blob_hex: &str,
) -> Result<Transaction<NotPruned>, TransactionFromHexError> {
    let bytes = hex::decode(blob_hex)?;
    let tx = Transaction::read(&mut bytes.as_slice())?;
    Ok(tx)
}

/// Create a fake ScannableBlock containing a single transaction.
pub fn create_scannable_block_for_tx(
    txs_with_id: Vec<([u8; 32], Transaction<Pruned>)>,
) -> ScannableBlock {
    let (txids, txs) = txs_with_id.into_iter().unzip();

    let miner_tx = Transaction::V1 {
        prefix: TransactionPrefix {
            additional_timelock: Timelock::None,
            inputs: vec![Input::Gen(0)],
            outputs: vec![],
            extra: vec![],
        },
        signatures: Vec::new(),
    };

    let header = BlockHeader {
        hardfork_version: HARDFORK_VERSION,
        hardfork_signal: 0,
        timestamp: 0,
        previous: [0u8; 32],
        nonce: 0,
    };

    let block =
        Block::new(header, miner_tx, txids).expect("block creation to succeed with valid miner tx");

    ScannableBlock {
        block,
        transactions: txs,
        output_index_for_first_ringct_output: Some(0),
    }
}
