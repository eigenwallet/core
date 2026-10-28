//! Background verification of Bitcoin lock-input ownership proofs.
//!
//! A single [`ProofVerifier`] is shared by all swap setup handlers. Its actor
//! runs at most one job per peer at a time, lets every peer cool down for
//! [`PEER_COOLDOWN`] after each of its jobs finished and limits the number of
//! jobs running concurrently across all peers. Ownership signatures are checked
//! on Tokio's blocking thread pool; this actor has no swap state or chain access.

mod metrics;

use anyhow::Context;
use futures::StreamExt;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::FuturesUnordered;
use libp2p::PeerId;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;
use swap_core::bitcoin;
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::Instant;
use tracing::Instrument;
use utxo_ownership_proof::{Challenge, OwnershipProof, VerifiedPsbt};
use uuid::Uuid;

/// How long a peer has to wait after one of its jobs finished before its next job starts.
pub const PEER_COOLDOWN: Duration = Duration::from_secs(5);
/// Maximum number of jobs running at the same time across all peers.
pub const MAX_CONCURRENT_JOBS: usize = 50;
/// Maximum number of queued (not yet running) jobs across all peers.
pub const MAX_PENDING_JOBS: usize = 128;
/// Maximum number of queued (not yet running) jobs of a single peer.
pub const MAX_PENDING_JOBS_PER_PEER: usize = 4;

const MAILBOX_CAPACITY: usize = 64;

/// Cloneable handle to the verification actor.
///
/// All clones share the same actor and therefore the same per-peer
/// serialization, cooldowns and limits.
#[derive(Clone, Debug)]
pub struct ProofVerifier {
    mailbox: mpsc::Sender<Request>,
    metrics: metrics::Metrics,
}

#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("Proof verifier is not running")]
    Closed,
    #[error("Too many verification requests are queued for this peer")]
    PeerQueueFull,
    #[error("Too many verification requests are queued")]
    GlobalQueueFull,
    #[error("Verification worker failed")]
    Worker(#[source] JoinError),
    #[error(transparent)]
    Setup(anyhow::Error),
}

impl VerificationError {
    /// Whether verification failed on our side rather than because of the proofs.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Closed | Self::PeerQueueFull | Self::GlobalQueueFull | Self::Worker(_)
        )
    }
}

struct Request {
    peer: PeerId,
    job: QueuedJob,
}

struct QueuedJob {
    work: Job,
    queued_at: Instant,
    id: Uuid,
    peer: PeerId,
    input_count: usize,
}

impl QueuedJob {
    fn new(peer: PeerId, work: Job) -> Self {
        let input_count = match &work {
            Job::Lock { psbt, .. } => psbt.unsigned_tx.input.len(),
            #[cfg(test)]
            Job::Test { .. } => 0,
        };
        Self {
            work,
            queued_at: Instant::now(),
            id: Uuid::new_v4(),
            peer,
            input_count,
        }
    }

    fn span(&self) -> tracing::Span {
        tracing::info_span!("lock_proof_verification", job_id = %self.id, peer = %self.peer, input_count = self.input_count)
    }

    fn is_canceled(&self) -> bool {
        self.work.is_canceled()
    }
    fn reject(self, error: VerificationError) {
        self.work.reject(error);
    }

    fn log_cancelled(&self, running: usize, queued: usize) {
        tracing::info!(parent: &self.span(), running, queued, queue_wait_seconds = self.queued_at.elapsed().as_secs_f64(), "Cancelled ownership-proof job before it started");
    }
}

#[allow(clippy::large_enum_variant)]
enum Job {
    Lock {
        psbt: bitcoin::PartiallySignedTransaction,
        proofs: Vec<OwnershipProof>,
        challenge: Challenge,
        reply: oneshot::Sender<Result<VerifiedPsbt, VerificationError>>,
    },
    #[cfg(test)]
    Test {
        work: Box<dyn FnOnce() -> anyhow::Result<()> + Send>,
        reply: oneshot::Sender<Result<(), VerificationError>>,
    },
}

/// Owns all scheduling state. Only ever accessed from the actor task.
struct Actor {
    mailbox: mpsc::Receiver<Request>,
    /// Peers that have queued, running or cooling down jobs. Peers without
    /// any of those are removed.
    peers: HashMap<PeerId, Peer>,
    /// Idle peers with queued jobs, in round-robin order.
    ready: VecDeque<PeerId>,
    /// Cooldown expiries, in ascending order because the cooldown is constant.
    cooldowns: VecDeque<(Instant, PeerId)>,
    /// Each future resolves once its job, including any blocking work, has actually ended.
    running: FuturesUnordered<BoxFuture<'static, PeerId>>,
    /// Number of queued jobs across all peers.
    pending: usize,
    metrics: metrics::Metrics,
}

#[derive(Default)]
struct Peer {
    queue: VecDeque<QueuedJob>,
    state: PeerState,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum PeerState {
    #[default]
    Idle,
    Running,
    Cooling,
}

impl ProofVerifier {
    /// Spawns the verification actor on the current Tokio runtime.
    ///
    /// The actor exits once all handles are dropped and all running work has ended.
    pub fn spawn() -> Self {
        Self::spawn_actor().0
    }

    /// Verify only ownership signatures. Cancellation never releases a worker
    /// until its blocking verification has actually ended.
    pub async fn verify_lock(
        &self,
        peer: PeerId,
        psbt: bitcoin::PartiallySignedTransaction,
        proofs: Vec<OwnershipProof>,
        challenge: Challenge,
    ) -> Result<VerifiedPsbt, VerificationError> {
        let (reply, response) = oneshot::channel();
        self.request(
            peer,
            Job::Lock {
                psbt,
                proofs,
                challenge,
                reply,
            },
            response,
        )
        .await
    }

    pub fn register_metrics(&self, registry: &mut prometheus_client::registry::Registry) {
        self.metrics.register(registry);
    }

    fn spawn_actor() -> (Self, JoinHandle<()>) {
        let (mailbox, receiver) = mpsc::channel(MAILBOX_CAPACITY);
        let actor = Actor::new(receiver);
        let metrics = actor.metrics.clone();
        let actor = tokio::spawn(actor.run());
        (Self { mailbox, metrics }, actor)
    }

    async fn request<T>(
        &self,
        peer: PeerId,
        job: Job,
        response: oneshot::Receiver<Result<T, VerificationError>>,
    ) -> Result<T, VerificationError> {
        let job = QueuedJob::new(peer, job);
        tracing::debug!(parent: &job.span(), "Submitting ownership-proof verification job");
        if let Err(error) = self.mailbox.send(Request { peer, job }).await {
            let job = error.0.job;
            tracing::warn!(parent: &job.span(), "Rejected ownership-proof job because the verifier is closed");
            self.metrics.rejected("actor_closed");
            return Err(VerificationError::Closed);
        }

        response.await.map_err(|_| VerificationError::Closed)?
    }
}

impl Actor {
    fn new(mailbox: mpsc::Receiver<Request>) -> Self {
        Self {
            mailbox,
            peers: HashMap::new(),
            ready: VecDeque::new(),
            cooldowns: VecDeque::new(),
            running: FuturesUnordered::new(),
            pending: 0,
            metrics: metrics::Metrics::default(),
        }
    }

    async fn run(mut self) {
        let mut mailbox_open = true;

        loop {
            self.expire_cooldowns();
            self.dispatch();
            self.metrics.running.set(self.running.len() as i64);
            self.metrics.queued.set(self.pending as i64);

            if !mailbox_open && self.running.is_empty() && self.pending == 0 {
                return;
            }

            let next_cooldown_expiry = self.cooldowns.front().map(|(expiry, _)| *expiry);

            tokio::select! {
                request = self.mailbox.recv(), if mailbox_open => {
                    let Some(request) = request else {
                        // No handle is left, so no caller can be waiting for a queued job anymore
                        mailbox_open = false;
                        self.purge_canceled();
                        continue;
                    };
                    self.on_request(request);
                }
                Some(peer) = self.running.next(), if !self.running.is_empty() => {
                    self.on_finished(peer);
                }
                () = sleep_until_expiry(next_cooldown_expiry) => {}
            }
        }
    }

    fn on_request(&mut self, Request { peer, job }: Request) {
        if job.is_canceled() {
            job.log_cancelled(self.running.len(), self.pending);
            self.metrics.completed("cancelled");
            return;
        }

        if self.queue_full(peer).is_some() {
            self.purge_canceled();
        }

        if let Some(error) = self.queue_full(peer) {
            tracing::warn!(parent: &job.span(), %error, running = self.running.len(), queued = self.pending, "Rejected ownership-proof verification job");
            self.metrics.rejected(match error {
                VerificationError::PeerQueueFull => "peer_queue_full",
                _ => "global_queue_full",
            });
            job.reject(error);
            return;
        }

        tracing::info!(parent: &job.span(), running = self.running.len(), queued = self.pending + 1, "Queued ownership-proof verification job");
        let entry = self.peers.entry(peer).or_default();
        entry.queue.push_back(job);
        self.pending += 1;

        if entry.state == PeerState::Idle && entry.queue.len() == 1 {
            self.ready.push_back(peer);
        }
    }

    fn queue_full(&self, peer: PeerId) -> Option<VerificationError> {
        if self
            .peers
            .get(&peer)
            .is_some_and(|entry| entry.queue.len() >= MAX_PENDING_JOBS_PER_PEER)
        {
            return Some(VerificationError::PeerQueueFull);
        }

        if self.pending >= MAX_PENDING_JOBS {
            return Some(VerificationError::GlobalQueueFull);
        }

        None
    }

    /// Starts queued jobs of ready peers in round-robin order while worker slots are free.
    fn dispatch(&mut self) {
        while self.running.len() < MAX_CONCURRENT_JOBS {
            let Some(peer_id) = self.ready.pop_front() else {
                return;
            };
            let peer = self
                .peers
                .get_mut(&peer_id)
                .expect("ready peers to have an entry");

            let Some(job) = pop_live_job(&mut peer.queue, &mut self.pending, &self.metrics) else {
                self.peers.remove(&peer_id);
                continue;
            };

            peer.state = PeerState::Running;
            let metrics = self.metrics.clone();
            let wait_seconds = job.queued_at.elapsed().as_secs_f64();
            let span = job.span();
            metrics.queue_wait.observe(wait_seconds);
            tracing::info!(parent: &span, queue_wait_seconds = wait_seconds, running = self.running.len() + 1, queued = self.pending, "Started ownership-proof verification job");
            self.running.push(
                async move {
                    let started = Instant::now();
                    let outcome = job.work.run().await;
                    let duration_seconds = started.elapsed().as_secs_f64();
                    metrics.duration.observe(duration_seconds);
                    metrics.completed(outcome);
                    tracing::info!(
                        outcome,
                        duration_seconds,
                        queue_wait_seconds = wait_seconds,
                        running = metrics.running.get(),
                        queued = metrics.queued.get(),
                        "Ownership-proof job finished; releasing its worker"
                    );
                    peer_id
                }
                .instrument(span)
                .boxed(),
            );
        }
    }

    fn on_finished(&mut self, peer_id: PeerId) {
        let peer = self
            .peers
            .get_mut(&peer_id)
            .expect("running peers to have an entry");
        peer.state = PeerState::Cooling;

        self.cooldowns
            .push_back((Instant::now() + PEER_COOLDOWN, peer_id));
    }

    fn expire_cooldowns(&mut self) {
        let now = Instant::now();

        while let Some(&(expiry, peer_id)) = self.cooldowns.front() {
            if expiry > now {
                return;
            }
            self.cooldowns.pop_front();

            let peer = self
                .peers
                .get_mut(&peer_id)
                .expect("cooling peers to have an entry");

            if peer.queue.is_empty() {
                self.peers.remove(&peer_id);
                continue;
            }

            peer.state = PeerState::Idle;
            self.ready.push_back(peer_id);
        }
    }

    /// Drops queued jobs whose caller stopped waiting and forgets peers left without work.
    fn purge_canceled(&mut self) {
        for peer in self.peers.values_mut() {
            peer.queue.retain(|job| {
                if job.is_canceled() {
                    job.log_cancelled(self.running.len(), self.pending);
                    self.metrics.completed("cancelled");
                    return false;
                }
                true
            });
        }
        self.pending = self.peers.values().map(|peer| peer.queue.len()).sum();

        self.peers
            .retain(|_, peer| peer.state != PeerState::Idle || !peer.queue.is_empty());
        self.ready
            .retain(|peer_id| self.peers.contains_key(peer_id));
    }
}

impl Job {
    fn is_canceled(&self) -> bool {
        match self {
            Job::Lock { reply, .. } => reply.is_closed(),
            #[cfg(test)]
            Job::Test { reply, .. } => reply.is_closed(),
        }
    }

    fn reject(self, error: VerificationError) {
        // The caller may have stopped waiting, then there is nobody to tell
        match self {
            Job::Lock { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            #[cfg(test)]
            Job::Test { reply, .. } => {
                let _ = reply.send(Err(error));
            }
        }
    }

    /// Runs the job to completion. Resolves only once all blocking work has
    /// ended, even if the caller stopped waiting in the meantime.
    async fn run(self) -> &'static str {
        // In all arms the caller may have stopped waiting, then the result is dropped
        match self {
            Job::Lock {
                psbt,
                proofs,
                challenge,
                reply,
            } => {
                let verification = verify_blocking(move || {
                    VerifiedPsbt::verify(psbt, &proofs, challenge)
                        .context("Failed to verify lock input ownership proofs")
                });
                finish_running_job(reply, verification).await
            }
            #[cfg(test)]
            Job::Test { work, reply } => finish_running_job(reply, verify_blocking(work)).await,
        }
    }
}

async fn finish_running_job<T>(
    mut reply: oneshot::Sender<Result<T, VerificationError>>,
    verification: impl std::future::Future<Output = Result<T, VerificationError>>,
) -> &'static str {
    tokio::pin!(verification);
    let result = tokio::select! {
        result = &mut verification => result,
        () = reply.closed() => {
            tracing::info!("Ownership-proof cancellation requested; waiting for started work to finish");
            verification.await
        }
    };
    finish_job(reply, result)
}

fn finish_job<T>(
    reply: oneshot::Sender<Result<T, VerificationError>>,
    result: Result<T, VerificationError>,
) -> &'static str {
    if let Err(error) = &result {
        tracing::warn!(error = %format!("{error:#}"), "Ownership-proof verification failed");
    }
    let outcome = match &result {
        Ok(_) => "success",
        Err(VerificationError::Worker(_)) => "worker_failure",
        Err(_) => "invalid",
    };
    if reply.send(result).is_err() {
        "cancelled"
    } else {
        outcome
    }
}

/// Runs `work` on the blocking thread pool and waits until it has actually ended.
async fn verify_blocking<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, VerificationError> {
    let span = tracing::Span::current();
    tokio::task::spawn_blocking(move || span.in_scope(work))
        .await
        .map_err(VerificationError::Worker)?
        .map_err(VerificationError::Setup)
}

/// Pops queued jobs until one whose caller is still waiting is found.
fn pop_live_job(
    queue: &mut VecDeque<QueuedJob>,
    pending: &mut usize,
    metrics: &metrics::Metrics,
) -> Option<QueuedJob> {
    while let Some(job) = queue.pop_front() {
        *pending -= 1;
        if !job.is_canceled() {
            return Some(job);
        }
        job.log_cancelled(metrics.running.get() as usize, *pending);
        metrics.completed("cancelled");
    }

    None
}

/// Sleeps until `expiry`, or forever if there is none.
async fn sleep_until_expiry(expiry: Option<Instant>) {
    let Some(expiry) = expiry else {
        return std::future::pending().await;
    };

    tokio::time::sleep_until(expiry).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use tokio::sync::mpsc::error::TryRecvError;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    enum Event {
        Started(String),
        Finished(String),
    }
    use Event::{Finished, Started};

    type Gate = std::sync::mpsc::Sender<anyhow::Result<()>>;

    #[tokio::test(start_paused = true)]
    async fn metrics_track_capacity_queueing_and_cancelled_work_until_it_finishes() {
        let (verifier, actor) = ProofVerifier::spawn_actor();
        let metrics = verifier.metrics.clone();
        let mut registry = prometheus_client::registry::Registry::default();
        verifier.register_metrics(&mut registry);
        let mut events = Events::new();
        let peer = PeerId::random();
        let (work, release) = events.gated("running");
        let running = submit(&verifier, peer, work);
        assert_eq!(events.next().await, Started("running".into()));
        let queued = submit(&verifier, peer, || Ok(()));
        settle().await;
        assert_eq!(metrics.running.get(), 1);
        assert_eq!(metrics.queued.get(), 1);
        queued.abort();
        running.abort();
        drop(verifier);
        settle().await;
        assert_eq!(
            metrics.running.get(),
            1,
            "cancellation must not free a busy worker"
        );
        assert_eq!(metrics.queued.get(), 0);
        release.send(Ok(())).unwrap();
        actor.await.unwrap();
        assert_eq!(metrics.running.get(), 0);
        let mut text = String::new();
        prometheus_client::encoding::text::encode(&mut text, &registry).unwrap();
        assert!(
            text.contains("lock_proof_verification_capacity 50"),
            "{text}"
        );
        assert!(
            text.contains("lock_proof_verification_completed_total{outcome=\"cancelled\"} 2"),
            "{text}"
        );
        assert!(
            text.contains("lock_proof_verification_duration_seconds_count 1"),
            "{text}"
        );
        assert!(!text.contains(&peer.to_string()));
    }

    #[tokio::test(start_paused = true)]
    async fn cloned_handles_share_serialization_and_cooldown_starts_after_completion() {
        let verifier = ProofVerifier::spawn();
        let other_handle = verifier.clone();
        let mut events = Events::new();
        let peer = PeerId::random();

        let (first, release_first) = events.gated("first");
        let (second, release_second) = events.gated("second");
        let first_job = submit(&verifier, peer, first);
        settle().await;
        let second_job = submit(&other_handle, peer, second);

        assert_eq!(events.next().await, Started("first".into()));
        settle().await;
        events.assert_quiet();

        // Cooldown must count from completion, not from start
        tokio::time::advance(Duration::from_secs(1)).await;
        release_first.send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("first".into()));
        first_job.await.unwrap().unwrap();
        let completed_at = Instant::now();

        assert_eq!(events.next().await, Started("second".into()));
        assert_eq!(Instant::now(), completed_at + PEER_COOLDOWN);

        release_second.send(Ok(())).unwrap();
        second_job.await.unwrap().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn cooldown_starts_after_failed_job() {
        let verifier = ProofVerifier::spawn();
        let mut events = Events::new();
        let peer = PeerId::random();

        let (failing, release_failing) = events.gated("failing");
        let (next, release_next) = events.gated("next");
        let failing_job = submit(&verifier, peer, failing);
        settle().await;
        let next_job = submit(&verifier, peer, next);

        assert_eq!(events.next().await, Started("failing".into()));
        tokio::time::advance(Duration::from_secs(2)).await;
        release_failing
            .send(Err(anyhow::anyhow!("invalid proof")))
            .unwrap();
        let result = failing_job.await.unwrap();
        assert!(
            matches!(result, Err(VerificationError::Setup(_))),
            "{result:?}"
        );
        let failed_at = Instant::now();

        assert_eq!(events.next().await, Finished("failing".into()));
        assert_eq!(events.next().await, Started("next".into()));
        assert_eq!(Instant::now(), failed_at + PEER_COOLDOWN);

        release_next.send(Ok(())).unwrap();
        next_job.await.unwrap().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn other_peers_progress_while_a_peer_cools_down() {
        let verifier = ProofVerifier::spawn();
        let mut events = Events::new();
        let alice = PeerId::random();
        let bob = PeerId::random();

        let (first, release_first) = events.gated("alice-1");
        let (second, release_second) = events.gated("alice-2");
        let (other, release_other) = events.gated("bob");
        let _first_job = submit(&verifier, alice, first);
        settle().await;
        let _second_job = submit(&verifier, alice, second);
        assert_eq!(events.next().await, Started("alice-1".into()));

        release_first.send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("alice-1".into()));
        settle().await;
        let completed_at = Instant::now();

        let _other_job = submit(&verifier, bob, other);
        assert_eq!(events.next().await, Started("bob".into()));
        assert_eq!(Instant::now(), completed_at);

        release_other.send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("bob".into()));
        assert_eq!(events.next().await, Started("alice-2".into()));
        assert_eq!(Instant::now(), completed_at + PEER_COOLDOWN);

        release_second.send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("alice-2".into()));
    }

    #[tokio::test(start_paused = true)]
    async fn at_most_max_concurrent_jobs_run_at_once() {
        let labels: Vec<_> = (0..=MAX_CONCURRENT_JOBS).map(|i| format!("p{i}")).collect();

        let verifier = ProofVerifier::spawn();
        let mut events = Events::new();

        let mut gates = HashMap::new();
        for label in &labels {
            let (job, gate) = events.gated(label);
            submit(&verifier, PeerId::random(), job);
            gates.insert(label.clone(), gate);
            settle().await;
        }

        let mut started = HashSet::new();
        for _ in 0..MAX_CONCURRENT_JOBS {
            started.insert(events.next().await);
        }
        let expected: HashSet<_> = labels
            .iter()
            .take(MAX_CONCURRENT_JOBS)
            .map(|label| Started(label.clone()))
            .collect();
        assert_eq!(started, expected);
        settle().await;
        events.assert_quiet();

        let started_at = Instant::now();
        gates["p0"].send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("p0".into()));
        assert_eq!(
            events.next().await,
            Started(format!("p{MAX_CONCURRENT_JOBS}"))
        );
        assert_eq!(
            Instant::now(),
            started_at,
            "other peers must not wait for a cooldown"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn canceled_queued_job_is_skipped() {
        let verifier = ProofVerifier::spawn();
        let mut events = Events::new();
        let peer = PeerId::random();

        let (running, release_running) = events.gated("running");
        let (canceled, _release_canceled) = events.gated("canceled");
        let (next, release_next) = events.gated("next");
        let _running_job = submit(&verifier, peer, running);
        assert_eq!(events.next().await, Started("running".into()));

        let canceled_job = submit(&verifier, peer, canceled);
        settle().await;
        canceled_job.abort();
        settle().await;
        let next_job = submit(&verifier, peer, next);
        settle().await;

        release_running.send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("running".into()));
        let completed_at = Instant::now();

        assert_eq!(events.next().await, Started("next".into()));
        assert_eq!(Instant::now(), completed_at + PEER_COOLDOWN);

        release_next.send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("next".into()));
        next_job.await.unwrap().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn dropped_caller_keeps_worker_slot_until_blocking_work_ends() {
        let labels: Vec<_> = (0..MAX_CONCURRENT_JOBS).map(|i| format!("p{i}")).collect();

        let verifier = ProofVerifier::spawn();
        let mut events = Events::new();
        let peers: Vec<_> = labels.iter().map(|_| PeerId::random()).collect();

        let mut gates = HashMap::new();
        let mut callers = Vec::new();
        for (label, peer) in labels.iter().zip(&peers) {
            let (job, gate) = events.gated(label);
            callers.push(submit(&verifier, *peer, job));
            gates.insert(label.clone(), gate);
        }
        for _ in &labels {
            assert!(matches!(events.next().await, Started(_)));
        }

        callers[0].abort();
        settle().await;
        let (waiting, release_waiting) = events.gated("waiting");
        let _waiting_job = submit(&verifier, PeerId::random(), waiting);
        settle().await;
        events.assert_quiet();

        gates["p0"].send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("p0".into()));
        let completed_at = Instant::now();
        assert_eq!(events.next().await, Started("waiting".into()));

        // The canceled job's peer still cools down from when its work ended
        let (again, release_again) = events.gated("p0-again");
        let _again_job = submit(&verifier, peers[0], again);
        settle().await;
        events.assert_quiet();

        for label in labels.iter().skip(1) {
            gates[label].send(Ok(())).unwrap();
            assert_eq!(events.next().await, Finished(label.clone()));
        }
        release_waiting.send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("waiting".into()));

        assert_eq!(events.next().await, Started("p0-again".into()));
        assert_eq!(Instant::now(), completed_at + PEER_COOLDOWN);
        release_again.send(Ok(())).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn per_peer_queue_is_bounded_by_live_requests() {
        let verifier = ProofVerifier::spawn();
        let mut events = Events::new();
        let peer = PeerId::random();

        let (running, _release_running) = events.gated("running");
        let _running_job = submit(&verifier, peer, running);
        assert_eq!(events.next().await, Started("running".into()));

        let mut queued: Vec<_> = (0..MAX_PENDING_JOBS_PER_PEER)
            .map(|_| submit(&verifier, peer, || Ok(())))
            .collect();
        settle().await;

        let result = verifier.run_test_job(peer, || Ok(())).await;
        assert!(
            matches!(result, Err(VerificationError::PeerQueueFull)),
            "{result:?}"
        );

        // Other peers are unaffected
        verifier
            .run_test_job(PeerId::random(), || Ok(()))
            .await
            .unwrap();

        // A canceled request frees its place
        queued.pop().unwrap().abort();
        settle().await;
        let accepted = submit(&verifier, peer, || Ok(()));
        settle().await;
        assert!(!accepted.is_finished());
    }

    #[tokio::test(start_paused = true)]
    async fn global_queue_is_bounded_by_live_requests() {
        let verifier = ProofVerifier::spawn();
        let mut events = Events::new();

        let _gates: Vec<_> = (0..MAX_CONCURRENT_JOBS)
            .map(|i| {
                let label = format!("r{i}");
                let (job, gate) = events.gated(label);
                submit(&verifier, PeerId::random(), job);
                gate
            })
            .collect();
        for _ in 0..MAX_CONCURRENT_JOBS {
            assert!(matches!(events.next().await, Started(_)));
        }

        let peers: Vec<_> = (0..MAX_PENDING_JOBS.div_ceil(MAX_PENDING_JOBS_PER_PEER))
            .map(|_| PeerId::random())
            .collect();
        let mut queued: Vec<_> = (0..MAX_PENDING_JOBS)
            .map(|i| submit(&verifier, peers[i / MAX_PENDING_JOBS_PER_PEER], || Ok(())))
            .collect();
        settle().await;

        let result = verifier.run_test_job(PeerId::random(), || Ok(())).await;
        assert!(
            matches!(result, Err(VerificationError::GlobalQueueFull)),
            "{result:?}"
        );

        queued.pop().unwrap().abort();
        settle().await;
        let accepted = submit(&verifier, PeerId::random(), || Ok(()));
        settle().await;
        assert!(!accepted.is_finished());
    }

    #[tokio::test(start_paused = true)]
    async fn idle_peers_are_forgotten_once_cooled_down() {
        let (_mailbox, receiver) = mpsc::channel(1);
        let mut actor = Actor::new(receiver);
        let peer = PeerId::random();

        let (reply, _response) = oneshot::channel();
        actor.on_request(Request {
            peer,
            job: QueuedJob::new(
                peer,
                Job::Test {
                    work: Box::new(|| Ok(())),
                    reply,
                },
            ),
        });
        actor.dispatch();
        assert_eq!(actor.running.next().await, Some(peer));
        actor.on_finished(peer);

        tokio::time::advance(PEER_COOLDOWN - Duration::from_millis(1)).await;
        actor.expire_cooldowns();
        assert_eq!(actor.peers[&peer].state, PeerState::Cooling);

        tokio::time::advance(Duration::from_millis(1)).await;
        actor.expire_cooldowns();
        assert!(actor.peers.is_empty());
        assert!(actor.ready.is_empty());
        assert!(actor.cooldowns.is_empty());
        assert_eq!(actor.pending, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn actor_exits_once_handles_are_dropped_and_running_work_ended() {
        let (verifier, actor) = ProofVerifier::spawn_actor();
        let mut events = Events::new();
        let peer = PeerId::random();

        let (running, release_running) = events.gated("running");
        let running_job = submit(&verifier, peer, running);
        assert_eq!(events.next().await, Started("running".into()));
        let queued_job = submit(&verifier, peer, || Ok(()));
        settle().await;

        running_job.abort();
        queued_job.abort();
        drop(verifier);
        settle().await;
        assert!(!actor.is_finished());

        release_running.send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("running".into()));
        let finished_at = Instant::now();
        actor.await.unwrap();
        assert_eq!(
            Instant::now(),
            finished_at,
            "actor must not wait for cooldowns on shutdown"
        );
    }

    impl ProofVerifier {
        async fn run_test_job(
            &self,
            peer: PeerId,
            work: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
        ) -> Result<(), VerificationError> {
            let (reply, response) = oneshot::channel();
            let job = Job::Test {
                work: Box::new(work),
                reply,
            };

            self.request(peer, job, response).await
        }
    }

    struct Events {
        sender: mpsc::UnboundedSender<Event>,
        receiver: mpsc::UnboundedReceiver<Event>,
    }

    impl Events {
        fn new() -> Self {
            let (sender, receiver) = mpsc::unbounded_channel();
            Self { sender, receiver }
        }

        /// Creates a job that reports its start, blocks its worker thread
        /// until the returned gate yields its result and then reports its end.
        fn gated(
            &self,
            label: impl Into<String>,
        ) -> (impl FnOnce() -> anyhow::Result<()> + Send + 'static, Gate) {
            let label = label.into();
            let (gate, gate_receiver) = std::sync::mpsc::channel();
            let events = self.sender.clone();
            let job = move || {
                events.send(Started(label.clone())).unwrap();
                let result = gate_receiver.recv().context("Test dropped the gate")?;
                events.send(Finished(label.clone())).unwrap();
                result
            };

            (job, gate)
        }

        async fn next(&mut self) -> Event {
            tokio::time::timeout(Duration::from_secs(60), self.receiver.recv())
                .await
                .expect("an event to arrive")
                .expect("the event channel to be open")
        }

        fn assert_quiet(&mut self) {
            assert_eq!(self.receiver.try_recv(), Err(TryRecvError::Empty));
        }
    }

    fn submit(
        verifier: &ProofVerifier,
        peer: PeerId,
        work: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
    ) -> JoinHandle<Result<(), VerificationError>> {
        let verifier = verifier.clone();
        tokio::spawn(async move { verifier.run_test_job(peer, work).await })
    }

    /// Gives the actor and worker threads real time to make progress while
    /// the paused clock stands still (blocking tasks inhibit auto-advance).
    async fn settle() {
        tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_millis(50)))
            .await
            .unwrap();
    }
}
