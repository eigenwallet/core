use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{body::to_bytes, extract::State, http::StatusCode};
use monero_address::Network;
use monero_rpc_pool::{
    AppState, config::Config, connection_pool::ConnectionPool, create_app_with_receiver,
    database::Database, database::network_to_string, pool::NodePool, proxy::stats_handler,
};
use serde_json::Value;
use tokio::time::timeout;

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "monero-rpc-pool-{name}-{}-{unique}",
            std::process::id()
        ));

        std::fs::create_dir_all(&path).expect("test data directory should be created");

        Self { path }
    }

    fn path(&self) -> PathBuf {
        self.path.clone()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[tokio::test]
async fn migrations_seed_default_nodes_and_zero_health_counts() {
    let data_dir = TestDir::new("migration");
    let db = Database::new(data_dir.path()).await.unwrap();

    let (total, reachable, reliable) = db
        .get_node_stats(network_to_string(&Network::Mainnet))
        .await
        .unwrap();
    assert!(total > 0);
    assert_eq!(reachable, 0);
    assert_eq!(reliable, 0);

    let (successful, unsuccessful) = db
        .get_health_check_stats(network_to_string(&Network::Mainnet))
        .await
        .unwrap();
    assert_eq!((successful, unsuccessful), (0, 0));

    let empty_testnet_stats = db
        .get_node_stats(network_to_string(&Network::Testnet))
        .await
        .unwrap();
    assert_eq!(empty_testnet_stats, (0, 0, 0));
}

#[tokio::test]
async fn database_records_health_checks_for_seeded_nodes() {
    let data_dir = TestDir::new("health");
    let db = Database::new(data_dir.path()).await.unwrap();
    let node = db
        .get_top_nodes_by_recent_success(network_to_string(&Network::Mainnet), 1)
        .await
        .unwrap()
        .pop()
        .expect("mainnet migrations should seed at least one node");

    db.record_health_check(&node.scheme, &node.host, node.port, true, Some(120.0))
        .await
        .unwrap();
    db.record_health_check(&node.scheme, &node.host, node.port, true, Some(80.0))
        .await
        .unwrap();
    db.record_health_check(&node.scheme, &node.host, node.port, false, None)
        .await
        .unwrap();

    let (successful, unsuccessful) = db
        .get_health_check_stats(network_to_string(&Network::Mainnet))
        .await
        .unwrap();
    assert_eq!((successful, unsuccessful), (2, 1));

    let (_total, reachable, reliable) = db
        .get_node_stats(network_to_string(&Network::Mainnet))
        .await
        .unwrap();
    assert_eq!(reachable, 1);
    assert_eq!(reliable, 1);

    let reliable_nodes = db
        .get_reliable_nodes(network_to_string(&Network::Mainnet))
        .await
        .unwrap();
    assert_eq!(reliable_nodes.len(), 1);
    assert_eq!(reliable_nodes[0].full_url(), node.full_url());
    assert_eq!(reliable_nodes[0].health.success_count, 2);
    assert_eq!(reliable_nodes[0].health.failure_count, 1);
    assert_eq!(reliable_nodes[0].health.avg_latency_ms, Some(100.0));
}

#[tokio::test]
async fn node_pool_status_reflects_recorded_health_checks() {
    let data_dir = TestDir::new("pool");
    let db = Database::new(data_dir.path()).await.unwrap();
    let node = db
        .get_top_nodes_by_recent_success(network_to_string(&Network::Mainnet), 1)
        .await
        .unwrap()
        .pop()
        .expect("mainnet migrations should seed at least one node");
    let (pool, mut receiver) = NodePool::new(db, Network::Mainnet);

    pool.record_success(&node.scheme, &node.host, node.port, 75.0)
        .await
        .unwrap();
    pool.record_failure(&node.scheme, &node.host, node.port)
        .await
        .unwrap();
    pool.publish_status_update().await.unwrap();

    let published = timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("status update should be published")
        .unwrap();

    assert!(published.total_node_count > 0);
    assert_eq!(published.healthy_node_count, 1);
    assert_eq!(published.successful_health_checks, 1);
    assert_eq!(published.unsuccessful_health_checks, 1);
    assert_eq!(published.top_reliable_nodes.len(), 1);
    assert_eq!(published.top_reliable_nodes[0].url, node.full_url());
    assert_eq!(published.top_reliable_nodes[0].success_rate, 0.5);
}

#[tokio::test]
async fn stats_handler_returns_json_for_fresh_database() {
    let data_dir = TestDir::new("stats");
    let db = Database::new(data_dir.path()).await.unwrap();
    let (node_pool, _receiver) = NodePool::new(db, Network::Mainnet);
    let state = AppState {
        node_pool: Arc::new(node_pool),
        tor_client: None,
        connection_pool: ConnectionPool::new(),
    };

    let response = stats_handler(State(state)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(payload["status"], "healthy");
    assert!(payload["total_node_count"].as_u64().unwrap() > 0);
    assert_eq!(payload["successful_health_checks"].as_u64().unwrap(), 0);
    assert_eq!(payload["unsuccessful_health_checks"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn create_app_with_receiver_publishes_initial_status() {
    let data_dir = TestDir::new("app");
    let config = Config::new_random_port(data_dir.path(), Network::Mainnet);
    let (_app, mut receiver, handle) = create_app_with_receiver(config).await.unwrap();

    let status = timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("initial status should be published")
        .unwrap();

    assert!(status.total_node_count > 0);
    assert_eq!(status.successful_health_checks, 0);
    assert_eq!(status.unsuccessful_health_checks, 0);
    assert_eq!(handle.server_info().port, 0);
}
