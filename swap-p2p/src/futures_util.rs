use libp2p::futures::future::{self, AbortHandle, Abortable, BoxFuture};
use libp2p::futures::stream::{FuturesUnordered, StreamExt};
use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::task::{Context, Poll};
use std::time::Duration;

/// A collection of futures with associated keys that can be checked for presence
/// before completion.
///
/// This combines a HashMap for key tracking and cancellation with FuturesUnordered for efficient polling.
/// The key is provided during insertion; the future only needs to yield the value.
/// If a future with the same key is inserted, the previous one is aborted/replaced.
pub struct FuturesHashSet<K, V> {
    futures: FuturesUnordered<BoxFuture<'static, Result<(K, V), future::Aborted>>>,
    handles: HashMap<K, AbortHandle>,
}

impl<K: Hash + Eq + Clone + Send + 'static, V: 'static> FuturesHashSet<K, V> {
    pub fn new() -> Self {
        Self {
            futures: FuturesUnordered::new(),
            handles: HashMap::new(),
        }
    }

    /// Check if a future with the given key is already pending
    pub fn contains_key(&self, key: &K) -> bool {
        self.handles.contains_key(key)
    }

    /// Insert a new future with the given key.
    /// If a future with the same key already exists, it returns false and does NOT replace it.
    pub fn insert(&mut self, key: K, future: BoxFuture<'static, V>) -> bool {
        if self.handles.contains_key(&key) {
            return false;
        }

        let (handle, registration) = AbortHandle::new_pair();
        self.handles.insert(key.clone(), handle);

        let key_clone = key;
        let wrapped = async move {
            let value = future.await;
            (key_clone, value)
        };

        let abortable = Abortable::new(Box::pin(wrapped), registration);
        self.futures.push(Box::pin(abortable));
        true
    }

    /// Removes a future with the given key, aborting it if it exists.
    pub fn remove(&mut self, key: &K) -> bool {
        if let Some(handle) = self.handles.remove(key) {
            handle.abort();
            return true;
        }
        false
    }

    /// Insert a new future with the given key.
    /// If a future with the same key already exists, it is aborted and replaced.
    ///
    /// Returns true if a future was replaced, false if no future was replaced.
    pub fn replace(&mut self, key: K, future: BoxFuture<'static, V>) -> bool {
        let did_remove_existing = self.remove(&key);
        self.insert(key, future);

        did_remove_existing
    }

    /// Poll for the next completed future.
    /// When a future completes, its key is automatically removed from the tracking set.
    pub fn poll_next_unpin(&mut self, cx: &mut Context) -> Poll<Option<(K, V)>> {
        loop {
            match self.futures.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok((k, v)))) => {
                    let did_remove_handle = self.handles.remove(&k).is_some();

                    // TODO: Make this a production assert
                    debug_assert!(
                        did_remove_handle,
                        "A future returned Ok but the key is not in handles. This should never happen."
                    );

                    // Still return the value to avoid panicking
                    return Poll::Ready(Some((k, v)));
                }
                Poll::Ready(Some(Err(future::Aborted))) => {
                    // Future was aborted, ignore and continue polling
                    continue;
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    pub fn clear(&mut self) {
        // Abort all futures
        for handle in self.handles.values() {
            handle.abort();
        }

        self.handles.clear();
        self.futures.clear();
    }

    pub fn len(&self) -> usize {
        self.handles.len()
    }
}

impl<K: Hash + Eq + Clone + Send + 'static, V: 'static> Default for FuturesHashSet<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs futures keyed by `K` with a per-key concurrency limit and a timeout.
pub struct BoundedKeyedFutures<K, V> {
    futures: FuturesUnordered<BoxFuture<'static, (K, Result<V, tokio::time::error::Elapsed>)>>,
    in_flight_per_key: HashMap<K, usize>,
    max_per_key: usize,
    timeout: Duration,
}

impl<K: Hash + Eq + Clone + Send + 'static, V: Send + 'static> BoundedKeyedFutures<K, V> {
    pub fn new(max_per_key: usize, timeout: Duration) -> Self {
        Self {
            futures: FuturesUnordered::new(),
            in_flight_per_key: HashMap::new(),
            max_per_key,
            timeout,
        }
    }

    pub fn try_push(&mut self, key: K, future: impl Future<Output = V> + Send + 'static) -> bool {
        let count = self.in_flight_per_key.get(&key).copied().unwrap_or(0);
        if count >= self.max_per_key {
            return false;
        }

        *self.in_flight_per_key.entry(key.clone()).or_default() += 1;

        let timeout = self.timeout;
        self.futures.push(Box::pin(async move {
            (key, tokio::time::timeout(timeout, future).await)
        }));
        true
    }

    /// Stays pending when empty so `tokio::select!` needs no sentinel future.
    pub async fn next(&mut self) -> (K, Result<V, tokio::time::error::Elapsed>) {
        let Some((key, result)) = self.futures.next().await else {
            return future::pending().await;
        };

        if let Some(count) = self.in_flight_per_key.get_mut(&key) {
            *count -= 1;
            if *count == 0 {
                self.in_flight_per_key.remove(&key);
            }
        }

        (key, result)
    }

    #[cfg(test)]
    fn in_flight(&self, key: &K) -> usize {
        self.in_flight_per_key.get(key).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_keyed_futures_enforces_per_key_limit_and_frees_slots() {
        let mut set = BoundedKeyedFutures::new(2, Duration::from_secs(5));

        assert!(set.try_push("peer_a", async { 1 }));
        assert!(set.try_push("peer_a", async { 2 }));
        assert!(!set.try_push("peer_a", async { 3 }));
        assert!(set.try_push("peer_b", async { 4 }));
        assert_eq!(set.in_flight(&"peer_a"), 2);
        assert_eq!(set.in_flight(&"peer_b"), 1);

        let (k1, v1) = set.next().await;
        assert!(v1.is_ok());
        if k1 == "peer_a" {
            assert_eq!(set.in_flight(&"peer_a"), 1);
            assert!(set.try_push("peer_a", async { 5 }));
            assert_eq!(set.in_flight(&"peer_a"), 2);
        }
    }

    #[tokio::test]
    async fn bounded_keyed_futures_applies_timeout_and_frees_slot() {
        let mut set = BoundedKeyedFutures::new(1, Duration::from_millis(20));

        assert!(set.try_push("peer_a", future::pending::<()>()));
        assert!(!set.try_push("peer_a", async {}));

        let (key, result) = set.next().await;
        assert_eq!(key, "peer_a");
        assert!(result.is_err());
        assert_eq!(set.in_flight(&"peer_a"), 0);
        assert!(set.try_push("peer_a", async {}));
    }
}
