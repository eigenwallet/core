use chrono::{DateTime, Utc};
use monero_address::Network;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeAddress {
    pub scheme: String, // "http" or "https"
    pub host: String,
    pub port: u16,
}

impl NodeAddress {
    pub fn new(scheme: String, host: String, port: u16) -> Self {
        Self { scheme, host, port }
    }

    pub fn full_url(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }
}

impl fmt::Display for NodeAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}:{}", self.scheme, self.host, self.port)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetadata {
    pub id: i64,
    #[serde(with = "swap_serde::monero::network")]
    pub network: Network,
    pub first_seen_at: DateTime<Utc>,
}

impl NodeMetadata {
    pub fn new(id: i64, network: Network, first_seen_at: DateTime<Utc>) -> Self {
        Self {
            id,
            network,
            first_seen_at,
        }
    }
}

/// Health check statistics for a node
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeHealthStats {
    pub success_count: i64,
    pub failure_count: i64,
    pub last_success: Option<DateTime<Utc>>,
    pub last_failure: Option<DateTime<Utc>>,
    pub last_checked: Option<DateTime<Utc>>,
    pub avg_latency_ms: Option<f64>,
    pub min_latency_ms: Option<f64>,
    pub max_latency_ms: Option<f64>,
    pub last_latency_ms: Option<f64>,
}

impl NodeHealthStats {
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            0.0
        } else {
            self.success_count as f64 / total as f64
        }
    }
}

/// A complete node record combining address, metadata, and health stats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    #[serde(flatten)]
    pub address: NodeAddress,
    #[serde(flatten)]
    pub metadata: NodeMetadata,
    #[serde(flatten)]
    pub health: NodeHealthStats,
}

impl NodeRecord {
    pub fn new(address: NodeAddress, metadata: NodeMetadata, health: NodeHealthStats) -> Self {
        Self {
            address,
            metadata,
            health,
        }
    }

    pub fn full_url(&self) -> String {
        self.address.full_url()
    }

    pub fn success_rate(&self) -> f64 {
        self.health.success_rate()
    }
}
