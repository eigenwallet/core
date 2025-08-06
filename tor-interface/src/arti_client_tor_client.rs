// standard
use std::future::Future;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

//extern
use arti_client::config::{CfgPath, TorClientConfigBuilder};
use arti_client::{BootstrapBehavior, DangerouslyIntoTorAddr, DataStream, IntoTorAddr, TorClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::{pin, runtime};
use tokio_mpmc as mpmc;
use tokio_stream::StreamExt;
use tor_cell::relaycell::msg::Connected;
use tor_config::ExplicitOrAuto;
use tor_llcrypto::pk::ed25519::ExpandedKeypair;
use tor_hsservice::config::OnionServiceConfigBuilder;
use tor_hsservice::config::restricted_discovery::HsClientNickname;
use tor_hsservice::status::State;
use tor_hsservice::{HsNickname, RunningOnionService, StreamRequest};
use tor_keymgr::{config::ArtiKeystoreKind, KeystoreSelector};
use tor_proto::stream::IncomingStreamRequest;
use tor_rtcompat::PreferredRuntime;

// internal crates
use crate::tor_crypto::*;
use crate::tor_provider;
use crate::tor_provider::*;

/// [`ArtiClientTorClient`]-specific error type
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("not implemented")]
    NotImplemented(),

    #[error("unable to bind TCP listener")]
    TcpListenerBindFailed(#[source] std::io::Error),

    #[error("unable to get TCP listener's local address")]
    TcpListenerLocalAddrFailed(#[source] std::io::Error),

    #[error("unable to accept connection on TCP Listener")]
    TcpListenerAcceptFailed(#[source] std::io::Error),

    #[error("unable to connect to TCP listener")]
    TcpStreamConnectFailed(#[source] std::io::Error),

    #[error("unable to convert tokio::TcpStream to std::net::TcpStream")]
    TcpStreamIntoFailed(#[source] std::io::Error),

    #[error("arti-client config-builder error: {0}")]
    ArtiClientConfigBuilderError(#[source] arti_client::config::ConfigBuildError),

    #[error("arti-client error: {0}")]
    ArtiClientError(#[source] arti_client::Error),

    #[error("arti-client tor-addr error: {0}")]
    ArtiClientTorAddrError(#[source] arti_client::TorAddrError),

    #[error("arti-client onion-service startup error: {0}")]
    ArtiClientOnionServiceLaunchError(#[source] arti_client::Error),

    #[error("tor-keymgr error: {0}")]
    TorKeyMgrError(#[source] tor_keymgr::Error),

    #[error("onion-service config-builder error: {0}")]
    OnionServiceConfigBuilderError(#[source] tor_config::ConfigBuildError),
}

impl From<Error> for crate::tor_provider::Error {
    fn from(error: Error) -> Self {
        crate::tor_provider::Error::Generic(error.to_string())
    }
}

/// The `ArtiClientTorClient` is an in-process [`arti-client`](https://crates.io/crates/arti-client)-based [`TorProvider`].
///
///
pub struct ArtiClientTorClient {
    tokio_runtime: Arc<runtime::Runtime>,
    arti_client: TorClient<PreferredRuntime>,
    pending_events: Arc<Mutex<Vec<TorEvent>>>,
    bootstrapped: Arc<AtomicBool>,
}

impl ArtiClientTorClient {
    /// Construct a new `ArtiClientTorClient` which uses a [Tokio](https://crates.io/crates/tokio) runtime internally for all async operations.
    pub fn new(
        tokio_runtime: Arc<runtime::Runtime>,
        root_data_directory: &Path,
    ) -> Result<Self, Error> {
        // set custom config options
        let mut config_builder: TorClientConfigBuilder = Default::default();

        // manually set arti cache and data directories so we can have
        // multiple concurrent instances and control where it writes
        let mut cache_dir = PathBuf::from(root_data_directory);
        cache_dir.push("cache");
        config_builder
            .storage()
            .cache_dir(CfgPath::new_literal(cache_dir))
            .keystore()
            .primary().kind(ExplicitOrAuto::Explicit(ArtiKeystoreKind::Ephemeral));

        let mut state_dir = PathBuf::from(root_data_directory);
        state_dir.push("state");
        config_builder
            .storage()
            .state_dir(CfgPath::new_literal(state_dir));

        // disable access to clearnet addresses and enable access to onion services
        config_builder
            .address_filter()
            .allow_local_addrs(false)
            .allow_onion_addrs(true);

        let config = match config_builder.build() {
            Ok(config) => config,
            Err(err) => return Err(err).map_err(Error::ArtiClientConfigBuilderError),
        };

        let arti_client = tokio_runtime.block_on(async {
            TorClient::builder()
                .config(config)
                .bootstrap_behavior(BootstrapBehavior::Manual)
                .create_unbootstrapped()
                .map_err(Error::ArtiClientError)

            // TODO: implement TorEvent::LogReceived events once upstream issue is resolved:
            // https://gitlab.torproject.org/tpo/core/arti/-/issues/1356
        })?;

        let pending_events = std::vec![TorEvent::LogReceived {
            line: "Starting arti-client TorProvider".to_string()
        }];
        let pending_events = Arc::new(Mutex::new(pending_events));

        Ok(Self {
            tokio_runtime,
            arti_client,
            pending_events,
            bootstrapped: Arc::new(AtomicBool::new(false)),
        })
    }
}

impl TorProvider for ArtiClientTorClient {
    type Stream = ArtiClientOnionStream;
    type Listener = ArtiClientOnionListener;

    fn update(&mut self) -> Result<Vec<TorEvent>, tor_provider::Error> {
        std::thread::sleep(std::time::Duration::from_millis(16));
        match self.pending_events.lock() {
            Ok(mut pending_events) => Ok(std::mem::take(pending_events.deref_mut())),
            Err(_) => {
                unreachable!("another thread panicked while holding this pending_events mutex")
            }
        }
    }

    fn bootstrap(&mut self) -> Result<(), tor_provider::Error> {
        // save progress events
        let mut bootstrap_events = self.arti_client.bootstrap_events();
        let pending_events = self.pending_events.clone();
        let bootstrapped = self.bootstrapped.clone();
        self.tokio_runtime.spawn(async move {
            while let Some(evt) = bootstrap_events.next().await {
                if bootstrapped.load(Ordering::Relaxed) {
                    break;
                }
                match pending_events.lock() {
                    Ok(mut pending_events) => {
                        pending_events.push(TorEvent::BootstrapStatus {
                            progress: (evt.as_frac().clamp(0.0f32, 1.0f32) * 100f32) as u32,
                            tag: "no-tag".to_string(),
                            summary: "no summary".to_string(),
                        });
                        // TODO: properly handle evt.blocked() with a new TorEvent::Error or something
                    }
                    Err(_) => unreachable!(
                        "another thread panicked while holding this pending_events mutex"
                    ),
                }
            }
        });

        // initiate bootstrap
        let arti_client = self.arti_client.clone();
        let pending_events = self.pending_events.clone();
        let bootstrapped = self.bootstrapped.clone();
        self.tokio_runtime.spawn(async move {
            match arti_client.bootstrap().await {
                Ok(()) => match pending_events.lock() {
                    Ok(mut pending_events) => {
                        pending_events.push(TorEvent::BootstrapStatus {
                            progress: 100,
                            tag: "no-tag".to_string(),
                            summary: "no summary".to_string(),
                        });
                        pending_events.push(TorEvent::BootstrapComplete);
                        bootstrapped.store(true, Ordering::Relaxed);
                        return;
                    }
                    Err(_) => unreachable!(
                        "another thread panicked while holding this pending_events mutex"
                    ),
                },
                Err(_err) => {
                    // TODO: add an error event to TorEvent
                }
            }
        });

        Ok(())
    }

    fn add_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
        client_auth: &X25519PrivateKey,
    ) -> Result<(), tor_provider::Error> {
        let ed25519_public = Ed25519PublicKey::from_service_id(service_id).unwrap();
        let hs_id = ed25519_public.as_bytes().clone();

        self.arti_client.insert_service_discovery_key(KeystoreSelector::Primary, hs_id.into(), client_auth.inner().clone().into()).map_err(Error::ArtiClientError)?;

        Ok(())
    }

    fn remove_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
    ) -> Result<(), tor_provider::Error> {
        let ed25519_public = Ed25519PublicKey::from_service_id(service_id).unwrap();
        let hs_id = ed25519_public.as_bytes().clone();

        self.arti_client.remove_service_discovery_key(KeystoreSelector::Primary, hs_id.into()).map_err(Error::ArtiClientError)?;

        Ok(())
    }

    fn connect(
        &mut self,
        target: TargetAddr,
        circuit: Option<CircuitToken>,
    ) -> Result<Self::Stream, tor_provider::Error> {
        // stream isolation not implemented yet
        if circuit.is_some() {
            return Err(Error::NotImplemented().into());
        }

        // connect to onion service
        let arti_target = match target.clone() {
            TargetAddr::Socket(socket_addr) => socket_addr.into_tor_addr_dangerously(),
            TargetAddr::Domain(domain_addr) => {
                (domain_addr.domain(), domain_addr.port()).into_tor_addr()
            }
            TargetAddr::OnionService(OnionAddr::V3(OnionAddrV3 {
                service_id,
                virt_port,
            })) => (format!("{}.onion", service_id), virt_port).into_tor_addr(),
        }
        .map_err(Error::ArtiClientTorAddrError)?;

        let arti_client = self.arti_client.clone();
        let data_stream = self
            .tokio_runtime
            .block_on(async move { arti_client.connect(arti_target).await })
            .map_err(Error::ArtiClientError)?;

        Ok(ArtiClientOnionStream {
            tokio_runtime: self.tokio_runtime.clone(),
            data_stream,
            nonblocking: AtomicBool::new(false),
            local_addr: None,
            peer_addr: Some(target),
        })
    }

    fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorized_clients: Option<&[X25519PublicKey]>,
        _bind_addr: Option<SocketAddr>,
    ) -> Result<Self::Listener, tor_provider::Error> {
        // generate a nickname to identify this onion service
        let service_id = V3OnionServiceId::from_private_key(private_key);
        let hs_nickname = match HsNickname::new(service_id.to_string()) {
            Ok(nickname) => nickname,
            Err(_) => {
                panic!("v3 onion service id string representation should be a valid HsNickname")
            }
        };
        // generate a new HsIdKeypair (from an Ed25519PrivateKey)
        // clone() isn't implemented for ExpandedKeypair >:[
        let secret_key_bytes = private_key.inner().to_secret_key_bytes();
        let hs_id_keypair = ExpandedKeypair::from_secret_key_bytes(secret_key_bytes)
            .unwrap();

        // create an OnionServiceConfig with the ephemeral nickname
        let mut onion_service_config_builder = OnionServiceConfigBuilder::default();
        onion_service_config_builder
            .nickname(hs_nickname);

        // add authorised client keys if they exist
        if let Some(authorized_clients) = authorized_clients {
            if !authorized_clients.is_empty() {
                let restricted_discovery_config = onion_service_config_builder
                    .restricted_discovery();
                restricted_discovery_config.enabled(true);

                for (i, key) in authorized_clients.iter().enumerate() {
                    let nickname = format!("client_{i}");
                    restricted_discovery_config
                        .static_keys()
                        .access()
                        .push((
                            HsClientNickname::from_str(nickname.as_str()).unwrap(),
                            key.inner().clone().into(),
                        ));
                }
            }
        }

        let onion_service_config = onion_service_config_builder.build()
            .map_err(Error::OnionServiceConfigBuilderError)?;

        let (onion_service, rend_requests) = self.arti_client
            .launch_onion_service_with_hsid(onion_service_config, hs_id_keypair.into())
            .map_err(Error::ArtiClientOnionServiceLaunchError)?;

        // start a task to signal onion service published
        let pending_events = self.pending_events.clone();
        let mut status_events = onion_service.status_events();
        let service_id_clone = service_id.clone();

        self.tokio_runtime.spawn(async move {
            while let Some(evt) = status_events.next().await {
                match evt.state() {
                    tor_hsservice::status::State::Running => match pending_events.lock() {
                        Ok(mut pending_events) => {
                            pending_events.push(TorEvent::OnionServicePublished { service_id: service_id_clone });
                            return;
                        }
                        Err(_) => unreachable!(
                            "another thread panicked while holding this pending_events mutex"
                        ),
                    },
                    _ => (),
                }
            }
        });

        let (sender, receiver) = mpmc::channel(1);
        self.tokio_runtime.spawn(async move {
            let mut stream_requests = tor_hsservice::handle_rend_requests(rend_requests);
            while let Some(stream_request) = stream_requests.next().await {
                if sender.send(stream_request).await.is_err() {
                    return;
                }
            }
        });

        let onion_addr = OnionAddr::V3(OnionAddrV3::new(service_id, virt_port));
        // onion-service is torn down when `onion_service` is dropped
        Ok(ArtiClientOnionListener {
            tokio_runtime: self.tokio_runtime.clone(),
            stream_requests: receiver,
            virt_port,

            nonblocking: AtomicBool::new(false),
            onion_service,
            onion_addr,
        })
    }

    fn generate_token(&mut self) -> CircuitToken {
        0usize
    }

    fn release_token(&mut self, _token: CircuitToken) {}
}

#[derive(Debug)]
pub struct ArtiClientOnionStream {
    tokio_runtime: Arc<runtime::Runtime>,
    data_stream: DataStream,

    nonblocking: AtomicBool,
    peer_addr: Option<TargetAddr>,
    local_addr: Option<OnionAddr>,
}

macro_rules! fwd {
    ($self:expr, $func:tt, $($args:expr),*) => {{
        pin! {
            let fut = $self.data_stream.$func($($args),*);
        }
        if $self.nonblocking.load(Ordering::Relaxed) {
            match fut.poll(&mut Context::from_waker(Waker::noop())) {
                Poll::Ready(ret) => ret,
                Poll::Pending => Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "")),
            }
        } else {
            $self.tokio_runtime.block_on(fut)
        }
    }}
}

impl Read for ArtiClientOnionStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        fwd!(self, read, buf)
    }
}

impl Write for ArtiClientOnionStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        fwd!(self, write, buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        fwd!(self, flush, )
    }
    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        fwd!(self, write_vectored, bufs)
    }
}

impl OnionStream for ArtiClientOnionStream {
    fn peer_addr(&self) -> Option<TargetAddr> {
        self.peer_addr.clone()
    }

    fn local_addr(&self) -> Option<OnionAddr> {
        self.local_addr.clone()
    }

    fn try_clone(&self) -> std::io::Result<Self> where Self: Sized {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "not available"))
    }

    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        self.nonblocking.store(nonblocking, Ordering::Relaxed);
        Ok(())
    }

    fn into_raw(self) -> OnionStreamIntoRaw {
        unimplemented!()
    }
}

pub struct ArtiClientOnionListener {
    tokio_runtime: Arc<runtime::Runtime>,
    stream_requests: mpmc::Receiver<StreamRequest>,
    virt_port: u16,

    nonblocking: AtomicBool,
    onion_service: Arc<RunningOnionService>,
    onion_addr: OnionAddr,
}

impl OnionListener for ArtiClientOnionListener {
    type Stream = ArtiClientOnionStream;

    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        self.nonblocking.store(nonblocking, Ordering::Relaxed);
        Ok(())
    }

    fn accept(&self) -> std::io::Result<Option<Self::Stream>> {
        pin! {
            let fut = async {
                while let Some(stream_request) = self.stream_requests.recv().await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))? {
                    let should_accept =
                        if let IncomingStreamRequest::Begin(begin) = stream_request.request() {
                            // we only accept connections on the virt port
                            begin.port() == self.virt_port
                        } else {
                            false
                        };

                    if should_accept {
                        let data_stream =
                            match stream_request.accept(Connected::new_empty()).await {
                                Ok(data_stream) => data_stream,
                                // TODO: probably not our problem
                                _ => continue,
                            };

                        return Ok(Some(ArtiClientOnionStream {
                            tokio_runtime: self.tokio_runtime.clone(),
                            data_stream,
                            nonblocking: AtomicBool::new(false),
                            local_addr: Some(self.onion_addr.clone()),
                            peer_addr: None,
                        }));
                    } else {
                        // either requesting the wrong port or the wrong type of stream request
                        let _ = stream_request.shutdown_circuit();
                    }
                }
                match self.onion_service.status().state() {
                    s @ State::Shutdown | s @ State::DegradedUnreachable | s @ State::Broken => Err(std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", s))),
                    _ => Ok(None),
                }
            };
        }
        if self.nonblocking.load(Ordering::Relaxed) {
            match fut.poll(&mut Context::from_waker(Waker::noop())) {
                Poll::Ready(ret) => ret,
                Poll::Pending => Ok(None),
            }
        } else {
            self.tokio_runtime.block_on(fut)
        }
    }

    fn address(&self) -> &OnionAddr {
        &self.onion_addr
    }
}
