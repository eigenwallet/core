// std
use std::collections::BTreeMap;
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

    #[error("arti not bootstrapped")]
    ArtiNotBootstrapped(),

    #[error("failed to connect: {0}")]
    ArtiOpenStreamFailed(#[source] arti_rpc_client_core::StreamError),

    #[error("invalid circuit token: {0}")]
    CircuitTokenInvalid(CircuitToken),

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
    pending_log_lines: Arc<Mutex<Vec<String>>>,
    pending_events: Arc<Mutex<Vec<TorEvent>>>,
    bootstrapped: bool,
    // our list of circuit tokens for the arti daemon
    circuit_token_counter: usize,
    circuit_tokens: BTreeMap<CircuitToken, String>,
}

impl ArtiTorClient {
    pub fn new(config: ArtiTorClientConfig) -> Result<Self, tor_provider::Error> {
        let pending_log_lines: Arc<Mutex<Vec<String>>> = Default::default();

        let (daemon, rpc_conn) = match &config {
            ArtiTorClientConfig::BundledArti {
                arti_bin_path,
                data_directory,
            } => {

                // launch arti
                let daemon =
                    ArtiProcess::new(arti_bin_path.as_path(), data_directory.as_path(), Arc::downgrade(&pending_log_lines))
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
            pending_log_lines,
            pending_events,
            bootstrapped: false,
            circuit_token_counter: 0,
            circuit_tokens: Default::default(),
        })
    }
}

impl TorProvider for ArtiTorClient {
    fn update(&mut self) -> Result<Vec<TorEvent>, tor_provider::Error> {
        std::thread::sleep(std::time::Duration::from_millis(16));
        let mut tor_events = match self.pending_events.lock() {
            Ok(mut pending_events) => std::mem::take(pending_events.deref_mut()),
            Err(_) => {
                unreachable!("another thread panicked while holding this pending_events mutex")
            }
        };
        // take our log lines
        let mut log_lines = match self.pending_log_lines.lock() {
            Ok(mut pending_log_lines) => std::mem::take(pending_log_lines.deref_mut()),
            Err(_) => {
                unreachable!("another thread panicked while holding this pending_log_lines mutex")
            }
        };

        // append raw lines as TorEvent
        for log_line in log_lines.iter_mut() {
            tor_events.push(TorEvent::LogReceived {
                line: std::mem::take(log_line),
            });
        }

        Ok(tor_events)
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
        target: TargetAddr,
        circuit_token: Option<CircuitToken>,
    ) -> Result<OnionStream, tor_provider::Error> {
        if !self.bootstrapped {
            return Err(Error::ArtiNotBootstrapped().into());
        }

        // convert TargetAddr to (String, u16) tuple
        let (host, port) = match &target {
            TargetAddr::Socket(socket_addr) => (format!("{:?}", socket_addr.ip()), socket_addr.port()),
            TargetAddr::OnionService(OnionAddr::V3(onion_addr)) => (format!("{}.onion", onion_addr.service_id()), onion_addr.virt_port()),
            TargetAddr::Domain(domain_addr) => (domain_addr.domain().to_string(), domain_addr.port()),
        };

        // map circuit_token to isolation string for arti
        let isolation = if let Some(circuit_token) = circuit_token {
            if let Some(isolation) = self.circuit_tokens.get(&circuit_token) {
                isolation.as_str()
            } else {
                return Err(Error::CircuitTokenInvalid(circuit_token))?;
            }
        } else {
            ""
        };

        // connect to target
        let stream = self.rpc_conn.open_stream(None, (host.as_str(), port), isolation)
            .map_err(Error::ArtiOpenStreamFailed)?;

        Ok(OnionStream {
            stream,
            local_addr: None,
            peer_addr: Some(target),
        })
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
        const ISOLATION_TOKEN_LEN: usize = 32;
        let new_token = self.circuit_token_counter;
        self.circuit_token_counter += 1;
        self.circuit_tokens.insert(
            new_token,
            generate_password(ISOLATION_TOKEN_LEN));

        new_token
    }

    fn release_token(&mut self, token: CircuitToken) {
        self.circuit_tokens.remove(&token);
    }
}
