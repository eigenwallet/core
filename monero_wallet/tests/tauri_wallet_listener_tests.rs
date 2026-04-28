mod harness;

use anyhow::{Context, Result};
use harness::TestContext;
use monero_sys::{TransactionInfo, WalletEventListener};
use monero_wallet::{MoneroTauriHandle, TauriWalletListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use swap_core::monero::Amount;
use tokio::sync::Notify;

struct RecordingHandle {
    balance_updates: Mutex<Vec<(Amount, Amount)>>,
    history_updates: Mutex<Vec<Vec<TransactionInfo>>>,
    sync_updates: Mutex<Vec<(u64, u64, f32)>>,

    balance_notified: Notify,
    history_notified: Notify,
    sync_notified: Notify,
}

impl RecordingHandle {
    fn new() -> Self {
        Self {
            balance_updates: Mutex::new(Vec::new()),
            history_updates: Mutex::new(Vec::new()),
            sync_updates: Mutex::new(Vec::new()),
            balance_notified: Notify::new(),
            history_notified: Notify::new(),
            sync_notified: Notify::new(),
        }
    }
}

impl MoneroTauriHandle for RecordingHandle {
    fn balance_change(&self, total: Amount, unlocked: Amount) {
        self.balance_updates.lock().unwrap().push((total, unlocked));
        self.balance_notified.notify_one();
    }

    fn history_update(&self, txs: Vec<TransactionInfo>) {
        self.history_updates.lock().unwrap().push(txs);
        self.history_notified.notify_one();
    }

    fn sync_progress(&self, current: u64, target: u64, pct: f32) {
        self.sync_updates.lock().unwrap().push((current, target, pct));
        self.sync_notified.notify_one();
    }
}

async fn wait_for(notify: &Notify, what: &str) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(8), notify.notified())
        .await
        .with_context(|| format!("timed out waiting for {what}"))?;
    Ok(())
}

async fn make_listener(
    context: &TestContext,
) -> Result<(Arc<RecordingHandle>, TauriWalletListener, monero_sys::WalletHandle)> {
    let handle = Arc::new(RecordingHandle::new());
    let wallet = context.open_regtest_wallet(context.wallet_path()).await?;
    let listener = TauriWalletListener::new(
        handle.clone() as Arc<dyn MoneroTauriHandle>,
        wallet.clone().into(),
    )
    .await;

    Ok((handle, listener, wallet))
}

integration_test!(test_tauri_listener_emits_balance_and_history_updates);

async fn test_tauri_listener_emits_balance_and_history_updates_case(ctx: Arc<TestContext>) -> Result<()> {
    let (handle, listener, _wallet) = make_listener(&ctx).await?;

    listener.on_money_received("txid", 1_000_000);
    listener.on_money_spent("txid2", 500_000);

    wait_for(&handle.balance_notified, "balance update").await?;
    wait_for(&handle.history_notified, "history update").await?;

    {
        let balance_guards = handle.balance_updates.lock().unwrap();
        assert!(!balance_guards.is_empty(), "expected at least one balance update");
        let history_guards = handle.history_updates.lock().unwrap();
        assert!(!history_guards.is_empty(), "expected at least one history update");
    }

    Ok(())
}

integration_test!(test_tauri_listener_emits_sync_progress_on_new_block);

async fn test_tauri_listener_emits_sync_progress_on_new_block_case(ctx: Arc<TestContext>) -> Result<()> {
    let (handle, listener, _wallet) = make_listener(&ctx).await?;

    listener.on_new_block(100);

    wait_for(&handle.sync_notified, "sync progress").await?;

    {
        let sync = handle.sync_updates.lock().unwrap();
        assert!(!sync.is_empty(), "expected at least one sync update");
    }
    
    Ok(())
}
