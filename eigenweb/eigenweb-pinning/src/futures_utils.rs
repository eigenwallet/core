use libp2p::futures::future::BoxFuture;
use libp2p::futures::stream::{FuturesUnordered, StreamExt};
use std::collections::HashSet;
use std::hash::Hash;
use std::task::{Context, Poll};

/// A collection of futures with associated keys that can be checked for presence
/// before completion.
///
/// This combines a HashSet for key tracking with FuturesUnordered for efficient polling.
/// The key is provided during insertion; the future only needs to yield the value.
pub struct FuturesHashSet<K, V> {
    keys: HashSet<K>,
    futures: FuturesUnordered<BoxFuture<'static, (K, V)>>,
}

impl<K: Hash + Eq + Clone + Send + 'static, V: 'static> FuturesHashSet<K, V> {
    pub fn new() -> Self {
        Self {
            keys: HashSet::new(),
            futures: FuturesUnordered::new(),
        }
    }

    /// Check if a future with the given key is already pending
    pub fn contains_key(&self, key: &K) -> bool {
        self.keys.contains(key)
    }

    /// Insert a new future with the given key.
    /// The future should yield V; the key will be paired with it when it completes.
    /// Returns true if the key was newly inserted, false if it was already present.
    /// If false is returned, the future is not added.
    pub fn insert(&mut self, key: K, future: BoxFuture<'static, V>) -> bool {
        if self.keys.insert(key.clone()) {
            let key_clone = key;
            let wrapped = async move {
                let value = future.await;
                (key_clone, value)
            };
            self.futures.push(Box::pin(wrapped));
            true
        } else {
            false
        }
    }

    /// Poll for the next completed future.
    /// When a future completes, its key is automatically removed from the tracking set.
    pub fn poll_next_unpin(&mut self, cx: &mut Context) -> Poll<Option<(K, V)>> {
        match self.futures.poll_next_unpin(cx) {
            Poll::Ready(Some((k, v))) => {
                self.keys.remove(&k);
                Poll::Ready(Some((k, v)))
            }
            other => other,
        }
    }
}

impl<K: Hash + Eq + Clone + Send + 'static, V: 'static> Default for FuturesHashSet<K, V> {
    fn default() -> Self {
        Self::new()
    }
}
