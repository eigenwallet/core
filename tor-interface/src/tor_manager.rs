// standard
use std::default::Default;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::ops::Drop;
use std::option::Option;
use std::path::Path;
use std::string::ToString;
use std::sync::{atomic, Arc};
use std::time::Duration;

// extern crates
#[cfg(test)]
use serial_test::serial;
use socks::Socks5Stream;
use url::Host;

// internal crates
use crate::tor_control_stream::*;
use crate::tor_controller::*;
use crate::tor_crypto::*;
use crate::tor_process::*;
use crate::tor_version::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed to create TorProcess object")]
    TorProcessCreationFailed(#[source] crate::tor_process::Error),

    #[error("failed to create ControlStream object")]
    ControlStreamCreationFailed(#[source] crate::tor_control_stream::Error),

    #[error("failed to create TorController object")]
    TorControllerCreationFailed(#[source] crate::tor_controller::Error),

    #[error("failed to authenticate with the tor process")]
    TorProcessAuthenticationFailed(#[source] crate::tor_controller::Error),

    #[error("failed to determine the tor process version")]
    GetInfoVersionFailed(#[source] crate::tor_controller::Error),

    #[error("tor process version to old; found {0} but must be at least {1}")]
    TorProcessTooOld(String, String),

    #[error("failed to register for STATUS_CLIENT and HS_DESC events")]
    SetEventsFailed(#[source] crate::tor_controller::Error),

    #[error("failed to delete unused onion service")]
    DelOnionFailed(#[source] crate::tor_controller::Error),

    #[error("failed waiting for async events")]
    WaitAsyncEventsFailed(#[source] crate::tor_controller::Error),

    #[error("failed to begin bootstrap")]
    SetConfDisableNetwork0Failed(#[source] crate::tor_controller::Error),

    #[error("failed to add client auth for onion service")]
    OnionClientAuthAddFailed(#[source] crate::tor_controller::Error),

    #[error("failed to remove client auth from onion service")]
    OnionClientAuthRemoveFailed(#[source] crate::tor_controller::Error),

    #[error("failed to get socks listener")]
    GetInfoNetListenersSocksFailed(#[source] crate::tor_controller::Error),

    #[error("no socks listeners available to connect through")]
    NoSocksListenersFound(),

    #[error("unable to connect to socks listener")]
    Socks5ConnectionFailed(#[source] std::io::Error),

    #[error("unable to bind TCP listener")]
    TcpListenerBindFailed(#[source] std::io::Error),

    #[error("unable to get TCP listener's local address")]
    TcpListenerLocalAddrFailed(#[source] std::io::Error),

    #[error("failed to create onion service")]
    AddOnionFailed(#[source] crate::tor_controller::Error),
}

pub struct CircuitToken {
    username: String,
    password: String,
}

impl CircuitToken {
    #[allow(dead_code)]
    pub fn new(first_party: Host) -> CircuitToken {
        const CIRCUIT_TOKEN_PASSWORD_LENGTH: usize = 32usize;
        let username = first_party.to_string();
        let password = generate_password(CIRCUIT_TOKEN_PASSWORD_LENGTH);

        CircuitToken { username, password }
    }
}

pub struct OnionStream {
    stream: TcpStream,
    peer_addr: Option<V3OnionServiceId>,
}

impl OnionStream {
    pub fn nodelay(&self) -> Result<bool, std::io::Error> {
        self.stream.nodelay()
    }

    pub fn peer_addr(&self) -> Option<&V3OnionServiceId> {
        self.peer_addr.as_ref()
    }

    pub fn read_timeout(&self) -> Result<Option<Duration>, std::io::Error> {
        self.stream.read_timeout()
    }

    pub fn set_nodelay(&self, nodelay: bool) -> Result<(), std::io::Error> {
        self.stream.set_nodelay(nodelay)
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<(), std::io::Error> {
        self.stream.set_nonblocking(nonblocking)
    }

    pub fn set_read_timeout(&self, dur: Option<Duration>) -> Result<(), std::io::Error> {
        self.stream.set_read_timeout(dur)
    }

    pub fn set_write_timeout(&self, dur: Option<Duration>) -> Result<(), std::io::Error> {
        self.stream.set_write_timeout(dur)
    }

    pub fn shutdown(&self, how: std::net::Shutdown) -> Result<(), std::io::Error> {
        self.stream.shutdown(how)
    }

    pub fn take_error(&self) -> Result<Option<std::io::Error>, std::io::Error> {
        self.stream.take_error()
    }

    pub fn write_timeout(&self) -> Result<Option<Duration>, std::io::Error> {
        self.stream.write_timeout()
    }

    pub fn try_clone(&self) -> Result<OnionStream, std::io::Error> {
        Ok(OnionStream {
            stream: self.stream.try_clone()?,
            peer_addr: self.peer_addr.clone(),
        })
    }
}

// pass-through to underlying Read stream
impl Read for OnionStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        self.stream.read(buf)
    }
}

// pass-through to underlying Write stream
impl Write for OnionStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        self.stream.flush()
    }
}

impl From<OnionStream> for TcpStream {
    fn from(onion_stream: OnionStream) -> Self {
        onion_stream.stream
    }
}

pub struct OnionListener {
    listener: TcpListener,
    is_active: Arc<atomic::AtomicBool>,
}

impl OnionListener {
    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<(), std::io::Error> {
        self.listener.set_nonblocking(nonblocking)
    }

    pub fn accept(&self) -> Result<Option<OnionStream>, std::io::Error> {
        match self.listener.accept() {
            Ok((stream, _socket_addr)) => Ok(Some(OnionStream {
                stream,
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

impl Drop for OnionListener {
    fn drop(&mut self) {
        self.is_active.store(false, atomic::Ordering::Relaxed);
    }
}

pub enum Event {
    BootstrapStatus {
        progress: u32,
        tag: String,
        summary: String,
    },
    BootstrapComplete,
    LogReceived {
        line: String,
    },
    OnionServicePublished {
        service_id: V3OnionServiceId,
    },
}

pub struct TorManager {
    daemon: TorProcess,
    version: TorVersion,
    controller: TorController,
    socks_listener: Option<SocketAddr>,
    // list of open onion services and their is_active flag
    onion_services: Vec<(V3OnionServiceId, Arc<atomic::AtomicBool>)>,
}

impl TorManager {
    pub fn new(tor_bin_path: &Path, data_directory: &Path) -> Result<TorManager, Error> {
        // launch tor
        let daemon = TorProcess::new(tor_bin_path, data_directory)
            .map_err(Error::TorProcessCreationFailed)?;
        // open a control stream
        let control_stream =
            ControlStream::new(daemon.get_control_addr(), Duration::from_millis(16))
                .map_err(Error::ControlStreamCreationFailed)?;

        // create a controler
        let mut controller =
            TorController::new(control_stream).map_err(Error::TorControllerCreationFailed)?;

        // authenticate
        controller
            .authenticate(daemon.get_password())
            .map_err(Error::TorProcessAuthenticationFailed)?;

        let min_required_version: TorVersion = TorVersion {
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
            return Err(Error::TorProcessTooOld(
                version.to_string(),
                min_required_version.to_string(),
            ));
        }

        // register for STATUS_CLIENT async events
        controller
            .setevents(&["STATUS_CLIENT", "HS_DESC"])
            .map_err(Error::SetEventsFailed)?;

        Ok(TorManager {
            daemon,
            version,
            controller,
            socks_listener: None,
            onion_services: Default::default(),
        })
    }

    pub fn update(&mut self) -> Result<Vec<Event>, Error> {
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

        let mut events: Vec<Event> = Default::default();
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
                        events.push(Event::BootstrapStatus {
                            progress,
                            tag,
                            summary,
                        });
                        if progress == 100u32 {
                            events.push(Event::BootstrapComplete);
                        }
                    }
                }
                AsyncEvent::HsDesc { action, hs_address } => {
                    if action == "UPLOADED" {
                        events.push(Event::OnionServicePublished {
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
            events.push(Event::LogReceived {
                line: std::mem::take(log_line),
            });
        }

        Ok(events)
    }

    #[allow(dead_code)]
    pub fn version(&mut self) -> TorVersion {
        self.version.clone()
    }

    pub fn bootstrap(&mut self) -> Result<(), Error> {
        self.controller
            .setconf(&[("DisableNetwork", "0")])
            .map_err(Error::SetConfDisableNetwork0Failed)
    }

    pub fn add_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
        client_auth: &X25519PrivateKey,
    ) -> Result<(), Error> {
        self.controller
            .onion_client_auth_add(service_id, client_auth, None, &Default::default())
            .map_err(Error::OnionClientAuthAddFailed)
    }

    pub fn remove_client_auth(&mut self, service_id: &V3OnionServiceId) -> Result<(), Error> {
        self.controller
            .onion_client_auth_remove(service_id)
            .map_err(Error::OnionClientAuthRemoveFailed)
    }

    // connect to an onion service and returns OnionStream
    pub fn connect(
        &mut self,
        service_id: &V3OnionServiceId,
        virt_port: u16,
        circuit: Option<CircuitToken>,
    ) -> Result<OnionStream, Error> {
        if self.socks_listener.is_none() {
            let mut listeners = self
                .controller
                .getinfo_net_listeners_socks()
                .map_err(Error::GetInfoNetListenersSocksFailed)?;
            if listeners.is_empty() {
                return Err(Error::NoSocksListenersFound());
            }
            self.socks_listener = Some(listeners.swap_remove(0));
        }

        let socks_listener = match self.socks_listener {
            Some(socks_listener) => socks_listener,
            None => unreachable!(),
        };

        // our onion domain
        let target =
            socks::TargetAddr::Domain(format!("{}.onion", service_id.to_string()), virt_port);
        // readwrite stream
        let stream = match &circuit {
            None => Socks5Stream::connect(socks_listener, target),
            Some(circuit) => Socks5Stream::connect_with_password(
                socks_listener,
                target,
                &circuit.username,
                &circuit.password,
            ),
        }
        .map_err(Error::Socks5ConnectionFailed)?;

        Ok(OnionStream {
            stream: stream.into_inner(),
            peer_addr: Some(service_id.clone()),
        })
    }

    // stand up an onion service and return an OnionListener
    pub fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorized_clients: Option<&[X25519PublicKey]>,
    ) -> Result<OnionListener, Error> {
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

        Ok(OnionListener {
            listener,
            is_active,
        })
    }
}

#[test]
#[serial]
fn test_tor_manager() -> anyhow::Result<()> {
    let tor_path = which::which(tor_exe_name())?;
    let mut data_path = std::env::temp_dir();
    data_path.push("test_tor_manager");

    let mut tor = TorManager::new(&tor_path, &data_path)?;
    println!("version : {}", tor.version().to_string());
    tor.bootstrap()?;

    let mut received_log = false;
    let mut bootstrap_complete = false;
    while !bootstrap_complete {
        for event in tor.update()?.iter() {
            match event {
                Event::BootstrapStatus {
                    progress,
                    tag,
                    summary,
                } => println!(
                    "BootstrapStatus: {{ progress: {}, tag: {}, summary: '{}' }}",
                    progress, tag, summary
                ),
                Event::BootstrapComplete => {
                    println!("Bootstrap Complete!");
                    bootstrap_complete = true;
                }
                Event::LogReceived { line } => {
                    received_log = true;
                    println!("--- {}", line);
                }
                _ => {}
            }
        }
    }
    assert!(
        received_log,
        "should have received a log line from tor daemon"
    );

    Ok(())
}

#[test]
#[serial]
fn test_onion_service() -> anyhow::Result<()> {
    let tor_path = which::which(tor_exe_name())?;
    let mut data_path = std::env::temp_dir();
    data_path.push("test_onion_service");

    let mut tor = TorManager::new(&tor_path, &data_path)?;

    // for 30secs for bootstrap
    tor.bootstrap()?;

    let mut bootstrap_complete = false;
    while !bootstrap_complete {
        for event in tor.update()?.iter() {
            match event {
                Event::BootstrapStatus {
                    progress,
                    tag,
                    summary,
                } => println!(
                    "BootstrapStatus: {{ progress: {}, tag: {}, summary: '{}' }}",
                    progress, tag, summary
                ),
                Event::BootstrapComplete => {
                    println!("Bootstrap Complete!");
                    bootstrap_complete = true;
                }
                Event::LogReceived { line } => {
                    println!("--- {}", line);
                }
                _ => {}
            }
        }
    }

    // vanilla V3 onion service
    {
        // create an onion service for this test
        let private_key = Ed25519PrivateKey::generate();

        println!("Starting and listening to onion service");
        const VIRT_PORT: u16 = 42069u16;
        let listener = tor.listener(&private_key, VIRT_PORT, None)?;

        let mut onion_published = false;
        while !onion_published {
            for event in tor.update()?.iter() {
                match event {
                    Event::LogReceived { line } => {
                        println!("--- {}", line);
                    }
                    Event::OnionServicePublished { service_id } => {
                        let expected_service_id = V3OnionServiceId::from_private_key(&private_key);
                        if expected_service_id == *service_id {
                            println!("Onion Service {} published", service_id.to_string());
                            onion_published = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        const MESSAGE: &str = "Hello World!";

        {
            let service_id = V3OnionServiceId::from_private_key(&private_key);

            println!("Connecting to onion service");
            let mut client = tor.connect(&service_id, VIRT_PORT, None)?;
            println!("Client writing message: '{}'", MESSAGE);
            client.write_all(MESSAGE.as_bytes())?;
            client.flush()?;
            println!("End of client scope");
        }

        if let Some(mut server) = listener.accept()? {
            println!("Server reading message");
            let mut buffer = Vec::new();
            server.read_to_end(&mut buffer)?;
            let msg = String::from_utf8(buffer)?;

            assert!(MESSAGE == msg);
            println!("Message received: '{}'", msg);
        } else {
            panic!("no listener");
        }
    }

    // authenticated onion service
    {
        // create an onion service for this test
        let private_key = Ed25519PrivateKey::generate();

        let private_auth_key = X25519PrivateKey::generate();
        let public_auth_key = X25519PublicKey::from_private_key(&private_auth_key);

        println!("Starting and listening to authenticated onion service");
        const VIRT_PORT: u16 = 42069u16;
        let listener = tor.listener(&private_key, VIRT_PORT, Some(&[public_auth_key]))?;

        let mut onion_published = false;
        while !onion_published {
            for event in tor.update()?.iter() {
                match event {
                    Event::LogReceived { line } => {
                        println!("--- {}", line);
                    }
                    Event::OnionServicePublished { service_id } => {
                        let expected_service_id = V3OnionServiceId::from_private_key(&private_key);
                        if expected_service_id == *service_id {
                            println!(
                                "Authenticated Onion Service {} published",
                                service_id.to_string()
                            );
                            onion_published = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        const MESSAGE: &str = "Hello World!";

        {
            let service_id = V3OnionServiceId::from_private_key(&private_key);

            println!("Connecting to onion service (should fail)");
            assert!(
                tor.connect(&service_id, VIRT_PORT, None).is_err(),
                "should not able to connect to an authenticated onion service without auth key"
            );

            println!("Add auth key for onion service");
            tor.add_client_auth(&service_id, &private_auth_key)?;

            println!("Connecting to onion service with authentication");
            let mut client = tor.connect(&service_id, VIRT_PORT, None)?;

            println!("Client writing message: '{}'", MESSAGE);
            client.write_all(MESSAGE.as_bytes())?;
            client.flush()?;
            println!("End of client scope");

            println!("Remove auth key for onion service");
            tor.remove_client_auth(&service_id)?;
        }

        if let Some(mut server) = listener.accept()? {
            println!("Server reading message");
            let mut buffer = Vec::new();
            server.read_to_end(&mut buffer)?;
            let msg = String::from_utf8(buffer)?;

            assert!(MESSAGE == msg);
            println!("Message received: '{}'", msg);
        } else {
            panic!("no listener");
        }
    }
    Ok(())
}
