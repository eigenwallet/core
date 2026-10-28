//! Background verification of the inputs of Bob's Bitcoin lock transaction.
//!
//! A single [`LockInputVerifier`] is shared by all swap setup handlers. Its actor
//! runs at most one job per peer at a time, lets every peer cool down for
//! [`PEER_COOLDOWN`] after each of its jobs finished and limits the number of
//! jobs running concurrently across all peers. A job checks the ownership
//! signatures on Tokio's blocking thread pool and then queries Electrum for the
//! spent outputs. Queued jobs do no work. This actor has no swap state.

mod metrics;

use anyhow::Context;
use bitcoin_wallet::{PsbtPrevoutVerificationError, VerifyPsbtPrevouts};
use futures::StreamExt;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::FuturesUnordered;
use libp2p::PeerId;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use swap_core::bitcoin;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::Instant;
use tracing::Instrument;
use utxo_ownership_proof::{Challenge, OwnershipProof, VerifiedPsbt};
use uuid::Uuid;

/// How long a peer has to wait after one of its jobs finished before its next job starts.
pub const PEER_COOLDOWN: Duration = Duration::from_secs(5);
/// Maximum number of jobs doing work (proof verification and Electrum calls)
/// at the same time across all peers. Queued jobs do not count.
pub const MAX_CONCURRENT_JOBS: usize = 25;
/// Maximum number of queued (not yet running) jobs across all peers. Further
/// jobs are rejected with [`VerificationError::GlobalQueueFull`].
pub const MAX_PENDING_JOBS: usize = 512;
/// Maximum number of queued (not yet running) jobs of a single peer.
pub const MAX_PENDING_JOBS_PER_PEER: usize = 4;

const MAILBOX_CAPACITY: usize = 64;
const LOCK_INPUT_CHECK_INITIAL_RETRY_DELAY: Duration = Duration::from_millis(500);
const LOCK_INPUT_CHECK_MAX_RETRY_DELAY: Duration = Duration::from_secs(5);

/// Cloneable handle to the verification actor.
///
/// All clones share the same actor and therefore the same per-peer
/// serialization, cooldowns and limits.
#[derive(Clone, Debug)]
pub struct LockInputVerifier {
    mailbox: mpsc::Sender<Request>,
    metrics: metrics::Metrics,
}

#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("Lock input verifier is not running")]
    Closed,
    #[error("Too many verification requests are queued for this peer")]
    PeerQueueFull,
    #[error("Too many verification requests are queued")]
    GlobalQueueFull,
    #[error("Verification worker failed")]
    Worker(#[source] JoinError),
    #[error(transparent)]
    Proofs(anyhow::Error),
    #[error("Failed to verify lock input prevouts")]
    Prevouts(#[source] PsbtPrevoutVerificationError),
}

impl VerificationError {
    /// Whether verification failed on our side or might pass later, rather
    /// than because the inputs are invalid.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Closed | Self::PeerQueueFull | Self::GlobalQueueFull | Self::Worker(_) => true,
            Self::Proofs(_) => false,
            // Bob's inputs may not have reached our Electrum servers yet
            Self::Prevouts(error) => {
                error.is_transient()
                    || matches!(
                        error,
                        PsbtPrevoutVerificationError::Cancelled
                            | PsbtPrevoutVerificationError::TimedOut
                            | PsbtPrevoutVerificationError::PrevoutNotFound { .. }
                    )
            }
        }
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
        tracing::info_span!("lock_input_verification", job_id = %self.id, peer = %self.peer, input_count = self.input_count)
    }

    fn is_canceled(&self) -> bool {
        self.work.is_canceled()
    }
    fn reject(self, error: VerificationError) {
        self.work.reject(error);
    }

    fn deadline(&self) -> Option<Instant> {
        self.work.deadline()
    }

    fn is_expired(&self) -> bool {
        self.deadline()
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    fn log_cancelled(&self, running: usize, queued: usize) {
        tracing::info!(parent: &self.span(), running, queued, queue_wait_seconds = self.queued_at.elapsed().as_secs_f64(), "Cancelled lock input verification job before it started");
    }

    /// Answers the job as timed out, like the Electrum retries would have.
    fn expire(self, running: usize, queued: usize) {
        tracing::info!(parent: &self.span(), running, queued, queue_wait_seconds = self.queued_at.elapsed().as_secs_f64(), "Lock input verification job expired before it started");
        self.reject(VerificationError::Prevouts(
            PsbtPrevoutVerificationError::TimedOut,
        ));
    }
}

#[cfg(test)]
type TestWork = Box<
    dyn FnOnce(watch::Receiver<bool>) -> BoxFuture<'static, Result<(), VerificationError>> + Send,
>;

#[allow(clippy::large_enum_variant)]
enum Job {
    Lock {
        psbt: bitcoin::PartiallySignedTransaction,
        proofs: Vec<OwnershipProof>,
        challenge: Challenge,
        deadline: Instant,
        reply: oneshot::Sender<Result<VerifiedPsbt, VerificationError>>,
    },
    #[cfg(test)]
    Test {
        work: TestWork,
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
    prevout_verifier: Arc<dyn VerifyPsbtPrevouts>,
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

impl LockInputVerifier {
    /// Spawns the verification actor on the current Tokio runtime.
    ///
    /// The actor exits once all handles are dropped and all running work has ended.
    pub fn spawn(prevout_verifier: Arc<dyn VerifyPsbtPrevouts>) -> Self {
        Self::spawn_actor(prevout_verifier).0
    }

    /// Verifies the ownership proofs, then that every input spends an existing
    /// unspent output with the claimed script and value. Transient Electrum
    /// failures are retried until `deadline`; a job still queued at `deadline`
    /// never starts. Cancellation never releases a worker until its started
    /// work has actually ended.
    pub async fn verify_lock(
        &self,
        peer: PeerId,
        psbt: bitcoin::PartiallySignedTransaction,
        proofs: Vec<OwnershipProof>,
        challenge: Challenge,
        deadline: Instant,
    ) -> Result<VerifiedPsbt, VerificationError> {
        let (reply, response) = oneshot::channel();
        self.request(
            peer,
            Job::Lock {
                psbt,
                proofs,
                challenge,
                deadline,
                reply,
            },
            response,
        )
        .await
    }

    pub fn register_metrics(&self, registry: &mut prometheus_client::registry::Registry) {
        self.metrics.register(registry);
    }

    fn spawn_actor(prevout_verifier: Arc<dyn VerifyPsbtPrevouts>) -> (Self, JoinHandle<()>) {
        let (mailbox, receiver) = mpsc::channel(MAILBOX_CAPACITY);
        let actor = Actor::new(receiver, prevout_verifier);
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
        tracing::debug!(parent: &job.span(), "Submitting lock input verification job");
        if let Err(error) = self.mailbox.send(Request { peer, job }).await {
            let job = error.0.job;
            tracing::warn!(parent: &job.span(), "Rejected lock input verification job because the verifier is closed");
            self.metrics.rejected("actor_closed");
            return Err(VerificationError::Closed);
        }

        response.await.map_err(|_| VerificationError::Closed)?
    }
}

impl Actor {
    fn new(
        mailbox: mpsc::Receiver<Request>,
        prevout_verifier: Arc<dyn VerifyPsbtPrevouts>,
    ) -> Self {
        Self {
            mailbox,
            peers: HashMap::new(),
            ready: VecDeque::new(),
            cooldowns: VecDeque::new(),
            running: FuturesUnordered::new(),
            pending: 0,
            metrics: metrics::Metrics::default(),
            prevout_verifier,
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
            let next_deadline = self
                .peers
                .values()
                .flat_map(|peer| &peer.queue)
                .filter_map(QueuedJob::deadline)
                .min();

            tokio::select! {
                request = self.mailbox.recv(), if mailbox_open => {
                    let Some(request) = request else {
                        // No handle is left, so no caller can be waiting for a queued job anymore
                        mailbox_open = false;
                        self.purge_stale();
                        continue;
                    };
                    self.on_request(request);
                }
                Some(peer) = self.running.next(), if !self.running.is_empty() => {
                    self.on_finished(peer);
                }
                () = sleep_until_expiry(next_cooldown_expiry) => {}
                // Answers expired queued jobs even while all workers are busy
                () = sleep_until_expiry(next_deadline) => self.purge_stale(),
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
            self.purge_stale();
        }

        if let Some(error) = self.queue_full(peer) {
            tracing::warn!(parent: &job.span(), %error, running = self.running.len(), queued = self.pending, "Rejected lock input verification job");
            self.metrics.rejected(match error {
                VerificationError::PeerQueueFull => "peer_queue_full",
                _ => "global_queue_full",
            });
            job.reject(error);
            return;
        }

        tracing::info!(parent: &job.span(), running = self.running.len(), queued = self.pending + 1, "Queued lock input verification job");
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
            let prevout_verifier = self.prevout_verifier.clone();
            let wait_seconds = job.queued_at.elapsed().as_secs_f64();
            let span = job.span();
            metrics.queue_wait.observe(wait_seconds);
            tracing::info!(parent: &span, queue_wait_seconds = wait_seconds, running = self.running.len() + 1, queued = self.pending, "Started lock input verification job");
            self.running.push(
                async move {
                    let started = Instant::now();
                    let outcome = job.work.run(prevout_verifier).await;
                    let duration_seconds = started.elapsed().as_secs_f64();
                    metrics.duration.observe(duration_seconds);
                    metrics.completed(outcome);
                    tracing::info!(
                        outcome,
                        duration_seconds,
                        queue_wait_seconds = wait_seconds,
                        running = metrics.running.get(),
                        queued = metrics.queued.get(),
                        "Lock input verification job finished; releasing its worker"
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

    /// Drops queued jobs whose caller stopped waiting, answers expired ones and
    /// forgets peers left without work.
    fn purge_stale(&mut self) {
        for peer in self.peers.values_mut() {
            for job in std::mem::take(&mut peer.queue) {
                if let Some(job) = keep_live(job, self.running.len(), self.pending, &self.metrics) {
                    peer.queue.push_back(job);
                }
            }
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

    fn deadline(&self) -> Option<Instant> {
        match self {
            Job::Lock { deadline, .. } => Some(*deadline),
            #[cfg(test)]
            Job::Test { .. } => None,
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

    /// Runs the job to completion. Resolves only once all started work has
    /// ended, even if the caller stopped waiting in the meantime.
    async fn run(self, prevout_verifier: Arc<dyn VerifyPsbtPrevouts>) -> &'static str {
        // In all arms the caller may have stopped waiting, then the result is dropped
        match self {
            Job::Lock {
                psbt,
                proofs,
                challenge,
                deadline,
                reply,
            } => {
                finish_running_job(reply, move |cancel| async move {
                    let verified = verify_blocking(move || {
                        VerifiedPsbt::verify(psbt, &proofs, challenge)
                            .context("Failed to verify lock input ownership proofs")
                    })
                    .await?;
                    let psbt = verified.psbt();
                    let prevout_verifier = prevout_verifier.as_ref();
                    retry_lock_input_check(
                        |attempt_cancel| async move {
                            prevout_verifier
                                .verify_psbt_prevouts(psbt, &attempt_cancel)
                                .await
                        },
                        deadline,
                        LOCK_INPUT_CHECK_MAX_RETRY_DELAY,
                        cancel,
                    )
                    .await
                    .map_err(VerificationError::Prevouts)?;
                    Ok(verified)
                })
                .await
            }
            #[cfg(test)]
            Job::Test { work, reply } => finish_running_job(reply, work).await,
        }
    }
}

/// Runs `verification` and delivers its result. If the caller stops waiting,
/// the work is told to stop through its cancellation receiver and still awaited.
async fn finish_running_job<T, F, Fut>(
    mut reply: oneshot::Sender<Result<T, VerificationError>>,
    verification: F,
) -> &'static str
where
    F: FnOnce(watch::Receiver<bool>) -> Fut,
    Fut: std::future::Future<Output = Result<T, VerificationError>>,
{
    let (cancel, cancellation) = watch::channel(false);
    let verification = verification(cancellation);
    tokio::pin!(verification);
    let result = tokio::select! {
        result = &mut verification => result,
        () = reply.closed() => {
            tracing::info!("Lock input verification cancellation requested; waiting for started work to finish");
            cancel.send_replace(true);
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
        tracing::warn!(error = %format!("{error:#}"), "Lock input verification failed");
    }
    let outcome = match &result {
        Ok(_) => "success",
        Err(VerificationError::Worker(_)) => "worker_failure",
        Err(error) if error.is_transient() => "inconclusive",
        Err(_) => "invalid",
    };
    if reply.send(result).is_err() {
        "cancelled"
    } else {
        outcome
    }
}

/// Retries only transient Electrum failures until `deadline`.
async fn retry_lock_input_check<F, Fut>(
    mut verify: F,
    deadline: Instant,
    max_interval: Duration,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), PsbtPrevoutVerificationError>
where
    F: FnMut(watch::Receiver<bool>) -> Fut,
    Fut: std::future::Future<Output = Result<(), PsbtPrevoutVerificationError>>,
{
    use backoff::backoff::Backoff;

    let mut retry_policy = backoff::ExponentialBackoffBuilder::new()
        .with_initial_interval(LOCK_INPUT_CHECK_INITIAL_RETRY_DELAY.min(max_interval))
        .with_max_elapsed_time(None)
        .with_max_interval(max_interval)
        .build();

    loop {
        if *cancel.borrow() || cancel.has_changed().is_err() {
            return Err(PsbtPrevoutVerificationError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(PsbtPrevoutVerificationError::TimedOut);
        }

        let (attempt_cancel, attempt_cancellation) = watch::channel(false);
        let verification = verify(attempt_cancellation);
        tokio::pin!(verification);
        let result = tokio::select! {
            result = &mut verification => result,
            _ = cancel.changed() => {
                attempt_cancel.send_replace(true);
                // Drain started RPCs before the worker is released.
                let _ = verification.await;
                return Err(PsbtPrevoutVerificationError::Cancelled);
            }
            _ = tokio::time::sleep_until(deadline) => {
                attempt_cancel.send_replace(true);
                let _ = verification.await;
                return Err(PsbtPrevoutVerificationError::TimedOut);
            }
        };
        let Err(error) = result else { return Ok(()) };
        if !error.is_transient() {
            return Err(error);
        }
        let retry_after = retry_policy
            .next_backoff()
            .expect("backoff without a maximum elapsed time to never stop");
        tracing::warn!(%error, retry_in_seconds = retry_after.as_secs_f64(), "Electrum verification failed, retrying");
        tokio::select! {
            _ = tokio::time::sleep_until((Instant::now() + retry_after).min(deadline)) => {}
            _ = cancel.changed() => return Err(PsbtPrevoutVerificationError::Cancelled),
        }
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
        .map_err(VerificationError::Proofs)
}

/// Pops queued jobs until one whose caller is still waiting and whose deadline
/// has not passed is found. Skipped jobs are answered without taking a worker.
fn pop_live_job(
    queue: &mut VecDeque<QueuedJob>,
    pending: &mut usize,
    metrics: &metrics::Metrics,
) -> Option<QueuedJob> {
    while let Some(job) = queue.pop_front() {
        *pending -= 1;
        if let Some(job) = keep_live(job, metrics.running.get() as usize, *pending, metrics) {
            return Some(job);
        }
    }

    None
}

/// Returns the job unless its caller stopped waiting or its deadline passed,
/// in which case it is accounted for and answered if anyone is waiting.
fn keep_live(
    job: QueuedJob,
    running: usize,
    queued: usize,
    metrics: &metrics::Metrics,
) -> Option<QueuedJob> {
    if job.is_canceled() {
        job.log_cancelled(running, queued);
        metrics.completed("cancelled");
        return None;
    }
    if job.is_expired() {
        job.expire(running, queued);
        metrics.completed("expired");
        return None;
    }
    Some(job)
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
    use anyhow::anyhow;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
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
        let (verifier, actor) = LockInputVerifier::spawn_actor(stub_prevouts());
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
            text.contains(&format!(
                "lock_proof_verification_capacity {MAX_CONCURRENT_JOBS}"
            )),
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
        let verifier = LockInputVerifier::spawn(stub_prevouts());
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
        let verifier = LockInputVerifier::spawn(stub_prevouts());
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
            matches!(result, Err(VerificationError::Proofs(_))),
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
        let verifier = LockInputVerifier::spawn(stub_prevouts());
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

        let verifier = LockInputVerifier::spawn(stub_prevouts());
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
        let verifier = LockInputVerifier::spawn(stub_prevouts());
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

        let verifier = LockInputVerifier::spawn(stub_prevouts());
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
        let verifier = LockInputVerifier::spawn(stub_prevouts());
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
        let verifier = LockInputVerifier::spawn(stub_prevouts());
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
        let mut actor = Actor::new(receiver, stub_prevouts());
        let peer = PeerId::random();

        let (reply, _response) = oneshot::channel();
        actor.on_request(Request {
            peer,
            job: QueuedJob::new(
                peer,
                Job::Test {
                    work: Box::new(|_| async { Ok(()) }.boxed()),
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
        let (verifier, actor) = LockInputVerifier::spawn_actor(stub_prevouts());
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

    #[tokio::test(start_paused = true)]
    async fn dropped_caller_cancels_async_work_and_keeps_worker_until_it_ends() {
        let (verifier, actor) = LockInputVerifier::spawn_actor(stub_prevouts());
        let (started, started_rx) = oneshot::channel();
        let (stopping, stopping_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let caller = tokio::spawn({
            let verifier = verifier.clone();
            async move {
                verifier
                    .run_async_test_job(PeerId::random(), move |mut cancel| {
                        async move {
                            started.send(()).unwrap();
                            cancel.changed().await.unwrap();
                            stopping.send(()).unwrap();
                            release_rx.await.unwrap();
                            Ok(())
                        }
                        .boxed()
                    })
                    .await
            }
        });
        started_rx.await.unwrap();

        caller.abort();
        stopping_rx.await.unwrap();
        drop(verifier);
        settle().await;
        assert!(
            !actor.is_finished(),
            "cancellation must not free a busy worker"
        );

        release.send(()).unwrap();
        actor.await.unwrap();
    }

    #[tokio::test]
    async fn lock_input_check_does_not_retry_missing_prevout() {
        let (_cancel, cancellation) = watch::channel(false);
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_check = attempts.clone();

        let result = retry_lock_input_check(
            move |_cancel| {
                let attempts = attempts_for_check.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Err(PsbtPrevoutVerificationError::PrevoutNotFound {
                        outpoint: ::bitcoin::OutPoint::null(),
                    })
                }
            },
            Instant::now() + Duration::from_millis(100),
            Duration::from_millis(1),
            cancellation,
        )
        .await;

        assert!(matches!(
            result,
            Err(PsbtPrevoutVerificationError::PrevoutNotFound { .. })
        ));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn transient_lock_input_failures_retry_within_one_deadline() {
        let (_cancel, cancellation) = watch::channel(false);
        let mut attempts = 0;
        let deadline = Instant::now() + Duration::from_secs(45);
        let result = retry_lock_input_check(
            |_| {
                attempts += 1;
                std::future::ready(if attempts < 3 {
                    Err(PsbtPrevoutVerificationError::Electrum(anyhow!(
                        "network unavailable"
                    )))
                } else {
                    Ok(())
                })
            },
            deadline,
            Duration::from_secs(5),
            cancellation.clone(),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(attempts, 3);
        assert!(Instant::now() < deadline);

        let result = retry_lock_input_check(
            |_| {
                std::future::ready(Err(PsbtPrevoutVerificationError::Electrum(anyhow!(
                    "still offline"
                ))))
            },
            deadline,
            Duration::from_secs(5),
            cancellation,
        )
        .await;
        assert!(matches!(
            result,
            Err(PsbtPrevoutVerificationError::TimedOut)
        ));
        assert_eq!(Instant::now(), deadline);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_lock_input_check_drains_started_work() {
        check_verification_drains_started_work(true).await;
    }

    #[tokio::test(start_paused = true)]
    async fn expired_lock_input_check_drains_started_work() {
        check_verification_drains_started_work(false).await;
    }

    async fn check_verification_drains_started_work(cancel_requested: bool) {
        let (cancel, cancellation) = watch::channel(false);
        let (started, started_rx) = oneshot::channel();
        let (stopping, stopping_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let mut channels = Some((started, stopping, release_rx));
        let job = tokio::spawn(retry_lock_input_check(
            move |mut attempt_cancel| {
                let (started, stopping, release) = channels.take().expect("one attempt only");
                async move {
                    started.send(()).unwrap();
                    attempt_cancel.changed().await.unwrap();
                    assert!(*attempt_cancel.borrow());
                    stopping.send(()).unwrap();
                    release.await.unwrap();
                    Err(PsbtPrevoutVerificationError::Cancelled)
                }
            },
            Instant::now() + Duration::from_secs(45),
            Duration::from_secs(5),
            cancellation,
        ));
        started_rx.await.unwrap();
        if cancel_requested {
            cancel.send(true).unwrap();
        } else {
            tokio::time::advance(Duration::from_secs(45)).await;
        }
        stopping_rx.await.unwrap();
        assert!(
            !job.is_finished(),
            "must wait for started RPC work to finish"
        );
        release.send(()).unwrap();
        let result = job.await.unwrap();
        if cancel_requested {
            assert!(matches!(
                result,
                Err(PsbtPrevoutVerificationError::Cancelled)
            ));
        } else {
            assert!(matches!(
                result,
                Err(PsbtPrevoutVerificationError::TimedOut)
            ));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_lock_input_check_does_not_start_a_retry() {
        let (cancel, cancellation) = watch::channel(false);
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_check = attempts.clone();
        let (started, started_rx) = oneshot::channel();
        let mut started = Some(started);
        let job = tokio::spawn(retry_lock_input_check(
            move |_cancel| {
                attempts_for_check.fetch_add(1, Ordering::SeqCst);
                started
                    .take()
                    .expect("no retry after cancellation")
                    .send(())
                    .unwrap();
                async {
                    Err(PsbtPrevoutVerificationError::Electrum(anyhow!(
                        "unavailable"
                    )))
                }
            },
            Instant::now() + Duration::from_secs(45),
            Duration::from_secs(5),
            cancellation,
        ));
        started_rx.await.unwrap();
        cancel.send(true).unwrap();
        assert!(matches!(
            job.await.unwrap(),
            Err(PsbtPrevoutVerificationError::Cancelled)
        ));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    impl LockInputVerifier {
        async fn run_test_job(
            &self,
            peer: PeerId,
            work: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
        ) -> Result<(), VerificationError> {
            self.run_async_test_job(peer, move |_| verify_blocking(work).boxed())
                .await
        }

        async fn run_async_test_job(
            &self,
            peer: PeerId,
            work: impl FnOnce(
                watch::Receiver<bool>,
            ) -> BoxFuture<'static, Result<(), VerificationError>>
            + Send
            + 'static,
        ) -> Result<(), VerificationError> {
            let (reply, response) = oneshot::channel();
            let job = Job::Test {
                work: Box::new(work),
                reply,
            };

            self.request(peer, job, response).await
        }
    }

    #[tokio::test]
    async fn invalid_proofs_never_reach_the_prevout_check() {
        let prevouts = stub_prevouts();
        let verifier = LockInputVerifier::spawn(prevouts.clone());
        let (psbt, _, challenge) = signed_lock_inputs();
        let wrong_challenge = utxo_ownership_proof::challenge(b"other swap");
        let proofs = vec![utxo_ownership_proof::sign(&secret_key(), wrong_challenge)];

        let error = verifier
            .verify_lock(PeerId::random(), psbt, proofs, challenge, deadline())
            .await
            .unwrap_err();

        assert!(matches!(error, VerificationError::Proofs(_)), "{error:?}");
        assert!(!error.is_transient());
        assert_eq!(prevouts.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn prevout_results_keep_their_classification() {
        let (result, calls) = verify_lock_with_prevouts(|| Ok(())).await;
        result.unwrap();
        assert_eq!(calls, 1);

        let (result, calls) = verify_lock_with_prevouts(|| {
            Err(PsbtPrevoutVerificationError::ValueMismatch {
                outpoint: ::bitcoin::OutPoint::null(),
                actual: ::bitcoin::Amount::from_sat(1),
                claimed: ::bitcoin::Amount::from_sat(2),
            })
        })
        .await;
        let error = result.unwrap_err();
        assert!(
            matches!(
                error,
                VerificationError::Prevouts(PsbtPrevoutVerificationError::ValueMismatch { .. })
            ),
            "{error:?}"
        );
        assert!(!error.is_transient());
        assert_eq!(calls, 1);

        // Bob's inputs may not have reached our Electrum servers yet
        let (result, calls) = verify_lock_with_prevouts(|| {
            Err(PsbtPrevoutVerificationError::PrevoutNotFound {
                outpoint: ::bitcoin::OutPoint::null(),
            })
        })
        .await;
        let error = result.unwrap_err();
        assert!(
            matches!(
                error,
                VerificationError::Prevouts(PsbtPrevoutVerificationError::PrevoutNotFound { .. })
            ),
            "{error:?}"
        );
        assert!(error.is_transient());
        assert_eq!(calls, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn job_expired_while_queued_is_answered_without_starting() {
        let prevouts = stub_prevouts();
        let verifier = LockInputVerifier::spawn(prevouts.clone());
        let mut events = Events::new();
        let peer = PeerId::random();

        let (running, release_running) = events.gated("running");
        let _running_job = submit(&verifier, peer, running);
        assert_eq!(events.next().await, Started("running".into()));

        // Invalid proofs: a started job would fail with a proof error instead
        let (psbt, _, challenge) = signed_lock_inputs();
        let expiring_job = tokio::spawn({
            let verifier = verifier.clone();
            async move {
                verifier
                    .verify_lock(
                        peer,
                        psbt,
                        vec![],
                        challenge,
                        Instant::now() + Duration::from_secs(1),
                    )
                    .await
            }
        });
        settle().await;
        tokio::time::advance(Duration::from_secs(2)).await;
        release_running.send(Ok(())).unwrap();
        assert_eq!(events.next().await, Finished("running".into()));
        let finished_at = Instant::now();

        let error = expiring_job.await.unwrap().unwrap_err();
        assert!(
            matches!(
                error,
                VerificationError::Prevouts(PsbtPrevoutVerificationError::TimedOut)
            ),
            "{error:?}"
        );
        assert!(error.is_transient());
        assert_eq!(prevouts.calls.load(Ordering::SeqCst), 0);

        // The expired job neither leaves the peer stuck nor adds a cooldown
        let (next, release_next) = events.gated("next");
        let _next_job = submit(&verifier, peer, next);
        assert_eq!(events.next().await, Started("next".into()));
        assert_eq!(Instant::now(), finished_at + PEER_COOLDOWN);
        release_next.send(Ok(())).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn queued_job_expires_while_all_workers_are_busy() {
        let prevouts = stub_prevouts();
        let verifier = LockInputVerifier::spawn(prevouts.clone());
        let mut events = Events::new();
        let _gates: Vec<_> = (0..MAX_CONCURRENT_JOBS)
            .map(|i| {
                let (job, gate) = events.gated(format!("r{i}"));
                submit(&verifier, PeerId::random(), job);
                gate
            })
            .collect();
        for _ in 0..MAX_CONCURRENT_JOBS {
            assert!(matches!(events.next().await, Started(_)));
        }

        // Invalid proofs: a started job would fail with a proof error instead
        let peer = PeerId::random();
        let (psbt, _, challenge) = signed_lock_inputs();
        let expiring_job = tokio::spawn({
            let verifier = verifier.clone();
            async move {
                verifier
                    .verify_lock(
                        peer,
                        psbt,
                        vec![],
                        challenge,
                        Instant::now() + Duration::from_secs(1),
                    )
                    .await
            }
        });
        let _queued: Vec<_> = (1..MAX_PENDING_JOBS_PER_PEER)
            .map(|_| submit(&verifier, peer, || Ok(())))
            .collect();
        settle().await;
        let result = verifier.run_test_job(peer, || Ok(())).await;
        assert!(
            matches!(result, Err(VerificationError::PeerQueueFull)),
            "{result:?}"
        );

        tokio::time::advance(Duration::from_secs(1)).await;
        settle().await;
        assert!(expiring_job.is_finished());
        let error = expiring_job.await.unwrap().unwrap_err();
        assert!(
            matches!(
                error,
                VerificationError::Prevouts(PsbtPrevoutVerificationError::TimedOut)
            ),
            "{error:?}"
        );
        assert!(error.is_transient());
        assert_eq!(prevouts.calls.load(Ordering::SeqCst), 0);
        events.assert_quiet();

        // The expired job no longer occupies the peer's queue
        let accepted = submit(&verifier, peer, || Ok(()));
        settle().await;
        assert!(!accepted.is_finished());
    }

    struct StubPrevouts {
        result: fn() -> Result<(), PsbtPrevoutVerificationError>,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl VerifyPsbtPrevouts for StubPrevouts {
        async fn verify_psbt_prevouts(
            &self,
            _psbt: &::bitcoin::Psbt,
            _cancel: &watch::Receiver<bool>,
        ) -> Result<(), PsbtPrevoutVerificationError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            (self.result)()
        }
    }

    fn stub_prevouts() -> Arc<StubPrevouts> {
        stub_prevouts_returning(|| Ok(()))
    }

    fn stub_prevouts_returning(
        result: fn() -> Result<(), PsbtPrevoutVerificationError>,
    ) -> Arc<StubPrevouts> {
        Arc::new(StubPrevouts {
            result,
            calls: AtomicUsize::new(0),
        })
    }

    /// Runs a lock job with valid proofs against a prevout check returning `result`.
    async fn verify_lock_with_prevouts(
        result: fn() -> Result<(), PsbtPrevoutVerificationError>,
    ) -> (Result<VerifiedPsbt, VerificationError>, usize) {
        let prevouts = stub_prevouts_returning(result);
        let verifier = LockInputVerifier::spawn(prevouts.clone());
        let (psbt, proofs, challenge) = signed_lock_inputs();
        let result = verifier
            .verify_lock(PeerId::random(), psbt, proofs, challenge, deadline())
            .await;
        (result, prevouts.calls.load(Ordering::SeqCst))
    }

    /// A one-input PSBT with a valid ownership proof for the returned challenge.
    fn signed_lock_inputs() -> (
        bitcoin::PartiallySignedTransaction,
        Vec<OwnershipProof>,
        Challenge,
    ) {
        let secp = ::bitcoin::secp256k1::Secp256k1::new();
        let public_key = ::bitcoin::CompressedPublicKey(secret_key().public_key(&secp));
        let transaction = ::bitcoin::Transaction {
            version: ::bitcoin::transaction::Version::TWO,
            lock_time: ::bitcoin::absolute::LockTime::ZERO,
            input: vec![::bitcoin::TxIn::default()],
            output: vec![],
        };
        let mut psbt = bitcoin::PartiallySignedTransaction::from_unsigned_tx(transaction).unwrap();
        psbt.inputs[0].witness_utxo = Some(::bitcoin::TxOut {
            value: ::bitcoin::Amount::from_sat(1_000),
            script_pubkey: ::bitcoin::ScriptBuf::new_p2wpkh(&public_key.wpubkey_hash()),
        });
        let challenge = utxo_ownership_proof::challenge(b"swap");
        let proofs = vec![utxo_ownership_proof::sign(&secret_key(), challenge)];
        (psbt, proofs, challenge)
    }

    fn secret_key() -> ::bitcoin::secp256k1::SecretKey {
        ::bitcoin::secp256k1::SecretKey::from_slice(&[1; 32]).unwrap()
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(45)
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
        verifier: &LockInputVerifier,
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
