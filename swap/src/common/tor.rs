use std::sync::Arc;
use std::{path::Path, time::Duration};

use crate::cli::api::tauri_bindings::{
    TauriBackgroundProgress, TauriEmitter, TauriHandle, TorBootstrapStatus,
};
use arti_client::{config::TorClientConfigBuilder, status::BootstrapStatus, Error, TorClient};
use futures::StreamExt;
use tor_rtcompat::tokio::TokioRustlsRuntime;

const TOR_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const TOR_RESOLVE_TIMEOUT: Duration = Duration::from_secs(20);

/// Creates an unbootstrapped Tor client
pub async fn create_tor_client(
    data_dir: &Path,
) -> Result<Arc<TorClient<TokioRustlsRuntime>>, Error> {
    // We store the Tor state in the data directory
    let data_dir = data_dir.join("tor");
    let state_dir = data_dir.join("state");
    let cache_dir = data_dir.join("cache");

    // Workaround for https://gitlab.torproject.org/tpo/core/arti/-/issues/2224
    // We delete guards.json (if it exists) on startup to prevent an issue where arti will not find any guards to connect to
    // This forces new guards on every startup
    //
    // TODO: This is not good for privacy and should be removed as soon as this is fixed in arti itself.
    let guards_file = state_dir.join("state").join("guards.json");
    let _ = tokio::fs::remove_file(&guards_file).await;

    // The client configuration describes how to connect to the Tor network,
    // and what directories to use for storing persistent state.
    let mut config = TorClientConfigBuilder::from_directories(state_dir, cache_dir);

    config
        .stream_timeouts()
        .connect_timeout(TOR_CONNECT_TIMEOUT);
    config
        .stream_timeouts()
        .resolve_timeout(TOR_RESOLVE_TIMEOUT);

    let config = config
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
