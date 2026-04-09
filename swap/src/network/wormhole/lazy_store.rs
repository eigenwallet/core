use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::Result;
use backoff::ExponentialBackoff;
use backoff::backoff::Backoff;
use futures::FutureExt;
use libp2p::{Multiaddr, PeerId};

use super::WormholeStore;

const WRITE_BACKOFF_INITIAL: Duration = Duration::from_secs(1);
const WRITE_BACKOFF_MAX: Duration = Duration::from_secs(5 * 60);

type BoxFut<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// In-memory cache in front of a [`WormholeStore`].
///
/// Reads are always synchronous against the cache.
/// Writes update the cache immediately and are flushed to the
/// database one at a time in [`poll`]. Failed writes are retried
/// with exponential backoff.
pub struct LazyWormholeStore {
    db: Arc<dyn WormholeStore + Send + Sync>,
    cache: HashMap<PeerId, (Multiaddr, bool)>,
    /// Peers whose cache entry has not yet been persisted.
    dirty: HashSet<PeerId>,
    /// Current inflight write (resolves to the peer on success) or backoff sleep (resolves to None).
    inflight_write: Option<BoxFut<Result<Option<PeerId>>>>,
    /// Exponential backoff for consecutive write failures.
    write_backoff: ExponentialBackoff,
    /// Initial load from the database. `None` once completed.
    inflight_load: Option<BoxFut<Result<Vec<(PeerId, Multiaddr)>>>>,
    loaded: bool,
}

impl LazyWormholeStore {
    pub fn new(db: Arc<dyn WormholeStore + Send + Sync>) -> Self {
        Self {
            inflight_load: Some(Self::load_future(&db)),
            db,
            cache: HashMap::new(),
            dirty: HashSet::new(),
            inflight_write: None,
            write_backoff: ExponentialBackoff {
                initial_interval: WRITE_BACKOFF_INITIAL,
                current_interval: WRITE_BACKOFF_INITIAL,
                max_interval: WRITE_BACKOFF_MAX,
                max_elapsed_time: None,
                ..ExponentialBackoff::default()
            },
            loaded: false,
        }
    }

    fn load_future(
        db: &Arc<dyn WormholeStore + Send + Sync>,
    ) -> BoxFut<Result<Vec<(PeerId, Multiaddr)>>> {
        let db = Arc::clone(db);
        Box::pin(async move { db.get_all_wormholes().await })
    }

    /// Read a wormhole address from the cache.
    pub fn get(&self, peer: &PeerId) -> Option<&Multiaddr> {
        self.cache.get(peer).map(|(addr, _)| addr)
    }

    /// Write a wormhole to the cache and mark it for persistence.
    pub fn insert(&mut self, peer: PeerId, address: Multiaddr, active: bool) {
        self.cache.insert(peer, (address, active));
        self.dirty.insert(peer);
    }

    /// Drive the initial load and pending writes.
    pub fn poll(&mut self, cx: &mut Context<'_>) {
        // Drive initial load
        if let Some(fut) = &mut self.inflight_load {
            if let Poll::Ready(result) = fut.poll_unpin(cx) {
                match result {
                    Ok(wormholes) => {
                        tracing::debug!(count = wormholes.len(), "Loaded wormholes from store");

                        for (peer_id, address) in wormholes {
                            // Don't overwrite entries that arrived via the wire
                            // while the load was in flight.
                            self.cache.entry(peer_id).or_insert((address, false));
                        }

                        self.loaded = true;
                        self.inflight_load = None;
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, "Failed to load wormholes, retrying");
                        self.inflight_load = Some(Self::load_future(&self.db));
                        cx.waker().wake_by_ref();
                    }
                }
            }
        }

        // Drive inflight write or backoff sleep
        if let Some(fut) = &mut self.inflight_write {
            if let Poll::Ready(result) = fut.poll_unpin(cx) {
                match result {
                    Ok(Some(peer)) => {
                        self.dirty.remove(&peer);
                        self.write_backoff.reset();
                    }
                    Ok(None) => {
                        // Backoff sleep completed, try next dirty peer
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, "Failed to persist wormhole, will retry");
                        let delay = self
                            .write_backoff
                            .next_backoff()
                            .unwrap_or(WRITE_BACKOFF_MAX);
                        self.inflight_write = Some(Box::pin(async move {
                            tokio::time::sleep(delay).await;
                            Ok(None)
                        }));
                        return;
                    }
                }
                self.inflight_write = None;
            }
        }

        // Start next write if idle and there are dirty entries
        if self.inflight_write.is_none() {
            if let Some(&peer) = self.dirty.iter().next() {
                if let Some((address, active)) = self.cache.get(&peer).cloned() {
                    let db = Arc::clone(&self.db);
                    self.inflight_write = Some(Box::pin(async move {
                        db.store_wormhole(peer, address, active).await?;
                        Ok(Some(peer))
                    }));
                    cx.waker().wake_by_ref();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A mock WormholeStore backed by an in-memory map.
    struct MockStore {
        inner: Mutex<HashMap<PeerId, (Multiaddr, bool)>>,
    }

    impl MockStore {
        fn new(initial: Vec<(PeerId, Multiaddr, bool)>) -> Arc<Self> {
            let mut map = HashMap::new();
            for (peer, addr, active) in initial {
                map.insert(peer, (addr, active));
            }
            Arc::new(Self {
                inner: Mutex::new(map),
            })
        }
    }

    #[async_trait::async_trait]
    impl WormholeStore for MockStore {
        async fn store_wormhole(
            &self,
            peer: PeerId,
            address: Multiaddr,
            active: bool,
        ) -> Result<()> {
            self.inner.lock().unwrap().insert(peer, (address, active));
            Ok(())
        }

        async fn get_wormhole(&self, peer: PeerId) -> Result<Option<(Multiaddr, bool)>> {
            Ok(self.inner.lock().unwrap().get(&peer).cloned())
        }

        async fn get_all_wormholes(&self) -> Result<Vec<(PeerId, Multiaddr)>> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .iter()
                .map(|(p, (a, _))| (*p, a.clone()))
                .collect())
        }
    }

    async fn poll_until_flushed(store: &mut LazyWormholeStore) {
        futures::future::poll_fn(|cx| {
            store.poll(cx);
            if store.dirty.is_empty()
                && store.inflight_write.is_none()
                && store.inflight_load.is_none()
            {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }

    #[tokio::test]
    async fn loads_persisted_wormholes_on_first_poll() {
        let peer = PeerId::random();
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9939".parse().unwrap();
        let db = MockStore::new(vec![(peer, addr.clone(), true)]);

        let mut store = LazyWormholeStore::new(db);

        // Before polling, cache is empty
        assert!(store.get(&peer).is_none());

        poll_until_flushed(&mut store).await;

        assert_eq!(store.get(&peer), Some(&addr));
    }

    #[tokio::test]
    async fn insert_flushes_to_db() {
        let db = MockStore::new(vec![]);

        let peer = PeerId::random();
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9939".parse().unwrap();

        let mut store = LazyWormholeStore::new(db.clone());
        poll_until_flushed(&mut store).await;

        // Insert and flush
        store.insert(peer, addr.clone(), true);
        assert_eq!(store.get(&peer), Some(&addr));

        poll_until_flushed(&mut store).await;

        // Verify it reached the DB
        let persisted = db.get_wormhole(peer).await.unwrap();
        assert_eq!(persisted, Some((addr, true)));
    }

    #[tokio::test]
    async fn wire_insert_not_overwritten_by_load() {
        let peer = PeerId::random();
        let db_addr: Multiaddr = "/ip4/1.1.1.1/tcp/9939".parse().unwrap();
        let wire_addr: Multiaddr = "/ip4/2.2.2.2/tcp/9939".parse().unwrap();

        let db = MockStore::new(vec![(peer, db_addr, true)]);
        let mut store = LazyWormholeStore::new(db);

        // Insert via wire before the load completes
        store.insert(peer, wire_addr.clone(), false);
        assert_eq!(store.get(&peer), Some(&wire_addr));

        // Load completes — should not overwrite the wire entry
        poll_until_flushed(&mut store).await;

        assert_eq!(store.get(&peer), Some(&wire_addr));
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn failed_write_retries_until_success() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// A store that fails the first N writes, then succeeds.
        struct FailingStore {
            inner: Mutex<HashMap<PeerId, (Multiaddr, bool)>>,
            remaining_failures: AtomicUsize,
        }

        #[async_trait::async_trait]
        impl WormholeStore for FailingStore {
            async fn store_wormhole(
                &self,
                peer: PeerId,
                address: Multiaddr,
                active: bool,
            ) -> Result<()> {
                if self.remaining_failures.fetch_sub(1, Ordering::Relaxed) > 0 {
                    anyhow::bail!("simulated write failure");
                }
                self.inner.lock().unwrap().insert(peer, (address, active));
                Ok(())
            }

            async fn get_wormhole(&self, peer: PeerId) -> Result<Option<(Multiaddr, bool)>> {
                Ok(self.inner.lock().unwrap().get(&peer).cloned())
            }

            async fn get_all_wormholes(&self) -> Result<Vec<(PeerId, Multiaddr)>> {
                Ok(vec![])
            }
        }

        let db = Arc::new(FailingStore {
            inner: Mutex::new(HashMap::new()),
            remaining_failures: AtomicUsize::new(2), // fail twice, then succeed
        });

        let mut store = LazyWormholeStore::new(db.clone() as Arc<dyn WormholeStore + Send + Sync>);
        poll_until_flushed(&mut store).await;

        let peer = PeerId::random();
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9939".parse().unwrap();
        store.insert(peer, addr.clone(), true);

        // Cache is immediately available
        assert_eq!(store.get(&peer), Some(&addr));

        // Poll until flushed — will fail twice then succeed
        poll_until_flushed(&mut store).await;

        // Verify it eventually reached the DB
        let persisted = db.get_wormhole(peer).await.unwrap();
        assert_eq!(persisted, Some((addr, true)));
    }
}
