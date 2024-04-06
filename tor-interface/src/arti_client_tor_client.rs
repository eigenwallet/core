// standard
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener};
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

//extern
use arti_client::{BootstrapBehavior, TorClient};
use arti_client::config::{CfgPath, TorClientConfigBuilder};
use fs_mistrust::Mistrust;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::runtime;
use tokio_stream::StreamExt;
use tor_cell::relaycell::msg::{Connected};
use tor_keymgr::{ArtiEphemeralKeystore, KeyMgrBuilder, KeystoreSelector};
use tor_hscrypto::pk::HsIdKeypair;
use tor_hsservice::{HsIdKeypairSpecifier, HsNickname, OnionService, RunningOnionService};
use tor_hsservice::config::{OnionServiceConfigBuilder};
use tor_llcrypto::pk::ed25519::ExpandedKeypair;
use tor_persist::state_dir::StateDirectory;
use tor_proto::stream::IncomingStreamRequest;
use tor_rtcompat::PreferredRuntime;

// internal crates
use crate::tor_crypto::*;
use crate::tor_provider;
use crate::tor_provider::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("not implemented")]
    NotImplemented(),

    #[error("unable to bind TCP listener")]
    TcpListenerBindFailed(#[source] std::io::Error),

    #[error("unable to get TCP listener's local address")]
    TcpListenerLocalAddrFailed(#[source] std::io::Error),

    #[error("arti-client config-builder error: {0}")]
    ArtiClientConfigBuilderError(#[source] arti_client::config::ConfigBuildError),

    #[error("arti-client error: {0}")]
    ArtiClientError(#[source] arti_client::Error),

    #[error("tor-keymgr error: {0}")]
    TorKeyMgrError(#[source] tor_keymgr::Error),

    // TODO: the 'real' error (tor_keymgr::mgr::KeyMgrBuilderError) isn't
    // actually defined anywhere from what I can tell
    #[error("failed to build KeyMgr")]
    TorKeyMgrBuilderError(),

    #[error("unexpected key found when inserting HsIdKeypair")]
    KeyMgrInsertionFailure(),

    #[error("onion-service config-builder error: {0}")]
    OnionServiceConfigBuilderError(#[source] tor_config::ConfigBuildError),

    #[error("tor-persist error: {0}")]
    TorPersistError(#[source] tor_persist::Error),

    #[error("tor-hsservice startup error: {0}")]
    TorHsServiceStartupError(#[source] tor_hsservice::StartupError),
}

impl From<Error> for crate::tor_provider::Error {
    fn from(error: Error) -> Self {
        crate::tor_provider::Error::Generic(error.to_string())
    }
}

pub struct ArtiClientOnionListener {
    listener: std::net::TcpListener,
    onion_addr: OnionAddr,
    // onion service terminates when this is dropped, we don't actually use it
    // for anything after construction
    _onion_service: Arc<RunningOnionService>,
}

impl OnionListenerImpl for ArtiClientOnionListener {
    fn set_nonblocking(&self, nonblocking: bool) -> Result<(), std::io::Error> {
        self.listener.set_nonblocking(nonblocking)
    }
    fn accept(&self) -> Result<Option<OnionStream>, std::io::Error> {
        match self.listener.accept() {
            Ok((stream, _socket_addr)) => Ok(Some(OnionStream {
                stream,
                local_addr: Some(self.onion_addr.clone()),
                peer_addr: None,
            })),
            Err(err) => {
                if err.kind() == ErrorKind::WouldBlock {
                    Ok(None)
                } else {
                    Err(err)
                }
            }
        }
    }
}

pub struct ArtiClientTorClient {
    tokio_runtime: Arc<runtime::Runtime>,
    arti_client: TorClient<PreferredRuntime>,
    state_dir: PathBuf,
    fs_mistrust: Mistrust,
    pending_events: Arc<Mutex<Vec<TorEvent>>>,
}

// used to forward traffic to/from arti to local tcp streams
async fn forward_stream<R, W>(mut reader: R, mut writer: W) -> ()
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = [0u8; 1024];
    loop {
        let count = match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(count) => count,
            Err(_) => break,
        };

        match writer.write_all(&buf[0..count]).await {
            Ok(()) => (),
            Err(_) => break,
        }
        match writer.flush().await {
            Ok(()) => (),
            Err(_) => break,
        }
    }
}

impl ArtiClientTorClient {
    pub fn new(tokio_runtime: Arc<runtime::Runtime>, root_data_directory: &Path) -> Result<Self, Error> {

        // set custom config options
        let mut config_builder: TorClientConfigBuilder = Default::default();

        // manually set arti cache and data directories so we can have
        // multiple concurrent instances and control where it writes
        let mut cache_dir = PathBuf::from(root_data_directory);
        cache_dir.push("cache");
        config_builder.storage().cache_dir(CfgPath::new_literal(cache_dir));

        let mut state_dir = PathBuf::from(root_data_directory);
        state_dir.push("state");
        config_builder.storage().state_dir(CfgPath::new_literal(state_dir.clone()));

        let config = match config_builder.build() {
            Ok(config) => config,
            Err(err) => return Err(err).map_err(Error::ArtiClientConfigBuilderError),
        };

        let fs_mistrust = config.fs_mistrust().clone();

        let arti_client = tokio_runtime.block_on(async {

            TorClient::builder()
                .config(config)
                .bootstrap_behavior(BootstrapBehavior::Manual)
                .create_unbootstrapped().map_err(Error::ArtiClientError)

            // TODO: implement TorEvent::LogReceived events once upstream issue is resolved:
            // https://gitlab.torproject.org/tpo/core/arti/-/issues/1356
        })?;

        let pending_events = std::vec![
            TorEvent::LogReceived { line: "Starting arti-client TorProvider".to_string() }
        ];
        let pending_events = Arc::new(Mutex::new(pending_events));

        Ok(Self {
            tokio_runtime,
            arti_client,
            state_dir,
            fs_mistrust,
            pending_events,
        })
    }
}

impl TorProvider for ArtiClientTorClient {
    fn update(&mut self) -> Result<Vec<TorEvent>, tor_provider::Error> {
        std::thread::sleep(std::time::Duration::from_millis(16));
        match self.pending_events.lock() {
            Ok(mut pending_events) => {
                Ok(std::mem::take(pending_events.deref_mut()))
            }
            Err(_) => unreachable!("another thread panicked while holding this pending_events mutex"),
        }
    }

    fn bootstrap(&mut self) -> Result<(), tor_provider::Error> {
        // save progress events
        let mut bootstrap_events = self.arti_client.bootstrap_events();
        let pending_events = self.pending_events.clone();
        self.tokio_runtime.spawn(async move {
            while let Some(evt) = bootstrap_events.next().await {
                match pending_events.lock() {
                    Ok(mut pending_events) => {
                        pending_events.push(TorEvent::BootstrapStatus {
                            progress: (evt.as_frac().clamp(0.0f32, 1.0f32) * 100f32) as u32,
                            tag: "no-tag".to_string(),
                            summary: "no summary".to_string(),
                        });
                        // TODO: properly handle evt.blocked() with a new TorEvent::Error or something
                    },
                    Err(_) => unreachable!("another thread panicked while holding this pending_events mutex"),
                }
            }
        });

        // initiate bootstrap
        let arti_client = self.arti_client.clone();
        let pending_events = self.pending_events.clone();
        self.tokio_runtime.spawn(async move {
            match arti_client.bootstrap().await {
                Ok(()) => {
                    match pending_events.lock() {
                        Ok(mut pending_events) => {
                            pending_events.push(TorEvent::BootstrapComplete);
                            return;
                        },
                        Err(_) => unreachable!("another thread panicked while holding this pending_events mutex"),
                    }
                },
                Err(_err) => {
                    // TODO: add an error event to TorEvent
                },
            }
        });

        Ok(())
    }

    fn add_client_auth(
        &mut self,
        _service_id: &V3OnionServiceId,
        _client_auth: &X25519PrivateKey,
    ) -> Result<(), tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn remove_client_auth(
        &mut self,
        _service_id: &V3OnionServiceId,
    ) -> Result<(), tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn connect(
        &mut self,
        _service_id: &V3OnionServiceId,
        _virt_port: u16,
        _circuit: Option<CircuitToken>,
    ) -> Result<OnionStream, tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorized_clients: Option<&[X25519PublicKey]>,
    ) -> Result<OnionListener, tor_provider::Error> {

        // client auth is not implemented yet
        if authorized_clients.is_some() {
            return Err(Error::NotImplemented().into());
        }

        // try to bind to a local address, let OS pick our port
        let socket_addr = SocketAddr::from(([127, 0, 0, 1], 0u16));
        let listener = TcpListener::bind(socket_addr).map_err(Error::TcpListenerBindFailed)?;
        let socket_addr = listener
            .local_addr()
            .map_err(Error::TcpListenerLocalAddrFailed)?;

        // create a new ephemeral store for storing our onion service keys
        let ephemeral_store: ArtiEphemeralKeystore = ArtiEphemeralKeystore::new("ephemeral".to_string());

        let keymgr = match KeyMgrBuilder::default()
            .default_store(Box::new(ephemeral_store))
            .build() {
            Ok(keymgr) => keymgr,
            Err(_) => return Err(Error::TorKeyMgrBuilderError().into()),
        };

        // generate a nickname to identify this onion service
        let service_id = V3OnionServiceId::from_private_key(private_key);
        let hs_nickname = match HsNickname::new(service_id.to_string()) {
            Ok(nickname) => nickname,
            Err(_) => panic!("v3 onion service id string representation should be a valid HsNickname"),
        };

        let hs_id_spec = HsIdKeypairSpecifier::new(hs_nickname.clone());
        // generate a new HsIdKeypair (from an Ed25519PrivateKey)
        // clone() isn't implemented for ExpandedKeypair >:[
        let secret_key_bytes = private_key.inner().to_secret_key_bytes();
        let expanded_keypair = ExpandedKeypair::from_secret_key_bytes(secret_key_bytes).unwrap().into();

        // write the HsIdKeypair to keymgr
        // TODO: for now this should return Ok(None) unless we persist the ephemeral store longer-term (ie for client auth keys in the future)
        match keymgr.insert::<HsIdKeypair>(expanded_keypair, &hs_id_spec, KeystoreSelector::Default) {
            Ok(None) => (), // expected
            Ok(Some(_)) => return Err(Error::KeyMgrInsertionFailure().into()),
            Err(err) => Err(err).map_err(Error::TorKeyMgrError)?,
        }

        // create an OnionServiceConfig with the ephemeral nickname
        let onion_service_config = match OnionServiceConfigBuilder::default()
            .nickname(hs_nickname)
            .build() {
            Ok(onion_service_config) => onion_service_config,
            Err(err) => Err(err).map_err(Error::OnionServiceConfigBuilderError)?,
        };

        // create OnionService
        let state_dir = match StateDirectory::new(self.state_dir.as_path(), &self.fs_mistrust) {
            Ok(state_dir) => state_dir,
            Err(err) => Err(err).map_err(Error::TorPersistError)?,
        };
        let onion_service = OnionService::new(onion_service_config, Arc::new(keymgr), &state_dir).map_err(Error::TorHsServiceStartupError)?;

        let onion_addr = OnionAddr::V3(OnionAddrV3::new(
            service_id.clone(),
            virt_port,
        ));

        // launch the OnionService and get a Stream of RendRequest
        let runtime = self.arti_client.runtime().clone();
        let dirmgr = self.arti_client.dirmgr().clone().upcast_arc();
        let hs_circ_pool = self.arti_client.hs_circ_pool().clone();
        // TODO: maybe make this not block, and instead we update the ArtiOnionListener to have like an
        // Arc<Mutex<Option<RunningOnionService>>> which this task sets and on setting *then* we push
        // the OnionServicePublished event
        let (onion_service, mut rend_requests) = self.tokio_runtime.block_on(async move {
            onion_service.launch(runtime.clone(), dirmgr, hs_circ_pool)
        }).map_err(Error::TorHsServiceStartupError)?;

        match self.pending_events.lock() {
            Ok(mut pending_events) => {
                pending_events.push(TorEvent::OnionServicePublished {service_id});
            }
            Err(_) => unreachable!("another thread panicked while holding this pending_events mutex"),
        }

        // start a task which accepts every RendRequest to get a StreamRequest
        self.tokio_runtime.spawn(async move {
            while let Some(request) = rend_requests.next().await {
                let mut stream_requests = match request.accept().await {
                    Ok(stream_requests) => stream_requests,
                    // TODO: probably not our problem?
                    _ => return,
                };
                // spawn a new task to consume the stream requsts
                tokio::task::spawn(async move {
                    while let Some(stream_request) = stream_requests.next().await {
                        let should_accept = if let IncomingStreamRequest::Begin(begin) = stream_request.request() {
                            // we only accept connections on the virt port
                            begin.port() == virt_port
                        } else {
                            false
                        };

                        if should_accept {
                            let (client_reader, client_writer) = match stream_request.accept(Connected::new_empty()).await {
                                Ok(data_stream) => data_stream.split(),
                                //  TODO: probably not our problem?
                                _ => continue,
                            };

                            let (local_reader, local_writer) = match TcpStream::connect(socket_addr).await {
                                Ok(tcp_stream) => tcp_stream.into_split(),
                                // TODO: possibly our problem?
                                _ => continue,
                            };
                            // now spawn new tasks to forward traffic to/from theonion listener

                            // read from connected client and write to local socket
                            tokio::task::spawn(async move {
                                forward_stream(client_reader, local_writer).await;
                            });
                            // read from local socket and write to connected client
                            tokio::task::spawn(async move {
                                forward_stream(local_reader, client_writer).await;
                            });
                        } else {
                            // either requesting the wrong port or the wrong type of stream request
                            let _ = stream_request.shutdown_circuit();
                        }
                    }
                });
            }
        });

        // return our OnionListener
        let onion_listener = Box::new(ArtiClientOnionListener {
            listener,
            onion_addr,
            _onion_service: onion_service,
        });

        Ok(OnionListener { onion_listener })
    }

    fn generate_token(&mut self) -> CircuitToken {
        0usize
    }

    fn release_token(&mut self, _token: CircuitToken) {

    }
}
