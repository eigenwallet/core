//! Background verification of Bitcoin lock-input ownership and unspentness.
//!
//! A single [`ProofVerifier`] is shared by all swap setup handlers. Its actor
//! runs at most one job per peer at a time, lets every peer cool down for
//! [`PEER_COOLDOWN`] after each of its jobs finished and limits the number of
//! jobs running concurrently across all peers. Ownership signatures are checked
//! on Tokio's blocking thread pool; chain checks go through the ASB event loop.

use super::alice::{LockInputCheck, LockInputCheckError, LockInputReservation};
use anyhow::Context;
use futures::StreamExt;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::FuturesUnordered;
use libp2p::PeerId;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;
use swap_core::bitcoin;
use swap_machine::alice::{State1, State2};
use swap_machine::common::Message2;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::Instant;

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
    #[error("Lock transaction input check failed: {0:?}")]
    LockInputs(LockInputCheckError),
}

type LockInputCheckSender =
    bmrng::RequestSender<LockInputCheck, Result<LockInputReservation, LockInputCheckError>>;

struct Request {
    peer: PeerId,
    job: Job,
}

#[allow(clippy::large_enum_variant)]
enum Job {
    Lock {
        state1: State1,
        message2: Message2,
        check_lock_inputs: LockInputCheckSender,
        reply: oneshot::Sender<Result<(State2, LockInputReservation), VerificationError>>,
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
}

#[derive(Default)]
struct Peer {
    queue: VecDeque<Job>,
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

    /// Verifies Bob's `Message2` and then checks the lock transaction inputs
    /// via `check_lock_inputs`, which reserves them on success.
    ///
    /// The ownership proofs and the lock amounts are verified on the blocking
    /// thread pool. If the caller stops waiting during that stage, the input
    /// check is skipped. If it stops waiting during the input check, the check
    /// is cancelled and drained before its worker slot is released.
    pub async fn verify_lock(
        &self,
        peer: PeerId,
        state1: State1,
        message2: Message2,
        check_lock_inputs: LockInputCheckSender,
    ) -> Result<(State2, LockInputReservation), VerificationError> {
        let (reply, response) = oneshot::channel();
        let job = Job::Lock {
            state1,
            message2,
            check_lock_inputs,
            reply,
        };

        self.request(peer, job, response).await
    }

    fn spawn_actor() -> (Self, JoinHandle<()>) {
        let (mailbox, receiver) = mpsc::channel(MAILBOX_CAPACITY);
        let actor = tokio::spawn(Actor::new(receiver).run());

        (Self { mailbox }, actor)
    }

    async fn request<T>(
        &self,
        peer: PeerId,
        job: Job,
        response: oneshot::Receiver<Result<T, VerificationError>>,
    ) -> Result<T, VerificationError> {
        self.mailbox
            .send(Request { peer, job })
            .await
            .map_err(|_| VerificationError::Closed)?;

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
        }
    }

    async fn run(mut self) {
        let mut mailbox_open = true;

        loop {
            self.expire_cooldowns();
            self.dispatch();

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
            return;
        }

        if self.queue_full(peer).is_some() {
            self.purge_canceled();
        }

        if let Some(error) = self.queue_full(peer) {
            tracing::debug!(%peer, %error, "Rejecting verification request");
            job.reject(error);
            return;
        }

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

            let Some(job) = pop_live_job(&mut peer.queue, &mut self.pending) else {
                self.peers.remove(&peer_id);
                continue;
            };

            peer.state = PeerState::Running;
            self.running.push(
                async move {
                    job.run().await;
                    peer_id
                }
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
            peer.queue.retain(|job| !job.is_canceled());
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
    async fn run(self) {
        // In all arms the caller may have stopped waiting, then the result is dropped
        match self {
            Job::Lock {
                state1,
                message2,
                check_lock_inputs,
                mut reply,
            } => {
                let result = verify_lock(state1, message2, check_lock_inputs, &mut reply).await;
                let _ = reply.send(result);
            }
            #[cfg(test)]
            Job::Test { work, reply } => {
                let result = verify_blocking(work).await;
                let _ = reply.send(result);
            }
        }
    }
}

async fn verify_lock(
    state1: State1,
    message2: Message2,
    check_lock_inputs: LockInputCheckSender,
    reply: &mut oneshot::Sender<Result<(State2, LockInputReservation), VerificationError>>,
) -> Result<(State2, LockInputReservation), VerificationError> {
    let (state2, tx_lock_psbt) = verify_blocking(move || {
        let tx_lock_psbt = message2.tx_lock_psbt.clone();
        let state2 = state1
            .receive(message2)
            .context("Failed to transition state1 -> state2 using message2")?;
        state2
            .tx_lock_fee()
            .context("Invalid TxLock input/output amounts")?;

        Ok((state2, tx_lock_psbt))
    })
    .await?;

    // Don't query the chain for a caller that is no longer waiting
    if reply.is_closed() {
        return Err(VerificationError::Closed);
    }

    let reservation = await_lock_input_check(tx_lock_psbt, check_lock_inputs, reply).await?;
    Ok((state2, reservation))
}

/// Cancellation requests cleanup; the response acknowledges that started work
/// has finished. Never release a worker just because its caller disappeared.
async fn await_lock_input_check<T: Send>(
    psbt: bitcoin::PartiallySignedTransaction,
    check_lock_inputs: LockInputCheckSender,
    reply: &mut oneshot::Sender<T>,
) -> Result<LockInputReservation, VerificationError> {
    let (cancel, cancellation) = watch::channel(false);
    let response = check_lock_inputs.send_receive(LockInputCheck {
        psbt,
        cancel: cancellation,
        deadline: Instant::now() + super::alice::LOCK_INPUT_CHECK_TIMEOUT,
    });
    tokio::pin!(response);
    let result = tokio::select! {
        result = &mut response => result,
        () = reply.closed() => {
            cancel.send_replace(true);
            response.await
        }
    };
    result
        .context("Failed to receive lock input verification result")
        .map_err(VerificationError::Setup)?
        .map_err(VerificationError::LockInputs)
}

/// Runs `work` on the blocking thread pool and waits until it has actually ended.
async fn verify_blocking<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, VerificationError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(VerificationError::Worker)?
        .map_err(VerificationError::Setup)
}

/// Pops queued jobs until one whose caller is still waiting is found.
fn pop_live_job(queue: &mut VecDeque<Job>, pending: &mut usize) -> Option<Job> {
    while let Some(job) = queue.pop_front() {
        *pending -= 1;
        if !job.is_canceled() {
            return Some(job);
        }
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

    #[tokio::test]
    async fn cancelled_chain_check_keeps_worker_until_cleanup_is_acknowledged() {
        let psbt = bitcoin::PartiallySignedTransaction::from_unsigned_tx(::bitcoin::Transaction {
            version: ::bitcoin::transaction::Version::TWO,
            lock_time: ::bitcoin::absolute::LockTime::ZERO,
            input: vec![],
            output: vec![],
        })
        .unwrap();
        let (sender, mut receiver) = bmrng::channel(1);
        let (mut reply, response) = oneshot::channel::<()>();
        let worker =
            tokio::spawn(async move { await_lock_input_check(psbt, sender, &mut reply).await });
        let (mut request, responder) = receiver.recv().await.unwrap();
        drop(response);
        request.cancel.changed().await.unwrap();
        assert!(*request.cancel.borrow());
        assert!(!worker.is_finished());
        responder
            .respond(Err(LockInputCheckError::Cancelled))
            .unwrap();
        assert!(matches!(
            worker.await.unwrap(),
            Err(VerificationError::LockInputs(
                LockInputCheckError::Cancelled
            ))
        ));
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
            job: Job::Test {
                work: Box::new(|| Ok(())),
                reply,
            },
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
