use std::{path::Path, sync::Arc, time::Duration};

use crate::cli::api::tauri_bindings::{
    TauriBackgroundProgress, TauriEmitter, TauriHandle, TorBootstrapStatus,
};
use arti_client::{config::TorClientConfigBuilder, status::BootstrapStatus, Error, TorClient};
use futures::StreamExt;
use libp2p::core::transport::{OptionalTransport, OrTransport};
use libp2p::Transport;
use libp2p_tor::{AddressConversion, TorTransport};
use swap_tor::*;
use tor_rtcompat::tokio::TokioRustlsRuntime;

/// Creates an unbootstrapped Tor client or connects to well-known Tor daemon, depending on configuration.
///
/// 1. if the caller requests (user enables) `tor`: prepare an Arti client
/// 2. `None`
pub async fn create_tor_client(data_dir: &Path, tor: bool) -> Result<TorBackend, Error> {
    Ok(if tor {
        TorBackend::Arti(Arc::new(create_arti_tor_client(data_dir).await?))
    } else {
        TorBackend::None
    })
}

#[allow(async_fn_in_trait)]
pub trait TorBackendSwap {
    async fn bootstrap(&self, tauri_handle: Option<TauriHandle>) -> anyhow::Result<()>;
    fn clone_for_monero_rpc(&self, enable_monero_tor: bool) -> TorBackend;
    fn into_transport(
        self,
        arti_address_conversion: AddressConversion,
        arti_transport_hook: impl FnOnce(&mut TorTransport),
    ) -> std::io::Result<IntoTransportT>;
}
type IntoTransportT = OrTransport<
    OrTransport<OptionalTransport<TorTransport>, OptionalTransport<Socks5Transport>>,
    TcpTransport,
>;
impl TorBackendSwap for TorBackend {
    async fn bootstrap(&self, tauri_handle: Option<TauriHandle>) -> anyhow::Result<()> {
        match self {
            TorBackend::Arti(arti) => bootstrap_arti_tor_client(arti, tauri_handle).await?,
            TorBackend::Socks(addr) => {
                addr.connect().await?; // validate the remote is actually listening
            }
            TorBackend::None => {}
        }
        Ok(())
    }

    /// Obey `enable_monero_tor` if it's meaningful on the current system.
    fn clone_for_monero_rpc(&self, enable_monero_tor: bool) -> TorBackend {
        match self {
            TorBackend::Arti(..) if enable_monero_tor => self.clone(),
            TorBackend::Arti(..) => TorBackend::None,
            TorBackend::Socks(..) | TorBackend::None => self.clone(),
        }
    }

    fn into_transport(
        self,
        arti_address_conversion: AddressConversion,
        arti_transport_hook: impl FnOnce(&mut TorTransport),
    ) -> std::io::Result<IntoTransportT> {
        fn plain_transport() -> std::io::Result<TcpTransport> {
            let tcp = libp2p::tcp::tokio::Transport::new(libp2p::tcp::Config::new().nodelay(true));
            libp2p::dns::tokio::Transport::system(tcp)
        }
        let tcp_with_dns = plain_transport()?;

        let tor = match self {
            TorBackend::Arti(tor_client) => {
                let mut tor_transport =
                    TorTransport::from_client(tor_client, arti_address_conversion);
                arti_transport_hook(&mut tor_transport);
                OrTransport::new(
                    OptionalTransport::some(tor_transport),
                    OptionalTransport::none(),
                )
            }
            TorBackend::Socks(universal_config) => OrTransport::new(
                OptionalTransport::none(),
                OptionalTransport::some(universal_config.transport()),
            ),
            TorBackend::None => {
                OrTransport::new(OptionalTransport::none(), OptionalTransport::none())
            }
        };
        Ok(tor.or_transport(tcp_with_dns))
    }
}

const TOR_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const TOR_RESOLVE_TIMEOUT: Duration = Duration::from_secs(20);

async fn create_arti_tor_client(data_dir: &Path) -> Result<TorClient<TokioRustlsRuntime>, Error> {
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

    TorClient::with_runtime(runtime)
        .config(config)
        .create_unbootstrapped_async()
        .await
}

/// Bootstraps an existing Tor client
async fn bootstrap_arti_tor_client(
    tor_client: &TorClient<TokioRustlsRuntime>,
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
