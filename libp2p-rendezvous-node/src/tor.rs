// TODO: This is essentially vendored from swap/src/common/tor.rs
// TODO: Consider extracting this into a common swap-tor crate
use anyhow::{Context, Result};
use arti_client::{config::TorClientConfigBuilder, TorClient};
use std::path::Path;
use std::sync::Arc;
use tor_rtcompat::tokio::TokioRustlsRuntime;

/// Creates an unbootstrapped Tor client with custom data directories
pub async fn create_tor_client(data_dir: &Path) -> Result<Arc<TorClient<TokioRustlsRuntime>>> {
    // We store the Tor state in the data directory
    let tor_data_dir = data_dir.join("tor");
    let state_dir = tor_data_dir.join("state");
    let cache_dir = tor_data_dir.join("cache");

    // Workaround for https://gitlab.torproject.org/tpo/core/arti/-/issues/2224
    // We delete guards.json (if it exists) on startup to prevent an issue where arti will not find any guards to connect to
    // This forces new guards on every startup
    //
    // TODO: This is not good for privacy and should be removed as soon as this is fixed in arti itself.
    let guards_file = state_dir.join("state").join("guards.json");
    let _ = tokio::fs::remove_file(&guards_file).await;

    // The client configuration describes how to connect to the Tor network,
    // and what directories to use for storing persistent state.
    let config = TorClientConfigBuilder::from_directories(&state_dir, &cache_dir);

    let config = config
        .build()
        .context("Failed to build Tor client config")?;

    // Create the Arti client without bootstrapping
    let runtime = TokioRustlsRuntime::current().context("We are always running with tokio")?;

    tracing::debug!("Creating unbootstrapped Tor client");

    let tor_client = TorClient::with_runtime(runtime)
        .config(config)
        .create_unbootstrapped_async()
        .await
        .context("Failed to create unbootstrapped Tor client")?;

    Ok(Arc::new(tor_client))
}

/// Bootstraps an existing Tor client
pub async fn bootstrap_tor_client(tor_client: Arc<TorClient<TokioRustlsRuntime>>) -> Result<()> {
    tracing::debug!("Bootstrapping Tor client");

    // Run the bootstrap until it's complete
    tor_client
        .bootstrap()
        .await
        .context("Failed to bootstrap Tor client")?;

    Ok(())
}
