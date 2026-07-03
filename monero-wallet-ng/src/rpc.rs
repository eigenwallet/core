//! RPC-related traits and implementations for monero-oxide.
//!
//! This module provides additional traits that extend monero-oxide's functionality,
//! particularly for querying transaction status information that isn't exposed
//! by the standard traits.

use core::future::Future;

use monero_daemon_rpc::{HttpTransport, MoneroDaemon};
use monero_interface::InterfaceError;
use monero_oxide_wallet::transaction::{Pruned, Transaction};

/// Spend status of a single key image, per the daemon's `is_key_image_spent` RPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyImageSpentStatus {
    /// The key image has not been spent.
    Unspent,
    /// The key image was spent by a transaction confirmed in the blockchain.
    SpentInBlockchain,
    /// The key image was spent by a transaction currently in the mempool.
    SpentInPool,
}

/// Query the spend status of key images directly, without submitting a transaction.
///
/// Unlike the `double_spend` flag from `send_raw_transaction`, this distinguishes a
/// confirmed spend (`SpentInBlockchain`) from a transient pool spend (`SpentInPool`).
pub trait IsKeyImageSpent: Sync {
    /// Returns the spend status of each key image, in the same order as the input.
    fn is_key_image_spent(
        &self,
        key_images: &[[u8; 32]],
    ) -> impl Send + Future<Output = Result<Vec<KeyImageSpentStatus>, InterfaceError>>;
}

impl<T: HttpTransport> IsKeyImageSpent for MoneroDaemon<T> {
    fn is_key_image_spent(
        &self,
        key_images: &[[u8; 32]],
    ) -> impl Send + Future<Output = Result<Vec<KeyImageSpentStatus>, InterfaceError>> {
        let key_images_hex: Vec<String> = key_images.iter().map(hex::encode).collect();

        async move {
            #[derive(serde::Deserialize)]
            struct IsKeyImageSpentResponse {
                status: String,
                spent_status: Vec<u8>,
            }

            let params = serde_json::json!({ "key_images": key_images_hex }).to_string();

            let response = self
                .rpc_call(
                    "is_key_image_spent",
                    Some(params),
                    // The response is a small array of integers.
                    65536,
                )
                .await?;

            let response: IsKeyImageSpentResponse =
                serde_json::from_str(&response).map_err(|e| {
                    InterfaceError::InvalidInterface(format!(
                        "Failed to parse is_key_image_spent response: {e}"
                    ))
                })?;

            if response.status != "OK" {
                return Err(InterfaceError::InvalidInterface(format!(
                    "is_key_image_spent returned status {}",
                    response.status
                )));
            }

            response
                .spent_status
                .into_iter()
                .map(|status| match status {
                    0 => Ok(KeyImageSpentStatus::Unspent),
                    1 => Ok(KeyImageSpentStatus::SpentInBlockchain),
                    2 => Ok(KeyImageSpentStatus::SpentInPool),
                    other => Err(InterfaceError::InvalidInterface(format!(
                        "Unknown key image spent status {other}"
                    ))),
                })
                .collect()
        }
    }
}

const MEMPOOL_HASHES_RESPONSE_SIZE_LIMIT: usize = 2 * 1024 * 1024;
const NON_MINER_TX_SIZE_UPPER_BOUND: usize = 1_000_000;

#[derive(Debug, Clone)]
pub struct MempoolTransaction {
    pub tx_id: [u8; 32],
    pub tx: Transaction<Pruned>,
}

#[derive(Debug, thiserror::Error)]
pub enum TransactionStatusError {
    #[error("Interface error: {0}")]
    Interface(#[from] InterfaceError),
}

#[derive(Debug, thiserror::Error)]
pub enum MempoolTransactionsError {
    #[error("Interface error: {0}")]
    Interface(#[from] InterfaceError),
}

#[derive(Debug, Clone)]
pub enum TransactionStatus {
    Unknown, // the daemon does not know about the transaction
    InPool,  // the transaction is in the mempool
    InBlock {
        // the transaction is included in a block which the daemon believes is part of the longest chain
        block_height: u64,
    },
}

/// Provides the ability to query transaction status
///
/// This trait is separate from `ProvidesTransactions` because monero-oxide's
/// `ProvidesTransactions` doesn't currently expose block_height/in_pool fields.
pub trait ProvidesTransactionStatus: Sync {
    /// Get the status of a transaction by its hash.
    ///
    /// Returns `Ok(TransactionStatus)` if the transaction is found.
    /// Returns `Err(TransactionStatusError::TransactionNotFound)` if not found.
    fn transaction_status(
        &self,
        tx_id: [u8; 32],
    ) -> impl Send + Future<Output = Result<TransactionStatus, TransactionStatusError>>;
}

pub trait ProvidesMempoolTransactions: Sync {
    fn mempool_transaction_hashes(
        &self,
    ) -> impl Send + Future<Output = Result<Vec<[u8; 32]>, MempoolTransactionsError>>;

    fn mempool_transactions(
        &self,
        hashes: &[[u8; 32]],
    ) -> impl Send + Future<Output = Result<Vec<MempoolTransaction>, MempoolTransactionsError>>;
}

/// Data structures we get back from the RPC server
///
/// See: https://github.com/monero-project/monero/blob/48ad374b0d6d6e045128729534dc2508e6999afe/src/rpc/core_rpc_server_commands_defs.h#L358-L439
mod monerod {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub(crate) struct GetTransactionsResponse {
        #[serde(default)]
        pub(crate) missed_tx: Vec<String>,
        #[serde(default)]
        pub(crate) txs: Vec<TransactionInfo>,
    }

    // See: https://github.com/SNeedlewoods/seraphis_wallet/blob/dbbccecc89e1121762a4ad6b531638ece82aa0c7/src/rpc/core_rpc_server_commands_defs.h#L406-L428
    #[derive(Deserialize)]
    pub(crate) struct TransactionInfo {
        pub(crate) tx_hash: Option<String>,
        pub(crate) pruned_as_hex: Option<String>,
        // `block_height` is only present if `in_pool` is false
        pub(crate) block_height: Option<u64>,
        // `in_pool` is always present
        pub(crate) in_pool: bool,
    }

    #[derive(Deserialize)]
    pub(crate) struct GetTransactionPoolHashesResponse {
        #[serde(default)]
        pub(crate) tx_hashes: Vec<String>,
    }
}

impl<T: HttpTransport> ProvidesTransactionStatus for MoneroDaemon<T> {
    fn transaction_status(
        &self,
        tx_id: [u8; 32],
    ) -> impl Send + Future<Output = Result<TransactionStatus, TransactionStatusError>> {
        async move {
            let tx_hash_hex = hex::encode(tx_id);

            // Use rpc_call to get transaction
            let response = self
                .rpc_call(
                    "get_transactions",
                    Some(format!(r#"{{ "txs_hashes": ["{}"] }}"#, tx_hash_hex)),
                    // 64kb, fairly arbitrary, but should be enough
                    65536,
                )
                .await?;

            let response: monerod::GetTransactionsResponse = serde_json::from_str(&response)
                .map_err(|e| {
                    InterfaceError::InvalidInterface(format!("Failed to parse response: {}", e))
                })?;

            // Check if transaction was missed
            if !response.missed_tx.is_empty() {
                return Ok(TransactionStatus::Unknown);
            }

            // Get the transaction info
            let tx_info = response.txs.first().ok_or_else(|| {
                InterfaceError::InvalidInterface(
                    "No transaction info in response despite no missed_tx".to_string(),
                )
            })?;

            if tx_info.in_pool {
                return Ok(TransactionStatus::InPool);
            }

            Ok(TransactionStatus::InBlock {
                block_height: tx_info.block_height.ok_or_else(|| {
                    InterfaceError::InvalidInterface(
                        "Transaction has in_pool=false but has no block_height".to_string(),
                    )
                })?,
            })
        }
    }
}

impl<T: HttpTransport> ProvidesMempoolTransactions for MoneroDaemon<T> {
    fn mempool_transaction_hashes(
        &self,
    ) -> impl Send + Future<Output = Result<Vec<[u8; 32]>, MempoolTransactionsError>> {
        async move {
            let response = self
                .rpc_call(
                    "get_transaction_pool_hashes",
                    Some("{}".to_string()),
                    MEMPOOL_HASHES_RESPONSE_SIZE_LIMIT,
                )
                .await?;

            let response: monerod::GetTransactionPoolHashesResponse =
                serde_json::from_str(&response).map_err(|e| {
                    InterfaceError::InvalidInterface(format!(
                        "Failed to parse get_transaction_pool_hashes response: {}",
                        e
                    ))
                })?;

            response
                .tx_hashes
                .into_iter()
                .map(|hash| decode_hash(&hash))
                .collect::<Result<Vec<_>, _>>()
        }
    }

    fn mempool_transactions(
        &self,
        hashes: &[[u8; 32]],
    ) -> impl Send + Future<Output = Result<Vec<MempoolTransaction>, MempoolTransactionsError>>
    {
        async move {
            if hashes.is_empty() {
                return Ok(Vec::new());
            }

            let hashes_json = hashes
                .iter()
                .map(hex::encode)
                .map(|hash| format!(r#""{}""#, hash))
                .collect::<Vec<_>>()
                .join(",");
            let response_size_limit = hashes.len().saturating_mul(NON_MINER_TX_SIZE_UPPER_BOUND);

            let response = self
                .rpc_call(
                    "get_transactions",
                    Some(format!(
                        r#"{{ "txs_hashes": [{}], "prune": true }}"#,
                        hashes_json
                    )),
                    response_size_limit,
                )
                .await?;

            let response: monerod::GetTransactionsResponse = serde_json::from_str(&response)
                .map_err(|e| {
                    InterfaceError::InvalidInterface(format!(
                        "Failed to parse get_transactions response: {}",
                        e
                    ))
                })?;

            if !response.missed_tx.is_empty() {
                return Err(InterfaceError::InvalidInterface(format!(
                    "Daemon missed mempool transactions: {}",
                    response.missed_tx.join(", ")
                ))
                .into());
            }

            if response.txs.len() != hashes.len() {
                return Err(InterfaceError::InvalidInterface(
                    "Daemon returned an unexpected number of mempool transactions".to_string(),
                )
                .into());
            }

            response
                .txs
                .into_iter()
                .map(parse_pruned_mempool_transaction)
                .collect()
        }
    }
}

fn parse_pruned_mempool_transaction(
    info: monerod::TransactionInfo,
) -> Result<MempoolTransaction, MempoolTransactionsError> {
    let tx_id = decode_hash(info.tx_hash.as_deref().ok_or_else(|| {
        InterfaceError::InvalidInterface("Mempool transaction response missing tx_hash".to_string())
    })?)?;

    let blob = hex::decode(info.pruned_as_hex.as_deref().ok_or_else(|| {
        InterfaceError::InvalidInterface(
            "Mempool transaction response missing pruned_as_hex".to_string(),
        )
    })?)
    .map_err(|e| {
        InterfaceError::InvalidInterface(format!("Failed to decode mempool tx blob hex: {}", e))
    })?;

    let mut reader = blob.as_slice();
    let tx = Transaction::<Pruned>::read(&mut reader).map_err(|e| {
        InterfaceError::InvalidInterface(format!("Failed to parse mempool transaction: {}", e))
    })?;

    if !reader.is_empty() {
        return Err(InterfaceError::InvalidInterface(
            "Mempool transaction blob has trailing bytes".to_string(),
        )
        .into());
    }

    Ok(MempoolTransaction { tx_id, tx })
}

fn decode_hash(hash: &str) -> Result<[u8; 32], MempoolTransactionsError> {
    hex::decode(hash)
        .map_err(|e| {
            InterfaceError::InvalidInterface(format!("Failed to decode transaction hash: {}", e))
        })?
        .try_into()
        .map_err(|_| {
            InterfaceError::InvalidInterface("Transaction hash was not 32 bytes".to_string()).into()
        })
}
