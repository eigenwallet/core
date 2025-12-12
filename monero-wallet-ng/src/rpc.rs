//! RPC-related traits and implementations for monero-oxide.
//!
//! This module provides additional traits that extend monero-oxide's functionality,
//! particularly for querying transaction status information that isn't exposed
//! by the standard traits.

use core::future::Future;

use monero_daemon_rpc::{HttpTransport, MoneroDaemon};
use monero_interface::InterfaceError;

#[derive(Debug, thiserror::Error)]
pub enum TransactionStatusError {
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
        // `block_height` is only present if `in_pool` is false
        pub(crate) block_height: Option<u64>,
        // `in_pool` is always present
        pub(crate) in_pool: bool,
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
                    4096,
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

            return Ok(TransactionStatus::InBlock {
                block_height: tx_info.block_height.ok_or_else(|| {
                    InterfaceError::InvalidInterface(
                        "Transaction has in_pool=false but has no block_height".to_string(),
                    )
                })?,
            });
        }
    }
}
