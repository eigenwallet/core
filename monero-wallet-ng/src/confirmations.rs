//! Transaction confirmation utilities.
//!
//! This module provides functions to check and wait for transaction confirmations
//! without depending on monero-sys.

use std::time::Duration;

use monero_interface::ProvidesBlockchainMeta;
use tracing::{Instrument, Span};

use crate::{
    rpc::TransactionStatus,
    rpc::{ProvidesTransactionStatus, TransactionStatusError},
};

#[derive(Debug, Clone)]
pub enum ConfirmationStatus {
    /// We haven't seen the transaction yet
    Unseen,
    /// The transaction is in the mempool
    InPool,
    /// The transaction is included in a block which we believe is part of the longest chain
    Confirmed {
        // 1 = included in latest block
        // 2 = included in a block upon which the latest block is based
        // ...
        confirmations: u64,
    },
}

impl ConfirmationStatus {
    /// Returns the number of confirmations
    /// Return 0 if in mempool or we haven't seen the transaction yet.
    pub fn confirmations(&self) -> u64 {
        match self {
            ConfirmationStatus::Confirmed { confirmations } => *confirmations,
            _ => 0,
        }
    }

    /// Returns true if the transaction has at least the required confirmations.
    pub fn has_confirmations(&self, required: u64) -> bool {
        self.confirmations() >= required
    }
}

/// A subscription to the confirmation status of a transaction.
///
/// This struct holds a receiver that will be updated as the transaction's
/// confirmation status changes. Use the `wait_until_*` methods to await
/// specific confirmation conditions.
#[derive(Debug, Clone)]
pub struct Subscription {
    /// A receiver used to await updates to the confirmation status.
    pub receiver: tokio::sync::watch::Receiver<ConfirmationStatus>,
    /// The transaction ID we are subscribing to.
    pub tx_id: [u8; 32],
}

/// Error returned when waiting for a subscription condition.
#[derive(Debug, thiserror::Error)]
#[error("Subscription closed before condition was met")]
pub struct SubscriptionClosed;

impl Subscription {
    /// Get the current confirmation status.
    pub fn status(&self) -> ConfirmationStatus {
        self.receiver.borrow().clone()
    }

    /// Wait until the given predicate returns true for the confirmation status.
    ///
    /// # Returns
    /// * `Ok(())` when the predicate returns true
    /// * `Err(SubscriptionClosed)` if the background task stopped before the condition was met
    pub async fn wait_until(
        &self,
        mut predicate: impl FnMut(&ConfirmationStatus) -> bool,
    ) -> Result<(), SubscriptionClosed> {
        let mut receiver = self.receiver.clone();

        loop {
            if predicate(&receiver.borrow()) {
                return Ok(());
            }

            receiver.changed().await.map_err(|_| SubscriptionClosed)?;
        }
    }

    /// Wait until the transaction has the required number of confirmations.
    ///
    /// # Arguments
    /// * `required` - The number of confirmations to wait for
    ///
    /// # Returns
    /// * `Ok(())` when the transaction has the required confirmations
    /// * `Err(SubscriptionClosed)` if the background task stopped before reaching the target
    pub async fn wait_until_confirmed(&self, required: u64) -> Result<(), SubscriptionClosed> {
        self.wait_until(|status| status.has_confirmations(required))
            .await
    }
}

/// Get the confirmation status of a transaction.
///
/// This function queries the daemon for the transaction's status and calculates
/// the number of confirmations based on the current blockchain height.
///
/// # Arguments
/// * `provider` - To fetch the transaction status and the latest block height
/// * `tx_id` - The transaction hash (ID)
pub async fn get_confirmations<P>(
    provider: &P,
    tx_id: [u8; 32],
) -> Result<ConfirmationStatus, TransactionStatusError>
where
    P: ProvidesTransactionStatus + ProvidesBlockchainMeta,
{
    // Get transaction status
    let tx_status = provider.transaction_status(tx_id).await?;

    let tx_status = match tx_status {
        TransactionStatus::InPool => ConfirmationStatus::InPool,
        TransactionStatus::InBlock { block_height } => {
            // We need the latest block height to calculate confirmations
            let latest_block = provider.latest_block_number().await? as u64;

            let confirmations = absolute_confirmations_into_relative(block_height, latest_block);

            ConfirmationStatus::Confirmed { confirmations }
        }
        TransactionStatus::Unknown => ConfirmationStatus::Unseen,
    };

    Ok(tx_status)
}

/// Subscribe to a transaction's confirmation status.
///
/// This function spawns a tokio task that periodically polls for the transaction's
/// confirmation status and notifies subscribers via a watch channel.
///
/// # Arguments
/// * `provider` - A provider that will be owned by the spawned task
/// * `tx_id` - The transaction hash (ID)
/// * `poll_interval` - How often to check for new confirmations
///
/// # Returns
/// A `Subscription` that can be used to wait for specific confirmation conditions.
/// The background task will automatically stop when all `Subscription` clones are dropped.
pub fn subscribe<P>(provider: P, tx_id: [u8; 32], poll_interval: Duration) -> Subscription
where
    P: ProvidesTransactionStatus + ProvidesBlockchainMeta + Send + 'static,
{
    use crate::retry::Backoff;

    let (sender, receiver) = tokio::sync::watch::channel(ConfirmationStatus::Unseen);

    let _ = tokio::spawn(async move {
        let mut backoff = Backoff::new();

        while !sender.is_closed() {
            match get_confirmations(&provider, tx_id).await {
                Ok(status) => {
                    backoff.reset();
                    if sender.send(status).is_err() {
                        return;
                    }
                    tokio::time::sleep(poll_interval).await;
                }
                Err(err) => {
                    backoff
                        .sleep_on_error(&err, "Failed to refresh confirmation subscription")
                        .await;
                }
            };
        }
    })
    .instrument(Span::current());

    Subscription { receiver, tx_id }
}

fn absolute_confirmations_into_relative(inclusion_height: u64, latest_block: u64) -> u64 {
    // If the `inclusion_height`` is greater than the latest block, we assume
    // that the `latest_block` and we assume that the `inclusion_height` is the latest block.
    //
    // This means that if `inclusion_height > latest_block`, we return 1.
    latest_block
        .saturating_sub(inclusion_height)
        .saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confirmation_status_has_confirmations() {
        let in_pool = ConfirmationStatus::InPool;
        assert!(!in_pool.has_confirmations(1));
        assert!(in_pool.has_confirmations(0));

        let confirmed_1 = ConfirmationStatus::Confirmed { confirmations: 1 };
        assert!(confirmed_1.has_confirmations(0));
        assert!(confirmed_1.has_confirmations(1));
        assert!(!confirmed_1.has_confirmations(2));

        let confirmed_10 = ConfirmationStatus::Confirmed { confirmations: 10 };
        assert!(confirmed_10.has_confirmations(1));
        assert!(confirmed_10.has_confirmations(10));
        assert!(!confirmed_10.has_confirmations(11));
    }

    #[test]
    fn test_confirmation_status_confirmations() {
        assert_eq!(ConfirmationStatus::InPool.confirmations(), 0);
        assert_eq!(
            ConfirmationStatus::Confirmed { confirmations: 5 }.confirmations(),
            5
        );
    }

    #[test]
    fn test_absolute_confirmations_into_relative_normal_case() {
        // Transaction included at block 95, latest block is 100
        // Confirmations = 100 - 95 + 1 = 6
        assert_eq!(absolute_confirmations_into_relative(95, 100), 6);
    }

    #[test]
    fn test_absolute_confirmations_into_relative_same_block() {
        // Transaction included in the latest block
        // Confirmations = 100 - 100 + 1 = 1
        assert_eq!(absolute_confirmations_into_relative(100, 100), 1);
    }

    #[test]
    fn test_absolute_confirmations_into_relative_inclusion_exceeds_latest() {
        // Edge case: inclusion_height > latest_block
        // Returns 1 due to saturating_sub
        assert_eq!(absolute_confirmations_into_relative(105, 100), 1);
    }
}
