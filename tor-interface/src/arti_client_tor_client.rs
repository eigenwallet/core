// standard
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

//extern
use tokio::*;
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
}

impl ArtiClientTorClient {
    pub fn new(tokio_runtime: Arc<runtime::Runtime>, data_directory: &Path) -> Result<Self, Error> {

        let arti_client = tokio_runtime.block_on(async {
            // set custom config options
            let mut config_builder: TorClientConfigBuilder = Default::default();

            // manually set arti data directory so we can have multiple concurrent instances and control
            // where it writes
            config_builder.storage().cache_dir(CfgPath::new_literal(PathBuf::from(data_directory)));
            config_builder.storage().state_dir(CfgPath::new_literal(PathBuf::from(data_directory)));

            let config = match config_builder.build() {
                Ok(config) => config,
                Err(err) => return Err(err).map_err(Error::ArtiClientConfigBuilderError),
            };

            TorClient::builder()
                .config(config)
                .bootstrap_behavior(BootstrapBehavior::Manual)
                .create_unbootstrapped().map_err(Error::ArtiClientError)
        })?;

        Ok(Self {
            tokio_runtime,
            arti_client,
        })
    }
}

impl TorProvider for ArtiClientTorClient {
    fn update(&mut self) -> Result<Vec<TorEvent>, tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn bootstrap(&mut self) -> Result<(), tor_provider::Error> {
        Err(Error::NotImplemented().into())
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
