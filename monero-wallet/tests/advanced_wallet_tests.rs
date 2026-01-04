mod harness;

use anyhow::Result;
use harness::{setup_test, TestContext, WALLET_NAME};
use monero::Network;
use monero_sys::TransactionInfo;
use monero_wallet::{MoneroTauriHandle, Wallets};
use serial_test::serial;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use swap_core::monero::Amount;
use uuid::Uuid;

/// Mock Tauri handle for testing.
/// Tauri handle requires the application to be running
/// which is not possible in tests
/// so we mock it.
/// Thread-safe implementation.
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

/// Mock Tauri handle implementation for testing.
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
async fn test_tauri_listener() -> Result<()> {
    setup_test(|context| async move {
        let handle = Arc::new(MockTauriHandle::new());
        let tauri_handle = Some(handle.clone() as Arc<dyn MoneroTauriHandle>);

        // Create wallets with tauri handle
        let _wallets = Wallets::new(
            context.wallet_dir.path().to_path_buf(),
            WALLET_NAME.to_string(),
            context.daemon.clone(),
            Network::Mainnet,
            true,
            tauri_handle,
            None,
        )
        .await?;

        // Create an action
        context.monero.generate_block().await?;

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let updates = handle.sync_updates.lock().unwrap();
        assert!(!updates.is_empty());

        Ok(())
    })
    .await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_change_monero_node() -> Result<()> {
    setup_test(|context| async move {
        let wallets = context.create_wallets().await?;
        let main_wallet = wallets.main_wallet().await;

        let initial_height = main_wallet.blockchain_height().await?;

        let same_daemon = context.daemon.clone();
        wallets.change_monero_node(same_daemon).await?;

        let height_after = main_wallet.blockchain_height().await?;
        assert!(height_after >= initial_height);

        Ok(())
    })
    .await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_recent_wallets() -> Result<()> {
    setup_test(|context| async move {
        let db_dir = tempfile::TempDir::new()?;
        let db = Arc::new(monero_sys::Database::new(db_dir.path().to_path_buf()).await?);

        let wallets = Wallets::new(
            context.wallet_dir.path().to_path_buf(),
            WALLET_NAME.to_string(),
            context.daemon.clone(),
            Network::Mainnet,
            true,
            None,
            Some(db.clone()),
        )
        .await?;

        let recent = wallets.get_recent_wallets().await?;
        assert!(!recent.is_empty());
        assert!(recent.iter().any(|p| p.contains(WALLET_NAME)));

        Ok(())
    })
    .await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_swap_wallet() -> Result<()> {
    setup_test(|context| async move {
        use swap_core::monero::primitives::PrivateViewKey;
        let mut rng = rand::thread_rng();

        let spend_key_prim = PrivateViewKey::new_random(&mut rng);
        let view_key_prim = PrivateViewKey::new_random(&mut rng);

        let spend_key: monero::PrivateKey = spend_key_prim.into();
        let view_key: monero::PrivateKey = view_key_prim.into();

        let public_spend = monero::PublicKey::from_private_key(&spend_key);
        let public_view = monero::PublicKey::from_private_key(&view_key);

        let address = monero::Address::standard(Network::Mainnet, public_spend, public_view);

        let amount = 1_000_000_000_000; // 1 XMR
        let miner_wallet = context.monero.wallet("miner")?;

        let receipt = miner_wallet.transfer(&address, amount).await?;
        let tx_id_str = receipt.txid;

        context.monero.generate_block().await?;

        let tx_hash = swap_core::monero::primitives::TxHash(tx_id_str.clone());

        let wallets = context.create_wallets().await?;

        let swap_id = Uuid::new_v4();

        let swap_wallet = wallets
            .swap_wallet_spendable(swap_id, spend_key, view_key_prim, tx_hash)
            .await;

        match swap_wallet {
            Ok(w) => {
                let balance = w.total_balance().await?;
                println!("Swap wallet balance: {:?}", balance);
                assert_eq!(balance.as_pico(), amount);
            }
            Err(e) => {
                panic!("Failed to open swap wallet: {}", e);
            }
        }

        Ok(())
    })
    .await;
    Ok(())
}
