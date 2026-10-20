use anyhow::{Context, Result};
use crossbeam::deque::{Injector, Steal};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::warn;
use typeshare::typeshare;

use crate::database::{Database, network_to_string};
use crate::types::NodeAddress;

#[derive(Debug, Clone, serde::Serialize)]
#[typeshare]
pub struct PoolStatus {
    pub total_node_count: u32,
    pub healthy_node_count: u32,
    #[typeshare(serialized_as = "number")]
    pub successful_health_checks: u64,
    #[typeshare(serialized_as = "number")]
    pub unsuccessful_health_checks: u64,
    pub top_reliable_nodes: Vec<ReliableNodeInfo>,
    pub bandwidth_kb_per_sec: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[typeshare]
pub struct ReliableNodeInfo {
    pub url: String,
    pub success_rate: f64,
    pub avg_latency_ms: Option<f64>,
}

#[derive(Debug, Clone)]
struct BandwidthEntry {
    timestamp: Instant,
    bytes: u64,
}

#[derive(Debug)]
pub struct BandwidthTracker {
    entries: Injector<BandwidthEntry>,
}

impl BandwidthTracker {
    const WINDOW_DURATION: Duration = Duration::from_secs(60 * 3);

    fn new() -> Self {
        Self {
            entries: Injector::new(),
        }
    }

    pub fn record_bytes(&self, bytes: u64) {
        let now = Instant::now();
        self.entries.push(BandwidthEntry {
            timestamp: now,
            bytes,
        });
    }

    fn get_kb_per_sec(&self) -> f64 {
        let now = Instant::now();
        let cutoff = now - Self::WINDOW_DURATION;

        // Collect valid entries from the injector
        let mut valid_entries = Vec::new();
        let mut total_bytes = 0u64;

        // Drain all entries, keeping only recent ones
        loop {
            match self.entries.steal() {
                Steal::Success(entry) => {
                    if entry.timestamp >= cutoff {
                        total_bytes += entry.bytes;
                        valid_entries.push(entry);
                    }
                }
                Steal::Empty | Steal::Retry => break,
            }
        }

        // Put back the valid entries
        for entry in valid_entries.iter() {
            self.entries.push(entry.clone());
        }

        if valid_entries.len() < 5 {
            return 0.0;
        }

        let oldest_time = valid_entries.iter().map(|e| e.timestamp).min().unwrap();
        let duration_secs = now.duration_since(oldest_time).as_secs_f64();

        if duration_secs > 0.0 {
            (total_bytes as f64 / 1024.0) / duration_secs
        } else {
            0.0
        }
    }
}

pub struct NodePool {
    db: Database,
    network: monero_address::Network,
    status_sender: broadcast::Sender<PoolStatus>,
    bandwidth_tracker: Arc<BandwidthTracker>,
}

impl NodePool {
    pub fn new(
        db: Database,
        network: monero_address::Network,
    ) -> (Self, broadcast::Receiver<PoolStatus>) {
        let (status_sender, status_receiver) = broadcast::channel(100);
        let pool = Self {
            db,
            network,
            status_sender,
            bandwidth_tracker: Arc::new(BandwidthTracker::new()),
        };
        (pool, status_receiver)
    }

    pub async fn record_success(
        &self,
        scheme: &str,
        host: &str,
        port: u16,
        latency_ms: f64,
    ) -> Result<()> {
        self.db
            .record_health_check(scheme, host, port, true, Some(latency_ms))
            .await?;
        Ok(())
    }

    pub async fn record_failure(&self, scheme: &str, host: &str, port: u16) -> Result<()> {
        self.db
            .record_health_check(scheme, host, port, false, None)
            .await?;
        Ok(())
    }

    pub fn record_bandwidth(&self, bytes: u64) {
        self.bandwidth_tracker.record_bytes(bytes);
    }

    pub fn get_bandwidth_tracker(&self) -> Arc<BandwidthTracker> {
        self.bandwidth_tracker.clone()
    }

    pub async fn publish_status_update(&self) -> Result<()> {
        let status = self.get_current_status().await?;

        if let Err(e) = self.status_sender.send(status.clone()) {
            warn!("Failed to send status update: {}", e);
        }

        Ok(())
    }

    pub async fn get_current_status(&self) -> Result<PoolStatus> {
        let network_str = network_to_string(&self.network);
        let (total, reachable, _reliable) = self.db.get_node_stats(network_str).await?;
        let reliable_nodes = self.db.get_reliable_nodes(network_str).await?;
        let (successful_checks, unsuccessful_checks) =
            self.db.get_health_check_stats(network_str).await?;

        let bandwidth_kb_per_sec = self.bandwidth_tracker.get_kb_per_sec();

        let top_reliable_nodes = reliable_nodes
            .into_iter()
            .take(5)
            .map(|node| ReliableNodeInfo {
                url: node.full_url(),
                success_rate: node.success_rate(),
                avg_latency_ms: node.health.avg_latency_ms,
            })
            .collect();

        Ok(PoolStatus {
            total_node_count: total as u32,
            healthy_node_count: reachable as u32,
            successful_health_checks: successful_checks,
            unsuccessful_health_checks: unsuccessful_checks,
            top_reliable_nodes,
            bandwidth_kb_per_sec,
        })
    }

    /// Get nodes to use, with weighted selection favoring top performers
    /// The list has some randomness, but the top nodes are still more likely to be chosen
    pub async fn get_top_reliable_nodes(&self, limit: usize) -> Result<Vec<NodeAddress>> {
        use rand::seq::SliceRandom;

        tracing::debug!(
            "Getting top reliable nodes for network {} (target: {})",
            network_to_string(&self.network),
            limit
        );

        let available_nodes = self
            .db
            .get_top_nodes_by_recent_success(network_to_string(&self.network), limit as i64)
            .await
            .context("Failed to get top nodes by recent success")?;

        let total_candidates = available_nodes.len();

        let weighted: Vec<(NodeAddress, f64)> = available_nodes
            .into_iter()
            .enumerate()
            .map(|(idx, node)| {
                // Higher-ranked (smaller idx) ⇒ larger weight
                let weight = 1.5_f64.powi((total_candidates - idx) as i32);
                (node, weight)
            })
            .collect();

        let mut rng = rand::thread_rng();

        let mut candidates = weighted;
        let mut selected_nodes = Vec::with_capacity(limit);

        while selected_nodes.len() < limit && !candidates.is_empty() {
            // Choose one node based on its weight using `choose_weighted`
            let chosen_pair = candidates
                .choose_weighted(&mut rng, |item| item.1)
                .map_err(|e| anyhow::anyhow!("Weighted choice failed: {}", e))?;

            // Locate index of the chosen pair and remove it
            let chosen_index = candidates
                .iter()
                .position(|x| std::ptr::eq(x, chosen_pair))
                .expect("Chosen item must exist in candidates");

            let (node, _) = candidates.swap_remove(chosen_index);
            selected_nodes.push(node);
        }

        tracing::debug!(
            "Pool size: {} nodes for network {} (target: {})",
            selected_nodes.len(),
            network_to_string(&self.network),
            limit
        );

        Ok(selected_nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monero_address::Network;
    use std::collections::HashSet;

    async fn test_db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = Database::new(dir.path().to_path_buf())
            .await
            .expect("database with migrations applied");
        (dir, db)
    }

    async fn insert_node(db: &Database, host: &str) {
        sqlx::query(
            r#"
            INSERT INTO monero_nodes (scheme, host, port, network, first_seen_at)
            VALUES ('http', ?, 18081, 'testnet', datetime('now'))
            "#,
        )
        .bind(host)
        .execute(&db.pool)
        .await
        .expect("insert fixture node");
    }

    #[tokio::test]
    async fn status_combines_node_stats_health_checks_and_bandwidth() {
        let (_dir, db) = test_db().await;
        insert_node(&db, "pool-node.example").await;

        let (pool, _receiver) = NodePool::new(db, Network::Testnet);
        for latency_ms in [120.0, 130.0, 140.0] {
            pool.record_success("http", "pool-node.example", 18081, latency_ms)
                .await
                .expect("record success");
        }
        pool.record_failure("http", "pool-node.example", 18081)
            .await
            .expect("record failure");

        let status = pool.get_current_status().await.expect("current status");

        assert_eq!(status.total_node_count, 1);
        assert_eq!(status.healthy_node_count, 1);
        assert_eq!(status.successful_health_checks, 3);
        assert_eq!(status.unsuccessful_health_checks, 1);

        let [top] = status.top_reliable_nodes.as_slice() else {
            panic!("expected exactly one reliable node");
        };
        assert_eq!(top.url, "http://pool-node.example:18081");
        assert_eq!(top.success_rate, 0.75);
        assert_eq!(top.avg_latency_ms, Some(130.0));

        assert_eq!(status.bandwidth_kb_per_sec, 0.0);
    }

    #[tokio::test]
    async fn publish_status_update_reaches_the_subscribed_receiver() {
        let (_dir, db) = test_db().await;
        insert_node(&db, "pool-node.example").await;

        let (pool, mut receiver) = NodePool::new(db, Network::Testnet);
        pool.record_success("http", "pool-node.example", 18081, 100.0)
            .await
            .expect("record success");

        pool.publish_status_update().await.expect("publish status");

        let status = receiver
            .try_recv()
            .expect("status update broadcast to subscriber");
        assert_eq!(status.total_node_count, 1);
        assert_eq!(status.successful_health_checks, 1);

        // A send without any subscriber must not surface as an error.
        drop(receiver);
        pool.publish_status_update()
            .await
            .expect("publish without subscribers");
    }

    #[test]
    fn bandwidth_rate_returns_zero_below_five_samples() {
        let tracker = BandwidthTracker::new();
        for _ in 0..4 {
            tracker.record_bytes(1024);
        }

        assert_eq!(tracker.get_kb_per_sec(), 0.0);
    }

    #[tokio::test]
    async fn bandwidth_rate_divides_total_kilobytes_by_window_duration() {
        let tracker = BandwidthTracker::new();

        tracker.record_bytes(1024);
        tokio::time::sleep(Duration::from_millis(10)).await;
        for _ in 0..4 {
            tracker.record_bytes(1024);
        }

        // Five KiB spread over at least 10 ms cannot exceed 500 KiB/s, and
        // any real duration produces a positive rate.
        let rate = tracker.get_kb_per_sec();
        assert!(rate > 0.0);
        assert!(rate <= 500.0);
    }

    #[test]
    fn bandwidth_window_discards_entries_older_than_three_minutes() {
        let tracker = BandwidthTracker::new();

        // Entries outside the three minute window must be dropped instead of
        // counted: four stale 100 KiB entries would push any rate far above
        // the bound asserted below.
        for _ in 0..4 {
            tracker.entries.push(BandwidthEntry {
                timestamp: Instant::now() - Duration::from_secs(4 * 60),
                bytes: 100 * 1024,
            });
        }

        tracker.record_bytes(1024);
        std::thread::sleep(Duration::from_millis(10));
        for _ in 0..4 {
            tracker.record_bytes(1024);
        }

        let rate = tracker.get_kb_per_sec();
        assert!(rate > 0.0);
        assert!(rate <= 500.0);

        // Dropping must be permanent: a stale entry that was put back into the
        // tracker would inflate the rate on every subsequent read.
        let rate_again = tracker.get_kb_per_sec();
        assert!(rate_again > 0.0);
        assert!(rate_again <= 500.0);
    }

    #[tokio::test]
    async fn top_reliable_nodes_respects_limit_and_returns_each_node_once() {
        let (_dir, db) = test_db().await;
        insert_node(&db, "first.example").await;
        insert_node(&db, "second.example").await;

        let (pool, _receiver) = NodePool::new(db, Network::Testnet);
        pool.record_success("http", "first.example", 18081, 100.0)
            .await
            .expect("record success");
        pool.record_success("http", "second.example", 18081, 100.0)
            .await
            .expect("record success");

        let both = pool.get_top_reliable_nodes(5).await.expect("selection");
        assert_eq!(both.len(), 2);
        let distinct: HashSet<_> = both.iter().map(|node| node.full_url()).collect();
        assert_eq!(distinct.len(), 2);

        let one = pool.get_top_reliable_nodes(1).await.expect("selection");
        assert_eq!(one.len(), 1);

        // An empty network must yield nothing rather than panic in the
        // weighted picker.
        let (_empty_dir, empty_db) = test_db().await;
        let (empty_pool, _receiver) = NodePool::new(empty_db, Network::Testnet);
        let selection = empty_pool
            .get_top_reliable_nodes(3)
            .await
            .expect("selection");
        assert!(selection.is_empty());
    }
}
