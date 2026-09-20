//! Integration tests for the runtime behaviour of [`Wallets`]:
//!
//! - changing the Monero node at runtime via [`Wallets::change_monero_node`]
//! - the [`monero_wallet::TauriWalletListener`] forwarding wallet events to the
//!   UI handle

mod harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use harness::TestEnv;
use monero_harness::Cli;
use monero_oxide_ext::Amount;
use monero_sys::TransactionInfo;
use monero_wallet::MoneroTauriHandle;

/// After [`Wallets::change_monero_node`] the lazily connected RPC client and
/// the main wallet both talk to the new daemon.
#[tokio::test]
async fn changes_monero_node_at_runtime() -> Result<()> {
    harness::init_tracing();

    let cli = Cli::default();
    let env = TestEnv::new(&cli).await?;

    let wallets = env.wallets("main-wallet").await?;

    let height_a = wallets.direct_rpc_block_height().await?;
    assert!(height_a > 0);

    // Spawn a second, independent regtest daemon. Its chain is fresh, so its
    // height is much lower than the first daemon's. `latest_block_number` is
    // the 0-based index of the top block.
    let (daemon_b, _monerod_b, _monero_b) = env.spawn_daemon().await?;
    let height_b = monero_daemon_height(&daemon_b).await?;
    assert!(height_b < height_a);

    wallets.change_monero_node(daemon_b).await?;

    // The RPC client now reaches the new daemon.
    wallets.rpc_health_check().await?;
    assert_eq!(wallets.direct_rpc_block_height().await?, height_b);

    // The main wallet talks to the new daemon as well. The wallet reports the
    // daemon's chain height (`get_info`'s `height`), one higher than the
    // 0-based top block index above.
    let main_wallet = wallets.main_wallet().await;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let observed = main_wallet.blockchain_height().await.unwrap_or_default();
        if observed == height_b + 1 {
            break;
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!(
                "main wallet did not switch to the new daemon (observed height {observed}, expected {})",
                height_b + 1
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Ok(())
}

/// Query a daemon's blockchain height directly over RPC.
async fn monero_daemon_height(daemon: &monero_sys::Daemon) -> Result<u64> {
    use monero_daemon_rpc::prelude::ProvidesBlockchainMeta;
    use monero_simple_request_rpc::SimpleRequestTransport;

    let client = SimpleRequestTransport::new(daemon.to_url_string())
        .await
        .context("Failed to connect to daemon")?;

    Ok(client.latest_block_number().await? as u64)
}

/// Records the events a Tauri handle receives.
#[derive(Default)]
struct RecordingHandle {
    balance_updates: Mutex<Vec<(u64, u64)>>,
    history_updates: Mutex<Vec<usize>>,
    sync_updates: Mutex<Vec<(u64, u64, f32)>>,
}

impl MoneroTauriHandle for RecordingHandle {
    fn balance_change(&self, total_balance: Amount, unlocked_balance: Amount) {
        self.balance_updates
            .lock()
            .unwrap()
            .push((total_balance.as_pico(), unlocked_balance.as_pico()));
    }

    fn history_update(&self, transactions: Vec<TransactionInfo>) {
        self.history_updates
            .lock()
            .unwrap()
            .push(transactions.len());
    }

    fn sync_progress(&self, current_block: u64, target_block: u64, progress_percentage: f32) {
        self.sync_updates
            .lock()
            .unwrap()
            .push((current_block, target_block, progress_percentage));
    }
}

/// Funding the main wallet triggers balance, history and sync progress
/// callbacks on the registered Tauri handle.
#[tokio::test]
async fn tauri_handle_receives_wallet_events() -> Result<()> {
    harness::init_tracing();

    let cli = Cli::default();
    let env = TestEnv::new(&cli).await?;

    let handle = Arc::new(RecordingHandle::default());
    let tauri_handle: monero_wallet::TauriHandle = handle.clone();
    let wallets = env
        .wallets_with("main-wallet", Some(tauri_handle), None)
        .await?;
    let main_wallet = wallets.main_wallet().await;

    let amount = 1_000_000_000u64;
    env.fund_wallet(&main_wallet, amount).await?;

    // The listener is throttled (2s), wait until all three event kinds arrived.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        let has_balance = !handle.balance_updates.lock().unwrap().is_empty();
        let has_history = !handle.history_updates.lock().unwrap().is_empty();
        let has_sync = !handle.sync_updates.lock().unwrap().is_empty();

        if has_balance && has_history && has_sync {
            break;
        }

        if std::time::Instant::now() >= deadline {
            anyhow::bail!(
                "Timed out waiting for Tauri events (balance: {has_balance}, history: {has_history}, sync: {has_sync})"
            );
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // The last balance update must reflect the received funds.
    let (total, _unlocked) = *handle.balance_updates.lock().unwrap().last().unwrap();
    assert_eq!(total, amount);

    Ok(())
}
