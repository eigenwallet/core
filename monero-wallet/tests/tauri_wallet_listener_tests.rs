mod harness;

use anyhow::Result;
use harness::{setup_test, TestContext, WALLET_NAME};
use monero_sys::{TransactionInfo, WalletEventListener};
use monero_wallet::{MoneroTauriHandle, TauriWalletListener, Wallets};
use serial_test::serial;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use swap_core::monero::Amount;

/// Mock Tauri handle for testing.
struct MockTauriHandle {
    balance_updates: Arc<Mutex<Vec<(Amount, Amount)>>>,
    history_updates: Arc<Mutex<Vec<Vec<TransactionInfo>>>>,
    sync_updates: Arc<Mutex<Vec<(u64, u64, f32)>>>,
}

impl MockTauriHandle {
    fn new() -> Self {
        Self {
            balance_updates: Arc::new(Mutex::new(Vec::new())),
            history_updates: Arc::new(Mutex::new(Vec::new())),
            sync_updates: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl MoneroTauriHandle for MockTauriHandle {
    fn balance_change(&self, total_balance: Amount, unlocked_balance: Amount) {
        self.balance_updates
            .lock()
            .unwrap()
            .push((total_balance, unlocked_balance));
    }

    fn history_update(&self, transactions: Vec<TransactionInfo>) {
        self.history_updates.lock().unwrap().push(transactions);
    }

    fn sync_progress(&self, current_block: u64, target_block: u64, progress_percentage: f32) {
        self.sync_updates
            .lock()
            .unwrap()
            .push((current_block, target_block, progress_percentage));
    }
}

#[tokio::test]
#[serial]
async fn test_on_money_received_triggers_updates() -> Result<()> {
    setup_test(|context| async move {
        let handle = Arc::new(MockTauriHandle::new());
        let tauri_handle = handle.clone() as Arc<dyn MoneroTauriHandle>;

        let wallets = context.create_wallets().await?;
        let wallet = wallets.main_wallet().await;

        let listener = TauriWalletListener::new(tauri_handle, wallet).await;

        // Trigger money received event
        listener.on_money_received("txid", 1000000);

        // Wait for throttle
        tokio::time::sleep(Duration::from_secs(3)).await;

        let balances = handle.balance_updates.lock().unwrap();
        // Assert that balance updates are not empty
        assert!(!balances.is_empty());

        let history = handle.history_updates.lock().unwrap();
        // Assert that history updates are not empty
        assert!(!history.is_empty());

        Ok(())
    })
    .await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_on_money_spent_triggers_updates() -> Result<()> {
    setup_test(|context| async move {
        let handle = Arc::new(MockTauriHandle::new());
        let tauri_handle = handle.clone() as Arc<dyn MoneroTauriHandle>;

        let wallets = context.create_wallets().await?;
        let wallet = wallets.main_wallet().await;

        let listener = TauriWalletListener::new(tauri_handle, wallet).await;

        // Trigger money spent event
        listener.on_money_spent("txid", 500000);

        // Wait for throttle
        tokio::time::sleep(Duration::from_secs(3)).await;

        let balances = handle.balance_updates.lock().unwrap();
        // Assert that balance updates are not empty
        assert!(!balances.is_empty());

        let history = handle.history_updates.lock().unwrap();
        // Assert that history updates are not empty
        assert!(!history.is_empty());

        Ok(())
    })
    .await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_on_new_block_triggers_sync_progress() -> Result<()> {
    setup_test(|context| async move {
        let handle = Arc::new(MockTauriHandle::new());
        let tauri_handle = handle.clone() as Arc<dyn MoneroTauriHandle>;

        let wallets = context.create_wallets().await?;
        let wallet = wallets.main_wallet().await;

        let listener = TauriWalletListener::new(tauri_handle, wallet).await;

        // Trigger new block event
        listener.on_new_block(100);

        // Wait for throttle
        tokio::time::sleep(Duration::from_secs(3)).await;

        let sync = handle.sync_updates.lock().unwrap();
        // Assert that sync updates are not empty
        assert!(!sync.is_empty());

        Ok(())
    })
    .await;
    Ok(())
}
