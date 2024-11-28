// std
use std::path::PathBuf;

// internal crates
use crate::tor_crypto::*;
use crate::tor_provider;
use crate::tor_provider::*;
use crate::arti_process::*;

/// [`ArtiTorClient`]-specific error type
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed to create ArtiProcess object: {0}")]
    ArtiProcessCreationFailed(#[source] crate::arti_process::Error),

    #[error("not implemented")]
    NotImplemented(),
}

impl From<Error> for crate::tor_provider::Error {
    fn from(error: Error) -> Self {
        crate::tor_provider::Error::Generic(error.to_string())
    }
}

#[derive(Clone, Debug)]
pub enum ArtiTorClientConfig {
    BundledArti {
        arti_bin_path: PathBuf,
        data_directory: PathBuf,
    },
    SystemArti {

    },
}

pub struct ArtiTorClient {
    daemon: Option<ArtiProcess>
}

impl ArtiTorClient {
    pub fn new(config: ArtiTorClientConfig) -> Result<Self, tor_provider::Error> {
        let (daemon) = match &config {
            ArtiTorClientConfig::BundledArti {
                arti_bin_path,
                data_directory,
            } => {
                // launch arti
                let daemon =
                    ArtiProcess::new(arti_bin_path.as_path(), data_directory.as_path())
                        .map_err(Error::ArtiProcessCreationFailed)?;
                (daemon)
            },
            _ => {
                return Err(Error::NotImplemented().into())
            }
        };

        Ok(ArtiTorClient {
            daemon: Some(daemon)
        })
    }
}

impl TorProvider for ArtiTorClient {
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
        _target: TargetAddr,
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

    fn release_token(&mut self, _token: CircuitToken) {}
}
