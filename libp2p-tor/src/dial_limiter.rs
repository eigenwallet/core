use libp2p::{Multiaddr, PeerId, multiaddr::Protocol};
use std::{
    collections::{HashMap, VecDeque},
    num::NonZeroUsize,
    sync::{Arc, RwLock},
    time::Duration,
};
use thiserror::Error;
use tokio::{
    sync::{Notify, mpsc, oneshot},
    time::Instant,
};

#[derive(Debug, Error)]
pub enum TorDialLimiterError {
    #[error("Tor dial limiter is closed")]
    Closed,
}

/// Variant order is significant: `Ord` keeps the highest priority ever set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum TorDialPriority {
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, Default)]
pub struct TorDialPriorityTracker {
    peer_priorities: Arc<RwLock<HashMap<PeerId, TorDialPriority>>>,
    /// Notified whenever a stored priority is raised, so the limiter wakes and
    /// re-buckets any already-queued dial for the promoted peer.
    promotions: Arc<Notify>,
}

impl TorDialPriorityTracker {
    /// Only ever raises the stored priority, never lowers it.
    pub fn set_peer_priority(&self, peer_id: PeerId, priority: TorDialPriority) {
        let promoted = {
            let mut peer_priorities = self
                .peer_priorities
                .write()
                .expect("Tor dial priority tracker lock to not be poisoned");

            match peer_priorities.get_mut(&peer_id) {
                Some(current) if priority > *current => {
                    *current = priority;
                    true
                }
                Some(_) => false,
                None => {
                    peer_priorities.insert(peer_id, priority);
                    // Absent peers are dialed at `Normal`, so only a higher mark
                    // can promote a dial that is already queued for this peer.
                    priority > TorDialPriority::Normal
                }
            }
        };

        if promoted {
            self.promotions.notify_one();
        }
    }

    pub fn mark_low_priority(&self, peer_id: PeerId) {
        self.set_peer_priority(peer_id, TorDialPriority::Low);
    }

    pub fn mark_high_priority(&self, peer_id: PeerId) {
        self.set_peer_priority(peer_id, TorDialPriority::High);
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
    /// One queue per priority. `high` and `normal` are independent; `low` is
    /// subordinate and only starts while no high or normal dial is waiting.
    ///
    /// Must be called from a Tokio runtime.
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

/// Moves queued dials whose priority was raised after enqueue into their
/// current queue. Only promotions happen (priorities never drop): normal→high
/// first, then low→{normal,high}. In-flight counts and spacing stay with each
/// queue; only waiting requests move.
fn promote_queued(
    priority_tracker: &TorDialPriorityTracker,
    high_queue: &mut PriorityQueue,
    normal_queue: &mut PriorityQueue,
    low_queue: &mut PriorityQueue,
) {
    let mut i = 0;
    while i < normal_queue.queue.len() {
        let peer_id = normal_queue.queue[i].peer_id;
        if priority_tracker.peer_priority(peer_id.as_ref()) == TorDialPriority::High {
            let request = normal_queue.queue.remove(i).expect("index in bounds");
            high_queue.queue.push_back(request);
        } else {
            i += 1;
        }
    }

    let mut i = 0;
    while i < low_queue.queue.len() {
        let peer_id = low_queue.queue[i].peer_id;
        let target = match priority_tracker.peer_priority(peer_id.as_ref()) {
            TorDialPriority::High => &mut *high_queue,
            TorDialPriority::Normal => &mut *normal_queue,
            TorDialPriority::Low => {
                i += 1;
                continue;
            }
        };
        let request = low_queue.queue.remove(i).expect("index in bounds");
        target.queue.push_back(request);
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
        // A peer's priority is captured when its dial is enqueued, but it can be
        // raised afterwards (e.g. rediscovered via rendezvous, or a swap starts).
        // Re-resolve queued dials so a promoted peer moves to its higher queue
        // instead of staying stuck under the old gating and budget. Priorities
        // only ever rise, so a request can only move to a higher queue.
        promote_queued(
            &priority_tracker,
            &mut high_queue,
            &mut normal_queue,
            &mut low_queue,
        );

        high_queue.release_ready(TorDialPriority::High, &release_sender);
        normal_queue.release_ready(TorDialPriority::Normal, &release_sender);

        // Low dials only start once no high or normal dial is waiting.
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

        // Low is gated off while higher queues wait, so skip its wake-up; their
        // activity wakes us and lets low through once they drain.
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
            () = priority_tracker.promotions.notified() => {}
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

        // First low, then bumped up.
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
    async fn queued_low_dial_is_promoted_when_priority_is_raised() {
        let priority_tracker = TorDialPriorityTracker::default();
        let limiter = limiter(priority_tracker.clone());

        // Occupy the normal slot and queue another normal dial so the low queue
        // stays gated off.
        let normal = limiter.wait(None).await.unwrap();
        let normal_waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(None).await.unwrap() }
        });

        // A low dial that cannot start while a normal dial is waiting.
        let peer = PeerId::random();
        priority_tracker.mark_low_priority(peer);
        let waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(Some(peer)).await.unwrap() }
        });

        settle().await;
        assert!(!waiter.is_finished());

        // Raise its priority to high; the queued request is re-bucketed and now
        // starts on the high queue despite the busy normal queue.
        priority_tracker.mark_high_priority(peer);
        settle().await;
        assert!(waiter.is_finished());

        drop(normal);
        tokio::time::advance(Duration::from_secs(4)).await;
        let _ = normal_waiter.await.unwrap();
        let _ = waiter.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn low_dials_wait_until_no_normal_dial_is_queued() {
        let priority_tracker = TorDialPriorityTracker::default();
        let limiter = limiter(priority_tracker.clone());

        // Occupy the normal slot, then queue another so the normal queue is non-empty.
        let normal = limiter.wait(None).await.unwrap();
        let normal_waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(None).await.unwrap() }
        });

        // Held back while a normal dial is waiting, despite a free low slot.
        let low_peer = PeerId::random();
        priority_tracker.mark_low_priority(low_peer);
        let low_waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.wait(Some(low_peer)).await.unwrap() }
        });

        settle().await;
        assert!(!normal_waiter.is_finished());
        assert!(!low_waiter.is_finished());

        // Drain the normal queue, which lets the low dial proceed.
        drop(normal);
        tokio::time::advance(Duration::from_secs(4)).await;
        settle().await;
        assert!(normal_waiter.is_finished());
        assert!(low_waiter.is_finished());

        let _ = normal_waiter.await.unwrap();
        let _ = low_waiter.await.unwrap();
    }
}
