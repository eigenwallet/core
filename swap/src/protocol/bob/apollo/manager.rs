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

            match self.pass(&progress, required_confirmations).await {
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
    async fn pass(&self, progress: &ApolloProgress, required_confirmations: u32) -> Result<Pass> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{AccessMode, SqliteDatabase};
    use crate::protocol::State;
    use anyhow::{anyhow, bail};
    use bdk_wallet::Balance;
    use bdk_wallet::export::FullyNodedExport;
    use bitcoin::absolute::LockTime;
    use bitcoin::transaction::Version;
    use bitcoin::{Address, Amount, FeeRate, Network, Psbt, ScriptBuf, TxIn, TxOut, Txid, Weight};
    use bitcoin_wallet::primitives::Confirmed;
    use bitcoin_wallet::{ApolloOutputShape, Subscription, Watchable};
    use std::sync::{Mutex, MutexGuard};
    use swap_machine::bob::BobState;
    use utxo_ownership_proof::{Challenge, OwnershipProof};

    const REQUIRED_CONFIRMATIONS: u32 = TX_APOLLO_MIN_CONFIRMATIONS;
    const APOLLO_VOUT: u32 = 1;
    const NO_POLL: Duration = Duration::from_secs(3600);

    /// What the network looks like to the publisher.
    #[derive(Default)]
    struct FakeChain {
        transactions: HashMap<Txid, Arc<Transaction>>,
        statuses: HashMap<Txid, ScriptStatus>,
        conflicted: bool,
        rejects_broadcasts: bool,
        broadcasts: Vec<Txid>,
        released: Vec<OutPoint>,
        conflict_checks: usize,
    }

    impl FakeChain {
        fn knows(&mut self, tx: &Transaction) {
            self.transactions
                .insert(tx.compute_txid(), Arc::new(tx.clone()));
        }

        fn sees(&mut self, tx: &Transaction, status: ScriptStatus) {
            self.knows(tx);
            self.statuses.insert(tx.compute_txid(), status);
        }
    }

    /// Implements only what the publisher calls.
    #[derive(Default)]
    struct FakeWallet(Mutex<FakeChain>);

    impl FakeWallet {
        fn chain(&self) -> MutexGuard<'_, FakeChain> {
            self.0.lock().unwrap()
        }
    }

    #[async_trait::async_trait]
    impl BitcoinWallet for FakeWallet {
        async fn is_transaction_conflicted(
            &self,
            _txid: Txid,
            _min_confirmations: u32,
        ) -> Result<bool> {
            let mut chain = self.chain();
            chain.conflict_checks += 1;
            Ok(chain.conflicted)
        }

        fn release_outpoint(&self, outpoint: &OutPoint) {
            self.chain().released.push(*outpoint);
        }

        async fn broadcast_without_subscription(
            &self,
            transaction: Transaction,
            _kind: &str,
        ) -> Result<Txid> {
            let mut chain = self.chain();
            if chain.rejects_broadcasts {
                bail!("The server rejected the transaction");
            }
            let txid = transaction.compute_txid();
            chain.broadcasts.push(txid);
            Ok(txid)
        }

        async fn status_of_script(&self, tx: &dyn Watchable) -> Result<ScriptStatus> {
            let chain = self.chain();
            Ok(chain
                .statuses
                .get(&tx.id())
                .copied()
                .unwrap_or(ScriptStatus::Unseen))
        }

        async fn get_raw_transaction(&self, txid: Txid) -> Result<Option<Arc<Transaction>>> {
            Ok(self.chain().transactions.get(&txid).cloned())
        }

        fn finality_confirmations(&self) -> u32 {
            1
        }

        async fn balance(&self) -> Result<Amount> {
            unimplemented!()
        }

        async fn balance_info(&self) -> Result<Balance> {
            unimplemented!()
        }

        async fn new_address(&self) -> Result<Address> {
            unimplemented!()
        }

        async fn send_to_address(
            &self,
            _address: Address,
            _amount: Amount,
            _spending_fee: Amount,
            _change_override: Option<Address>,
            _max_inputs: Option<usize>,
        ) -> Result<Psbt> {
            unimplemented!()
        }

        async fn send_to_address_dynamic_fee(
            &self,
            _address: Address,
            _amount: Amount,
            _change_override: Option<Address>,
            _max_inputs: Option<usize>,
        ) -> Result<Psbt> {
            unimplemented!()
        }

        async fn send_to_address_with_apollo_output(
            &self,
            _address: Address,
            _amount: Amount,
            _apollo_address: Address,
            _tx_apollo_fee: Amount,
            _spending_fee: Amount,
            _change_override: Option<Address>,
            _max_inputs: Option<usize>,
        ) -> Result<Psbt> {
            unimplemented!()
        }

        fn reserve_outpoints(&self, _outpoints: Vec<OutPoint>) {
            unimplemented!()
        }

        async fn wallet_spending_transactions(&self) -> Vec<Arc<Transaction>> {
            unimplemented!()
        }

        async fn sweep_balance_to_address_dynamic_fee(&self, _address: Address) -> Result<Psbt> {
            unimplemented!()
        }

        async fn sign_and_finalize(&self, _psbt: Psbt) -> Result<Transaction> {
            unimplemented!()
        }

        fn prove_utxo_ownership(
            &self,
            _psbt: &Psbt,
            _challenge: Challenge,
        ) -> Result<Vec<OwnershipProof>> {
            unimplemented!()
        }

        async fn ensure_broadcasted(
            &self,
            _transaction: Transaction,
            _kind: &str,
        ) -> Result<(Txid, Subscription)> {
            unimplemented!()
        }

        async fn broadcast(
            &self,
            _transaction: Transaction,
            _kind: &str,
        ) -> Result<(Txid, Subscription)> {
            unimplemented!()
        }

        async fn sync(&self) -> Result<()> {
            unimplemented!()
        }

        async fn health_check(&self) -> Result<()> {
            unimplemented!()
        }

        async fn subscribe_to(&self, _tx: Box<dyn Watchable>) -> Subscription {
            unimplemented!()
        }

        async fn max_giveable_with_optional_apollo(
            &self,
            _locking_script_size: usize,
            _max_inputs: Option<usize>,
            _apollo: Option<ApolloOutputShape>,
        ) -> Result<(Amount, Amount)> {
            unimplemented!()
        }

        async fn estimate_fee(
            &self,
            _weight: Weight,
            _transfer_amount: Option<Amount>,
        ) -> Result<Amount> {
            unimplemented!()
        }

        async fn estimate_fee_for_target(
            &self,
            _weight: Weight,
            _target_block: u32,
        ) -> Result<Amount> {
            unimplemented!()
        }

        async fn swap_fee_rate(&self) -> Result<FeeRate> {
            unimplemented!()
        }

        fn network(&self) -> Network {
            unimplemented!()
        }

        async fn wallet_export(&self, _role: &str) -> Result<FullyNodedExport> {
            unimplemented!()
        }
    }

    async fn test_db() -> Arc<dyn Database + Send + Sync> {
        let path = tempfile::tempdir().unwrap().keep().join("tempdb");
        std::fs::File::create(&path).unwrap();

        Arc::new(
            SqliteDatabase::open(path, AccessMode::ReadWrite)
                .await
                .unwrap(),
        )
    }

    /// `seed` makes the transactions of different swaps differ.
    fn lock_tx(seed: u32) -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::from_consensus(seed),
            input: vec![],
            output: vec![
                TxOut {
                    value: Amount::from_sat(100_000),
                    script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
                },
                TxOut {
                    value: Amount::from_sat(2_000),
                    script_pubkey: ScriptBuf::from_bytes(vec![0x52]),
                },
            ],
        }
    }

    fn apollo_tx(lock: &Transaction, vout: u32, inputs: usize) -> Transaction {
        let input = TxIn {
            previous_output: OutPoint::new(lock.compute_txid(), vout),
            ..Default::default()
        };

        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![input; inputs],
            output: vec![TxOut {
                value: Amount::ZERO,
                script_pubkey: ScriptBuf::new_op_return([1, 2, 3]),
            }],
        }
    }

    fn confirmed(confirmations: u32) -> ScriptStatus {
        ScriptStatus::Confirmed(Confirmed::new(confirmations - 1))
    }

    /// One publisher that the test drives pass by pass.
    struct PublisherTest {
        swap_id: Uuid,
        wallet: Arc<FakeWallet>,
        db: Arc<dyn Database + Send + Sync>,
        lock: Transaction,
        apollo: Transaction,
        events: mpsc::Receiver<Event>,
        publisher: Publisher,
    }

    impl PublisherTest {
        async fn new(progress: fn(Transaction) -> ApolloProgress) -> Self {
            let lock = lock_tx(0);
            Self::with_apollo(progress, lock.clone(), apollo_tx(&lock, APOLLO_VOUT, 1)).await
        }

        async fn with_apollo(
            progress: fn(Transaction) -> ApolloProgress,
            lock: Transaction,
            apollo: Transaction,
        ) -> Self {
            let swap_id = Uuid::new_v4();
            let wallet = Arc::new(FakeWallet::default());
            let db = test_db().await;
            db.insert_apollo_if_absent(swap_id, progress(apollo.clone()))
                .await
                .unwrap();
            let (events_tx, events) = mpsc::channel(EVENT_QUEUE_SIZE);
            let publisher = Publisher {
                swap_id,
                bitcoin_wallet: wallet.clone(),
                db: db.clone(),
                events_tx,
                rebroadcast_interval: Duration::from_millis(1),
            };

            Self {
                swap_id,
                wallet,
                db,
                lock,
                apollo,
                events,
                publisher,
            }
        }

        fn see_lock(&self) {
            self.wallet
                .chain()
                .sees(&self.lock, ScriptStatus::InMempool);
        }

        async fn pass(&self) -> Result<Pass> {
            let progress = self.progress().await;
            self.publisher.pass(&progress, REQUIRED_CONFIRMATIONS).await
        }

        async fn progress(&self) -> ApolloProgress {
            self.db
                .get_apollo_progress(self.swap_id)
                .await
                .unwrap()
                .unwrap()
        }

        fn assert_nothing_left_the_wallet(&self) {
            let chain = self.wallet.chain();
            assert!(chain.broadcasts.is_empty());
            assert!(chain.released.is_empty());
        }

        fn assert_outpoints_released(&self) {
            let expected: Vec<OutPoint> = self
                .apollo
                .input
                .iter()
                .map(|input| input.previous_output)
                .collect();
            assert_eq!(self.wallet.chain().released, expected);
        }
    }

    #[tokio::test]
    async fn unseen_lock_of_a_finished_swap_keeps_apollo_pending() {
        let mut test = PublisherTest::new(ApolloProgress::Published).await;
        let tx_lock_id = test.lock.compute_txid();
        test.db
            .insert_latest_state(
                test.swap_id,
                State::Bob(BobState::XmrRedeemed { tx_lock_id }),
            )
            .await
            .unwrap();

        for _ in 0..3 {
            assert!(matches!(test.pass().await.unwrap(), Pass::Wait));
            assert!(matches!(test.events.try_recv(), Ok(Event::Transient(_))));
        }

        assert_eq!(
            test.progress().await,
            ApolloProgress::Published(test.apollo.clone())
        );
        test.assert_nothing_left_the_wallet();
    }

    #[tokio::test]
    async fn lock_missing_from_the_script_history_keeps_apollo_pending() {
        let mut test = PublisherTest::new(ApolloProgress::Constructed).await;
        // The transaction lookup finds the lock, but the script history lags behind
        test.wallet.chain().knows(&test.lock);

        assert!(matches!(test.pass().await.unwrap(), Pass::Wait));

        assert!(matches!(test.events.try_recv(), Ok(Event::Transient(_))));
        assert_eq!(
            test.progress().await,
            ApolloProgress::Constructed(test.apollo.clone())
        );
        test.assert_nothing_left_the_wallet();
    }

    #[tokio::test]
    async fn replaced_lock_abandons_constructed_apollo_and_releases_its_input() {
        let mut test = PublisherTest::new(ApolloProgress::Constructed).await;
        test.wallet.chain().conflicted = true;

        assert!(matches!(test.pass().await.unwrap(), Pass::Done));

        assert!(matches!(test.events.try_recv(), Ok(Event::Abandoned(_))));
        assert_eq!(
            test.progress().await,
            ApolloProgress::Abandoned(test.apollo.clone())
        );
        test.assert_outpoints_released();
        assert!(test.wallet.chain().broadcasts.is_empty());
    }

    #[tokio::test]
    async fn replaced_lock_abandons_published_apollo() {
        let test = PublisherTest::new(ApolloProgress::Published).await;
        test.wallet.chain().conflicted = true;

        assert!(matches!(test.pass().await.unwrap(), Pass::Done));

        assert_eq!(
            test.progress().await,
            ApolloProgress::Abandoned(test.apollo.clone())
        );
    }

    #[tokio::test]
    async fn lock_without_the_apollo_output_abandons_apollo() {
        let lock = lock_tx(0);
        let apollo = apollo_tx(&lock, 7, 1);
        let mut test = PublisherTest::with_apollo(ApolloProgress::Constructed, lock, apollo).await;
        test.see_lock();

        assert!(matches!(test.pass().await.unwrap(), Pass::Done));

        assert!(matches!(test.events.try_recv(), Ok(Event::Failed(_))));
        assert_eq!(
            test.progress().await,
            ApolloProgress::Abandoned(test.apollo.clone())
        );
        test.assert_outpoints_released();
    }

    #[tokio::test]
    async fn apollo_with_two_inputs_is_abandoned() {
        let lock = lock_tx(0);
        let apollo = apollo_tx(&lock, APOLLO_VOUT, 2);
        let mut test = PublisherTest::with_apollo(ApolloProgress::Constructed, lock, apollo).await;

        assert!(matches!(test.pass().await.unwrap(), Pass::Done));

        assert!(matches!(test.events.try_recv(), Ok(Event::Failed(_))));
        assert_eq!(
            test.progress().await,
            ApolloProgress::Abandoned(test.apollo.clone())
        );
        test.assert_outpoints_released();
    }

    #[tokio::test]
    async fn substituted_lock_fails_the_pass_and_keeps_apollo_pending() {
        let test = PublisherTest::new(ApolloProgress::Constructed).await;
        test.wallet
            .chain()
            .transactions
            .insert(test.lock.compute_txid(), Arc::new(lock_tx(1)));

        assert!(test.pass().await.is_err());

        assert_eq!(
            test.progress().await,
            ApolloProgress::Constructed(test.apollo.clone())
        );
        test.assert_nothing_left_the_wallet();
    }

    #[tokio::test]
    async fn seen_lock_publishes_constructed_apollo() {
        let mut test = PublisherTest::new(ApolloProgress::Constructed).await;
        test.see_lock();

        assert!(matches!(test.pass().await.unwrap(), Pass::Advanced));

        assert!(matches!(test.events.try_recv(), Ok(Event::Published(_))));
        assert_eq!(
            test.progress().await,
            ApolloProgress::Published(test.apollo.clone())
        );
        assert_eq!(
            test.wallet.chain().broadcasts,
            vec![test.apollo.compute_txid()]
        );
    }

    #[tokio::test]
    async fn rejected_broadcast_of_an_unseen_apollo_keeps_it_constructed() {
        let test = PublisherTest::new(ApolloProgress::Constructed).await;
        test.see_lock();
        test.wallet.chain().rejects_broadcasts = true;

        assert!(test.pass().await.is_err());

        assert_eq!(
            test.progress().await,
            ApolloProgress::Constructed(test.apollo.clone())
        );
    }

    #[tokio::test]
    async fn rejected_rebroadcast_of_a_known_apollo_is_ignored() {
        let test = PublisherTest::new(ApolloProgress::Published).await;
        test.see_lock();
        {
            let mut chain = test.wallet.chain();
            chain.sees(&test.apollo, ScriptStatus::InMempool);
            chain.rejects_broadcasts = true;
        }

        assert!(matches!(test.pass().await.unwrap(), Pass::Wait));

        assert_eq!(
            test.progress().await,
            ApolloProgress::Published(test.apollo.clone())
        );
    }

    #[tokio::test]
    async fn apollo_confirms_only_at_the_required_depth() {
        let mut test = PublisherTest::new(ApolloProgress::Published).await;
        test.see_lock();
        test.wallet
            .chain()
            .sees(&test.apollo, confirmed(REQUIRED_CONFIRMATIONS - 1));

        assert!(matches!(test.pass().await.unwrap(), Pass::Wait));
        assert!(matches!(test.events.try_recv(), Ok(Event::Published(_))));
        assert_eq!(
            test.progress().await,
            ApolloProgress::Published(test.apollo.clone())
        );
        assert_eq!(test.wallet.chain().broadcasts.len(), 1);

        test.wallet
            .chain()
            .sees(&test.apollo, confirmed(REQUIRED_CONFIRMATIONS));

        assert!(matches!(test.pass().await.unwrap(), Pass::Done));
        assert!(matches!(test.events.try_recv(), Ok(Event::Confirmed(_))));
        assert_eq!(
            test.progress().await,
            ApolloProgress::Confirmed(test.apollo.clone())
        );
        assert_eq!(test.wallet.chain().broadcasts.len(), 1);
    }

    #[tokio::test]
    async fn claimed_depth_without_the_transaction_does_not_confirm_apollo() {
        let test = PublisherTest::new(ApolloProgress::Published).await;
        test.see_lock();
        let txid = test.apollo.compute_txid();
        test.wallet
            .chain()
            .statuses
            .insert(txid, confirmed(REQUIRED_CONFIRMATIONS));

        assert!(test.pass().await.is_err());

        assert_eq!(
            test.progress().await,
            ApolloProgress::Published(test.apollo.clone())
        );
    }

    fn test_config(poll_interval: Duration, max_concurrent: usize) -> ApolloManagerConfig {
        ApolloManagerConfig {
            poll_interval,
            rebroadcast_interval: Duration::from_millis(1),
            max_concurrent,
        }
    }

    /// Saves a swap with a constructed Apollo. Nothing of it is on the chain yet.
    async fn insert_swap(
        db: &Arc<dyn Database + Send + Sync>,
        seed: u32,
    ) -> (Uuid, Transaction, Transaction) {
        let swap_id = Uuid::new_v4();
        let lock = lock_tx(seed);
        let apollo = apollo_tx(&lock, APOLLO_VOUT, 1);
        db.insert_apollo_if_absent(swap_id, ApolloProgress::Constructed(apollo.clone()))
            .await
            .unwrap();

        (swap_id, lock, apollo)
    }

    fn confirm_on_chain(wallet: &FakeWallet, lock: &Transaction, apollo: &Transaction) {
        let mut chain = wallet.chain();
        chain.sees(lock, ScriptStatus::InMempool);
        chain.sees(apollo, confirmed(REQUIRED_CONFIRMATIONS));
    }

    async fn wait_for_status(
        handle: &ApolloManagerHandle,
        mut done: impl FnMut(&ApolloStatus) -> bool,
    ) {
        let mut status = handle.status();
        tokio::time::timeout(
            Duration::from_secs(10),
            status.wait_for(|status| done(status)),
        )
        .await
        .expect("The Apollo status did not reach the expected value")
        .expect("The Apollo manager stopped");
    }

    async fn wait_for_state(handle: &ApolloManagerHandle, swap_id: Uuid, state: ApolloSwapState) {
        wait_for_status(handle, |status| {
            status
                .swaps
                .iter()
                .any(|swap| swap.swap_id == swap_id && swap.state == state)
        })
        .await;
    }

    async fn latest_progress(
        db: &Arc<dyn Database + Send + Sync>,
        swap_id: Uuid,
    ) -> ApolloProgress {
        db.get_apollo_progress(swap_id).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn manager_confirms_a_pending_apollo_at_the_start() {
        let wallet = Arc::new(FakeWallet::default());
        let db = test_db().await;
        let (swap_id, lock, apollo) = insert_swap(&db, 0).await;
        confirm_on_chain(&wallet, &lock, &apollo);

        let (handle, join) = spawn_apollo_manager(wallet, db.clone(), test_config(NO_POLL, 16));

        wait_for_state(&handle, swap_id, ApolloSwapState::Confirmed).await;
        assert_eq!(
            latest_progress(&db, swap_id).await,
            ApolloProgress::Confirmed(apollo)
        );

        handle.shutdown();
        join.await.unwrap();
    }

    #[tokio::test]
    async fn wake_finds_a_row_written_after_the_start() {
        let wallet = Arc::new(FakeWallet::default());
        let db = test_db().await;
        // The lock of this swap stays unseen, so its publisher never ends and starts no new pass
        let (first_swap_id, _, _) = insert_swap(&db, 0).await;
        let (handle, _join) =
            spawn_apollo_manager(wallet.clone(), db.clone(), test_config(NO_POLL, 16));
        wait_for_state(&handle, first_swap_id, ApolloSwapState::Retrying).await;

        let (swap_id, lock, apollo) = insert_swap(&db, 1).await;
        confirm_on_chain(&wallet, &lock, &apollo);
        handle.wake();

        wait_for_state(&handle, swap_id, ApolloSwapState::Confirmed).await;
    }

    #[tokio::test]
    async fn poll_finds_a_row_without_a_wake() {
        let wallet = Arc::new(FakeWallet::default());
        let db = test_db().await;
        let (handle, _join) = spawn_apollo_manager(
            wallet.clone(),
            db.clone(),
            test_config(Duration::from_millis(5), 16),
        );

        let (swap_id, lock, apollo) = insert_swap(&db, 0).await;
        confirm_on_chain(&wallet, &lock, &apollo);

        wait_for_state(&handle, swap_id, ApolloSwapState::Confirmed).await;
    }

    #[tokio::test]
    async fn publishers_stay_below_the_limit_and_a_free_slot_starts_the_next_one() {
        let wallet = Arc::new(FakeWallet::default());
        let db = test_db().await;
        let mut swaps = Vec::new();
        for seed in 0..3 {
            swaps.push(insert_swap(&db, seed).await);
        }

        let (handle, _join) =
            spawn_apollo_manager(wallet.clone(), db.clone(), test_config(NO_POLL, 1));

        wait_for_status(&handle, |status| status.swaps.len() == 3).await;
        let waiting = handle
            .status()
            .borrow()
            .swaps
            .iter()
            .filter(|swap| swap.state == ApolloSwapState::Waiting)
            .count();
        assert_eq!(waiting, 2);

        for (_, lock, apollo) in &swaps {
            confirm_on_chain(&wallet, lock, apollo);
        }

        for (swap_id, _, _) in &swaps {
            wait_for_state(&handle, *swap_id, ApolloSwapState::Confirmed).await;
        }
    }

    #[tokio::test]
    async fn failed_broadcast_is_retried_until_it_works() {
        let wallet = Arc::new(FakeWallet::default());
        let db = test_db().await;
        let (swap_id, lock, apollo) = insert_swap(&db, 0).await;
        {
            let mut chain = wallet.chain();
            chain.sees(&lock, ScriptStatus::InMempool);
            chain.rejects_broadcasts = true;
        }

        let (handle, _join) =
            spawn_apollo_manager(wallet.clone(), db.clone(), test_config(NO_POLL, 16));

        wait_for_state(&handle, swap_id, ApolloSwapState::Retrying).await;
        assert_eq!(
            latest_progress(&db, swap_id).await,
            ApolloProgress::Constructed(apollo.clone())
        );

        {
            let mut chain = wallet.chain();
            chain.rejects_broadcasts = false;
            chain.sees(&apollo, confirmed(REQUIRED_CONFIRMATIONS));
        }

        wait_for_state(&handle, swap_id, ApolloSwapState::Confirmed).await;
    }

    #[tokio::test]
    async fn abandoned_apollo_gets_no_new_publisher() {
        let wallet = Arc::new(FakeWallet::default());
        let db = test_db().await;
        let (swap_id, _, apollo) = insert_swap(&db, 0).await;
        wallet.chain().conflicted = true;

        let (handle, _join) = spawn_apollo_manager(
            wallet.clone(),
            db.clone(),
            test_config(Duration::from_millis(5), 16),
        );

        wait_for_state(&handle, swap_id, ApolloSwapState::Abandoned).await;
        // Many poll ticks pass here
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(wallet.chain().conflict_checks, 1);
        assert_eq!(
            latest_progress(&db, swap_id).await,
            ApolloProgress::Abandoned(apollo)
        );
    }

    #[tokio::test]
    async fn shutdown_keeps_the_row_and_the_next_manager_resumes_it() {
        let wallet = Arc::new(FakeWallet::default());
        let db = test_db().await;
        let (swap_id, lock, apollo) = insert_swap(&db, 0).await;

        let (handle, join) =
            spawn_apollo_manager(wallet.clone(), db.clone(), test_config(NO_POLL, 16));
        wait_for_state(&handle, swap_id, ApolloSwapState::Retrying).await;

        handle.shutdown();
        join.await.unwrap();
        // A wake after the shutdown must neither block nor panic
        handle.wake();

        assert_eq!(
            latest_progress(&db, swap_id).await,
            ApolloProgress::Constructed(apollo.clone())
        );

        confirm_on_chain(&wallet, &lock, &apollo);
        let (handle, _join) = spawn_apollo_manager(wallet, db, test_config(NO_POLL, 16));

        wait_for_state(&handle, swap_id, ApolloSwapState::Confirmed).await;
    }

    #[tokio::test]
    async fn wakes_never_block_on_a_full_queue() {
        let wallet = Arc::new(FakeWallet::default());
        let db = test_db().await;
        let (handle, _join) =
            spawn_apollo_manager(wallet.clone(), db.clone(), test_config(NO_POLL, 16));

        // The test runs on one thread, so the manager cannot empty the queue in between
        for _ in 0..COMMAND_QUEUE_SIZE * 10 {
            handle.wake();
        }

        let (swap_id, lock, apollo) = insert_swap(&db, 0).await;
        confirm_on_chain(&wallet, &lock, &apollo);
        handle.wake();

        wait_for_state(&handle, swap_id, ApolloSwapState::Confirmed).await;
    }

    /// A manager that does not run, so a test can call its handlers one at a time.
    struct IdleManager {
        manager: Manager,
        _events: mpsc::Receiver<Event>,
        _status: watch::Receiver<ApolloStatus>,
    }

    fn idle_manager(wallet: Arc<FakeWallet>, db: Arc<dyn Database + Send + Sync>) -> IdleManager {
        let (events_tx, events) = mpsc::channel(EVENT_QUEUE_SIZE);
        let (status_tx, status) = watch::channel(ApolloStatus::default());

        IdleManager {
            manager: Manager {
                bitcoin_wallet: wallet,
                db,
                config: test_config(NO_POLL, 16),
                events_tx,
                status_tx,
                tasks: JoinSet::new(),
                running: HashMap::new(),
                backoff: HashMap::new(),
                poisoned: HashSet::new(),
                reported_corrupt: HashSet::new(),
                states: BTreeMap::new(),
            },
            _events: events,
            _status: status,
        }
    }

    #[tokio::test]
    async fn panicking_publisher_restarts_after_a_growing_backoff() {
        let db = test_db().await;
        let (swap_id, _, _) = insert_swap(&db, 0).await;
        let mut idle = idle_manager(Arc::new(FakeWallet::default()), db);
        let manager = &mut idle.manager;

        for seconds in [10, 20, 40, 80, 160, 300, 300] {
            let abort = manager
                .tasks
                .spawn(async move { panic!("publisher panics") });
            manager.running.insert(swap_id, abort);
            let joined = manager.tasks.join_next().await.unwrap();

            manager.on_task_end(joined);

            assert!(manager.running.is_empty());
            assert_eq!(manager.backoff[&swap_id].1, Duration::from_secs(seconds));
            assert_eq!(manager.states[&swap_id], ApolloSwapState::Retrying);
        }

        manager.reconcile().await;
        assert!(manager.running.is_empty());
        assert_eq!(manager.states[&swap_id], ApolloSwapState::Retrying);

        let (_, delay) = manager.backoff[&swap_id];
        manager.backoff.insert(swap_id, (Instant::now(), delay));
        manager.reconcile().await;
        assert!(manager.running.contains_key(&swap_id));
    }

    #[tokio::test]
    async fn failed_publisher_restarts_but_an_undecodable_row_does_not() {
        let db = test_db().await;
        let (failed, _, _) = insert_swap(&db, 0).await;
        let (undecodable, _, _) = insert_swap(&db, 1).await;
        let mut idle = idle_manager(Arc::new(FakeWallet::default()), db);
        let manager = &mut idle.manager;

        manager.on_task_end(Ok((failed, Err(anyhow!("The publisher stopped")))));
        let decode_error = serde_json::from_str::<u32>("not a number").unwrap_err();
        manager.on_task_end(Ok((
            undecodable,
            Err(anyhow::Error::new(decode_error).context("Failed to load the row")),
        )));

        assert!(manager.backoff.contains_key(&failed));
        assert!(!manager.poisoned.contains(&failed));
        assert!(manager.poisoned.contains(&undecodable));
        assert!(!manager.backoff.contains_key(&undecodable));

        manager
            .backoff
            .insert(failed, (Instant::now(), MIN_RESTART_BACKOFF));
        manager.reconcile().await;

        assert!(manager.running.contains_key(&failed));
        assert!(!manager.running.contains_key(&undecodable));
    }

    #[tokio::test]
    async fn only_progress_resets_the_restart_backoff() {
        let db = test_db().await;
        let mut idle = idle_manager(Arc::new(FakeWallet::default()), db);
        let manager = &mut idle.manager;
        let swap_id = Uuid::new_v4();
        manager
            .backoff
            .insert(swap_id, (Instant::now(), MAX_RESTART_BACKOFF));

        manager.on_event(Event::Transient(swap_id));
        assert!(manager.backoff.contains_key(&swap_id));
        assert_eq!(manager.states[&swap_id], ApolloSwapState::Retrying);

        manager.on_event(Event::Published(swap_id));
        assert!(!manager.backoff.contains_key(&swap_id));
        assert_eq!(manager.states[&swap_id], ApolloSwapState::Published);
    }

    #[tokio::test]
    async fn status_keeps_final_states_and_drops_rows_that_are_gone() {
        let db = test_db().await;
        let mut idle = idle_manager(Arc::new(FakeWallet::default()), db);
        let manager = &mut idle.manager;
        let confirmed = Uuid::new_v4();
        let abandoned = Uuid::new_v4();
        let gone = Uuid::new_v4();
        manager.on_event(Event::Confirmed(confirmed));
        manager.on_event(Event::Failed(abandoned));
        manager.on_event(Event::Transient(gone));

        manager.reconcile().await;

        assert_eq!(manager.states[&confirmed], ApolloSwapState::Confirmed);
        assert_eq!(manager.states[&abandoned], ApolloSwapState::Abandoned);
        assert!(!manager.states.contains_key(&gone));
    }
}
