//! Very small HTTP/1 connection pool for both clearnet (TCP) and Tor streams.
//!
//! After investigation we learned that pooling **raw** sockets is not useful
//! because once Hyper finishes a `Connection` the socket is closed.  The correct
//! thing to cache is the HTTP client pair returned by
//! `hyper::client::conn::http1::handshake` – specifically the
//! `SendRequest<Body>` handle.
//!
//! A `SendRequest` can serve multiple sequential requests as long as the
//! `Connection` future that Hyper gives us keeps running in the background.
//! Therefore `ConnectionPool` stores those senders and a separate background
//! task drives the corresponding `Connection` until the peer closes it.  When
//! that happens any future `send_request` will error and we will drop that entry
//! from the pool automatically.
//!
//! The internal data-structure:
//!
//! ```text
//! Arc<RwLock<HashMap<(scheme, host, port, via_tor),
//!                    RwLock<Vec<Arc<Mutex<hyper::client::conn::http1::SendRequest<Body>>>>>>>>
//! ```
//!
//! Locking strategy
//! ----------------
//! * **Outer `RwLock`** – protects the HashMap (rare contention).
//! * **Per-host `RwLock`** – protects the Vec for that host.
//! * **`Mutex` around each `SendRequest`** – guarantees only one request at a
//!   time per connection.
//!
//! The `GuardedSender` returned by `ConnectionPool::get()` derefs to
//! `SendRequest<Body>`.  Once the guard is dropped the mutex unlocks and the
//! connection is again available.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};

/// Key for the map – `(scheme, host, port, via_tor)`.
pub type StreamKey = (String, String, u16, bool);

/// Alias for hyper's HTTP/1 sender.
pub type HttpSender = hyper::client::conn::http1::SendRequest<Body>;

/// Connection pool.
#[derive(Clone, Default)]
pub struct ConnectionPool {
    inner: Arc<RwLock<HashMap<StreamKey, Arc<RwLock<Vec<Arc<Mutex<HttpSender>>>>>>>>,
}

/// Guard returned by `get()`.  Derefs to the underlying `SendRequest` so callers
/// can invoke `send_request()` directly.
pub struct GuardedSender {
    guard: OwnedMutexGuard<HttpSender>,
    pool: ConnectionPool,
    key: StreamKey,
    sender_arc: Arc<Mutex<HttpSender>>,
}

impl std::ops::Deref for GuardedSender {
    type Target = HttpSender;
    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}
impl std::ops::DerefMut for GuardedSender {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl GuardedSender {
    /// Mark this sender as failed and remove it from the pool.
    pub async fn mark_failed(self) {
        // Dropping the guard releases the mutex, then we remove from pool
        drop(self.guard);
        self.pool.remove_sender(&self.key, &self.sender_arc).await;
    }
}

impl ConnectionPool {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Try to fetch an idle connection.  Returns `None` if all are busy or the
    /// host has no pool yet.
    pub async fn try_get(&self, key: &StreamKey) -> Option<GuardedSender> {
        let map = self.inner.read().await;
        let vec_lock = map.get(key)?.clone();
        drop(map);

        let vec = vec_lock.write().await;
        let total_connections = vec.len();
        let mut busy_connections = 0;

        for sender_mutex in vec.iter() {
            if let Ok(guard) = sender_mutex.clone().try_lock_owned() {
                tracing::debug!(
                    "Reusing connection for {}://{}:{} (via_tor={}). Pool stats: {}/{} connections available",
                    key.0, key.1, key.2, key.3, total_connections - busy_connections, total_connections
                );
                return Some(GuardedSender {
                    guard,
                    pool: self.clone(),
                    key: key.clone(),
                    sender_arc: sender_mutex.clone(),
                });
            } else {
                busy_connections += 1;
            }
        }

        tracing::debug!(
            "No idle connections for {}://{}:{} (via_tor={}). Pool stats: 0/{} connections available",
            key.0, key.1, key.2, key.3, total_connections
        );
        None
    }

    /// Insert `sender` into the pool and return an *exclusive* handle ready to
    /// send the first request.
    pub async fn insert_and_lock(&self, key: StreamKey, sender: HttpSender) -> GuardedSender {
        let sender_mutex = Arc::new(Mutex::new(sender));
        let key_clone = key.clone();
        let sender_mutex_clone = sender_mutex.clone();

        {
            let mut map = self.inner.write().await;
            let vec_lock = map
                .entry(key)
                .or_insert_with(|| Arc::new(RwLock::new(Vec::new())))
                .clone();
            let mut vec = vec_lock.write().await;
            vec.push(sender_mutex.clone());
        }

        let guard = sender_mutex.lock_owned().await;

        // Log the new connection count after insertion
        let map_read = self.inner.read().await;
        if let Some(vec_lock) = map_read.get(&key_clone) {
            let vec = vec_lock.read().await;
            tracing::debug!(
                "Created new connection for {}://{}:{} (via_tor={}). Pool stats: 1/{} connections available",
                key_clone.0, key_clone.1, key_clone.2, key_clone.3, vec.len()
            );
        }
        drop(map_read);

        GuardedSender {
            guard,
            pool: self.clone(),
            key: key_clone,
            sender_arc: sender_mutex_clone,
        }
    }

    /// Remove a specific sender from the pool (used when connection fails).
    pub async fn remove_sender(&self, key: &StreamKey, sender_arc: &Arc<Mutex<HttpSender>>) {
        if let Some(vec_lock) = self.inner.read().await.get(key).cloned() {
            let mut vec = vec_lock.write().await;
            let old_count = vec.len();
            vec.retain(|arc_mutex| !Arc::ptr_eq(arc_mutex, sender_arc));
            let new_count = vec.len();

            if old_count != new_count {
                tracing::debug!(
                    "Removed failed connection for {}://{}:{} (via_tor={}). Pool stats: {}/{} connections remaining",
                    key.0, key.1, key.2, key.3, new_count, new_count
                );
            }
        }
    }

    /// Check if there's an available (unlocked) connection for the given key.
    pub async fn has_available_connection(&self, key: &StreamKey) -> bool {
        let map = self.inner.read().await;
        if let Some(vec_lock) = map.get(key) {
            let vec = vec_lock.read().await;
            for sender_mutex in vec.iter() {
                if sender_mutex.try_lock().is_ok() {
                    return true;
                }
            }
        }
        false
    }
}
