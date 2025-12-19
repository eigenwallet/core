use std::{sync::Arc, time::Duration};

use monero_sys::WalletEventListener;

use crate::Wallet;
use monero_oxide_ext::Amount;
use throttle::{throttle, Throttle};

use crate::TauriHandle;

pub trait MoneroTauriHandle: Send + Sync {
    fn balance_change(&self, total_balance: Amount, unlocked_balance: Amount);

    fn history_update(&self, transactions: Vec<monero_sys::TransactionInfo>);

    fn sync_progress(&self, current_block: u64, target_block: u64, progress_percentage: f32);
}

pub(crate) struct TauriWalletListener {
    balance_throttle: Throttle<()>,
    history_throttle: Throttle<()>,
    sync_throttle: Throttle<()>,
}

impl TauriWalletListener {
    const BALANCE_UPDATE_THROTTLE: Duration = Duration::from_millis(2 * 1000);
    const HISTORY_UPDATE_THROTTLE: Duration = Duration::from_millis(2 * 1000);
    const SYNC_UPDATE_THROTTLE: Duration = Duration::from_millis(2 * 1000);

    pub async fn new(tauri_handle: TauriHandle, wallet: Arc<Wallet>) -> Self {
        let rt_handle = tokio::runtime::Handle::current();

        let balance_job = {
            let wallet = wallet.clone();
            let tauri = tauri_handle.clone();
            let rt = rt_handle.clone();
            move |()| {
                let wallet = wallet.clone();
                let tauri = tauri.clone();
                let rt = rt.clone();
                rt.spawn(async move {
                    let total_balance = match wallet.total_balance().await {
                        Ok(total_balance) => total_balance,
                        Err(e) => {
                            tracing::error!("Failed to get total balance: {}", e);
                            return;
                        }
                    };
                    let unlocked_balance = match wallet.unlocked_balance().await {
                        Ok(unlocked_balance) => unlocked_balance,
                        Err(e) => {
                            tracing::error!("Failed to get unlocked balance: {}", e);
                            return;
                        }
                    };

                    tauri.balance_change(total_balance.into(), unlocked_balance.into());
                });
            }
        };

        let history_job = {
            let wallet = wallet.clone();
            let tauri = tauri_handle.clone();
            let rt = rt_handle.clone();
            move |()| {
                let wallet = wallet.clone();
                let tauri = tauri.clone();
                let rt = rt.clone();
                rt.spawn(async move {
                    let transactions = match wallet.history().await {
                        Ok(transactions) => transactions,
                        Err(e) => {
                            tracing::error!("Failed to get history: {}", e);
                            return;
                        }
                    };

                    tauri.history_update(transactions);
                });
            }
        };

        let sync_job = {
            let wallet = wallet.clone();
            let tauri = tauri_handle.clone();
            let rt = rt_handle.clone();
            move |()| {
                let wallet = wallet.clone();
                let tauri = tauri.clone();
                let rt = rt.clone();
                rt.spawn(async move {
                    let sync_progress = match wallet.sync_progress().await {
                        Ok(sync_progress) => sync_progress,
                        Err(e) => {
                            tracing::error!("Failed to get sync progress: {}", e);
                            return;
                        }
                    };

                    let progress_percentage = sync_progress.percentage();
                    tauri.sync_progress(
                        sync_progress.current_block,
                        sync_progress.target_block,
                        progress_percentage,
                    );
                });
            }
        };

        Self {
            balance_throttle: throttle(balance_job, Self::BALANCE_UPDATE_THROTTLE),
            history_throttle: throttle(history_job, Self::HISTORY_UPDATE_THROTTLE),
            sync_throttle: throttle(sync_job, Self::SYNC_UPDATE_THROTTLE),
        }
    }

    fn send_balance_update(&self) {
        self.balance_throttle.call(());
    }

    fn send_history_update(&self) {
        self.history_throttle.call(());
    }

    fn send_sync_progress(&self) {
        self.sync_throttle.call(());
    }
}

impl WalletEventListener for TauriWalletListener {
    fn on_money_spent(&self, _txid: &str, _amount: u64) {
        self.send_balance_update();
        self.send_history_update();
    }

    fn on_money_received(&self, _txid: &str, _amount: u64) {
        self.send_balance_update();
        self.send_history_update();
    }

    fn on_unconfirmed_money_received(&self, _txid: &str, _amount: u64) {
        self.send_balance_update();
        self.send_history_update();
    }

    fn on_new_block(&self, _height: u64) {
        // We send an update here because a new might mean that funds have been unlocked
        // because a UTXO reached 10 confirmations.
        self.send_sync_progress();
    }

    fn on_updated(&self) {
        self.send_balance_update();
    }

    fn on_refreshed(&self) {
        self.send_balance_update();
        self.send_history_update();
    }

    fn on_reorg(&self, _height: u64, _blocks_detached: u64, _transfers_detached: usize) {
        // We send an update here because a reorg might mean that a UTXO has been double spent
        // or that a UTXO has been confirmed is now unconfirmed.
        self.send_balance_update();
    }

    fn on_pool_tx_removed(&self, _txid: &str) {
        // We send an update here because a pool tx removed might mean that our unconfirmed
        // balance has gone down because a UTXO has been removed from the pool.
        self.send_balance_update();
    }
}
