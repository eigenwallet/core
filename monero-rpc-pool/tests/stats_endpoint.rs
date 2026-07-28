use anyhow::Result;
use monero_address::Network;
use monero_rpc_pool::{config::Config, start_server_with_random_port};
use tempfile::TempDir;
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn stats_endpoint_returns_health_payload() -> Result<()> {
    let data_dir = TempDir::new()?;
    let config = Config::new_random_port(data_dir.path().to_path_buf(), Network::Mainnet);
    let (server_info, _status_receiver, _pool_handle) = start_server_with_random_port(config).await?;
    let url = format!("http://{}:{}/stats", server_info.host, server_info.port);

    let client = reqwest::Client::new();
    let mut last_error = None;

    for _ in 0..10 {
        match client.get(&url).send().await {
            Ok(response) => {
                assert!(response.status().is_success());

                let payload: serde_json::Value = response.json().await?;
                assert_eq!(payload["status"], "healthy");
                assert!(payload.get("total_node_count").is_some());
                assert!(payload.get("healthy_node_count").is_some());
                assert!(payload.get("successful_health_checks").is_some());
                assert!(payload.get("unsuccessful_health_checks").is_some());
                assert!(payload.get("top_reliable_nodes").is_some());
                assert!(payload.get("bandwidth_kb_per_sec").is_some());

                return Ok(());
            }
            Err(error) => {
                last_error = Some(error);
                sleep(Duration::from_millis(100)).await;
            }
        }
    }

    Err(last_error
        .expect("expected at least one request attempt")
        .into())
}
