// standard
use std::boxed::Box;
use std::collections::BTreeMap;
use std::convert::From;
use std::default::Default;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener};
use std::ops::Drop;
use std::option::Option;
use std::path::PathBuf;
use std::string::ToString;
use std::sync::{atomic, Arc};
use std::time::Duration;

// extern crates
use socks::Socks5Stream;

// internal crates
use crate::legacy_tor_control_stream::*;
use crate::legacy_tor_controller::*;
use crate::legacy_tor_process::*;
use crate::legacy_tor_version::*;
use crate::tor_crypto::*;
use crate::tor_provider;
use crate::tor_provider::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed to create LegacyTorProcess object")]
    LegacyTorProcessCreationFailed(#[source] crate::legacy_tor_process::Error),

    #[error("failed to create LegacyControlStream object")]
    LegacyControlStreamCreationFailed(#[source] crate::legacy_tor_control_stream::Error),

    #[error("failed to create LegacyTorController object")]
    LegacyTorControllerCreationFailed(#[source] crate::legacy_tor_controller::Error),

    #[error("failed to authenticate with the tor process")]
    LegacyTorProcessAuthenticationFailed(#[source] crate::legacy_tor_controller::Error),

    #[error("failed to determine the tor process version")]
    GetInfoVersionFailed(#[source] crate::legacy_tor_controller::Error),

    #[error("tor process version to old; found {0} but must be at least {1}")]
    LegacyTorProcessTooOld(String, String),

    #[error("failed to register for STATUS_CLIENT and HS_DESC events")]
    SetEventsFailed(#[source] crate::legacy_tor_controller::Error),

    #[error("failed to delete unused onion service")]
    DelOnionFailed(#[source] crate::legacy_tor_controller::Error),

    #[error("failed waiting for async events")]
    WaitAsyncEventsFailed(#[source] crate::legacy_tor_controller::Error),

    #[error("failed to begin bootstrap")]
    SetConfDisableNetwork0Failed(#[source] crate::legacy_tor_controller::Error),

    #[error("failed to add client auth for onion service")]
    OnionClientAuthAddFailed(#[source] crate::legacy_tor_controller::Error),

    #[error("failed to remove client auth from onion service")]
    OnionClientAuthRemoveFailed(#[source] crate::legacy_tor_controller::Error),

    #[error("failed to get socks listener")]
    GetInfoNetListenersSocksFailed(#[source] crate::legacy_tor_controller::Error),

    #[error("no socks listeners available to connect through")]
    NoSocksListenersFound(),

    #[error("invalid circuit token")]
    CircuitTokenInvalid(),

    #[error("unable to connect to socks listener")]
    Socks5ConnectionFailed(#[source] std::io::Error),

    #[error("unable to bind TCP listener")]
    TcpListenerBindFailed(#[source] std::io::Error),

    #[error("unable to get TCP listener's local address")]
    TcpListenerLocalAddrFailed(#[source] std::io::Error),

    #[error("failed to create onion service")]
    AddOnionFailed(#[source] crate::legacy_tor_controller::Error),

    #[error("not implemented")]
    NotImplemented(),
}

impl From<Error> for crate::tor_provider::Error {
    fn from(error: Error) -> Self {
        crate::tor_provider::Error::Generic(error.to_string())
    }
}

//
// CircuitToken Implementation
//
struct LegacyCircuitToken {
    username: String,
    password: String,
}

impl LegacyCircuitToken {
    fn new() -> LegacyCircuitToken {
        const CIRCUIT_TOKEN_USERNAME_LENGTH: usize = 32usize;
        const CIRCUIT_TOKEN_PASSWORD_LENGTH: usize = 32usize;
        let username = generate_password(CIRCUIT_TOKEN_USERNAME_LENGTH);
        let password = generate_password(CIRCUIT_TOKEN_PASSWORD_LENGTH);

        LegacyCircuitToken { username, password }
    }
}

impl Default for LegacyCircuitToken {
    fn default() -> Self {
        Self::new()
    }
}

//
// LegacyOnionListener
//

pub struct LegacyOnionListener {
    listener: TcpListener,
    is_active: Arc<atomic::AtomicBool>,
    onion_addr: OnionAddr,
}

impl OnionListenerImpl for LegacyOnionListener {
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

impl Drop for LegacyOnionListener {
    fn drop(&mut self) {
        self.is_active.store(false, atomic::Ordering::Relaxed);
    }
}

//
// LegacyTorClientConfig
//

#[derive(Clone)]
pub enum LegacyTorClientConfig {
    SystemTor {
        tor_socks_addr: SocketAddr,
        tor_control_addr: SocketAddr,
        tor_control_passwd: String,
    },
    BundledTor {
        tor_bin_path: PathBuf,
        data_directory: PathBuf,
    },
}

//
// LegacyTorClient
//

pub struct LegacyTorClient {
    daemon: LegacyTorProcess,
    version: LegacyTorVersion,
    controller: LegacyTorController,
    socks_listener: Option<SocketAddr>,
    // list of open onion services and their is_active flag
    onion_services: Vec<(V3OnionServiceId, Arc<atomic::AtomicBool>)>,
    // our list of circuit tokens for the tor daemon
    circuit_token_counter: usize,
    circuit_tokens: BTreeMap<CircuitToken, LegacyCircuitToken>,
}

impl LegacyTorClient {
    pub fn new(config: LegacyTorClientConfig) -> Result<LegacyTorClient, Error> {
        match config {
            LegacyTorClientConfig::BundledTor{tor_bin_path, data_directory} => {
                // launch tor
                let daemon = LegacyTorProcess::new(tor_bin_path.as_path(), data_directory.as_path())
                    .map_err(Error::LegacyTorProcessCreationFailed)?;
                // open a control stream
                let control_stream =
                    LegacyControlStream::new(daemon.get_control_addr(), Duration::from_millis(16))
                        .map_err(Error::LegacyControlStreamCreationFailed)?;

                // create a controler
                let mut controller = LegacyTorController::new(control_stream)
                    .map_err(Error::LegacyTorControllerCreationFailed)?;

                // authenticate
                controller
                    .authenticate(daemon.get_password())
                    .map_err(Error::LegacyTorProcessAuthenticationFailed)?;

                // min required version for v3 client auth (see control-spec.txt)
                let min_required_version = LegacyTorVersion {
                    major: 0u32,
                    minor: 4u32,
                    micro: 6u32,
                    patch_level: 1u32,
                    status_tag: None,
                };

                let version = controller
                    .getinfo_version()
                    .map_err(Error::GetInfoVersionFailed)?;

                if version < min_required_version {
                    return Err(Error::LegacyTorProcessTooOld(
                        version.to_string(),
                        min_required_version.to_string(),
                    ));
                }

                // register for STATUS_CLIENT async events
                controller
                    .setevents(&["STATUS_CLIENT", "HS_DESC"])
                    .map_err(Error::SetEventsFailed)?;

                Ok(LegacyTorClient {
                    daemon,
                    version,
                    controller,
                    socks_listener: None,
                    onion_services: Default::default(),
                    circuit_token_counter: 0usize,
                    circuit_tokens: Default::default(),
                })
            },
            LegacyTorClientConfig::SystemTor{..} => Err(Error::NotImplemented())
        }
    }

    #[allow(dead_code)]
    pub fn version(&mut self) -> LegacyTorVersion {
        self.version.clone()
    }
}

impl TorProvider for LegacyTorClient {
    fn update(&mut self) -> Result<Vec<TorEvent>, tor_provider::Error> {
        let mut i = 0;
        while i < self.onion_services.len() {
            // remove onion services with no active listeners
            if !self.onion_services[i].1.load(atomic::Ordering::Relaxed) {
                let entry = self.onion_services.swap_remove(i);
                let service_id = entry.0;

                self.controller
                    .del_onion(&service_id)
                    .map_err(Error::DelOnionFailed)?;
            } else {
                i += 1;
            }
        }

        let mut events: Vec<TorEvent> = Default::default();
        for async_event in self
            .controller
            .wait_async_events()
            .map_err(Error::WaitAsyncEventsFailed)?
            .iter()
        {
            match async_event {
                AsyncEvent::StatusClient {
                    severity,
                    action,
                    arguments,
                } => {
                    if severity == "NOTICE" && action == "BOOTSTRAP" {
                        let mut progress: u32 = 0;
                        let mut tag: String = Default::default();
                        let mut summary: String = Default::default();
                        for (key, val) in arguments.iter() {
                            match key.as_str() {
                                "PROGRESS" => progress = val.parse().unwrap_or(0u32),
                                "TAG" => tag = val.to_string(),
                                "SUMMARY" => summary = val.to_string(),
                                _ => {} // ignore unexpected arguments
                            }
                        }
                        events.push(TorEvent::BootstrapStatus {
                            progress,
                            tag,
                            summary,
                        });
                        if progress == 100u32 {
                            events.push(TorEvent::BootstrapComplete);
                        }
                    }
                }
                AsyncEvent::HsDesc { action, hs_address } => {
                    if action == "UPLOADED" {
                        events.push(TorEvent::OnionServicePublished {
                            service_id: hs_address.clone(),
                        });
                    }
                }
                AsyncEvent::Unknown { lines } => {
                    println!("Received Unknown Event:");
                    for line in lines.iter() {
                        println!(" {}", line);
                    }
                }
            }
        }

        for log_line in self.daemon.wait_log_lines().iter_mut() {
            events.push(TorEvent::LogReceived {
                line: std::mem::take(log_line),
            });
        }

        Ok(events)
    }

    fn bootstrap(&mut self) -> Result<(), tor_provider::Error> {
        Ok(self
            .controller
            .setconf(&[("DisableNetwork", "0")])
            .map_err(Error::SetConfDisableNetwork0Failed)?)
    }

    fn add_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
        client_auth: &X25519PrivateKey,
    ) -> Result<(), tor_provider::Error> {
        Ok(self
            .controller
            .onion_client_auth_add(service_id, client_auth, None, &Default::default())
            .map_err(Error::OnionClientAuthAddFailed)?)
    }

    fn remove_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
    ) -> Result<(), tor_provider::Error> {
        Ok(self
            .controller
            .onion_client_auth_remove(service_id)
            .map_err(Error::OnionClientAuthRemoveFailed)?)
    }

    // connect to an onion service and returns OnionStream
    fn connect(
        &mut self,
        target: TargetAddr,
        circuit: Option<CircuitToken>,
    ) -> Result<OnionStream, tor_provider::Error> {
        if self.socks_listener.is_none() {
            let mut listeners = self
                .controller
                .getinfo_net_listeners_socks()
                .map_err(Error::GetInfoNetListenersSocksFailed)?;
            if listeners.is_empty() {
                return Err(Error::NoSocksListenersFound())?;
            }
            self.socks_listener = Some(listeners.swap_remove(0));
        }

        let socks_listener = match self.socks_listener {
            Some(socks_listener) => socks_listener,
            None => unreachable!(),
        };

        // our target
        let socks_target = match target.clone() {
            TargetAddr::Ip(socket_addr) => socks::TargetAddr::Ip(socket_addr),
            TargetAddr::Domain(domain_addr) => {
                socks::TargetAddr::Domain(domain_addr.domain().to_string(), domain_addr.port())
            }
            TargetAddr::OnionService(OnionAddr::V3(OnionAddrV3 {
                service_id,
                virt_port,
            })) => socks::TargetAddr::Domain(format!("{}.onion", service_id), virt_port),
        };

        // readwrite stream
        let stream = match &circuit {
            None => Socks5Stream::connect(socks_listener, socks_target),
            Some(circuit) => {
                if let Some(circuit) = self.circuit_tokens.get(circuit) {
                    Socks5Stream::connect_with_password(
                        socks_listener,
                        socks_target,
                        &circuit.username,
                        &circuit.password,
                    )
                } else {
                    return Err(Error::CircuitTokenInvalid())?;
                }
            }
        }
        .map_err(Error::Socks5ConnectionFailed)?;

        Ok(OnionStream {
            stream: stream.into_inner(),
            local_addr: None,
            peer_addr: Some(target),
        })
    }

    // stand up an onion service and return an LegacyOnionListener
    fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorized_clients: Option<&[X25519PublicKey]>,
    ) -> Result<OnionListener, tor_provider::Error> {
        // try to bind to a local address, let OS pick our port
        let socket_addr = SocketAddr::from(([127, 0, 0, 1], 0u16));
        let listener = TcpListener::bind(socket_addr).map_err(Error::TcpListenerBindFailed)?;
        let socket_addr = listener
            .local_addr()
            .map_err(Error::TcpListenerLocalAddrFailed)?;

        let mut flags = AddOnionFlags {
            discard_pk: true,
            ..Default::default()
        };
        if authorized_clients.is_some() {
            flags.v3_auth = true;
        }

        let onion_addr = OnionAddr::V3(OnionAddrV3::new(
            V3OnionServiceId::from_private_key(private_key),
            virt_port,
        ));

        // start onion service
        let (_, service_id) = self
            .controller
            .add_onion(
                Some(private_key),
                &flags,
                None,
                virt_port,
                Some(socket_addr),
                authorized_clients,
            )
            .map_err(Error::AddOnionFailed)?;

        let is_active = Arc::new(atomic::AtomicBool::new(true));
        self.onion_services
            .push((service_id, Arc::clone(&is_active)));

        let onion_listener = Box::new(LegacyOnionListener {
            listener,
            is_active,
            onion_addr,
        });

        Ok(OnionListener { onion_listener })
    }

    fn generate_token(&mut self) -> CircuitToken {
        let new_token = self.circuit_token_counter;
        self.circuit_token_counter += 1;
        self.circuit_tokens
            .insert(new_token, LegacyCircuitToken::new());
        new_token
    }

    fn release_token(&mut self, circuit_token: CircuitToken) {
        self.circuit_tokens.remove(&circuit_token);
    }
}
