use std::sync::Arc;

use anyhow::Result;
use arti_client::TorClient;
use axum::{
    routing::{any, get},
    Router,
};

use tokio::task::JoinHandle;
use tor_rtcompat::tokio::TokioRustlsRuntime;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

/// Type alias for the Tor client used throughout the crate
pub type TorClientArc = Arc<TorClient<TokioRustlsRuntime>>;

pub mod config;
pub mod connection_pool;
pub mod database;
pub mod pool;
pub mod proxy;
pub mod types;

use config::Config;
use database::Database;
use pool::{NodePool, PoolStatus};
use proxy::{proxy_handler, stats_handler};

#[derive(Clone)]
pub struct AppState {
    pub node_pool: Arc<NodePool>,
    pub tor_client: Option<TorClientArc>,
    pub connection_pool: crate::connection_pool::ConnectionPool,
}

/// Manages background tasks for the RPC pool
pub struct PoolHandle {
    pub status_update_handle: JoinHandle<()>,
    pub server_info: ServerInfo,
}

impl PoolHandle {
    /// Get the current server info for the pool
    pub fn server_info(&self) -> &ServerInfo {
        &self.server_info
    }
}

impl Drop for PoolHandle {
    fn drop(&mut self) {
        self.status_update_handle.abort();
    }
}

/// Information about a running RPC pool server
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub port: u16,
    pub host: String,
}

impl Into<String> for ServerInfo {
    fn into(self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

pub async fn create_app_with_receiver(
    config: Config,
) -> Result<(
    Router,
    tokio::sync::broadcast::Receiver<PoolStatus>,
    PoolHandle,
)> {
    // Initialize database
    let db = Database::new(config.data_dir.clone()).await?;

    // Initialize node pool with network from config
    let (node_pool, status_receiver) = NodePool::new(db.clone(), config.network.clone());
    let node_pool = Arc::new(node_pool);

    // Publish initial status immediately to ensure first event is sent
    if let Err(e) = node_pool.publish_status_update().await {
        error!("Failed to publish initial status update: {}", e);
    }

    // Send status updates every 10 seconds
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    let node_pool_for_health_check = node_pool.clone();
    let status_update_handle = tokio::spawn(async move {
        loop {
            if let Err(e) = node_pool_for_health_check.publish_status_update().await {
                error!("Failed to publish status update: {}", e);
            }

            interval.tick().await;
        }
    });

    let pool_handle = PoolHandle {
        status_update_handle,
        server_info: ServerInfo {
            port: config.port,
            host: config.host.clone(),
        },
    };

    let app_state = AppState {
        node_pool,
        tor_client: config.tor_client,
        connection_pool: crate::connection_pool::ConnectionPool::new(),
    };

    // Build the app
    let app = Router::new()
        .route("/stats", get(stats_handler))
        .route("/{*path}", any(proxy_handler))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    Ok((app, status_receiver, pool_handle))
}

pub async fn create_app(config: Config) -> Result<Router> {
    let (app, _, _pool_handle) = create_app_with_receiver(config).await?;
    Ok(app)
}

pub async fn run_server(config: Config) -> Result<()> {
    let app = create_app(config.clone()).await?;

    let bind_address = format!("{}:{}", config.host, config.port);
    info!("Starting server on {}", bind_address);

    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    info!("Server listening on {}", bind_address);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Run a server with a custom data directory
pub async fn run_server_with_data_dir(config: Config, data_dir: std::path::PathBuf) -> Result<()> {
    let config_with_data_dir =
        Config::new_with_port(config.host, config.port, data_dir, config.network);
    run_server(config_with_data_dir).await
}

/// Start a server with a random port for library usage
/// Returns the server info with the actual port used, a receiver for pool status updates, and pool handle
pub async fn start_server_with_random_port(
    config: Config,
) -> Result<(
    ServerInfo,
    tokio::sync::broadcast::Receiver<PoolStatus>,
    PoolHandle,
)> {
    let host = config.host.clone();
    let (app, status_receiver, mut pool_handle) = create_app_with_receiver(config).await?;

    // Bind to port 0 to get a random available port
    let listener = tokio::net::TcpListener::bind(format!("{}:0", host)).await?;
    let actual_addr = listener.local_addr()?;

    let server_info = ServerInfo {
        port: actual_addr.port(),
        host: host.clone(),
    };

    // Update the pool handle with the actual server info
    pool_handle.server_info = server_info.clone();

    info!(
        "Started server on {}:{} (random port)",
        server_info.host, server_info.port
    );

    // Start the server in a background task
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("Server error: {}", e);
        }
    });

    Ok((server_info, status_receiver, pool_handle))
}
