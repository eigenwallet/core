use std::sync::Arc;
use std::{path::Path, time::Duration};

use crate::cli::api::tauri_bindings::{
    TauriBackgroundProgress, TauriEmitter, TauriHandle, TorBootstrapStatus,
};
use arti_client::{Error, TorClient, config::TorClientConfigBuilder, status::BootstrapStatus};
use futures::StreamExt;
use tor_rtcompat::tokio::TokioRustlsRuntime;

static TOR_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
static TOR_RESOLVE_TIMEOUT: Duration = Duration::from_secs(20);



use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Minimal SOCKS5 bridge that routes traffic through the embedded Arti Tor client
pub async fn spawn_socks5_bridge(tor_client: Arc<TorClient<TokioRustlsRuntime>>, port: u16) {
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind local SOCKS5 bridge on port {}: {}", port, e);
            return;
        }
    };
    tracing::info!("Local SOCKS5 bridge is listening on 127.0.0.1:{} for Electrum", port);

    while let Ok((mut stream, _)) = listener.accept().await {
        let tor_client = tor_client.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 256];
            // 1. SOCKS5 greeting
            if stream.read_exact(&mut buf[0..2]).await.is_err() || buf[0] != 0x05 { return; }
            let n_methods = buf[1] as usize;
            if stream.read_exact(&mut buf[0..n_methods]).await.is_err() { return; }
            // 2. Reply: No auth required
            if stream.write_all(&[0x05, 0x00]).await.is_err() { return; }
            // 3. Read request
            if stream.read_exact(&mut buf[0..4]).await.is_err() { return; }
            if buf[0] != 0x05 || buf[1] != 0x01 { return; } // SOCKS5, CONNECT
            
            let dst_addr = match buf[3] {
                0x01 => { // IPv4
                    if stream.read_exact(&mut buf[0..4]).await.is_err() { return; }
                    std::net::Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]).to_string()
                }
                0x03 => { // Domain
                    if stream.read_exact(&mut buf[0..1]).await.is_err() { return; }
                    let len = buf[0] as usize;
                    if stream.read_exact(&mut buf[0..len]).await.is_err() { return; }
                    String::from_utf8_lossy(&buf[0..len]).to_string()
                }
                _ => return, // Only IPv4 and Domain supported for simplicity
            };

            if stream.read_exact(&mut buf[0..2]).await.is_err() { return; }
            let dst_port = u16::from_be_bytes([buf[0], buf[1]]);

            // 4. Connect via Arti Tor Client
            match tor_client.connect((dst_addr.as_str(), dst_port)).await {
                Ok(mut tor_stream) => {
                    // Reply success
                    if stream.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await.is_err() { return; }
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut tor_stream).await;
                }
                Err(_) => {
                    // Reply host unreachable
                    let _ = stream.write_all(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await;
                }
            }
        });
    }
}


pub async fn create_tor_client(
    data_dir: &Path,
    system_socks5: Option<String>, // EKLENDİ
) -> Result<Arc<TorClient<TokioRustlsRuntime>>, Error> {
    let data_dir = data_dir.join("tor");
    let state_dir = data_dir.join("state");
    let cache_dir = data_dir.join("cache");

    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("ARTI_FS_DISABLE_PERMISSION_CHECKS", "1")
    };

    let guards_file = state_dir.join("state").join("guards.json");
    let _ = tokio::fs::remove_file(&guards_file).await;

    let mut config = TorClientConfigBuilder::from_directories(state_dir, cache_dir);

    config
        .stream_timeouts()
        .connect_timeout(TOR_CONNECT_TIMEOUT);
    config
        .stream_timeouts()
        .resolve_timeout(TOR_RESOLVE_TIMEOUT);

    // EKLENDİ: Sistem Tor modu aktifse bilgi ver. 
    // Tails OS şeffaf proxy (transparent proxy) kullandığı için Arti'nin Guard TCP bağlantıları 
    // otomatik olarak SOCKS5 ağına (Tor-over-Tor) yönlendirilecektir.
    if let Some(proxy_addr) = system_socks5 {
        tracing::info!(
            "System Tor (SOCKS5) is enabled ({}). Arti P2P client is starting and will be routed transparently by the OS.",
            proxy_addr
        );
    }

    let config = config
        .build()
        .expect("We initialized the Tor client all required attributes");

    let runtime = TokioRustlsRuntime::current().expect("We are always running with tokio");
    tracing::debug!("Creating unbootstrapped Tor client");

    let tor_client = TorClient::with_runtime(runtime)
        .config(config)
        .create_unbootstrapped_async()
        .await?;

    Ok(tor_client)
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

    // Start the local SOCKS5 bridge once Tor is successfully bootstrapped
    let tor_client_for_socks = tor_client.clone();
    tokio::spawn(async move {
        spawn_socks5_bridge(tor_client_for_socks, 9150).await;
    });
    // ------------------------

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
