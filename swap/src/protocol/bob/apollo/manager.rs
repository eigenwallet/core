//! The Apollo manager: one actor that publishes the Apollo transactions of all swaps.
//!
//! The manager owns all publisher state. Other code only talks to it through an [`ApolloManagerHandle`]:
//! it wakes the manager after it wrote a progress row, and it watches the [`ApolloStatus`].
//! The database is the source of truth. The manager reads the pending rows on every wake and on every poll tick,
//! so a lost wake or a stopped publisher only delays a publish.

use anyhow::{Context, Result, ensure};
use bitcoin::{OutPoint, Transaction};
use bitcoin_wallet::BitcoinWallet;
use bitcoin_wallet::primitives::ScriptStatus;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use swap_machine::bob::ApolloProgress;
use tokio::sync::{mpsc, watch};
use tokio::task::{AbortHandle, JoinHandle, JoinSet};
use tokio::time::{Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use typeshare::typeshare;
use uuid::Uuid;

use super::{
    TX_APOLLO_MIN_CONFIRMATIONS, ensure_tx_apollo_published, lock_conflicted, lock_is_seen,
    retry_db,
};
use crate::protocol::Database;

/// Concurrent publishers. More pending swaps wait for a free slot.
const MAX_CONCURRENT_PUBLISHERS: usize = 16;
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60);
/// Delay before the manager starts a publisher again that failed or panicked. It doubles up to the maximum.
const MIN_RESTART_BACKOFF: Duration = Duration::from_secs(10);
const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(5 * 60);
/// A wake is a hint. One queued wake covers all wakes that follow, so a tiny queue is enough.
const COMMAND_QUEUE_SIZE: usize = 4;
const EVENT_QUEUE_SIZE: usize = 64;

/// What the UI shows for the Apollo transaction of one swap.
#[typeshare]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ApolloSwapState {
    /// The swap is pending but has no publisher yet (all slots are busy or the publisher restarts soon).
    Waiting,
    /// The publisher is sending the Apollo transaction.
    Publishing,
    /// The Apollo transaction was sent and is not confirmed yet.
    Published,
    Confirmed,
    Abandoned,
    /// The last attempt failed or the lock transaction is not seen yet. The publisher tries again.
    Retrying,
}

#[typeshare]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApolloSwapStatus {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
    pub state: ApolloSwapState,
}

/// The Apollo state of all swaps that the manager knows about since it started.
#[typeshare]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ApolloStatus {
    pub swaps: Vec<ApolloSwapStatus>,
}

#[derive(Clone, Copy, Debug)]
pub struct ApolloManagerConfig {
    /// The manager looks for pending swaps at least this often, even without a wake.
    pub poll_interval: Duration,
    /// How long a publisher waits between two attempts.
    pub rebroadcast_interval: Duration,
    pub max_concurrent: usize,
}

impl ApolloManagerConfig {
    pub fn new(rebroadcast_interval: Duration) -> Self {
        Self {
            poll_interval: DEFAULT_POLL_INTERVAL,
            rebroadcast_interval,
            max_concurrent: MAX_CONCURRENT_PUBLISHERS,
        }
    }
}

/// Cheap to clone. All clones talk to the same manager.
#[derive(Clone)]
pub struct ApolloManagerHandle {
    commands: mpsc::Sender<Command>,
    status: watch::Receiver<ApolloStatus>,
    shutdown: CancellationToken,
}

impl ApolloManagerHandle {
    /// Tells the manager to look at the database now. Call it after you wrote a progress row.
    /// Never blocks: a full queue already holds a wake that the manager did not handle yet.
    pub fn wake(&self) {
        match self.commands.try_send(Command::Wake) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::debug!("Apollo manager is stopped, cannot wake it");
            }
        }
    }

    pub fn status(&self) -> watch::Receiver<ApolloStatus> {
        self.status.clone()
    }

    /// Cancelled when the manager is shut down.
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// Stops the manager and all its publishers. They resume on the next start.
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }
}

enum Command {
    Wake,
}

/// What a publisher tells the manager. The manager only uses it for the status and the restart backoff.
#[derive(Debug)]
enum Event {
    Published(Uuid),
    Confirmed(Uuid),
    Abandoned(Uuid),
    Transient(Uuid),
    Failed(Uuid),
}

/// Starts the manager. It reads the pending swaps right away.
/// The caller keeps the join handle to wait for the manager after `shutdown`.
pub fn spawn_apollo_manager(
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
    config: ApolloManagerConfig,
) -> (ApolloManagerHandle, JoinHandle<()>) {
    let (commands_tx, commands_rx) = mpsc::channel(COMMAND_QUEUE_SIZE);
    let (events_tx, events_rx) = mpsc::channel(EVENT_QUEUE_SIZE);
    let (status_tx, status_rx) = watch::channel(ApolloStatus::default());
    let shutdown = CancellationToken::new();

    let manager = Manager {
        bitcoin_wallet,
        db,
        config,
        events_tx,
        status_tx,
        tasks: JoinSet::new(),
        running: HashMap::new(),
        backoff: HashMap::new(),
        poisoned: HashSet::new(),
        reported_corrupt: HashSet::new(),
        states: BTreeMap::new(),
    };
    let join = tokio::spawn(manager.run(commands_rx, events_rx, shutdown.clone()));

    let handle = ApolloManagerHandle {
        commands: commands_tx,
        status: status_rx,
        shutdown,
    };

    (handle, join)
}

struct Manager {
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
    config: ApolloManagerConfig,
    events_tx: mpsc::Sender<Event>,
    status_tx: watch::Sender<ApolloStatus>,
    tasks: JoinSet<(Uuid, Result<()>)>,
    running: HashMap<Uuid, AbortHandle>,
    /// Per swap: the publisher does not restart before the instant, and the last delay.
    backoff: HashMap<Uuid, (Instant, Duration)>,
    /// Swaps with a row that cannot be decoded. Retrying never helps.
    poisoned: HashSet<Uuid>,
    /// Stored IDs of corrupt rows that we logged already.
    reported_corrupt: HashSet<String>,
    states: BTreeMap<Uuid, ApolloSwapState>,
}

impl Manager {
    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Command>,
        mut events: mpsc::Receiver<Event>,
        shutdown: CancellationToken,
    ) {
        let mut poll = tokio::time::interval(self.config.poll_interval);
        poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // The first tick completes at once and covers the start-up pass
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                Some(Command::Wake) = commands.recv() => self.reconcile().await,
                Some(event) = events.recv() => self.on_event(event),
                Some(joined) = self.tasks.join_next() => {
                    self.on_task_end(joined);
                    self.reconcile().await;
                }
                _ = poll.tick() => self.reconcile().await,
            }
        }

        // Dropping the manager aborts the publishers
        tracing::debug!("Apollo manager stopped");
    }

    /// Starts a publisher for each pending swap that has none, then publishes the status.
    async fn reconcile(&mut self) {
        let pending = match self.db.pending_apollo_progress().await {
            Ok(pending) => pending,
            Err(error) => {
                tracing::error!(?error, "Failed to load the pending Apollo progress");
                return;
            }
        };

        for (swap_id, error) in pending.corrupt {
            if self.reported_corrupt.insert(swap_id.clone()) {
                tracing::error!(%swap_id, ?error, "Apollo progress row is corrupt, it is skipped");
            }
        }

        let pending_ids: HashSet<Uuid> = pending.ready.iter().map(|(id, _)| *id).collect();
        self.backoff.retain(|id, _| pending_ids.contains(id));
        // Keep the final states of this session for the UI
        self.states.retain(|id, state| {
            pending_ids.contains(id)
                || matches!(
                    state,
                    ApolloSwapState::Confirmed | ApolloSwapState::Abandoned
                )
        });

        for (swap_id, progress) in pending.ready {
            if self.running.contains_key(&swap_id) || self.poisoned.contains(&swap_id) {
                continue;
            }
            let backoff_active = self
                .backoff
                .get(&swap_id)
                .is_some_and(|(not_before, _)| Instant::now() < *not_before);
            if backoff_active || self.running.len() >= self.config.max_concurrent {
                self.states
                    .entry(swap_id)
                    .or_insert(ApolloSwapState::Waiting);
                continue;
            }

            self.spawn_publisher(swap_id);
            let state = match progress {
                ApolloProgress::Published(_) => ApolloSwapState::Published,
                _ => ApolloSwapState::Publishing,
            };
            self.states.insert(swap_id, state);
        }

        self.publish_status();
    }

    fn spawn_publisher(&mut self, swap_id: Uuid) {
        let publisher = Publisher {
            swap_id,
            bitcoin_wallet: self.bitcoin_wallet.clone(),
            db: self.db.clone(),
            events_tx: self.events_tx.clone(),
            rebroadcast_interval: self.config.rebroadcast_interval,
        };
        let abort = self
            .tasks
            .spawn(async move { (swap_id, publisher.run().await) });
        self.running.insert(swap_id, abort);
    }

    fn on_event(&mut self, event: Event) {
        let (swap_id, state) = match event {
            Event::Published(id) => (id, ApolloSwapState::Published),
            Event::Confirmed(id) => (id, ApolloSwapState::Confirmed),
            Event::Abandoned(id) | Event::Failed(id) => (id, ApolloSwapState::Abandoned),
            Event::Transient(id) => (id, ApolloSwapState::Retrying),
        };
        if !matches!(event, Event::Transient(_)) {
            // The publisher made progress, so the next failure starts with a short delay again
            self.backoff.remove(&swap_id);
        }
        self.states.insert(swap_id, state);
        self.publish_status();
    }

    fn on_task_end(&mut self, joined: Result<(Uuid, Result<()>), tokio::task::JoinError>) {
        let (swap_id, error) = match joined {
            Ok((swap_id, Ok(()))) => {
                self.running.remove(&swap_id);
                return;
            }
            Ok((swap_id, Err(error))) => (swap_id, error),
            Err(join_error) => {
                let Some(swap_id) = self
                    .running
                    .iter()
                    .find(|(_, abort)| abort.id() == join_error.id())
                    .map(|(swap_id, _)| *swap_id)
                else {
                    tracing::error!(?join_error, "An unknown Apollo publisher ended abnormally");
                    return;
                };
                (swap_id, anyhow::Error::new(join_error))
            }
        };

        self.running.remove(&swap_id);
        // A row that cannot be decoded never becomes decodable
        if error.chain().any(|cause| cause.is::<serde_json::Error>()) {
            tracing::error!(%swap_id, ?error, "Apollo progress cannot be decoded, the swap is skipped");
            self.poisoned.insert(swap_id);
            return;
        }

        let delay = self
            .backoff
            .get(&swap_id)
            .map_or(MIN_RESTART_BACKOFF, |(_, last)| {
                (*last * 2).min(MAX_RESTART_BACKOFF)
            });
        tracing::error!(%swap_id, ?error, ?delay, "Apollo publisher stopped, the manager restarts it");
        self.backoff
            .insert(swap_id, (Instant::now() + delay, delay));
        self.states.insert(swap_id, ApolloSwapState::Retrying);
        self.publish_status();
    }

    fn publish_status(&self) {
        let status = ApolloStatus {
            swaps: self
                .states
                .iter()
                .map(|(swap_id, state)| ApolloSwapStatus {
                    swap_id: *swap_id,
                    state: *state,
                })
                .collect(),
        };
        self.status_tx.send_if_modified(|current| {
            if *current == status {
                return false;
            }
            *current = status;
            true
        });
    }
}

/// Publishes the Apollo transaction of one swap until it is confirmed or abandoned.
/// It is the only writer of the progress rows of its swap after the first insert.
/// It never changes the swap state and it never broadcasts the lock transaction.
struct Publisher {
    swap_id: Uuid,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    db: Arc<dyn Database + Send + Sync>,
    events_tx: mpsc::Sender<Event>,
    rebroadcast_interval: Duration,
}

/// What the lock transaction of an Apollo transaction looks like on the network.
enum LockCheck {
    Seen,
    Unseen,
    /// The lock transaction has no such output. This never changes.
    MissingOutput,
}

impl Publisher {
    async fn run(self) -> Result<()> {
        let required_confirmations =
            TX_APOLLO_MIN_CONFIRMATIONS.max(self.bitcoin_wallet.finality_confirmations());

        loop {
            let progress = retry_db("Loading the Apollo progress", || {
                self.db.get_apollo_progress(self.swap_id)
            })
            .await?
            .context("Swap has no Apollo progress")?;
            if progress.is_complete() {
                return Ok(());
            }

            match self
                .pass(&progress, required_confirmations)
                .await
            {
                Ok(Pass::Done) => return Ok(()),
                // The next pass loads the new row
                Ok(Pass::Advanced) => continue,
                Ok(Pass::Wait) => {}
                Err(error) => {
                    tracing::warn!(swap_id = %self.swap_id, ?error, "Failed to advance Apollo, will retry");
                    self.send(Event::Transient(self.swap_id)).await;
                }
            }
            tokio::time::sleep(self.rebroadcast_interval).await;
        }
    }

    /// One attempt to move the Apollo transaction forward.
    /// Every step comes from the chain state, so the caller can drop this future at any time.
    async fn pass(
        &self,
        progress: &ApolloProgress,
        required_confirmations: u32,
    ) -> Result<Pass> {
        let swap_id = self.swap_id;
        let tx = progress.tx();
        let txid = tx.compute_txid();

        let [input] = tx.input.as_slice() else {
            tracing::error!(%swap_id, %txid, "Apollo transaction must have exactly one input, abandoning it");
            return self.give_up(tx, Event::Failed(swap_id)).await;
        };
        let outpoint = input.previous_output;

        if lock_conflicted(self.bitcoin_wallet.as_ref(), tx, required_confirmations).await? {
            tracing::warn!(%swap_id, %txid, "The lock transaction was replaced, abandoning Apollo");
            return self.give_up(tx, Event::Abandoned(swap_id)).await;
        }

        // The Apollo transaction is only valid after the lock transaction. Bob's swap broadcasts the lock.
        match self.check_lock(outpoint).await? {
            LockCheck::Seen => {}
            LockCheck::MissingOutput => {
                tracing::error!(%swap_id, %outpoint, "The lock transaction has no Apollo output, abandoning Apollo");
                return self.give_up(tx, Event::Failed(swap_id)).await;
            }
            // Never final. A swap only has Apollo once its lock may be public, so the lock can still appear.
            LockCheck::Unseen => {
                tracing::warn!(%swap_id, "The lock transaction is not seen yet, Apollo waits");
                self.send(Event::Transient(swap_id)).await;
                return Ok(Pass::Wait);
            }
        }

        let status =
            ensure_tx_apollo_published(self.bitcoin_wallet.as_ref(), tx, required_confirmations)
                .await?;

        if matches!(progress, ApolloProgress::Constructed(_)) {
            self.append(ApolloProgress::Published(tx.clone())).await?;
            self.send(Event::Published(swap_id)).await;
            return Ok(Pass::Advanced);
        }
        if let ScriptStatus::Confirmed(confirmed) = status
            && confirmed.meets_target(required_confirmations)
        {
            tracing::debug!(%txid, "Apollo transaction is confirmed");
            self.append(ApolloProgress::Confirmed(tx.clone())).await?;
            self.send(Event::Confirmed(swap_id)).await;
            return Ok(Pass::Done);
        }

        tracing::debug!(%txid, %status, "Apollo transaction is not confirmed yet");
        self.send(Event::Published(swap_id)).await;

        Ok(Pass::Wait)
    }

    /// Looks up the lock transaction that the Apollo transaction spends.
    async fn check_lock(&self, outpoint: OutPoint) -> Result<LockCheck> {
        let wallet = self.bitcoin_wallet.as_ref();
        let Some(lock) = wallet.get_raw_transaction(outpoint.txid).await? else {
            return Ok(LockCheck::Unseen);
        };
        // A malicious Electrum server must not be able to substitute another transaction
        ensure!(
            lock.compute_txid() == outpoint.txid,
            "The server returned another transaction than the lock transaction {}",
            outpoint.txid
        );
        let Some(output) = usize::try_from(outpoint.vout)
            .ok()
            .and_then(|vout| lock.output.get(vout))
        else {
            return Ok(LockCheck::MissingOutput);
        };

        if lock_is_seen(wallet, outpoint.txid, output.script_pubkey.clone()).await? {
            return Ok(LockCheck::Seen);
        }

        Ok(LockCheck::Unseen)
    }

    /// Gives up on the Apollo transaction and lets the wallet spend its parent output again.
    async fn give_up(&self, tx: &Transaction, event: Event) -> Result<Pass> {
        self.append(ApolloProgress::Abandoned(tx.clone())).await?;
        for input in &tx.input {
            self.bitcoin_wallet.release_outpoint(&input.previous_output);
        }
        self.send(event).await;

        Ok(Pass::Done)
    }

    async fn append(&self, progress: ApolloProgress) -> Result<()> {
        retry_db("Persisting the Apollo progress", || {
            self.db
                .append_apollo_progress(self.swap_id, progress.clone())
        })
        .await
    }

    async fn send(&self, event: Event) {
        if self.events_tx.send(event).await.is_err() {
            tracing::debug!(swap_id = %self.swap_id, "Apollo manager is gone, dropping the event");
        }
    }
}

enum Pass {
    /// The progress is final.
    Done,
    /// A new row was written. Run the next pass without a delay.
    Advanced,
    /// Nothing to do until the rebroadcast interval passed.
    Wait,
}
