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

/// Dial priorities, ordered from lowest to highest. The variant order is
/// significant: it defines the [`Ord`] used to keep the highest priority ever
/// set for a peer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum TorDialPriority {
    /// Peers we restore from the database. We have connected to them before,
    /// but have no fresh signal that they are reachable right now, so they get
    /// the smallest dial budget.
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, Default)]
pub struct TorDialPriorityTracker {
    peer_priorities: Arc<RwLock<HashMap<PeerId, TorDialPriority>>>,
}

impl TorDialPriorityTracker {
    /// Records a priority for a peer, keeping the highest priority ever set:
    /// the stored value is only ever raised, never lowered. A peer with no
    /// recorded priority takes the given priority directly, so the first call
    /// always takes effect (even when it is [`TorDialPriority::Low`]).
    pub fn set_peer_priority(&self, peer_id: PeerId, priority: TorDialPriority) {
        let mut peer_priorities = self
            .peer_priorities
            .write()
            .expect("Tor dial priority tracker lock to not be poisoned");

        peer_priorities
            .entry(peer_id)
            .and_modify(|current| *current = (*current).max(priority))
            .or_insert(priority);
    }

    pub fn mark_low_priority(&self, peer_id: PeerId) {
        self.set_peer_priority(peer_id, TorDialPriority::Low);
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
    /// Creates a limiter with one queue per priority, each with its own
    /// concurrency limit and minimum delay between dial starts. `high` and
    /// `normal` run independently and do not compete for a shared budget.
    /// `low` is subordinate: its dials only start while neither the high nor
    /// the normal queue has a dial waiting.
    ///
    /// This must be called from a Tokio runtime.
    #[must_use]
    pub fn new(
        priority_tracker: TorDialPriorityTracker,
        high: TorDialPriorityConfig,
        normal: TorDialPriorityConfig,
        low: TorDialPriorityConfig,
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
            low,
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
    low: TorDialPriorityConfig,
) {
    let now = Instant::now();
    let mut high_queue = PriorityQueue::new(high, now);
    let mut normal_queue = PriorityQueue::new(normal, now);
    let mut low_queue = PriorityQueue::new(low, now);
    let mut request_closed = false;

    loop {
        high_queue.release_ready(TorDialPriority::High, &release_sender);
        normal_queue.release_ready(TorDialPriority::Normal, &release_sender);

        // Low-priority dials are strictly subordinate: they only start once no
        // higher-priority dial is waiting. As long as a high or normal dial is
        // queued, low dials are held back entirely (rather than merely competing
        // for a separate budget). Releases above run first in this same
        // iteration, so a low dial gets its chance the moment the higher queues
        // drain.
        let higher_priority_waiting =
            !high_queue.queue.is_empty() || !normal_queue.queue.is_empty();
        if !higher_priority_waiting {
            low_queue.release_ready(TorDialPriority::Low, &release_sender);
        }

        // Every limiter handle is gone, so no slot will ever free up again.
        // Abandon the backlog; dropping the queues makes pending `wait()`
        // callers observe `Closed` instead of hanging.
        if request_closed {
            return;
        }

        // While higher-priority dials are waiting we deliberately omit the low
        // queue's wake-up: releasing it is gated off, so waking for its delay
        // would spin. The activity of the higher queues (their own timers or
        // slot releases) wakes us, and once they drain we release low in the
        // same iteration.
        let low_next_wake = (!higher_priority_waiting)
            .then(|| low_queue.next_wake())
            .flatten();
        let next_wake = [
            high_queue.next_wake(),
            normal_queue.next_wake(),
            low_next_wake,
        ]
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
                            TorDialPriority::Low => low_queue.queue.push_back(request),
                        }
                    }
                    None => request_closed = true,
                }
            }
            Some(priority) = release_receiver.recv() => {
                let queue = match priority {
                    TorDialPriority::High => &mut high_queue,
                    TorDialPriority::Normal => &mut normal_queue,
                    TorDialPriority::Low => &mut low_queue,
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

    /// High = 2 concurrent / 0.5s spacing, Normal = 1 concurrent / 4s spacing,
    /// Low = 1 concurrent / 8s spacing.
    fn limiter(priority_tracker: TorDialPriorityTracker) -> TorDialLimiter {
        TorDialLimiter::new(
            priority_tracker,
            config(2, Duration::from_millis(500)),
            config(1, Duration::from_secs(4)),
            config(1, Duration::from_secs(8)),
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

    #[test]
    fn highest_set_priority_wins_regardless_of_order() {
        let tracker = TorDialPriorityTracker::default();
        let peer = PeerId::random();

        // First low (e.g. restored from the database), then bumped up.
        tracker.mark_low_priority(peer);
        assert_eq!(tracker.peer_priority(Some(&peer)), TorDialPriority::Low);

        tracker.set_peer_priority(peer, TorDialPriority::Normal);
        assert_eq!(tracker.peer_priority(Some(&peer)), TorDialPriority::Normal);

        tracker.mark_high_priority(peer);
        assert_eq!(tracker.peer_priority(Some(&peer)), TorDialPriority::High);

        // A lower priority never overrides a higher one already set.
        tracker.set_peer_priority(peer, TorDialPriority::Normal);
        tracker.mark_low_priority(peer);
        assert_eq!(tracker.peer_priority(Some(&peer)), TorDialPriority::High);
    }

    #[tokio::test(start_paused = true)]
    async fn low_dials_do_not_block_normal_dials() {
        let priority_tracker = TorDialPriorityTracker::default();
        let limiter = limiter(priority_tracker.clone());

        // Saturate the low queue (concurrency 1).
        let low_peer = PeerId::random();
        priority_tracker.mark_low_priority(low_peer);
        let low = limiter.wait(Some(low_peer)).await.unwrap();

        let low_waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(Some(low_peer)).await.unwrap() }
        });

        // A normal dial proceeds despite the low queue being saturated.
        let normal_waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(None).await.unwrap() }
        });

        settle().await;
        assert!(normal_waiter.is_finished());
        assert!(!low_waiter.is_finished());

        drop(low);
        let _ = normal_waiter.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn low_dials_wait_until_no_normal_dial_is_queued() {
        let priority_tracker = TorDialPriorityTracker::default();
        let limiter = limiter(priority_tracker.clone());

        // Occupy the single normal slot, then queue another normal dial so the
        // normal queue is non-empty (a normal dial is waiting).
        let normal = limiter.wait(None).await.unwrap();
        let normal_waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(None).await.unwrap() }
        });

        // The low dial is held back while a normal dial is waiting, even though
        // the low queue has a free slot.
        let low_peer = PeerId::random();
        priority_tracker.mark_low_priority(low_peer);
        let low_waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(Some(low_peer)).await.unwrap() }
        });

        settle().await;
        assert!(!normal_waiter.is_finished());
        assert!(!low_waiter.is_finished());

        // Free the normal slot; after the normal spacing the queued normal dial
        // starts and the normal queue drains, which lets the low dial proceed.
        drop(normal);
        tokio::time::advance(Duration::from_secs(4)).await;
        settle().await;
        assert!(normal_waiter.is_finished());
        assert!(low_waiter.is_finished());

        let _ = normal_waiter.await.unwrap();
        let _ = low_waiter.await.unwrap();
    }
}
