use std::path::Path;
use std::sync::Arc;

use crate::cli::api::tauri_bindings::{
    TauriBackgroundProgress, TauriEmitter, TauriHandle, TorBootstrapStatus,
};
use arti_client::{config::TorClientConfigBuilder, status::BootstrapStatus, Error, TorClient};
use futures::StreamExt;
use swap_env::env::is_whonix;
use tor_rtcompat::tokio::TokioRustlsRuntime;

pub fn existing_tor_config() -> Option<(
    libp2p_tor::tor_interface::legacy_tor_client::LegacyTorClientConfig,
    std::net::SocketAddr,
)> {
    if is_whonix() {
        Some((libp2p_tor::tor_interface::legacy_tor_client::LegacyTorClientConfig::system_from_environment().expect("whonix always has $TOR_... set"),
            ([0, 0, 0, 0], 9939).into()))
    } else {
        None
    }
}

/// Creates an unbootstrapped Tor client
pub async fn create_tor_client(
    data_dir: &Path,
) -> Result<Arc<TorClient<TokioRustlsRuntime>>, Error> {
    // We store the Tor state in the data directory
    let data_dir = data_dir.join("tor");
    let state_dir = data_dir.join("state");
    let cache_dir = data_dir.join("cache");

    // The client configuration describes how to connect to the Tor network,
    // and what directories to use for storing persistent state.
    let config = TorClientConfigBuilder::from_directories(state_dir, cache_dir)
        .build()
        .expect("We initialized the Tor client all required attributes");

    // Create the Arti client without bootstrapping
    let runtime = TokioRustlsRuntime::current().expect("We are always running with tokio");

    tracing::debug!("Creating unbootstrapped Tor client");

    let tor_client = TorClient::with_runtime(runtime)
        .config(config)
        .create_unbootstrapped_async()
        .await?;

    Ok(Arc::new(tor_client))
}

/// Bootstraps an existing Tor client
pub async fn bootstrap_tor_client(
    tor_client: Arc<TorClient<TokioRustlsRuntime>>,
    tauri_handle: Option<TauriHandle>,
) -> Result<(), Error> {
    let mut bootstrap_events = tor_client.bootstrap_events();

    tracing::debug!("Bootstrapping Tor client");

    // Create a background progress handle for the Tor bootstrap process
    // The handle manages the TauriHandle internally, so we don't need to worry about it anymore
    let progress_handle =
        tauri_handle.new_background_process(TauriBackgroundProgress::EstablishingTorCircuits);

    // Clone the handle for the task
    let progress_handle_clone = progress_handle.clone();

    // Start a task to monitor bootstrap events
    let progress_task = tokio::spawn(async move {
        loop {
            match bootstrap_events.next().await {
                Some(event) => {
                    let status = event.to_tauri_bootstrap_status();
                    progress_handle_clone.update(status);
                }
                None => continue,
            }
        }
    });

    // Run the bootstrap until it's complete
    tokio::select! {
        _ = progress_task => unreachable!("Tor bootstrap progress handle should never exit"),
        res = tor_client.bootstrap() => {
            progress_handle.finish();
            res
        },
    }?;

    Ok(())
}

// A trait to convert the Tor bootstrap event into a TauriBootstrapStatus
trait ToTauriBootstrapStatus {
    fn to_tauri_bootstrap_status(&self) -> TorBootstrapStatus;
}

impl ToTauriBootstrapStatus for BootstrapStatus {
    fn to_tauri_bootstrap_status(&self) -> TorBootstrapStatus {
        TorBootstrapStatus {
            frac: self.as_frac(),
            ready_for_traffic: self.ready_for_traffic(),
            blockage: self.blocked().map(|b| b.to_string()),
        }
    }
}
