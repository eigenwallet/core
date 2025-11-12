use backoff::{backoff::Backoff, ExponentialBackoff};
use libp2p::PeerId;
use std::{collections::HashMap, hash::Hash, time::Duration};

// Stores multiple backoffs for multiple types of requests for multiple peers
pub struct Pool<K: Hash + Eq + Clone> {
    backoff: HashMap<(PeerId, K), ExponentialBackoff>,
    initial_interval: Duration,
    max_interval: Duration,
}

impl<K: Hash + Eq + Clone> Pool<K> {
    pub fn new(initial_interval: Duration, max_interval: Duration) -> Self {
        Self {
            backoff: HashMap::new(),
            initial_interval,
            max_interval,
        }
    }

    pub fn get_backoff(&mut self, peer_id: PeerId, backoff_type: K) -> &mut ExponentialBackoff {
        self.backoff
            .entry((peer_id, backoff_type))
            .or_insert_with(|| ExponentialBackoff {
                initial_interval: self.initial_interval,
                current_interval: self.initial_interval,
                max_interval: self.max_interval,
                max_elapsed_time: None,
                ..ExponentialBackoff::default()
            })
    }

    /// Gives us a future that resolves after the backoff for that peer + the additional wait time has passed
    pub fn schedule_backoff<T>(
        &mut self,
        peer: PeerId,
        value: T,
        kind: K,
        wait: Duration, // we add this on top of the backoff
    ) -> (
        std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>,
        Duration,
    )
    where
        T: Send + 'static,
    {
        let backoff = self.get_backoff(peer, kind).current_interval + wait;

        (
            Box::pin(async move {
                tokio::time::sleep(backoff).await;
                value
            }),
            backoff,
        )
    }

    /// Resets the backoff for a given peer and kind
    pub fn reset_backoff(&mut self, peer: PeerId, kind: K) {
        self.get_backoff(peer, kind).reset();
    }

    pub fn increase_backoff(&mut self, peer: PeerId, kind: K) {
        self.get_backoff(peer, kind)
            .next_backoff()
            .expect("backoff should never run out of attempts");
    }
}
