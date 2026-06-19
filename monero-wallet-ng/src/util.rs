use monero_oxide::ed25519::{Point, Scalar};
use monero_oxide_wallet::transaction::{NotPruned, Transaction};

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
