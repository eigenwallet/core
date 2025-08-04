use anyhow::{Context, Result};
use crossbeam::deque::{Injector, Steal};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::warn;
use typeshare::typeshare;

use crate::database::Database;
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
    network: String,
    status_sender: broadcast::Sender<PoolStatus>,
    bandwidth_tracker: Arc<BandwidthTracker>,
}

impl NodePool {
    pub fn new(db: Database, network: String) -> (Self, broadcast::Receiver<PoolStatus>) {
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
        port: i64,
        latency_ms: f64,
    ) -> Result<()> {
        self.db
            .record_health_check(scheme, host, port, true, Some(latency_ms))
            .await?;
        Ok(())
    }

    pub async fn record_failure(&self, scheme: &str, host: &str, port: i64) -> Result<()> {
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
        let (total, reachable, _reliable) = self.db.get_node_stats(&self.network).await?;
        let reliable_nodes = self.db.get_reliable_nodes(&self.network).await?;
        let (successful_checks, unsuccessful_checks) =
            self.db.get_health_check_stats(&self.network).await?;

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
            self.network,
            limit
        );

        let available_nodes = self
            .db
            .get_top_nodes_by_recent_success(&self.network, limit as i64)
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
            self.network,
            limit
        );

        Ok(selected_nodes)
    }
}
