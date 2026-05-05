//! Owns the lifecycle of Bob state machines.
//!
//! [`SwapManager`] is the single entry point for starting, resuming, suspending
//! and refunding swaps. It internally tracks per-swap [`JoinHandle`]s and
//! force-suspension senders, and coordinates the globally exclusive
//! "initiation" phase (the pre-swap maker selection / deposit waiting) via
//! [`run_exclusive_initiation`].
//!
//! Read-only swap inspection (history, swap info, timelock checks, monero
//! recovery) intentionally stays in `cli::api::request` — this manager is
//! about state-machine lifecycle, not generic database access.

use crate::cli;
use crate::cli::EventLoopHandle;
use crate::cli::api::tauri_bindings::{TauriEmitter, TauriHandle, TauriSwapProgressEvent};
use crate::monero;
use crate::monero::MoneroAddressPool;
use crate::protocol::Database;
use crate::protocol::bob::{self, BobState, Swap};
use anyhow::{Context as AnyContext, Error, Result, bail};
use backoff::backoff::Backoff;
use futures::future::{BoxFuture, try_join_all};
use libp2p::{Multiaddr, PeerId};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use swap_core::bitcoin;
use swap_env::env::Config as EnvConfig;
use tokio::sync::{Mutex as TokioMutex, RwLock, broadcast, oneshot};
use tokio::task::JoinHandle;
use tracing::Instrument;
use uuid::Uuid;

const RETRY_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
const RETRY_MAX_INTERVAL: Duration = Duration::from_secs(60);

/// Closure that produces a fresh [`bob::Swap`] for each attempt of a retry
/// loop. The closure is invoked once per retry; it is responsible for
/// reloading state from the DB on subsequent invocations and for registering
/// a fresh swap-handle with the event loop.
type MakeSwap = Box<dyn FnMut() -> BoxFuture<'static, Result<Swap>> + Send + 'static>;

/// Inputs needed to start a fresh swap, after the user has selected a maker
/// and the wallet has enough deposited bitcoin to cover the lock amount + fee.
pub struct StartSwapInputs {
    pub swap_id: Uuid,
    pub seller_peer_id: PeerId,
    pub seller_multiaddr: Multiaddr,
    pub monero_receive_pool: MoneroAddressPool,
    pub bitcoin_change_address: bitcoin::Address,
    pub tx_lock_amount: bitcoin::Amount,
    pub tx_lock_fee: bitcoin::Amount,
}

/// Owns the lifecycle of Bob state machines.
pub struct SwapManager {
    /// Per-swap force-suspension senders + JoinHandles.
    running: TokioMutex<HashMap<Uuid, RunningSwap>>,
    /// Tracks the currently-running initiation phase, if any.
    current_initiation: RwLock<Option<Uuid>>,
    /// Trigger to force-suspend the currently-running initiation.
    initiation_suspend: broadcast::Sender<()>,
}

struct RunningSwap {
    /// Force-suspension trigger for this swap's state machine task.
    suspend: broadcast::Sender<()>,
    /// JoinHandle for the spawned state-machine task. `None` once
    /// [`SwapManager::suspend`] has taken it. Removal of the entry itself is
    /// always done by [`SwapManager::release_running`] on the task's exit
    /// path, so that [`is_running`](Self::is_running) stays true until the
    /// state machine has actually finished cleaning up.
    handle: Option<JoinHandle<()>>,
}

impl SwapManager {
    pub fn new() -> Self {
        let (initiation_suspend, _) = broadcast::channel(10);
        Self {
            running: TokioMutex::new(HashMap::new()),
            current_initiation: RwLock::new(None),
            initiation_suspend,
        }
    }

    /// Whether a swap state machine is currently running.
    pub async fn is_running(&self, swap_id: Uuid) -> bool {
        self.running.lock().await.contains_key(&swap_id)
    }

    /// Returns the swap-ids of all currently running swaps.
    pub async fn running_swap_ids(&self) -> Vec<Uuid> {
        self.running.lock().await.keys().copied().collect()
    }

    /// Returns the swap-id of the swap currently in its initiation phase, if any.
    pub async fn current_initiation_swap_id(&self) -> Option<Uuid> {
        *self.current_initiation.read().await
    }

    /// Acquire the globally exclusive initiation lock for `swap_id`. Most
    /// callers should use [`run_exclusive_initiation`] instead, which pairs
    /// this with the suspension `select!` and an unconditional release.
    pub async fn acquire_initiation_lock(&self, swap_id: Uuid) -> Result<()> {
        let mut current = self.current_initiation.write().await;
        if current.is_some() {
            bail!("There already exists an active swap initiation");
        }
        tracing::debug!(%swap_id, "Acquiring swap initiation lock");
        *current = Some(swap_id);
        Ok(())
    }

    /// Release the initiation lock for `swap_id`.
    pub async fn release_initiation_lock(&self, swap_id: Uuid) -> Result<()> {
        let mut current = self.current_initiation.write().await;
        let Some(current_swap_id) = *current else {
            bail!("There is no current swap initiation lock to release");
        };
        if current_swap_id != swap_id {
            bail!(
                "Cannot release swap initiation lock for {swap_id}; current initiation is {current_swap_id}"
            );
        }
        tracing::debug!(%swap_id, "Releasing swap initiation lock");
        *current = None;
        Ok(())
    }

    /// Start a fresh swap state machine.
    ///
    /// Persists peer/address/monero-pool to the DB, registers the swap as
    /// running, and spawns the [`bob::run`] task. The task retries the state
    /// machine with exponential backoff on error and exits when either:
    ///   - `bob::run` returns `Ok` (the swap reached a terminal state), or
    ///   - [`suspend`](Self::suspend) is called for `swap_id`.
    ///
    /// The first attempt uses [`Swap::new`] with the inputs supplied here;
    /// subsequent retries reload state from the DB via [`Swap::from_db`]
    /// (which sees whatever progress `bob::run` persisted on the previous
    /// attempt).
    ///
    /// The pre-swap maker selection (currently `determine_btc_to_swap`) must
    /// run before calling this and produce the [`StartSwapInputs`]. Use
    /// [`run_exclusive_initiation`] to guard that pre-swap phase.
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        self: &Arc<Self>,
        inputs: StartSwapInputs,
        db: Arc<dyn Database + Send + Sync>,
        bitcoin_wallet: Arc<bitcoin_wallet::Wallet>,
        monero_wallet: Arc<monero::Wallets>,
        env_config: EnvConfig,
        mut event_loop_handle: EventLoopHandle,
        tauri_handle: Option<TauriHandle>,
    ) -> Result<()> {
        let StartSwapInputs {
            swap_id,
            seller_peer_id,
            seller_multiaddr,
            monero_receive_pool,
            bitcoin_change_address,
            tx_lock_amount,
            tx_lock_fee,
        } = inputs;

        db.insert_peer_id(swap_id, seller_peer_id).await?;
        db.insert_address(seller_peer_id, seller_multiaddr.clone())
            .await?;
        db.insert_monero_address_pool(swap_id, monero_receive_pool.clone())
            .await?;

        event_loop_handle
            .queue_peer_address(seller_peer_id, seller_multiaddr.clone())
            .await?;

        // Persist the initial `Started` state so every retry — including the
        // very first one if its prior attempt failed before any transition —
        // can uniformly reload via `Swap::from_db`.
        let initial_state = BobState::Started {
            btc_amount: tx_lock_amount,
            tx_lock_fee,
            change_address: bitcoin_change_address,
        };
        db.insert_latest_state(swap_id, initial_state.into())
            .await
            .context("Failed to persist initial swap state")?;

        let tauri_handle_for_release = tauri_handle.clone();

        let make_swap: MakeSwap = Box::new(move || {
            let mut event_loop_handle = event_loop_handle.clone();
            let db = Arc::clone(&db);
            let bitcoin_wallet = Arc::clone(&bitcoin_wallet);
            let monero_wallet = Arc::clone(&monero_wallet);
            let monero_receive_pool = monero_receive_pool.clone();
            let tauri_handle = tauri_handle.clone();
            Box::pin(async move {
                let swap_event_loop_handle = event_loop_handle
                    .swap_handle(seller_peer_id, swap_id)
                    .await?;
                let swap = Swap::from_db(
                    db,
                    swap_id,
                    bitcoin_wallet,
                    monero_wallet,
                    env_config,
                    swap_event_loop_handle,
                    monero_receive_pool,
                )
                .await?
                .with_event_emitter(tauri_handle);
                Ok(swap)
            })
        });

        self.spawn_swap_task(swap_id, tauri_handle_for_release, make_swap)
            .await
    }

    /// Resume a swap state machine from its persisted state.
    /// Retries with exponential backoff until completion or suspension.
    pub async fn resume(
        self: &Arc<Self>,
        swap_id: Uuid,
        db: Arc<dyn Database + Send + Sync>,
        bitcoin_wallet: Arc<bitcoin_wallet::Wallet>,
        monero_wallet: Arc<monero::Wallets>,
        env_config: EnvConfig,
        mut event_loop_handle: EventLoopHandle,
        tauri_handle: Option<TauriHandle>,
    ) -> Result<()> {
        let seller_peer_id = db.get_peer_id(swap_id).await?;
        let seller_addresses = db.get_addresses(seller_peer_id).await?;
        for addr in seller_addresses {
            event_loop_handle
                .queue_peer_address(seller_peer_id, addr)
                .await?;
        }

        let monero_receive_pool = db.get_monero_address_pool(swap_id).await?;

        tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Resuming);

        let tauri_handle_for_release = tauri_handle.clone();

        let make_swap: MakeSwap = Box::new(move || {
            let mut event_loop_handle = event_loop_handle.clone();
            let db = Arc::clone(&db);
            let bitcoin_wallet = Arc::clone(&bitcoin_wallet);
            let monero_wallet = Arc::clone(&monero_wallet);
            let monero_receive_pool = monero_receive_pool.clone();
            let tauri_handle = tauri_handle.clone();
            Box::pin(async move {
                let swap_event_loop_handle = event_loop_handle
                    .swap_handle(seller_peer_id, swap_id)
                    .await?;
                let swap = Swap::from_db(
                    db,
                    swap_id,
                    bitcoin_wallet,
                    monero_wallet,
                    env_config,
                    swap_event_loop_handle,
                    monero_receive_pool,
                )
                .await?
                .with_event_emitter(tauri_handle);
                Ok(swap)
            })
        });

        self.spawn_swap_task(swap_id, tauri_handle_for_release, make_swap)
            .await
    }

    /// Suspend a swap.
    ///
    /// If `swap_id` is currently in the initiation phase, sends an initiation
    /// suspend signal and waits for the lock to be released. Otherwise sends a
    /// per-swap suspend signal and awaits the spawned task's completion. The
    /// running-map entry is left in place; the task's own exit path
    /// ([`release_running`](Self::release_running)) is what removes it, so
    /// [`is_running`](Self::is_running) stays true until the state machine has
    /// finished cleaning up.
    pub async fn suspend(&self, swap_id: Uuid) -> Result<()> {
        if self.current_initiation_swap_id().await == Some(swap_id) {
            return self.suspend_initiation(swap_id).await;
        }

        let handle = {
            let mut running = self.running.lock().await;
            let Some(entry) = running.get_mut(&swap_id) else {
                return Ok(());
            };
            // Best-effort: a task with no live subscriber means it already
            // raced past the select! and we'll just await it below.
            let _ = entry.suspend.send(());
            entry.handle.take()
        };

        let Some(handle) = handle else {
            // Another suspend has already taken the handle. Fall back to
            // polling so this call still upholds the "returns only after the
            // swap is no longer running" contract.
            return self.wait_until_not_running(swap_id).await;
        };

        tracing::debug!(%swap_id, "Awaiting state machine task completion after suspend");
        match tokio::time::timeout(Duration::from_secs(10), handle).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(join_err)) => {
                Err(Error::from(join_err)
                    .context("State machine task panicked while shutting down"))
            }
            Err(_) => bail!("Timed out waiting for swap state machine task to exit"),
        }
    }

    async fn suspend_initiation(&self, swap_id: Uuid) -> Result<()> {
        let _ = self.initiation_suspend.send(());
        self.wait_until_not_initiating(swap_id).await
    }

    async fn wait_until_not_initiating(&self, swap_id: Uuid) -> Result<()> {
        wait_with_timeout(|| async { self.current_initiation_swap_id().await != Some(swap_id) })
            .await
            .map_err(|_| {
                anyhow::anyhow!("Timed out waiting for swap initiation lock to be released")
            })
    }

    async fn wait_until_not_running(&self, swap_id: Uuid) -> Result<()> {
        wait_with_timeout(|| async { !self.is_running(swap_id).await })
            .await
            .map_err(|_| anyhow::anyhow!("Timed out waiting for swap to exit"))
    }

    /// Cancel and refund a swap. Bails if the swap is currently running, since
    /// the running state machine is responsible for its own refunds.
    pub async fn cancel_and_refund(
        &self,
        swap_id: Uuid,
        bitcoin_wallet: Arc<bitcoin_wallet::Wallet>,
        db: Arc<dyn Database + Send + Sync>,
        tauri_handle: Option<TauriHandle>,
    ) -> Result<BobState> {
        if self.is_running(swap_id).await {
            bail!("Cannot cancel and refund swap {swap_id} because it is currently running");
        }

        let result = cli::cancel_and_refund(swap_id, bitcoin_wallet, db).await;

        tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

        result
    }

    /// Wait for all currently-running swap tasks to complete.
    /// Used during graceful shutdown.
    pub async fn wait_for_tasks(&self) -> Result<()> {
        let handles: Vec<JoinHandle<()>> = {
            let mut running = self.running.lock().await;
            running
                .values_mut()
                .filter_map(|entry| entry.handle.take())
                .collect()
        };

        try_join_all(handles)
            .await
            .context("Failed to await running swap tasks")?;
        Ok(())
    }

    /// Subscribe to the initiation force-suspension signal. Used internally
    /// by [`run_exclusive_initiation`].
    async fn await_initiation_force_suspension(&self) -> Result<()> {
        let mut listener = self.initiation_suspend.subscribe();
        listener
            .recv()
            .await
            .context("initiation suspend channel closed")?;
        Ok(())
    }

    /// Spawn `make_swap` as a tracked, retrying state-machine task under
    /// `swap_id`. See [`run_swap_task`] for the retry semantics.
    ///
    /// The spawn / register sequence is gated on a oneshot so that the
    /// running map entry is guaranteed to exist (with the real
    /// [`JoinHandle`]) before any code in `make_swap` executes — this rules
    /// out a race in which `release_running` is called by the task before
    /// the entry is inserted, or `suspend` finds an entry whose handle is a
    /// placeholder.
    async fn spawn_swap_task(
        self: &Arc<Self>,
        swap_id: Uuid,
        tauri_handle: Option<TauriHandle>,
        make_swap: MakeSwap,
    ) -> Result<()> {
        let suspend_tx = broadcast::channel::<()>(10).0;
        let suspend_rx = suspend_tx.subscribe();
        let (gate_tx, gate_rx) = oneshot::channel::<()>();

        let manager = Arc::clone(self);
        let span = tracing::Span::current();
        let handle = tokio::spawn(
            async move {
                if gate_rx.await.is_err() {
                    return;
                }
                run_swap_task(manager, swap_id, suspend_rx, tauri_handle, make_swap).await;
            }
            .instrument(span),
        );

        {
            let mut running = self.running.lock().await;
            if running.contains_key(&swap_id) {
                handle.abort();
                bail!("Swap {swap_id} is already running");
            }
            running.insert(
                swap_id,
                RunningSwap {
                    suspend: suspend_tx,
                    handle: Some(handle),
                },
            );
        }

        let _ = gate_tx.send(());
        tracing::debug!(%swap_id, "Registered running swap");
        Ok(())
    }

    async fn release_running(&self, swap_id: Uuid) {
        let mut running = self.running.lock().await;
        if running.remove(&swap_id).is_some() {
            tracing::debug!(%swap_id, "Released running swap");
        }
    }
}

impl Default for SwapManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Acquire the initiation lock for `swap_id`, run `body` while listening for
/// force-suspension, and release the lock on every exit path. The lock is
/// held across the *entire* `body`, so callers can perform DB writes and
/// spawn the state-machine task without a gap between selection and
/// registration.
///
/// Returns `Ok(None)` if the initiation was force-suspended, otherwise
/// `Ok(Some(value))` where `value` is whatever `body` produced.
pub async fn run_exclusive_initiation<F, T>(
    manager: &SwapManager,
    swap_id: Uuid,
    body: F,
    tauri_handle: Option<TauriHandle>,
) -> Result<Option<T>>
where
    F: Future<Output = Result<T>>,
{
    manager.acquire_initiation_lock(swap_id).await?;

    let result = tokio::select! {
        result = body => result.map(Some),
        _ = manager.await_initiation_force_suspension() => {
            tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);
            Ok(None)
        }
    };

    manager
        .release_initiation_lock(swap_id)
        .await
        .context("Failed to release initiation lock")?;
    result
}

/// Drive a single swap task. Retries the state machine with exponential
/// backoff on `Err`, exits on `Ok` (terminal state reached) or on receipt of
/// a force-suspension signal. Always releases the running-map entry and
/// emits `Released` on exit.
///
/// The retry behaviour is intentional: individual states inside `bob::run`
/// already retry their own operations, but `bob::run` itself can still
/// return `Err`.
async fn run_swap_task(
    manager: Arc<SwapManager>,
    swap_id: Uuid,
    mut suspend_rx: broadcast::Receiver<()>,
    tauri_handle: Option<TauriHandle>,
    mut make_swap: MakeSwap,
) {
    let mut backoff = backoff::ExponentialBackoffBuilder::new()
        .with_initial_interval(RETRY_INITIAL_INTERVAL)
        .with_max_interval(RETRY_MAX_INTERVAL)
        // Retry indefinitely; the only stop conditions are Ok or suspend.
        .with_max_elapsed_time(None)
        .build();

    'retry: loop {
        let outcome: Result<BobState> = tokio::select! {
            biased;
            _ = suspend_rx.recv() => {
                tracing::debug!(%swap_id, "Suspend signal received, exiting state machine");
                break 'retry;
            }
            result = async {
                let swap = make_swap().await?;
                bob::run(swap).await
            } => result,
        };

        match outcome {
            Ok(state) => {
                tracing::debug!(%swap_id, %state, "Swap completed");
                break 'retry;
            }
            Err(error) => {
                let next = backoff.next_backoff().unwrap_or(RETRY_MAX_INTERVAL);
                tracing::error!(
                    %swap_id,
                    retry_in_secs = next.as_secs(),
                    "Swap state machine failed: {:#}; retrying",
                    error,
                );

                tokio::select! {
                    biased;
                    _ = suspend_rx.recv() => {
                        tracing::debug!(
                            %swap_id,
                            "Suspend signal received during retry backoff, exiting state machine",
                        );
                        break 'retry;
                    }
                    _ = tokio::time::sleep(next) => {}
                }
            }
        }
    }

    manager.release_running(swap_id).await;
    tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);
}

/// Poll `predicate` every 50ms for up to 10s, returning `Ok(())` when it
/// returns true and `Err(())` on timeout. Used as a fallback for the rare
/// suspend-after-suspend case where we no longer own a JoinHandle.
async fn wait_with_timeout<F, Fut>(mut predicate: F) -> Result<(), ()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    const TIMEOUT_MS: u64 = 10_000;
    const INTERVAL_MS: u64 = 50;
    for _ in 0..(TIMEOUT_MS / INTERVAL_MS) {
        if predicate().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(INTERVAL_MS)).await;
    }
    Err(())
}
