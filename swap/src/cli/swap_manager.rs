//! Owns the lifecycle of Bob state machines.
//!
//! [`SwapManager`] is the single entry point for starting, resuming, suspending
//! and refunding swaps. It internally tracks per-swap [`JoinHandle`]s and
//! force-suspension senders, and coordinates the globally exclusive
//! "initiation" phase (the pre-swap maker selection / deposit waiting) via
//! [`run_exclusive_initiation`](SwapManager::run_exclusive_initiation).
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
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use swap_core::bitcoin;
use swap_env::env::Config as EnvConfig;
use tokio::sync::{Mutex as TokioMutex, RwLock, broadcast};
use tokio::task::JoinHandle;
use tracing::{Instrument, debug_span};
use uuid::Uuid;

const RETRY_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
const RETRY_MAX_INTERVAL: Duration = Duration::from_secs(60);
const TASK_EXIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Builds the [`bob::Swap`] for one attempt of the retry loop. Called with
/// `is_first_attempt = true` exactly once; retries are rebuilt from the DB —
/// `bob::run` persists state transitions itself, so a retry simply picks up
/// whatever was last persisted.
type MakeSwap = Box<dyn FnMut(bool) -> BoxFuture<'static, Result<Swap>> + Send + 'static>;

/// Why a swap-task was asked to suspend. Lets the task decide whether to
/// emit a final `Released` event on the way out: a regular `Terminate`
/// (user-initiated suspend, shutdown, etc.) does emit, but an
/// `ExternalTakeover` (another `start`/`resume`/`cancel_and_refund` is about
/// to take over the swap) suppresses it so the frontend doesn't see a
/// spurious "released" flicker before the new owner emits its own progress
/// event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SuspendReason {
    Terminate,
    ExternalTakeover,
}

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
    suspend: broadcast::Sender<SuspendReason>,
    /// JoinHandle for the spawned state-machine task. `None` once
    /// [`SwapManager::suspend`] has taken it, or when the entry is a refund
    /// reservation made by [`SwapManager::cancel_and_refund`] (which has no
    /// task of its own). Removal of the entry itself is always done by
    /// [`SwapManager::release_running`] on the owning operation's exit path,
    /// so that [`is_running`](SwapManager::is_running) stays true until the
    /// swap has actually finished cleaning up.
    handle: Option<JoinHandle<()>>,
    /// `true` while the task is sleeping in retry backoff after an error.
    /// In that state the state machine is idle, so `start`/`resume`/
    /// `cancel_and_refund` can pre-empt the pending retry by signalling
    /// `ExternalTakeover` on `suspend` rather than bailing.
    in_retry_backoff: bool,
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

    async fn acquire_initiation_lock(&self, swap_id: Uuid) -> Result<()> {
        let mut current = self.current_initiation.write().await;
        if current.is_some() {
            bail!("There already exists an active swap initiation");
        }
        tracing::debug!(%swap_id, "Acquiring swap initiation lock");
        *current = Some(swap_id);
        Ok(())
    }

    async fn release_initiation_lock(&self, swap_id: Uuid) -> Result<()> {
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

    /// Acquire the initiation lock for `swap_id`, run `body` while listening
    /// for force-suspension, and release the lock on every exit path. The
    /// lock is held across the *entire* `body`, so callers can perform DB
    /// writes and spawn the state-machine task without a gap between
    /// selection and registration.
    ///
    /// Returns `Ok(None)` if the initiation was force-suspended, otherwise
    /// `Ok(Some(value))` where `value` is whatever `body` produced.
    pub async fn run_exclusive_initiation<F, T>(
        &self,
        swap_id: Uuid,
        body: F,
        tauri_handle: Option<TauriHandle>,
    ) -> Result<Option<T>>
    where
        F: Future<Output = Result<T>>,
    {
        self.acquire_initiation_lock(swap_id).await?;

        let result = tokio::select! {
            result = body => result.map(Some),
            _ = self.await_initiation_force_suspension() => Ok(None),
        };

        // Unless `body` completed and spawned the state machine, nothing will
        // ever emit another progress event for this swap — send a final
        // `Released` so the frontend can drop the entry.
        if !matches!(result, Ok(Some(_))) {
            tauri_handle.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::Released {
                    next_auto_resume_at_unix_ms: None,
                },
            );
        }

        self.release_initiation_lock(swap_id)
            .await
            .context("Failed to release initiation lock")?;
        result
    }

    /// Start a fresh swap state machine.
    ///
    /// Persists peer/address/monero-pool to the DB, registers the swap as
    /// running, and spawns the [`bob::run`] task. The task retries the state
    /// machine with exponential backoff on error and exits when either:
    ///   - `bob::run` returns `Ok` (the swap reached a terminal state), or
    ///   - [`suspend`](Self::suspend) is called for `swap_id`.
    ///
    /// The pre-swap maker selection (currently `determine_btc_to_swap`) must
    /// run before calling this and produce the [`StartSwapInputs`]. Use
    /// [`run_exclusive_initiation`](Self::run_exclusive_initiation) to guard
    /// that pre-swap phase.
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
            .queue_peer_address(seller_peer_id, seller_multiaddr)
            .await?;

        let make_swap: MakeSwap = Box::new({
            let tauri_handle = tauri_handle.clone();
            move |is_first_attempt| {
                let mut event_loop_handle = event_loop_handle.clone();
                let db = Arc::clone(&db);
                let bitcoin_wallet = Arc::clone(&bitcoin_wallet);
                let monero_wallet = Arc::clone(&monero_wallet);
                let monero_receive_pool = monero_receive_pool.clone();
                let bitcoin_change_address = bitcoin_change_address.clone();
                let tauri_handle = tauri_handle.clone();
                Box::pin(async move {
                    let swap_handle = event_loop_handle
                        .swap_handle(seller_peer_id, swap_id)
                        .await?;
                    let swap = if is_first_attempt {
                        Swap::new(
                            db,
                            swap_id,
                            bitcoin_wallet,
                            monero_wallet,
                            env_config,
                            swap_handle,
                            monero_receive_pool,
                            bitcoin_change_address,
                            tx_lock_amount,
                            tx_lock_fee,
                        )
                    } else {
                        Swap::from_db(
                            db,
                            swap_id,
                            bitcoin_wallet,
                            monero_wallet,
                            env_config,
                            swap_handle,
                            monero_receive_pool,
                        )
                        .await?
                    };
                    Ok(swap.with_event_emitter(tauri_handle))
                })
            }
        });

        self.spawn_swap_task(swap_id, tauri_handle, make_swap).await
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

        let make_swap: MakeSwap = Box::new({
            let tauri_handle = tauri_handle.clone();
            move |is_first_attempt| {
                let mut event_loop_handle = event_loop_handle.clone();
                let db = Arc::clone(&db);
                let bitcoin_wallet = Arc::clone(&bitcoin_wallet);
                let monero_wallet = Arc::clone(&monero_wallet);
                let monero_receive_pool = monero_receive_pool.clone();
                let tauri_handle = tauri_handle.clone();
                Box::pin(async move {
                    if is_first_attempt {
                        tauri_handle
                            .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Resuming);
                    }
                    let swap_handle = event_loop_handle
                        .swap_handle(seller_peer_id, swap_id)
                        .await?;
                    let swap = Swap::from_db(
                        db,
                        swap_id,
                        bitcoin_wallet,
                        monero_wallet,
                        env_config,
                        swap_handle,
                        monero_receive_pool,
                    )
                    .await?;
                    Ok(swap.with_event_emitter(tauri_handle))
                })
            }
        });

        self.spawn_swap_task(swap_id, tauri_handle, make_swap).await
    }

    /// Resume every Bob swap that is in a resumable state.
    ///
    /// A swap is considered resumable when it has not reached a terminal
    /// state and is not already running. Each resumable swap is started via
    /// [`resume`](Self::resume); failures for individual swaps are logged
    /// and skipped, so one bad swap does not prevent the rest from resuming.
    pub async fn resume_all(
        self: &Arc<Self>,
        db: Arc<dyn Database + Send + Sync>,
        bitcoin_wallet: Arc<bitcoin_wallet::Wallet>,
        monero_wallet: Arc<monero::Wallets>,
        env_config: EnvConfig,
        event_loop_handle: EventLoopHandle,
        tauri_handle: Option<TauriHandle>,
    ) -> Result<Vec<Uuid>> {
        let swaps = db.all().await.context("Failed to load swaps from db")?;

        let mut resumed = Vec::new();
        for (_, swap_id, state) in swaps {
            let crate::protocol::State::Bob(bob_state) = &state else {
                continue;
            };
            if !bob::is_resumable(bob_state) {
                continue;
            }
            if self.is_running(swap_id).await {
                continue;
            }
            if !db
                .get_auto_resume(swap_id)
                .await
                .context("Failed to read auto-resume preference")?
            {
                continue;
            }

            // Match the per-swap span that `request()` attaches for a
            // single `resume_swap` call so the spawned state-machine task
            // is tagged with `swap{swap_id=…}` and log lines stay
            // filterable by swap.
            let swap_span = debug_span!("swap", %swap_id);
            match self
                .resume(
                    swap_id,
                    Arc::clone(&db),
                    Arc::clone(&bitcoin_wallet),
                    Arc::clone(&monero_wallet),
                    env_config,
                    event_loop_handle.clone(),
                    tauri_handle.clone(),
                )
                .instrument(swap_span)
                .await
            {
                Ok(()) => resumed.push(swap_id),
                Err(error) => {
                    tracing::error!(%swap_id, "Failed to resume swap: {:#}", error);
                }
            }
        }

        Ok(resumed)
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
            let _ = entry.suspend.send(SuspendReason::Terminate);
            entry.handle.take()
        };

        tracing::debug!(%swap_id, "Awaiting state machine task completion after suspend");
        self.await_task_exit(swap_id, handle).await
    }

    /// If a swap-task is currently sleeping in retry backoff, signal it to
    /// exit silently and await its completion. No-op if the swap is not
    /// running, or is running but not in backoff.
    ///
    /// Used by `start`, `resume`, and `cancel_and_refund` to take over a swap
    /// whose state machine is idle between retries.
    async fn cancel_pending_retry_if_any(&self, swap_id: Uuid) -> Result<()> {
        let handle = {
            let mut running = self.running.lock().await;
            let Some(entry) = running.get_mut(&swap_id) else {
                return Ok(());
            };
            if !entry.in_retry_backoff {
                return Ok(());
            }
            let _ = entry.suspend.send(SuspendReason::ExternalTakeover);
            entry.handle.take()
        };

        tracing::debug!(%swap_id, "Awaiting pending-retry task exit before takeover");
        self.await_task_exit(swap_id, handle).await
    }

    /// Await the exit of a swap task whose suspension has already been
    /// signalled. `None` means another caller has taken the [`JoinHandle`];
    /// in that case fall back to polling the running map so this call still
    /// upholds the "returns only after the swap is no longer running"
    /// contract.
    async fn await_task_exit(&self, swap_id: Uuid, handle: Option<JoinHandle<()>>) -> Result<()> {
        let Some(handle) = handle else {
            return self.wait_until_not_running(swap_id).await;
        };

        match tokio::time::timeout(TASK_EXIT_TIMEOUT, handle).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(join_err)) => {
                Err(Error::from(join_err).context("Swap task panicked while shutting down"))
            }
            Err(_) => bail!("Timed out waiting for swap task to exit"),
        }
    }

    /// Set the `in_retry_backoff` flag on the running entry. Called by the
    /// task when it enters / exits the inter-retry sleep.
    async fn set_in_retry_backoff(&self, swap_id: Uuid, value: bool) {
        let mut running = self.running.lock().await;
        if let Some(entry) = running.get_mut(&swap_id) {
            entry.in_retry_backoff = value;
        }
    }

    async fn suspend_initiation(&self, swap_id: Uuid) -> Result<()> {
        let _ = self.initiation_suspend.send(());
        self.wait_until_not_initiating(swap_id).await
    }

    async fn wait_until_not_initiating(&self, swap_id: Uuid) -> Result<()> {
        wait_until(|| async { self.current_initiation_swap_id().await != Some(swap_id) })
            .await
            .context("Timed out waiting for swap initiation lock to be released")
    }

    async fn wait_until_not_running(&self, swap_id: Uuid) -> Result<()> {
        wait_until(|| async { !self.is_running(swap_id).await })
            .await
            .context("Timed out waiting for swap to exit")
    }

    /// Cancel and refund a swap. Bails if the swap is actively running (its
    /// state machine is in flight), since the running state machine is
    /// responsible for its own refunds. A swap that is sleeping in retry
    /// backoff is pre-empted: we cancel the pending retry and then run the
    /// refund ourselves.
    pub async fn cancel_and_refund(
        &self,
        swap_id: Uuid,
        bitcoin_wallet: Arc<bitcoin_wallet::Wallet>,
        db: Arc<dyn Database + Send + Sync>,
        tauri_handle: Option<TauriHandle>,
    ) -> Result<BobState> {
        self.cancel_pending_retry_if_any(swap_id).await?;

        // Reserve the running slot for the duration of the refund so no
        // concurrent `start`/`resume` can spawn a state machine that races
        // our refund transaction.
        {
            let mut running = self.running.lock().await;
            if running.contains_key(&swap_id) {
                bail!("Cannot cancel and refund swap {swap_id} because it is currently running");
            }
            running.insert(
                swap_id,
                RunningSwap {
                    suspend: broadcast::channel(1).0,
                    handle: None,
                    in_retry_backoff: false,
                },
            );
        }

        let result = cli::cancel_and_refund(swap_id, bitcoin_wallet, db).await;
        self.release_running(swap_id).await;

        tauri_handle.emit_swap_progress_event(
            swap_id,
            TauriSwapProgressEvent::Released {
                next_auto_resume_at_unix_ms: None,
            },
        );

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
    /// The running-map lock is held across the conflict check, the spawn and
    /// the insert. The task's first access to the map (`release_running` /
    /// `set_in_retry_backoff`) blocks on that same lock, so the entry is
    /// guaranteed to be in place before the task can observe the map.
    async fn spawn_swap_task(
        self: &Arc<Self>,
        swap_id: Uuid,
        tauri_handle: Option<TauriHandle>,
        make_swap: MakeSwap,
    ) -> Result<()> {
        // If this swap is currently asleep between retries, pre-empt it: the
        // existing task will exit silently and free the slot. An actively-
        // running task is left alone (the conflict check below will surface
        // a clear error to the caller).
        self.cancel_pending_retry_if_any(swap_id).await?;

        let mut running = self.running.lock().await;
        if running.contains_key(&swap_id) {
            bail!("Swap {swap_id} is already running");
        }

        let suspend_tx = broadcast::channel::<SuspendReason>(10).0;
        let suspend_rx = suspend_tx.subscribe();
        let handle = tokio::spawn(
            run_swap_task(
                Arc::clone(self),
                swap_id,
                suspend_rx,
                tauri_handle,
                make_swap,
            )
            .instrument(tracing::Span::current()),
        );

        running.insert(
            swap_id,
            RunningSwap {
                suspend: suspend_tx,
                handle: Some(handle),
                in_retry_backoff: false,
            },
        );
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

/// Drive a single swap task. Retries the state machine with exponential
/// backoff on `Err`, exits on `Ok` (terminal state reached) or on receipt of
/// a force-suspension signal. Always releases the running-map entry and
/// (unless pre-empted by an external takeover) emits a final `Released` on
/// exit.
///
/// The retry behaviour is intentional: individual states inside `bob::run`
/// already retry their own operations, but `bob::run` itself can still
/// return `Err`. While we're sleeping between retries the `in_retry_backoff`
/// flag is set on our running entry so `start`/`resume`/`cancel_and_refund`
/// can pre-empt us instead of bailing with "already running".
async fn run_swap_task(
    manager: Arc<SwapManager>,
    swap_id: Uuid,
    mut suspend_rx: broadcast::Receiver<SuspendReason>,
    tauri_handle: Option<TauriHandle>,
    mut make_swap: MakeSwap,
) {
    let mut backoff = backoff::ExponentialBackoffBuilder::new()
        .with_initial_interval(RETRY_INITIAL_INTERVAL)
        .with_max_interval(RETRY_MAX_INTERVAL)
        // Retry indefinitely; the only stop conditions are Ok or suspend.
        .with_max_elapsed_time(None)
        .build();

    let mut external_takeover = false;
    let mut is_first_attempt = true;

    'retry: loop {
        let outcome: Result<BobState> = tokio::select! {
            biased;
            reason = suspend_rx.recv() => {
                tracing::debug!(%swap_id, "Suspend signal received, exiting state machine");
                external_takeover = matches!(reason, Ok(SuspendReason::ExternalTakeover));
                break 'retry;
            }
            result = async {
                let swap = make_swap(is_first_attempt).await?;
                bob::run(swap).await
            } => result,
        };
        is_first_attempt = false;

        match outcome {
            Ok(state) => {
                tracing::debug!(%swap_id, %state, "Swap completed");
                break 'retry;
            }
            Err(error) => {
                let next = backoff.next_backoff().unwrap_or(RETRY_MAX_INTERVAL);
                let next_at_unix_ms = unix_now_ms().saturating_add(next.as_millis() as u64);

                tracing::error!(
                    %swap_id,
                    retry_in_secs = next.as_secs(),
                    "Swap state machine failed: {:#}; retrying",
                    error,
                );

                // Mark the slot as idle and tell the frontend we've released
                // the swap *with* a hint about when we'll auto-resume — the
                // user can manually resume / cancel during this window and
                // pre-empt us.
                manager.set_in_retry_backoff(swap_id, true).await;
                tauri_handle.emit_swap_progress_event(
                    swap_id,
                    TauriSwapProgressEvent::Released {
                        next_auto_resume_at_unix_ms: Some(next_at_unix_ms),
                    },
                );

                tokio::select! {
                    biased;
                    reason = suspend_rx.recv() => {
                        tracing::debug!(
                            %swap_id,
                            "Suspend signal received during retry backoff, exiting state machine",
                        );
                        external_takeover = matches!(reason, Ok(SuspendReason::ExternalTakeover));
                        break 'retry;
                    }
                    _ = tokio::time::sleep(next) => {}
                }

                // Sleep finished naturally — clear the flag so the next
                // iteration's attempt runs under "actively running"
                // semantics again.
                manager.set_in_retry_backoff(swap_id, false).await;
            }
        }
    }

    manager.release_running(swap_id).await;

    // Suppress the final Released only when another caller is about to take
    // over the swap and will emit its own progress event. This avoids a
    // brief "released" flash in the frontend between takeovers.
    if !external_takeover {
        tauri_handle.emit_swap_progress_event(
            swap_id,
            TauriSwapProgressEvent::Released {
                next_auto_resume_at_unix_ms: None,
            },
        );
    }
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is set before the unix epoch")
        .as_millis() as u64
}

/// Poll `predicate` every 50ms until it returns true, for at most
/// [`TASK_EXIT_TIMEOUT`]. Used as a fallback when we no longer own a
/// [`JoinHandle`] for the task we're waiting on.
async fn wait_until<F, Fut>(mut predicate: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let poll = async {
        while !predicate().await {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(TASK_EXIT_TIMEOUT, poll)
        .await
        .map_err(Error::from)
}
