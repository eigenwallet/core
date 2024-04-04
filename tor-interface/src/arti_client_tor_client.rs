// standard
use std::io::ErrorKind;
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

//extern
use tokio::*;
use tokio_stream::StreamExt;
use arti_client::{BootstrapBehavior, TorClient};
use arti_client::config::{CfgPath, TorClientConfigBuilder};
use tor_rtcompat::PreferredRuntime;

// internal crates
use crate::tor_crypto::*;
use crate::tor_provider;
use crate::tor_provider::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("not implemented")]
    NotImplemented(),
    #[error("arti-client config-builder error: {0}")]
    ArtiClientConfigBuilderError(#[source] arti_client::config::ConfigBuildError),
    #[error("arti-client error: {0}")]
    ArtiClientError(#[source] arti_client::Error),
}

impl From<Error> for crate::tor_provider::Error {
    fn from(error: Error) -> Self {
        crate::tor_provider::Error::Generic(error.to_string())
    }
}

pub struct ArtiClientOnionListener {
}

impl OnionListenerImpl for ArtiClientOnionListener {
    fn set_nonblocking(&self, _nonblocking: bool) -> Result<(), std::io::Error> {
        Err(std::io::Error::new(ErrorKind::Other, "not implemented"))
    }
    fn accept(&self) -> Result<Option<OnionStream>, std::io::Error> {
        Err(std::io::Error::new(ErrorKind::Other, "not implemented"))
    }
}

pub struct ArtiClientTorClient {
    tokio_runtime: Arc<runtime::Runtime>,
    arti_client: TorClient<PreferredRuntime>,
    pending_events: Arc<Mutex<Vec<TorEvent>>>,
}

impl ArtiClientTorClient {
    pub fn new(tokio_runtime: Arc<runtime::Runtime>, root_data_directory: &Path) -> Result<Self, Error> {

        let arti_client = tokio_runtime.block_on(async {
            // set custom config options
            let mut config_builder: TorClientConfigBuilder = Default::default();

            // manually set arti cache and data directories so we can have
            // multiple concurrent instances and control where it writes
            let mut cache_dir = PathBuf::from(root_data_directory);
            cache_dir.push("cache");
            config_builder.storage().cache_dir(CfgPath::new_literal(cache_dir));

            let mut data_dir = PathBuf::from(root_data_directory);
            data_dir.push("data");
            config_builder.storage().state_dir(CfgPath::new_literal(data_dir));

            let config = match config_builder.build() {
                Ok(config) => config,
                Err(err) => return Err(err).map_err(Error::ArtiClientConfigBuilderError),
            };

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
        _private_key: &Ed25519PrivateKey,
        _virt_port: u16,
        _authorized_clients: Option<&[X25519PublicKey]>,
    ) -> Result<OnionListener, tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn generate_token(&mut self) -> CircuitToken {
        0usize
    }

    fn release_token(&mut self, _token: CircuitToken) {

    }
}
