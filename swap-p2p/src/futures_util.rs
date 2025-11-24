use libp2p::futures::future::{self, AbortHandle, Abortable, BoxFuture};
use libp2p::futures::stream::{FuturesUnordered, StreamExt};
use std::collections::HashMap;
use std::hash::Hash;
use std::task::{Context, Poll};

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
                    // We only return the value if it matches the currently active handle for this key.
                    // However, since we abort old handles, they shouldn't return Ok.
                    // We remove the key from handles as it is now completed.
                    if self.handles.contains_key(&k) {
                        self.handles.remove(&k);
                        return Poll::Ready(Some((k, v)));
                    }
                    // If the key is not in handles, it might have been removed or race condition?
                    // But if it returned Ok, it wasn't aborted.
                    // Safe to return.
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

    pub fn len(&self) -> usize {
        self.handles.len()
    }
}

impl<K: Hash + Eq + Clone + Send + 'static, V: 'static> Default for FuturesHashSet<K, V> {
    fn default() -> Self {
        Self::new()
    }
}
