use crate::{config::Config, create_app, database::Database};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use mockito::{Mock, Server, ServerGuard};
use monero_address::Network;
use std::time::Duration;
use tempfile::TempDir;
use tower::ServiceExt;

const REQUEST: &str = r#"{"jsonrpc":"2.0","id":"0","method":"get_info"}"#;
const RESPONSE: &str = r#"{"jsonrpc":"2.0","id":"0","result":{"height":1}}"#;

/// The node order is random, so tests that depend on it are repeated.
const TRIALS: usize = 16;

struct TestPool {
    app: Router,
    db: Database,
    _data_dir: TempDir,
}

/// Testnet has no default nodes, so only the given nodes are eligible.
async fn test_pool(ports: &[u16]) -> TestPool {
    let data_dir = TempDir::new().unwrap();
    let db = Database::new(data_dir.path().to_path_buf()).await.unwrap();

    for port in ports {
        sqlx::query(
            "INSERT INTO monero_nodes (scheme, host, port, network, first_seen_at)
            VALUES ('http', '127.0.0.1', ?, 'testnet', datetime('now'))",
        )
        .bind(*port as i64)
        .execute(&db.pool)
        .await
        .unwrap();
    }

    let config = Config::new_random_port(data_dir.path().to_path_buf(), Network::Testnet);
    let app = create_app(config).await.unwrap();

    TestPool {
        app,
        db,
        _data_dir: data_dir,
    }
}

async fn mock_node(status: usize, body: &str) -> (ServerGuard, Mock) {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/json_rpc")
        .with_status(status)
        .with_body(body)
        .create_async()
        .await;
    (server, mock)
}

fn port(server: &ServerGuard) -> u16 {
    server.socket_address().port()
}

fn unreachable_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

async fn call(app: &Router) -> (StatusCode, String) {
    let request = Request::post("/json_rpc")
        .body(Body::from(REQUEST))
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(10), app.clone().oneshot(request))
        .await
        .expect("proxy request hung")
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

/// (successes, failures) recorded for the node with this port.
async fn health_checks(db: &Database, port: u16) -> (i64, i64) {
    sqlx::query_as(
        "SELECT CAST(COALESCE(SUM(hc.was_successful), 0) AS INTEGER),
            CAST(COALESCE(SUM(NOT hc.was_successful), 0) AS INTEGER)
        FROM health_checks hc JOIN monero_nodes n ON n.id = hc.node_id
        WHERE n.port = ?",
    )
    .bind(port as i64)
    .fetch_one(&db.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn forwards_response_and_records_success() {
    let (node, mock) = mock_node(200, RESPONSE).await;
    let pool = test_pool(&[port(&node)]).await;

    let (status, body) = call(&pool.app).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, RESPONSE);
    mock.assert_async().await;
    assert_eq!(health_checks(&pool.db, port(&node)).await, (1, 0));
}

#[tokio::test]
async fn fails_over_from_unreachable_node() {
    for _ in 0..TRIALS {
        let (node, _mock) = mock_node(200, RESPONSE).await;
        let pool = test_pool(&[port(&node), unreachable_port()]).await;

        let (status, body) = call(&pool.app).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, RESPONSE);
        assert_eq!(health_checks(&pool.db, port(&node)).await, (1, 0));
    }
}

#[tokio::test]
async fn skips_nodes_returning_http_or_jsonrpc_errors() {
    for (bad_status, bad_body) in [(500, "oops"), (200, r#"{"error":"boom"}"#)] {
        for _ in 0..TRIALS {
            let (bad, bad_mock) = mock_node(bad_status, bad_body).await;
            let (good1, _mock1) = mock_node(200, RESPONSE).await;
            let (good2, _mock2) = mock_node(200, RESPONSE).await;
            let pool = test_pool(&[port(&bad), port(&good1), port(&good2)]).await;

            let (status, body) = call(&pool.app).await;

            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, RESPONSE);
            // A failure is only recorded if the bad node was actually tried
            let tried = bad_mock.matched_async().await;
            assert_eq!(health_checks(&pool.db, port(&bad)).await, (0, tried as i64));
        }
    }
}

#[tokio::test]
async fn does_not_forward_http_or_jsonrpc_errors() {
    for (bad_status, bad_body) in [(500, "oops"), (200, r#"{"error":"boom"}"#)] {
        let (bad, bad_mock) = mock_node(bad_status, bad_body).await;
        let pool = test_pool(&[port(&bad)]).await;

        let (status, _) = call(&pool.app).await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        bad_mock.assert_async().await;
    }
}

#[tokio::test]
async fn returns_bad_gateway_when_all_nodes_are_unreachable() {
    let pool = test_pool(&[unreachable_port(), unreachable_port()]).await;

    let (status, body) = call(&pool.app).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(body.contains("All requests failed"), "{body}");
}

#[tokio::test]
async fn health_checks_rank_nodes_and_feed_stats() {
    let pool = test_pool(&[1, 2, 3]).await;
    let checks = [
        (1, true),
        (1, true),
        (1, true),
        (2, true),
        (2, false),
        (2, false),
    ];
    for (port, success) in checks {
        pool.db
            .record_health_check("http", "127.0.0.1", port, success, success.then_some(10.0))
            .await
            .unwrap();
    }

    let ranked: Vec<u16> = pool
        .db
        .get_reliable_nodes("testnet")
        .await
        .unwrap()
        .iter()
        .map(|node| node.address.port)
        .collect();

    // Node 3 has never been checked
    assert_eq!(ranked, [1, 2]);
    // (total, reachable, reliable)
    assert_eq!(pool.db.get_node_stats("testnet").await.unwrap(), (3, 2, 1));
    assert_eq!(
        pool.db.get_health_check_stats("testnet").await.unwrap(),
        (4, 2)
    );
}
