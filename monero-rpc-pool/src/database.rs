use std::path::PathBuf;

use crate::types::{NodeAddress, NodeHealthStats, NodeMetadata, NodeRecord};
use anyhow::Result;
use monero_address::Network;
use sqlx::SqlitePool;
use tracing::{info, warn};

/// Convert a string to a Network enum
pub fn parse_network(s: &str) -> Result<Network, anyhow::Error> {
    match s.to_lowercase().as_str() {
        "mainnet" => Ok(Network::Mainnet),
        "stagenet" => Ok(Network::Stagenet),
        "testnet" => Ok(Network::Testnet),
        _ => anyhow::bail!(
            "Invalid network: {}. Must be mainnet, stagenet, or testnet",
            s
        ),
    }
}

/// Convert a Network enum to a string for database storage
pub fn network_to_string(network: &Network) -> &'static str {
    match network {
        Network::Mainnet => "mainnet",
        Network::Stagenet => "stagenet",
        Network::Testnet => "testnet",
    }
}

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)?;
            info!("Created application data directory: {}", data_dir.display());
        }

        let db_path = data_dir.join("nodes_v3.db");

        info!("Using database at {}", db_path.display());

        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePool::connect(&database_url).await?;

        let db = Self { pool };
        db.migrate().await?;

        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;

        info!("Database migration completed");

        Ok(())
    }

    /// Record a health check event
    pub async fn record_health_check(
        &self,
        scheme: &str,
        host: &str,
        port: u16,
        was_successful: bool,
        latency_ms: Option<f64>,
    ) -> Result<()> {
        let result = sqlx::query!(
            r#"
            INSERT INTO health_checks (node_id, timestamp, was_successful, latency_ms)
            SELECT id, datetime('now'), ?, ?
            FROM monero_nodes 
            WHERE scheme = ? AND host = ? AND port = ?
            "#,
            was_successful,
            latency_ms,
            scheme,
            host,
            port
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            warn!(
                "Cannot record health check for unknown node: {}://{}:{}",
                scheme, host, port
            );
        }

        Ok(())
    }

    /// Get reliable nodes (top 4 by reliability score)
    pub async fn get_reliable_nodes(&self, network: &str) -> Result<Vec<NodeRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                n.id as "id!: i64",
                n.scheme,
                n.host,
                n.port,
                n.network,
                n.first_seen_at,
                CAST(COALESCE(stats.success_count, 0) AS INTEGER) as "success_count!: i64",
                CAST(COALESCE(stats.failure_count, 0) AS INTEGER) as "failure_count!: i64",
                stats.last_success as "last_success?: String",
                stats.last_failure as "last_failure?: String",
                stats.last_checked as "last_checked?: String",
                CAST(1 AS INTEGER) as "is_reliable!: i64",
                stats.avg_latency_ms as "avg_latency_ms?: f64",
                stats.min_latency_ms as "min_latency_ms?: f64",
                stats.max_latency_ms as "max_latency_ms?: f64",
                stats.last_latency_ms as "last_latency_ms?: f64"
            FROM monero_nodes n
            LEFT JOIN (
                SELECT 
                    node_id,
                    SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count,
                    MAX(CASE WHEN was_successful THEN timestamp END) as last_success,
                    MAX(CASE WHEN NOT was_successful THEN timestamp END) as last_failure,
                    MAX(timestamp) as last_checked,
                    AVG(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as avg_latency_ms,
                    MIN(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as min_latency_ms,
                    MAX(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as max_latency_ms,
                    (SELECT latency_ms FROM health_checks hc2 WHERE hc2.node_id = health_checks.node_id ORDER BY timestamp DESC LIMIT 1) as last_latency_ms
                FROM health_checks 
                GROUP BY node_id
            ) stats ON n.id = stats.node_id
            WHERE n.network = ? AND (COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0)) > 0
            ORDER BY 
                (CAST(COALESCE(stats.success_count, 0) AS REAL) / CAST(COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0) AS REAL)) * 
                (MIN(COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0), 200) / 200.0) * 0.8 +
                CASE 
                    WHEN stats.avg_latency_ms IS NOT NULL THEN (1.0 - (MIN(stats.avg_latency_ms, 2000) / 2000.0)) * 0.2
                    ELSE 0.0 
                END DESC
            LIMIT 4
            "#,
            network
        )
        .fetch_all(&self.pool)
        .await?;

        let nodes: Vec<NodeRecord> = rows
            .into_iter()
            .map(|row| {
                let address = NodeAddress::new(row.scheme, row.host, row.port as u16);
                let first_seen_at = row
                    .first_seen_at
                    .parse()
                    .unwrap_or_else(|_| chrono::Utc::now());

                let network = parse_network(&row.network).unwrap_or(Network::Mainnet);
                let metadata = NodeMetadata::new(row.id, network, first_seen_at);
                let health = NodeHealthStats {
                    success_count: row.success_count,
                    failure_count: row.failure_count,
                    last_success: row.last_success.and_then(|s| s.parse().ok()),
                    last_failure: row.last_failure.and_then(|s| s.parse().ok()),
                    last_checked: row.last_checked.and_then(|s| s.parse().ok()),
                    avg_latency_ms: row.avg_latency_ms,
                    min_latency_ms: row.min_latency_ms,
                    max_latency_ms: row.max_latency_ms,
                    last_latency_ms: row.last_latency_ms,
                };
                NodeRecord::new(address, metadata, health)
            })
            .collect();

        Ok(nodes)
    }

    /// Get node statistics for a network
    pub async fn get_node_stats(&self, network: &str) -> Result<(i64, i64, i64)> {
        let row = sqlx::query!(
            r#"
            SELECT 
                COUNT(*) as total,
                CAST(SUM(CASE WHEN stats.success_count > 0 THEN 1 ELSE 0 END) AS INTEGER) as "reachable!: i64",
                CAST(SUM(CASE WHEN stats.success_count > stats.failure_count AND stats.success_count > 0 THEN 1 ELSE 0 END) AS INTEGER) as "reliable!: i64"
            FROM monero_nodes n
            LEFT JOIN (
                SELECT 
                    node_id,
                    SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count
                FROM health_checks 
                GROUP BY node_id
            ) stats ON n.id = stats.node_id
            WHERE n.network = ?
            "#,
            network
        )
        .fetch_one(&self.pool)
        .await?;

        Ok((row.total, row.reachable, row.reliable))
    }

    /// Get health check statistics for a network
    pub async fn get_health_check_stats(&self, network: &str) -> Result<(u64, u64)> {
        let row = sqlx::query!(
            r#"
            SELECT 
                CAST(SUM(CASE WHEN hc.was_successful THEN 1 ELSE 0 END) AS INTEGER) as "successful!: i64",
                CAST(SUM(CASE WHEN NOT hc.was_successful THEN 1 ELSE 0 END) AS INTEGER) as "unsuccessful!: i64"
            FROM (
                SELECT hc.was_successful
                FROM health_checks hc
                JOIN monero_nodes n ON hc.node_id = n.id
                WHERE n.network = ?
                ORDER BY hc.timestamp DESC
                LIMIT 100
            ) hc
            "#,
            network
        )
        .fetch_one(&self.pool)
        .await?;

        let successful = row.successful as u64;
        let unsuccessful = row.unsuccessful as u64;

        Ok((successful, unsuccessful))
    }

    /// Get top nodes based on success rate
    /// Adds randomness
    pub async fn get_top_nodes_by_recent_success(
        &self,
        network: &str,
        limit: i64,
    ) -> Result<Vec<NodeAddress>> {
        // Randomized ordering: r = max of 3 Uniform(0,1) (biased toward 1).
        // Rank by (base_score * r) so top nodes remain preferred but can shuffle.
        // r is drawn once per row in the CTE and reused in ORDER BY.
        // Increase RANDOM() terms in MAX(...) to strengthen the bias.
        let rows = sqlx::query!(
            r#"
            WITH scored AS (
                SELECT 
                    n.scheme,
                    n.host,
                    n.port,
                    CASE 
                        WHEN (COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0)) > 0 
                        THEN CAST(COALESCE(stats.success_count, 0) AS REAL) / CAST(COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0) AS REAL)
                        ELSE 0.0 
                    END as base_score,
                    MAX(
                        ABS(RANDOM()) / CAST(0x7fffffffffffffff AS REAL),
                        ABS(RANDOM()) / CAST(0x7fffffffffffffff AS REAL),
                        ABS(RANDOM()) / CAST(0x7fffffffffffffff AS REAL)
                    ) as r
                FROM monero_nodes n
                LEFT JOIN (
                    SELECT 
                        node_id,
                        SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                        SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count
                    FROM (
                        SELECT node_id, was_successful
                        FROM health_checks 
                        ORDER BY timestamp DESC 
                        LIMIT 1000
                    ) recent_checks
                    GROUP BY node_id
                ) stats ON n.id = stats.node_id
                WHERE n.network = ?
            )
            SELECT scheme, host, port
            FROM scored
            ORDER BY (base_score * r) DESC, r DESC
            LIMIT ?
            "#,
            network,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let addresses: Vec<NodeAddress> = rows
            .into_iter()
            .map(|row| NodeAddress::new(row.scheme, row.host, row.port as u16))
            .collect();

        Ok(addresses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    async fn test_db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = Database::new(dir.path().to_path_buf())
            .await
            .expect("database with migrations applied");
        (dir, db)
    }

    /// Inserts a fixture node on the testnet network, which the migrations
    /// leave empty, so tests stay independent of the seeded node lists.
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

    /// Records a health check for a fixture node (all fixtures use http and
    /// port 18081).
    async fn record(db: &Database, host: &str, was_successful: bool, latency_ms: Option<f64>) {
        db.record_health_check("http", host, 18081, was_successful, latency_ms)
            .await
            .expect("record health check");
    }

    /// Inserts a health check with an explicit timestamp, which the production
    /// write path cannot do because it always stores datetime('now').
    async fn insert_check(db: &Database, host: &str, was_successful: bool, timestamp: &str) {
        sqlx::query(
            r#"
            INSERT INTO health_checks (node_id, timestamp, was_successful)
            SELECT id, ?, ? FROM monero_nodes WHERE host = ? AND network = 'testnet'
            "#,
        )
        .bind(timestamp)
        .bind(was_successful)
        .bind(host)
        .execute(&db.pool)
        .await
        .expect("insert fixture health check");
    }

    #[test]
    fn parse_network_accepts_all_networks_case_insensitively_and_rejects_others() {
        assert!(matches!(
            parse_network("mainnet").expect("mainnet parses"),
            Network::Mainnet
        ));
        assert!(matches!(
            parse_network("MAINNET").expect("uppercase mainnet parses"),
            Network::Mainnet
        ));
        assert!(matches!(
            parse_network("Stagenet").expect("mixed case stagenet parses"),
            Network::Stagenet
        ));
        assert!(matches!(
            parse_network("testNet").expect("mixed case testnet parses"),
            Network::Testnet
        ));

        assert!(parse_network("devnet").is_err());
        assert!(parse_network("").is_err());
    }

    #[test]
    fn network_to_string_round_trips_through_parse_network() {
        for network in [Network::Mainnet, Network::Stagenet, Network::Testnet] {
            let parsed = parse_network(network_to_string(&network)).expect("valid network string");
            assert_eq!(parsed, network);
        }
    }

    #[tokio::test]
    async fn migrations_seed_mainnet_and_stagenet_but_no_testnet() {
        let (_dir, db) = test_db().await;

        // The exact counts are the node lists inserted by
        // migrations/20250628093515_add_default_nodes_from_feather.sql plus the
        // extra mainnet node from
        // migrations/20250813225842_cryptostorm_mainnet_node.sql. They are a
        // deliberate contract: a migration that adds or removes seed nodes
        // must update this test in the same change.
        assert_eq!(db.get_node_stats("mainnet").await.unwrap(), (18, 0, 0));
        assert_eq!(db.get_node_stats("stagenet").await.unwrap(), (11, 0, 0));
        assert_eq!(db.get_node_stats("testnet").await.unwrap(), (0, 0, 0));
    }

    #[tokio::test]
    async fn record_health_check_ignores_unknown_nodes() {
        let (_dir, db) = test_db().await;
        insert_node(&db, "known.example").await;

        db.record_health_check("http", "unknown.example", 18081, true, Some(10.0))
            .await
            .expect("unknown node is skipped without error");

        assert_eq!(db.get_health_check_stats("testnet").await.unwrap(), (0, 0));
    }

    #[tokio::test]
    async fn node_stats_distinguish_total_reachable_and_reliable() {
        let (_dir, db) = test_db().await;

        insert_node(&db, "unchecked.example").await;
        insert_node(&db, "succeeding.example").await;
        insert_node(&db, "failing.example").await;
        insert_node(&db, "mostly-failing.example").await;

        record(&db, "succeeding.example", true, Some(100.0)).await;
        record(&db, "failing.example", false, None).await;
        record(&db, "failing.example", false, None).await;
        record(&db, "mostly-failing.example", true, Some(100.0)).await;
        record(&db, "mostly-failing.example", false, None).await;
        record(&db, "mostly-failing.example", false, None).await;

        assert_eq!(db.get_node_stats("testnet").await.unwrap(), (4, 2, 1));
    }

    #[tokio::test]
    async fn health_check_stats_are_scoped_to_network() {
        let (_dir, db) = test_db().await;
        insert_node(&db, "testnet-node.example").await;

        for _ in 0..3 {
            record(&db, "testnet-node.example", true, Some(100.0)).await;
        }
        for _ in 0..2 {
            record(&db, "testnet-node.example", false, None).await;
        }

        assert_eq!(db.get_health_check_stats("testnet").await.unwrap(), (3, 2));

        // A checked mainnet node must not leak into the testnet numbers.
        let mainnet_nodes = db
            .get_top_nodes_by_recent_success("mainnet", 1)
            .await
            .expect("seeded mainnet nodes");
        let Some(mainnet_node) = mainnet_nodes.first() else {
            panic!("expected at least one seeded mainnet node");
        };
        db.record_health_check(
            &mainnet_node.scheme,
            &mainnet_node.host,
            mainnet_node.port,
            true,
            Some(10.0),
        )
        .await
        .expect("record mainnet health check");

        assert_eq!(db.get_health_check_stats("testnet").await.unwrap(), (3, 2));
        assert_eq!(db.get_health_check_stats("mainnet").await.unwrap(), (1, 0));
    }

    #[tokio::test]
    async fn health_check_stats_only_count_the_hundred_most_recent() {
        let (_dir, db) = test_db().await;
        insert_node(&db, "busy.example").await;

        // 50 successes that are all older than 100 more recent failures. The
        // timestamps are zero padded so their lexicographic order in SQLite
        // matches the chronological order.
        for minute in 0..150 {
            let timestamp = format!("2026-01-01 {:02}:{:02}", minute / 60, minute % 60);
            insert_check(&db, "busy.example", minute < 50, &timestamp).await;
        }

        assert_eq!(
            db.get_health_check_stats("testnet").await.unwrap(),
            (0, 100)
        );
    }

    #[tokio::test]
    async fn reliable_nodes_exclude_unchecked_and_rank_by_latency_score() {
        let (_dir, db) = test_db().await;
        insert_node(&db, "fast.example").await;
        insert_node(&db, "slow.example").await;
        insert_node(&db, "unchecked.example").await;

        record(&db, "fast.example", true, Some(100.0)).await;
        record(&db, "fast.example", true, Some(100.0)).await;
        record(&db, "slow.example", true, Some(1500.0)).await;
        record(&db, "slow.example", true, Some(1500.0)).await;
        // A failure carrying a latency must not pollute the success-only
        // latency aggregates.
        record(&db, "fast.example", false, Some(50.0)).await;

        let reliable = db.get_reliable_nodes("testnet").await.unwrap();

        assert_eq!(reliable.len(), 2);
        assert_eq!(reliable[0].address.host, "fast.example");
        assert_eq!(reliable[1].address.host, "slow.example");

        let fast = &reliable[0];
        assert_eq!(fast.health.success_count, 2);
        assert_eq!(fast.health.failure_count, 1);
        assert_eq!(fast.health.avg_latency_ms, Some(100.0));
        assert_eq!(fast.health.min_latency_ms, Some(100.0));
        assert_eq!(fast.health.max_latency_ms, Some(100.0));

        // The reliability score is the success ratio scaled by check volume
        // plus a latency bonus, so the 100 ms node outranks the 1500 ms node.
        let slow = &reliable[1];
        assert_eq!(slow.health.success_count, 2);
        assert_eq!(slow.health.failure_count, 0);
        assert_eq!(slow.health.avg_latency_ms, Some(1500.0));
        assert_eq!(slow.health.last_latency_ms, Some(1500.0));
    }

    #[tokio::test]
    async fn reliable_nodes_cap_at_four_entries() {
        let (_dir, db) = test_db().await;

        for idx in 0..6 {
            let host = format!("node-{idx}.example");
            insert_node(&db, &host).await;
            record(&db, &host, true, Some(100.0)).await;
        }

        assert_eq!(db.get_reliable_nodes("testnet").await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn top_nodes_include_unchecked_nodes_up_to_limit() {
        let (_dir, db) = test_db().await;

        let total = db.get_node_stats("mainnet").await.unwrap().0;

        // The pool must offer nodes to try even before any health check exists.
        let five = db
            .get_top_nodes_by_recent_success("mainnet", 5)
            .await
            .unwrap();
        assert_eq!(five.len(), 5);
        let distinct: HashSet<_> = five.iter().collect();
        assert_eq!(distinct.len(), 5);

        // A limit beyond the number of candidates returns every candidate.
        let all = db
            .get_top_nodes_by_recent_success("mainnet", total + 10)
            .await
            .unwrap();
        assert_eq!(all.len() as i64, total);

        // The empty testnet network yields no candidates.
        assert!(
            db.get_top_nodes_by_recent_success("testnet", 5)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
