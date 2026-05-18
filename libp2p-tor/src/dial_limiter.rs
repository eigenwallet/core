use libp2p::{Multiaddr, PeerId, multiaddr::Protocol};
use std::{
    collections::{HashMap, VecDeque},
    num::NonZeroUsize,
    sync::{Arc, RwLock},
    time::Duration,
};
use thiserror::Error;
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};

#[derive(Debug, Error)]
pub enum TorDialLimiterError {
    #[error("Tor dial limiter is closed")]
    Closed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TorDialPriority {
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, Default)]
pub struct TorDialPriorityTracker {
    peer_priorities: Arc<RwLock<HashMap<PeerId, TorDialPriority>>>,
}

impl TorDialPriorityTracker {
    pub fn set_peer_priority(&self, peer_id: PeerId, priority: TorDialPriority) {
        let mut peer_priorities = self
            .peer_priorities
            .write()
            .expect("Tor dial priority tracker lock to not be poisoned");

        if priority == TorDialPriority::Normal {
            peer_priorities.remove(&peer_id);
            return;
        }

        peer_priorities.insert(peer_id, priority);
    }

    pub fn mark_high_priority(&self, peer_id: PeerId) {
        self.set_peer_priority(peer_id, TorDialPriority::High);
    }

    pub fn remove_peer_priority(&self, peer_id: &PeerId) {
        let mut peer_priorities = self
            .peer_priorities
            .write()
            .expect("Tor dial priority tracker lock to not be poisoned");

        peer_priorities.remove(peer_id);
    }

    #[must_use]
    pub fn peer_priority(&self, peer_id: Option<&PeerId>) -> TorDialPriority {
        let Some(peer_id) = peer_id else {
            return TorDialPriority::Normal;
        };

        let peer_priorities = self
            .peer_priorities
            .read()
            .expect("Tor dial priority tracker lock to not be poisoned");

        peer_priorities
            .get(peer_id)
            .copied()
            .unwrap_or(TorDialPriority::Normal)
    }
}

/// Per-priority dial budget: how many dials of this priority may be in flight
/// at once, and the minimum spacing between consecutive dial starts.
#[derive(Debug, Clone, Copy)]
pub struct TorDialPriorityConfig {
    pub max_concurrent: NonZeroUsize,
    pub min_delay: Duration,
}

#[derive(Clone)]
pub struct TorDialLimiter {
    request_sender: mpsc::UnboundedSender<DialRequest>,
    priority_tracker: TorDialPriorityTracker,
}

impl TorDialLimiter {
    /// Creates a limiter with an independent queue per priority. Each priority
    /// has its own concurrency limit and its own minimum delay between dial
    /// starts; the two priorities do not compete for a shared budget.
    ///
    /// This must be called from a Tokio runtime.
    #[must_use]
    pub fn new(
        priority_tracker: TorDialPriorityTracker,
        high: TorDialPriorityConfig,
        normal: TorDialPriorityConfig,
    ) -> Self {
        let (request_sender, request_receiver) = mpsc::unbounded_channel();
        let (release_sender, release_receiver) = mpsc::unbounded_channel();

        tokio::spawn(run_limiter(
            request_receiver,
            release_receiver,
            release_sender,
            priority_tracker.clone(),
            high,
            normal,
        ));

        Self {
            request_sender,
            priority_tracker,
        }
    }

    #[must_use]
    pub fn priority_tracker(&self) -> TorDialPriorityTracker {
        self.priority_tracker.clone()
    }

    /// Wait until a dial slot for this peer's priority is free.
    ///
    /// The priority is resolved once, here, and the request is routed into that
    /// priority's queue. The returned [`TorDialPermit`] occupies a slot until it
    /// is dropped, so it must be held for the entire duration of the dial.
    ///
    /// # Errors
    ///
    /// Returns an error if the background limiter task has stopped.
    pub async fn wait(&self, peer_id: Option<PeerId>) -> Result<TorDialPermit, TorDialLimiterError> {
        let (permit_sender, permit_receiver) = oneshot::channel();
        let request = DialRequest {
            peer_id,
            permit_sender,
        };

        self.request_sender
            .send(request)
            .map_err(|_| TorDialLimiterError::Closed)?;

        permit_receiver
            .await
            .map_err(|_| TorDialLimiterError::Closed)
    }
}

/// Occupies one dial slot of its priority for as long as it is alive. Dropping
/// it frees the slot and lets the limiter release the next waiting dial.
pub struct TorDialPermit {
    priority: TorDialPriority,
    release_sender: Option<mpsc::UnboundedSender<TorDialPriority>>,
}

impl TorDialPermit {
    /// Stops this permit from freeing a slot on drop. Used when a permit was
    /// minted but never handed to a waiter (the waiter was cancelled), so it
    /// never occupied a slot.
    fn disarm(&mut self) {
        self.release_sender = None;
    }
}

impl Drop for TorDialPermit {
    fn drop(&mut self) {
        if let Some(release_sender) = &self.release_sender {
            let _ = release_sender.send(self.priority);
        }
    }
}

struct DialRequest {
    peer_id: Option<PeerId>,
    permit_sender: oneshot::Sender<TorDialPermit>,
}

struct PriorityQueue {
    queue: VecDeque<DialRequest>,
    in_flight: usize,
    max_concurrent: usize,
    min_delay: Duration,
    /// Earliest instant at which the next dial of this priority may start.
    next_release_at: Instant,
}

impl PriorityQueue {
    fn new(config: TorDialPriorityConfig, now: Instant) -> Self {
        Self {
            queue: VecDeque::new(),
            in_flight: 0,
            max_concurrent: config.max_concurrent.get(),
            min_delay: config.min_delay,
            next_release_at: now,
        }
    }

    /// Releases as many waiting dials as the concurrency limit and the delay
    /// currently allow.
    fn release_ready(
        &mut self,
        priority: TorDialPriority,
        release_sender: &mpsc::UnboundedSender<TorDialPriority>,
    ) {
        while self.in_flight < self.max_concurrent && !self.queue.is_empty() {
            let now = Instant::now();
            if now < self.next_release_at {
                break;
            }

            let request = self.queue.pop_front().expect("queue to be non-empty");
            let permit = TorDialPermit {
                priority,
                release_sender: Some(release_sender.clone()),
            };

            match request.permit_sender.send(permit) {
                Ok(()) => {
                    self.in_flight += 1;
                    self.next_release_at = now + self.min_delay;
                }
                // The waiter was cancelled before it could receive the permit,
                // so it never occupied a slot and must not consume the delay.
                Err(mut permit) => permit.disarm(),
            }
        }
    }

    /// The instant at which `release_ready` could make progress on its own (a
    /// slot is free and dials are waiting, but the delay has not elapsed yet).
    fn next_wake(&self) -> Option<Instant> {
        if self.queue.is_empty() || self.in_flight >= self.max_concurrent {
            return None;
        }

        Some(self.next_release_at)
    }
}

async fn run_limiter(
    mut request_receiver: mpsc::UnboundedReceiver<DialRequest>,
    mut release_receiver: mpsc::UnboundedReceiver<TorDialPriority>,
    release_sender: mpsc::UnboundedSender<TorDialPriority>,
    priority_tracker: TorDialPriorityTracker,
    high: TorDialPriorityConfig,
    normal: TorDialPriorityConfig,
) {
    let now = Instant::now();
    let mut high_queue = PriorityQueue::new(high, now);
    let mut normal_queue = PriorityQueue::new(normal, now);
    let mut request_closed = false;

    loop {
        high_queue.release_ready(TorDialPriority::High, &release_sender);
        normal_queue.release_ready(TorDialPriority::Normal, &release_sender);

        // Every limiter handle is gone, so no slot will ever free up again.
        // Abandon the backlog; dropping the queues makes pending `wait()`
        // callers observe `Closed` instead of hanging.
        if request_closed {
            return;
        }

        let next_wake = [high_queue.next_wake(), normal_queue.next_wake()]
            .into_iter()
            .flatten()
            .min();

        tokio::select! {
            request = request_receiver.recv(), if !request_closed => {
                match request {
                    Some(request) => {
                        let priority = priority_tracker.peer_priority(request.peer_id.as_ref());
                        match priority {
                            TorDialPriority::High => high_queue.queue.push_back(request),
                            TorDialPriority::Normal => normal_queue.queue.push_back(request),
                        }
                    }
                    None => request_closed = true,
                }
            }
            Some(priority) = release_receiver.recv() => {
                let queue = match priority {
                    TorDialPriority::High => &mut high_queue,
                    TorDialPriority::Normal => &mut normal_queue,
                };
                debug_assert!(queue.in_flight > 0, "release without a matching in-flight dial");
                queue.in_flight = queue.in_flight.saturating_sub(1);
            }
            () = wait_until(next_wake) => {}
        }
    }
}

/// Sleeps until `deadline`, or never resolves if there is nothing to wait for.
async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

pub(crate) fn extract_peer_id(address: &Multiaddr) -> Option<PeerId> {
    let Protocol::P2p(peer_id) = address.iter().last()? else {
        return None;
    };

    Some(peer_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(max_concurrent: usize, min_delay: Duration) -> TorDialPriorityConfig {
        TorDialPriorityConfig {
            max_concurrent: NonZeroUsize::new(max_concurrent).expect("non-zero concurrency"),
            min_delay,
        }
    }

    /// High = 2 concurrent / 0.5s spacing, Normal = 1 concurrent / 4s spacing.
    fn limiter(priority_tracker: TorDialPriorityTracker) -> TorDialLimiter {
        TorDialLimiter::new(
            priority_tracker,
            config(2, Duration::from_millis(500)),
            config(1, Duration::from_secs(4)),
        )
    }

    async fn settle() {
        for _ in 0..32 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn normal_dials_are_serialized_by_concurrency_and_delay() {
        let limiter = limiter(TorDialPriorityTracker::default());

        let first = limiter.wait(None).await.unwrap();

        let second = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(None).await.unwrap() }
        });

        // Blocked by the concurrency limit of 1.
        settle().await;
        assert!(!second.is_finished());

        // Even after the slot frees, the 4s delay since the first start applies.
        drop(first);
        settle().await;
        assert!(!second.is_finished());

        tokio::time::advance(Duration::from_secs(4)).await;
        settle().await;
        assert!(second.is_finished());
        let _second = second.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn high_dials_respect_spacing_then_concurrency() {
        let priority_tracker = TorDialPriorityTracker::default();
        let limiter = limiter(priority_tracker.clone());

        let spawn_wait = |peer: PeerId| {
            priority_tracker.mark_high_priority(peer);
            let limiter = limiter.clone();
            tokio::spawn(async move { limiter.wait(Some(peer)).await.unwrap() })
        };
        let first = spawn_wait(PeerId::random());
        let second = spawn_wait(PeerId::random());
        let third = spawn_wait(PeerId::random());

        // First starts immediately; the 0.5s spacing holds the second back.
        settle().await;
        assert!(first.is_finished());
        assert!(!second.is_finished());

        // Second starts after the spacing; now 2 are in flight (concurrency 2).
        tokio::time::advance(Duration::from_millis(500)).await;
        settle().await;
        assert!(second.is_finished());

        // Third is blocked by the concurrency limit, not just the delay.
        tokio::time::advance(Duration::from_secs(10)).await;
        settle().await;
        assert!(!third.is_finished());

        let first_permit = first.await.unwrap();
        drop(first_permit);
        settle().await;
        assert!(third.is_finished());
        let _ = second.await.unwrap();
        let _ = third.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn priorities_do_not_block_each_other() {
        let priority_tracker = TorDialPriorityTracker::default();
        let limiter = limiter(priority_tracker.clone());

        // Saturate the normal queue (concurrency 1).
        let normal = limiter.wait(None).await.unwrap();

        let normal_waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(None).await.unwrap() }
        });

        let high_peer = PeerId::random();
        priority_tracker.mark_high_priority(high_peer);
        let high_waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(Some(high_peer)).await.unwrap() }
        });

        // The high dial proceeds despite the normal queue being saturated.
        settle().await;
        assert!(high_waiter.is_finished());
        assert!(!normal_waiter.is_finished());

        drop(normal);
        let _ = high_waiter.await.unwrap();
    }
}
