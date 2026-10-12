pub mod request;
pub mod tauri_bindings;
mod wallet_setup;

use crate::cli::api::tauri_bindings::ContextStatus;
use crate::cli::command::{Bitcoin, Monero};
use crate::common::tor::{bootstrap_tor_client, create_tor_client};
use crate::common::tracing_util::Format;
use crate::database::{AccessMode, open_db};
use crate::network::rendezvous::XmrBtcNamespace;
use crate::protocol::Database;
use crate::seed::Seed;
use crate::{common, monero};
use anyhow::{Context as AnyContext, Error, Result, bail};
use arti_client::TorClient;
use futures::future::try_join_all;
use libp2p::{Multiaddr, PeerId};
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};
use swap_env::env::{Config as EnvConfig, GetConfig, Mainnet, Testnet};
use swap_fs::system_data_dir;
use tauri_bindings::{MoneroNodeConfig, TauriBackgroundProgress, TauriEmitter, TauriHandle};
use tokio::sync::{Mutex as TokioMutex, RwLock, broadcast, broadcast::Sender};
use tokio::task::JoinHandle;
use tokio_util::task::AbortOnDropHandle;
use tor_rtcompat::tokio::TokioRustlsRuntime;
use uuid::Uuid;

use super::watcher::Watcher;

static START: Once = Once::new();

mod config {
    use super::*;

    #[derive(Clone, PartialEq, Debug)]
    pub struct Config {
        pub(super) namespace: XmrBtcNamespace,
        pub env_config: EnvConfig,
        pub(super) seed: Option<Seed>,
        pub(super) json: bool,
        pub(super) log_dir: PathBuf,
        pub(super) data_dir: PathBuf,
        pub(super) is_testnet: bool,
    }

    impl Config {
        pub fn for_harness(seed: Seed, env_config: EnvConfig) -> Self {
            let data_dir =
                super::data::data_dir_from(None, false).expect("Could not find data directory");
            let log_dir = data_dir.join("logs"); // not used in production

            Self {
                namespace: XmrBtcNamespace::from_is_testnet(false),
                env_config,
                seed: seed.into(),
                json: false,
                is_testnet: false,
                data_dir,
                log_dir,
            }
        }
    }

    pub(super) fn env_config_from(testnet: bool) -> EnvConfig {
        if testnet {
            Testnet::get_config()
        } else {
            Mainnet::get_config()
        }
    }
}

pub use config::Config;

mod swap_lock {
    use super::*;

    #[derive(Default)]
    pub struct PendingTaskList(TokioMutex<Vec<JoinHandle<()>>>);

    impl PendingTaskList {
        pub async fn spawn<F, T>(&self, future: F)
        where
            F: Future<Output = T> + Send + 'static,
            T: Send + 'static,
        {
            let handle = tokio::spawn(async move {
                let _ = future.await;
            });

            self.0.lock().await.push(handle);
        }

        pub async fn wait_for_tasks(&self) -> Result<()> {
            let tasks = {
                // Scope for the lock, to avoid holding it for the entire duration of the async block
                let mut guard = self.0.lock().await;
                guard.drain(..).collect::<Vec<_>>()
            };

            try_join_all(tasks).await?;

            Ok(())
        }
    }

    /// The `SwapLock` manages the state of the current swap, ensuring that only one swap can be active at a time.
    /// It includes:
    /// - A lock for the current swap (`current_swap`)
    /// - A broadcast channel for suspension signals (`suspension_trigger`)
    ///
    /// The `SwapLock` provides methods to acquire and release the swap lock, and to listen for suspension signals.
    /// This ensures that swap operations do not overlap and can be safely suspended if needed.
    pub struct SwapLock {
        current_swap: RwLock<Option<Uuid>>,
        suspension_trigger: Sender<()>,
    }

    impl SwapLock {
        pub fn new() -> Self {
            let (suspension_trigger, _) = broadcast::channel(10);
            SwapLock {
                current_swap: RwLock::new(None),
                suspension_trigger,
            }
        }

        pub async fn listen_for_swap_force_suspension(&self) -> Result<(), Error> {
            let mut listener = self.suspension_trigger.subscribe();
            let event = listener.recv().await;
            match event {
                Ok(_) => Ok(()),
                Err(e) => {
                    tracing::error!("Error receiving swap suspension signal: {}", e);
                    bail!(e)
                }
            }
        }

        pub async fn acquire_swap_lock(&self, swap_id: Uuid) -> Result<(), Error> {
            let mut current_swap = self.current_swap.write().await;
            if current_swap.is_some() {
                bail!("There already exists an active swap lock");
            }

            tracing::debug!(swap_id = %swap_id, "Acquiring swap lock");
            *current_swap = Some(swap_id);
            Ok(())
        }

        pub async fn get_current_swap_id(&self) -> Option<Uuid> {
            *self.current_swap.read().await
        }

        /// Sends a signal to suspend all ongoing swap processes.
        ///
        /// This function performs the following steps:
        /// 1. Triggers the suspension by sending a unit `()` signal to all listeners via `self.suspension_trigger`.
        /// 2. Polls the `current_swap` state every 50 milliseconds to check if it has been set to `None`, indicating that the swap processes have been suspended and the lock released.
        /// 3. If the lock is not released within 10 seconds, the function returns an error.
        ///
        /// If we send a suspend signal while no swap is in progress, the function will not fail, but will return immediately.
        ///
        /// # Returns
        /// - `Ok(())` if the swap lock is successfully released.
        /// - `Err(Error)` if the function times out waiting for the swap lock to be released.
        ///
        /// # Notes
        /// The 50ms polling interval is considered negligible overhead compared to the typical time required to suspend ongoing swap processes.
        pub async fn send_suspend_signal(&self) -> Result<(), Error> {
            const TIMEOUT: u64 = 10_000;
            const INTERVAL: u64 = 50;

            let _ = self.suspension_trigger.send(())?;

            for _ in 0..(TIMEOUT / INTERVAL) {
                if self.get_current_swap_id().await.is_none() {
                    return Ok(());
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(INTERVAL)).await;
            }

            bail!("Timed out waiting for swap lock to be released");
        }

        pub async fn release_swap_lock(&self) -> Result<Uuid, Error> {
            let mut current_swap = self.current_swap.write().await;
            if let Some(swap_id) = current_swap.as_ref() {
                tracing::debug!(swap_id = %swap_id, "Releasing swap lock");

                let prev_swap_id = *swap_id;
                *current_swap = None;
                drop(current_swap);
                Ok(prev_swap_id)
            } else {
                bail!("There is no current swap lock to release");
            }
        }
    }

    impl Default for SwapLock {
        fn default() -> Self {
            Self::new()
        }
    }
}

pub use swap_lock::{PendingTaskList, SwapLock};

mod context {
    use super::*;
    use crate::cli::EventLoopHandle;

    /// Combined state for the EventLoop and its background task
    /// These must always exist together
    pub struct EventLoopState {
        pub handle: EventLoopHandle,
        #[allow(dead_code)]
        pub task: JoinHandle<()>,
    }

    impl EventLoopState {
        pub fn new(handle: EventLoopHandle, task: JoinHandle<()>) -> Self {
            Self { handle, task }
        }
    }

    /// Holds shared data for different parts of the CLI.
    ///
    /// Some components are optional, allowing initialization of only necessary parts.
    /// For example, the `history` command doesn't require wallet initialization.
    ///
    /// Components are wrapped in Arc<RwLock> to allow independent initialization and cloning.
    #[derive(Clone)]
    pub struct Context {
        pub db: Arc<RwLock<Option<Arc<dyn Database + Send + Sync>>>>,
        pub swap_lock: Arc<SwapLock>,
        pub config: Arc<RwLock<Option<Config>>>,
        pub tasks: Arc<PendingTaskList>,
        pub tauri_handle: Option<TauriHandle>,
        pub(super) bitcoin_wallet: Arc<RwLock<Option<Arc<bitcoin_wallet::Wallet>>>>,
        pub monero_manager: Arc<RwLock<Option<Arc<monero::Wallets>>>>,
        pub(super) tor_client: Arc<RwLock<Option<Arc<TorClient<TokioRustlsRuntime>>>>>,
        #[allow(dead_code)]
        pub(super) monero_rpc_pool_handle: Arc<RwLock<Option<Arc<monero_rpc_pool::PoolHandle>>>>,
        pub(super) event_loop_state: Arc<RwLock<Option<EventLoopState>>>,
    }

    impl Context {
        pub fn new_with_tauri_handle(tauri_handle: TauriHandle) -> Self {
            Self::new(Some(tauri_handle))
        }

        pub fn new_without_tauri_handle() -> Self {
            Self::new(None)
        }

        /// Creates an empty Context with only the swap_lock and tasks initialized
        fn new(tauri_handle: Option<TauriHandle>) -> Self {
            Self {
                db: Arc::new(RwLock::new(None)),
                swap_lock: Arc::new(SwapLock::new()),
                config: Arc::new(RwLock::new(None)),
                tasks: Arc::new(PendingTaskList::default()),
                tauri_handle,
                bitcoin_wallet: Arc::new(RwLock::new(None)),
                monero_manager: Arc::new(RwLock::new(None)),
                tor_client: Arc::new(RwLock::new(None)),
                monero_rpc_pool_handle: Arc::new(RwLock::new(None)),
                event_loop_state: Arc::new(RwLock::new(None)),
            }
        }

        pub async fn status(&self) -> ContextStatus {
            ContextStatus {
                bitcoin_wallet_available: self.try_get_bitcoin_wallet().await.is_ok(),
                monero_wallet_available: self.try_get_monero_manager().await.is_ok(),
                database_available: self.try_get_db().await.is_ok(),
                tor_available: self.try_get_tor_client().await.is_ok(),
            }
        }

        /// Get the Bitcoin wallet, returning an error if not initialized
        pub async fn try_get_bitcoin_wallet(&self) -> Result<Arc<bitcoin_wallet::Wallet>> {
            self.bitcoin_wallet
                .read()
                .await
                .clone()
                .context("Bitcoin wallet not initialized")
        }

        /// Get the Monero manager, returning an error if not initialized
        pub async fn try_get_monero_manager(&self) -> Result<Arc<monero::Wallets>> {
            self.monero_manager
                .read()
                .await
                .clone()
                .context("Monero wallet manager not initialized")
        }

        /// Get the database, returning an error if not initialized
        pub async fn try_get_db(&self) -> Result<Arc<dyn Database + Send + Sync>> {
            self.db
                .read()
                .await
                .clone()
                .context("Database not initialized")
        }

        /// Get the config, returning an error if not initialized
        pub async fn try_get_config(&self) -> Result<Config> {
            self.config
                .read()
                .await
                .clone()
                .context("Config not initialized")
        }

        /// Get the Tor client, returning an error if not initialized
        pub async fn try_get_tor_client(&self) -> Result<Arc<TorClient<TokioRustlsRuntime>>> {
            self.tor_client
                .read()
                .await
                .clone()
                .context("Tor client not initialized")
        }

        /// Get the event loop handle, returning an error if not initialized
        pub async fn try_get_event_loop_handle(&self) -> Result<EventLoopHandle> {
            self.event_loop_state
                .read()
                .await
                .as_ref()
                .map(|state| state.handle.clone())
                .context("Event loop not initialized")
        }

        pub async fn for_harness(
            seed: Seed,
            env_config: EnvConfig,
            db_path: PathBuf,
            bob_bitcoin_wallet: Arc<bitcoin_wallet::Wallet>,
            bob_monero_wallet: Arc<monero::Wallets>,
        ) -> Self {
            let config = Config::for_harness(seed, env_config);
            let db = open_db(db_path, AccessMode::ReadWrite, None)
                .await
                .expect("Could not open sqlite database");

            Self {
                bitcoin_wallet: Arc::new(RwLock::new(Some(bob_bitcoin_wallet))),
                monero_manager: Arc::new(RwLock::new(Some(bob_monero_wallet))),
                config: Arc::new(RwLock::new(Some(config))),
                db: Arc::new(RwLock::new(Some(db))),
                swap_lock: SwapLock::new().into(),
                tasks: PendingTaskList::default().into(),
                tauri_handle: None,
                tor_client: Arc::new(RwLock::new(None)),
                monero_rpc_pool_handle: Arc::new(RwLock::new(None)),
                event_loop_state: Arc::new(RwLock::new(None)),
            }
        }

        pub fn cleanup(&self) -> Result<()> {
            // TODO: close all monero wallets
            // call store(..) on all wallets

            // TODO: This doesn't work because "there is no reactor running, must be called from the context of a Tokio 1.x runtime"
            // let monero_manager = self.monero_manager.clone();
            // tokio::spawn(async move {
            //     if let Some(monero_manager) = monero_manager {
            //         let wallet = monero_manager.main_wallet().await;
            //         wallet.store(None).await;
            //     }
            // });

            Ok(())
        }

        pub async fn bitcoin_wallet(&self) -> Option<Arc<bitcoin_wallet::Wallet>> {
            self.bitcoin_wallet.read().await.clone()
        }

        /// Change the Monero node configuration for all wallets
        pub async fn change_monero_node(&self, node_config: MoneroNodeConfig) -> Result<()> {
            let monero_manager = self.try_get_monero_manager().await?;

            // Determine the daemon configuration based on the node config
            let daemon = match node_config {
                MoneroNodeConfig::Pool => {
                    // Use the pool handle to get server info
                    let pool_handle = self
                        .monero_rpc_pool_handle
                        .read()
                        .await
                        .clone()
                        .context("Pool handle not available")?;

                    let server_info = pool_handle.server_info();
                    let pool_url: String = server_info.clone().into();
                    tracing::info!("Switching to Monero RPC pool: {}", pool_url);

                    monero_sys::Daemon::try_from(pool_url)?
                }
                MoneroNodeConfig::SingleNode { url } => {
                    tracing::info!("Switching to single Monero node: {}", url);

                    monero_sys::Daemon::try_from(url.clone())?
                }
            };

            // Update the wallet manager's daemon configuration
            monero_manager
                .change_monero_node(daemon.clone())
                .await
                .context("Failed to change Monero node in wallet manager")?;

            tracing::info!(?daemon, "Switched Monero node");
