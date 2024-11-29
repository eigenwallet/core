// std
use std::ops::DerefMut;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// extern
use arti_rpc_client_core::{ObjectId, RpcConn, RpcConnBuilder};

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

    #[error("failed to connect to ArtiProcess after {0:?}")]
    ArtiRpcConnectFailed(std::time::Duration),

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
    daemon: Option<ArtiProcess>,
    rpc_conn: RpcConn,
    pending_events: Arc<Mutex<Vec<TorEvent>>>,
    bootstrapped: bool,

}

impl ArtiTorClient {
    pub fn new(config: ArtiTorClientConfig) -> Result<Self, tor_provider::Error> {
        let (daemon, rpc_conn) = match &config {
            ArtiTorClientConfig::BundledArti {
                arti_bin_path,
                data_directory,
            } => {
                // launch arti
                let daemon =
                    ArtiProcess::new(arti_bin_path.as_path(), data_directory.as_path())
                        .map_err(Error::ArtiProcessCreationFailed)?;

                let builder = RpcConnBuilder::from_connect_string(daemon.connect_string()).unwrap();

                let rpc_conn = {
                    // try to open an rpc conneciton for 5 seconds beore giving up
                    let timeout = Duration::from_secs(5);
                    let mut rpc_conn: Option<RpcConn> = None;

                    let start = Instant::now();
                    while rpc_conn.is_none() && start.elapsed() < timeout {
                        rpc_conn = builder.connect().map_or(None, |rpc_conn| Some(rpc_conn));
                    }

                    if let Some(rpc_conn) = rpc_conn {
                        rpc_conn
                    } else {
                        return Err(Error::ArtiRpcConnectFailed(timeout))?
                    }
                };

                (daemon, rpc_conn)
            },
            _ => {
                return Err(Error::NotImplemented().into())
            }
        };

        let pending_events = std::vec![TorEvent::LogReceived {
            line: "Starting arti-client TorProvider".to_string()
        }];
        let pending_events = Arc::new(Mutex::new(pending_events));

        Ok(Self {
            daemon: Some(daemon),
            rpc_conn,
            pending_events,
            bootstrapped: false,
        })
    }
}

impl TorProvider for ArtiTorClient {
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
        // TODO: seems no way to start arti without automatically bootstrapping
        if !self.bootstrapped {
            match self.pending_events.lock() {
                Ok(mut pending_events) => {
                    pending_events.push(TorEvent::BootstrapStatus {
                        progress: 0,
                        tag: "no-tag".to_string(),
                        summary: "no summary".to_string(),
                    });
                    pending_events.push(TorEvent::BootstrapStatus {
                        progress: 100,
                        tag: "no-tag".to_string(),
                        summary: "no summary".to_string(),
                    });
                    pending_events.push(TorEvent::BootstrapComplete);
                }
                Err(_) => unreachable!(
                    "another thread panicked while holding this pending_events mutex"
                ),
            }
            self.bootstrapped = true;
        }
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
