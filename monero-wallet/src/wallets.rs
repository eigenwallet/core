//! This module contains the [`Wallets`] struct, which we use to manage and access the
//! Monero blockchain and wallets.
//!
//! Mostly we do two things:
//!  - wait for transactions to be confirmed
//!  - send money from one wallet to another.

use monero_oxide_wallet::GuaranteedViewPair;
pub use monero_sys::{Daemon, WalletHandle as Wallet, WalletHandleListener};

use anyhow::{Context, Result};
use monero::Network;
use monero_daemon_rpc::MoneroDaemon;
use monero_simple_request_rpc::SimpleRequestTransport;
use std::time::Duration;
use std::{path::PathBuf, sync::Arc};
use swap_core::monero::primitives::{Amount, BlockHeight, PrivateViewKey, TxHash, WatchRequest};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::compat::{private_view_key_to_scalar, public_key_to_point, tx_hash_to_bytes};
use crate::listener::{MoneroTauriHandle, TauriWalletListener};

/// Default poll interval for blockchain queries.
const POLL_INTERVAL: Duration = Duration::from_secs(10);

pub type TauriHandle = Arc<dyn MoneroTauriHandle>;

/// Entrance point to the Monero blockchain.
/// You can use this struct to open specific wallets and monitor the blockchain.
pub struct Wallets {
    /// The directory we store the wallets in.
    wallet_dir: PathBuf,
    /// The network we're on.
    network: Network,
    /// The monero node we connect to.
    daemon: Arc<RwLock<(Daemon, MoneroDaemon<SimpleRequestTransport>)>>,
    /// Keep the main wallet open and synced.
    main_wallet: Arc<Wallet>,
    /// Since Network::Regtest isn't a thing we have to use an extra flag.
    /// When we're in regtest mode, we need to unplug some safty nets to make the wallet work.
    regtest: bool,
    /// A handle we use to send status updates to the UI i.e. when
    /// waiting for a transaction to be confirmed.
    #[expect(dead_code)]
    tauri_handle: Option<TauriHandle>,
    /// Database for tracking wallet usage history.
    wallet_database: Option<Arc<monero_sys::Database>>,
}

impl Wallets {
    /// Create a new `Wallets` instance.
    /// Wallets will be opened on the specified network, connected to the specified daemon
    /// and stored in the specified directory.
    ///
    /// The main wallet will be kept alive and synced, other wallets are
    /// opened and closed on demand.
    pub async fn new(
        wallet_dir: PathBuf,
        main_wallet_name: String,
        daemon: Daemon,
        network: Network,
        regtest: bool,
        tauri_handle: Option<TauriHandle>,
        wallet_database: Option<Arc<monero_sys::Database>>,
    ) -> Result<Self> {
        let main_wallet = Wallet::open_or_create(
            wallet_dir.join(&main_wallet_name).display().to_string(),
            daemon.clone(),
            network,
            true,
        )
        .await
        .context("Failed to open main wallet")?;

        if regtest {
            main_wallet.unsafe_prepare_for_regtest().await;
        }

        let main_wallet = Arc::new(main_wallet);

        // We always register this listener
        // It does essential things like storing the wallet on certain events
        let handle_listener = WalletHandleListener::new(main_wallet.clone());
        main_wallet
            .call(move |wallet| {
                wallet.add_listener(Box::new(handle_listener));
            })
            .await?;

        // We only register the UI listener if we are running with Tauri
        if let Some(tauri_handle) = tauri_handle.clone() {
            let tauri_wallet_listener =
                TauriWalletListener::new(tauri_handle, main_wallet.clone()).await;

            main_wallet
                .call(move |wallet| {
                    wallet.add_listener(Box::new(tauri_wallet_listener));
                })
                .await?;
        }

        let rpc_client = SimpleRequestTransport::new(daemon.to_url_string()).await?;
        let daemon = Arc::new(RwLock::new((daemon, rpc_client)));

        let wallets = Self {
            wallet_dir,
            network,
            daemon,
            main_wallet,
            regtest,
            tauri_handle,
            wallet_database,
        };

        // Record wallet access in database
        let wallet_path = wallets.main_wallet.path().await;
        let _ = wallets.record_wallet_access(&wallet_path?).await;

        Ok(wallets)
    }

    /// Create a new `Wallets` instance with an existing wallet as the main wallet.
    /// This is used when we want to use a user-selected wallet instead of creating a new one.
    pub async fn new_with_existing_wallet(
        wallet_dir: PathBuf,
        daemon: Daemon,
        network: Network,
        regtest: bool,
        tauri_handle: Option<TauriHandle>,
        existing_wallet: Wallet,
        wallet_database: Option<Arc<monero_sys::Database>>,
    ) -> Result<Self> {
        // TODO: This code is duplicated in [`Wallets::new`]. Unify it.
        if regtest {
            existing_wallet.unsafe_prepare_for_regtest().await;
        }

        let main_wallet = Arc::new(existing_wallet);

        let handle_listener = WalletHandleListener::new(main_wallet.clone());

        // We always register this listener.
        // It does essential things like storing the wallet on certain events
        main_wallet
            .call(move |wallet| {
                wallet.add_listener(Box::new(handle_listener));
            })
            .await?;

        // We only register the UI listener if we are running with Tauri
        if let Some(tauri_handle) = tauri_handle.clone() {
            let tauri_wallet_listener =
                TauriWalletListener::new(tauri_handle, main_wallet.clone()).await;

            main_wallet
                .call(move |wallet| {
                    wallet.add_listener(Box::new(tauri_wallet_listener));
                })
                .await?;
        }

        let rpc_client = SimpleRequestTransport::new(daemon.to_url_string()).await?;
        let daemon = Arc::new(RwLock::new((daemon, rpc_client)));

        let wallets = Self {
            wallet_dir,
            network,
            daemon,
            main_wallet,
            regtest,
            tauri_handle,
            wallet_database,
        };

        // Record wallet access in database
        let wallet_path = wallets.main_wallet.path().await;
        let _ = wallets.record_wallet_access(&wallet_path?).await;

        Ok(wallets)
    }

    pub async fn change_monero_node(&self, new_daemon: Daemon) -> Result<()> {
        {
            let mut daemon = self.daemon.write().await;
            let rpc_client = SimpleRequestTransport::new(new_daemon.to_url_string()).await?;
            *daemon = (new_daemon.clone(), rpc_client);
        }

        self.main_wallet
            .call(move |wallet| wallet.set_daemon(&new_daemon))
            .await??;

        Ok(())
    }
}

impl Wallets {
    /// Get the main wallet (specified when initializing the `Wallets` instance).
    pub async fn main_wallet(&self) -> Arc<Wallet> {
        self.main_wallet.clone()
    }

    /// Open the lock wallet of a specific swap from a given Monero TxLock ID.
    /// We can therefore only use this if we already know the txid of the lock transaction.
    /// It will skip the syncing and only import the given transaction.
    ///
    /// Used to redeem (Bob) or refund (Alice) the Monero.
    pub async fn swap_wallet_spendable(
        &self,
        swap_id: Uuid,
        spend_key: monero::PrivateKey,
        view_key: PrivateViewKey,
        tx_lock_id: TxHash,
    ) -> Result<Arc<Wallet>> {
        // Derive wallet address from the keys
        let address = {
            let public_spend_key = monero::PublicKey::from_private_key(&spend_key);
            let public_view_key = monero::PublicKey::from_private_key(&view_key.into());

            monero::Address::standard(self.network, public_spend_key, public_view_key)
        };

        let wallet_path = swap_wallet_path(swap_id, &self.wallet_dir, true)
            .display()
            .to_string();

        // We get the current blockheight from the daemon
        let blockheight = self.direct_rpc_block_height().await?;

        let (daemon, _) = self.daemon.read().await.clone();

        let wallet = Wallet::open_or_create_from_keys(
            wallet_path.clone(),
            None,
            self.network,
            address,
            view_key.into(),
            spend_key,
            blockheight,
            // We don't sync the swap wallet, just import the transaction
            false,
            daemon,
        )
        .await
        .context(format!(
            "Failed to open or create wallet `{}` from the specified keys",
            wallet_path
        ))?;

        if self.regtest {
            wallet.unsafe_prepare_for_regtest().await;
        }

        tracing::debug!(
            %swap_id,
            "Opened temporary Monero wallet, loading lock transaction"
        );

        wallet
            .scan_transaction(tx_lock_id.0.clone())
            .await
            .context("Couldn't import Monero lock transaction")?;

        wallet.set_restore_height(blockheight).await?;

        // We synchronously refresh the wallet
        // This should be quick because we just set the restore height to the current blockheight.
        wallet.refresh_blocking().await?;

        // Now we start the refresh thread Why?
        // Because if the user later tries to spend the funds (after a new block is mined), the wallet will not be synchronized anymore
        // We start the refresh thread such that the wallet will keep up with the chain tip in the background.
        wallet.start_refresh_thread().await?;

        Ok(Arc::new(wallet))
    }
}

impl Wallets {
    /// Get a clone of the RPC client for direct daemon communication.
    pub async fn rpc_client(&self) -> MoneroDaemon<SimpleRequestTransport> {
        let (_daemon, rpc_client) = self.daemon.read().await.clone();
        rpc_client
    }

    pub async fn direct_rpc_block_height(&self) -> Result<u64> {
        use monero_daemon_rpc::prelude::ProvidesBlockchainMeta;
        let rpc_client = self.rpc_client().await;

        let height = rpc_client
            .latest_block_number()
            .await
            .context("Failed to get block height from daemon")?;

        Ok(height as u64)
    }

    /// Verify a transfer using the new monero-wallet-ng implementation.
    ///
    /// This verifies that a transaction sends the expected amount to the given view pair
    /// by fetching the transaction from the RPC and scanning it.
    pub async fn verify_transfer(
        &self,
        tx_hash: &TxHash,
        public_spend_key: monero::PublicKey,
        private_view_key: PrivateViewKey,
        expected_amount: Amount,
    ) -> Result<bool> {
        let rpc_client = self.rpc_client().await;

        let tx_id = tx_hash_to_bytes(tx_hash)?;
        let public_spend_key = public_key_to_point(public_spend_key)?;
        let private_view_key = private_view_key_to_scalar(private_view_key)?;
        let expected_amount: u64 = expected_amount.into();

        let result = monero_wallet_ng::verify::verify_transfer(
            &rpc_client,
            tx_id,
            public_spend_key,
            private_view_key,
            expected_amount,
        )
        .await
        .context("Failed to verify transfer")?;

        Ok(result)
    }

    /// Wait until a transfer is verified and confirmed using monero-wallet-ng.
    ///
    /// This first verifies that the transaction sends the expected amount to the given view pair,
    /// then subscribes to confirmations and waits until the target confirmations are reached.
    ///
    /// You can pass a listener function that will be called with
    /// the current number of confirmations every time the status changes.
    pub async fn wait_until_confirmed(
        &self,
        tx_hash: &TxHash,
        confirmation_target: u64,
        listener: Option<impl Fn((u64, u64)) + Send + 'static>,
    ) -> Result<()> {
        use monero_wallet_ng::confirmations;

        let rpc_client = self.rpc_client().await;
        let tx_id = tx_hash_to_bytes(tx_hash)?;
        let subscription = confirmations::subscribe(rpc_client, tx_id, POLL_INTERVAL);

        // Wait until target confirmations, emitting events on each status change
        subscription
            .wait_until(|status| {
                let current = status.confirmations();

                // Emit event
                if let Some(ref listener) = listener {
                    listener((current, confirmation_target));
                }

                tracing::debug!(
                    confirmations = current,
                    target = confirmation_target,
                    "Monero lock transaction confirmation update"
                );

                status.has_confirmations(confirmation_target)
            })
            .await
            .map_err(|_| {
                // TODO: We need some way to spawn a new task if the existing one dies
                anyhow::anyhow!(
                    "Confirmation subscription closed before reaching {} confirmations",
                    confirmation_target
                )
            })?;

        tracing::info!(
            tx_hash = %tx_hash.0,
            "Monero lock transaction fully confirmed"
        );

        Ok(())
    }

    /// Wait for an incoming transfer using the new monero-wallet-ng scanner.
    ///
    /// This scans the blockchain from `restore_height` looking for an output
    /// with the expected amount sent to the given view pair. Returns the
    /// transaction hash when found.
    pub async fn wait_for_incoming_transfer(
        &self,
        public_spend_key: monero::PublicKey,
        private_view_key: PrivateViewKey,
        expected_amount: Amount,
        restore_height: BlockHeight,
    ) -> Result<TxHash> {
        use monero_oxide_wallet::ViewPair;
        use monero_wallet_ng::scanner;

        let rpc_client = self.rpc_client().await;

        let public_spend_key = public_key_to_point(public_spend_key)?;
        let private_view_key = private_view_key_to_scalar(private_view_key)?;

        let expected_amount: u64 = expected_amount.into();
        let restore_height = restore_height.height as usize;

        let mut subscription = scanner::scanner(
            rpc_client,
            public_spend_key,
            private_view_key,
            restore_height,
            POLL_INTERVAL,
        )
        .context("Failed to create scanner")?;

        let output = subscription
            .wait_until(|output| output.commitment().amount == expected_amount)
            .await
            .context("Scanner subscription closed before finding output")?;

        let tx_hash = hex::encode(output.transaction());

        tracing::debug!(
            %tx_hash,
            amount = expected_amount,
            "Found incoming transfer with expected amount"
        );

        Ok(TxHash(tx_hash))
    }
}

impl Wallets {
    /// Get the last 5 recently used wallets
    pub async fn get_recent_wallets(&self) -> Result<Vec<String>> {
        if let Some(db) = &self.wallet_database {
            let recent_wallets = db.get_recent_wallets(5).await?;
            Ok(recent_wallets.into_iter().map(|w| w.wallet_path).collect())
        } else {
            Ok(vec![])
        }
    }

    /// Record that a wallet was accessed
    pub async fn record_wallet_access(&self, wallet_path: &str) -> Result<()> {
        if let Some(db) = &self.wallet_database {
            db.record_wallet_access(wallet_path).await?;
        }
        Ok(())
    }
}

/// Pass this to [`Wallet::wait_until_confirmed`] or [`Wallet::wait_until_synced`]
/// to not receive any confirmation callbacks.
pub fn no_listener<T>() -> Option<impl Fn(T) + Send + 'static> {
    Some(|_| {})
}

fn swap_wallet_path(swap_id: Uuid, wallet_dir: &PathBuf, spendable: bool) -> PathBuf {
    let suffix = if spendable { "spendable" } else { "view_only" };
    let name = format!("swap_{}_{}", &swap_id.to_string(), suffix);

    wallet_dir.join(name)
}
