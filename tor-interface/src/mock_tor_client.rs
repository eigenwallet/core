// standard
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{atomic, Arc, Mutex};

// internal crates
use crate::tor_crypto::*;
use crate::tor_provider;
use crate::tor_provider::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("client not bootstrapped")]
    ClientNotBootstrapped(),

    #[error("client already bootstrapped")]
    ClientAlreadyBootstrapped(),

    #[error("onion service not found: {}", .0)]
    OnionServiceNotFound(OnionAddr),

    #[error("onion service not published: {}", .0)]
    OnionServiceNoPublished(OnionAddr),

    #[error("onion service requires onion auth")]
    OnionServiceRequiresOnionAuth(),

    #[error("provided onion auth key invalid")]
    OnionServiceAuthInvalid(),

    #[error("unable to bind TCP listener")]
    TcpListenerBindFailed(#[source] std::io::Error),

    #[error("unable to get TCP listener's local adress")]
    TcpListenerLocalAddrFailed(#[source] std::io::Error),
}

impl From<Error> for crate::tor_provider::Error {
    fn from(error: Error) -> Self {
        crate::tor_provider::Error::Generic(error.to_string())
    }
}

pub struct MockOnionListener {
    listener: std::net::TcpListener,
    is_active: Arc<atomic::AtomicBool>,
    onion_addr: OnionAddr,
}

impl OnionListenerImpl for MockOnionListener {
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

impl Drop for MockOnionListener {
    fn drop(&mut self) {
        self.is_active.store(false, atomic::Ordering::Relaxed);
    }
}

struct MockTorNetwork {
    onion_services: BTreeMap<OnionAddr, (Vec<X25519PublicKey>, SocketAddr)>,
}

impl MockTorNetwork {
    const fn new() -> MockTorNetwork {
        MockTorNetwork {
            onion_services: BTreeMap::new(),
        }
    }

    fn connect_to_onion(
        &mut self,
        service_id: &V3OnionServiceId,
        virt_port: u16,
        client_auth: Option<&X25519PublicKey>,
    ) -> Result<OnionStream, Error> {
        let onion_addr = OnionAddr::V3(OnionAddrV3::new(service_id.clone(), virt_port));
        if let Some((client_auth_keys, socket_addr)) = self.onion_services.get(&onion_addr) {
            match (client_auth_keys.len(), client_auth) {
                (0, None) => (),
                (_, None) => return Err(Error::OnionServiceRequiresOnionAuth()),
                (0, Some(_)) => return Err(Error::OnionServiceAuthInvalid()),
                (_, Some(client_auth)) => {
                    if !client_auth_keys.contains(client_auth) {
                        return Err(Error::OnionServiceAuthInvalid());
                    }
                }
            }

            if let Ok(stream) = TcpStream::connect(socket_addr) {
                Ok(OnionStream {
                    stream,
                    local_addr: None,
                    peer_addr: Some(TargetAddr::OnionService(onion_addr)),
                })
            } else {
                Err(Error::OnionServiceNotFound(onion_addr))
            }
        } else {
            Err(Error::OnionServiceNoPublished(onion_addr))
        }
    }

    fn start_onion(
        &mut self,
        service_id: V3OnionServiceId,
        virt_port: u16,
        client_auth_keys: Vec<X25519PublicKey>,
        address: SocketAddr,
    ) {
        let onion_addr = OnionAddr::V3(OnionAddrV3::new(service_id, virt_port));
        self.onion_services
            .insert(onion_addr, (client_auth_keys, address));
    }

    fn stop_onion(&mut self, onion_addr: &OnionAddr) {
        self.onion_services.remove(onion_addr);
    }
}

static MOCK_TOR_NETWORK: Mutex<MockTorNetwork> = Mutex::new(MockTorNetwork::new());

pub struct MockTorClient {
    events: Vec<TorEvent>,
    bootstrapped: bool,
    client_auth_keys: BTreeMap<V3OnionServiceId, X25519PublicKey>,
    onion_services: Vec<(OnionAddr, Arc<atomic::AtomicBool>)>,
}

impl MockTorClient {
    pub fn new() -> MockTorClient {
        let mut events: Vec<TorEvent> = Default::default();
        let line = "[notice] MockTorClient running".to_string();
        events.push(TorEvent::LogReceived { line });

        MockTorClient {
            events,
            bootstrapped: false,
            client_auth_keys: Default::default(),
            onion_services: Default::default(),
        }
    }
}

impl Default for MockTorClient {
    fn default() -> Self {
        Self::new()
    }
}

impl TorProvider for MockTorClient {
    fn update(&mut self) -> Result<Vec<TorEvent>, tor_provider::Error> {
        match MOCK_TOR_NETWORK.lock() {
            Ok(mut mock_tor_network) => {
                let mut i = 0;
                while i < self.onion_services.len() {
                    // remove onion services with no active listeners
                    if !self.onion_services[i].1.load(atomic::Ordering::Relaxed) {
                        let entry = self.onion_services.swap_remove(i);
                        let onion_addr = entry.0;
                        mock_tor_network.stop_onion(&onion_addr);
                    } else {
                        i += 1;
                    }
                }
            }
            Err(_) => unreachable!("another thread panicked while holding mock tor network's lock"),
        }

        Ok(std::mem::take(&mut self.events))
    }

    fn bootstrap(&mut self) -> Result<(), tor_provider::Error> {
        if self.bootstrapped {
            Err(Error::ClientAlreadyBootstrapped())?
        } else {
            self.events.push(TorEvent::BootstrapStatus {
                progress: 0u32,
                tag: "start".to_string(),
                summary: "bootstrapping started".to_string(),
            });
            self.events.push(TorEvent::BootstrapStatus {
                progress: 50u32,
                tag: "middle".to_string(),
                summary: "bootstrapping continues".to_string(),
            });
            self.events.push(TorEvent::BootstrapStatus {
                progress: 100u32,
                tag: "finished".to_string(),
                summary: "bootstrapping completed".to_string(),
            });
            self.events.push(TorEvent::BootstrapComplete);
            self.bootstrapped = true;
            Ok(())
        }
    }

    fn add_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
        client_auth: &X25519PrivateKey,
    ) -> Result<(), tor_provider::Error> {
        let client_auth_public = X25519PublicKey::from_private_key(client_auth);
        if let Some(key) = self.client_auth_keys.get_mut(service_id) {
            *key = client_auth_public;
        } else {
            self.client_auth_keys
                .insert(service_id.clone(), client_auth_public);
        }
        Ok(())
    }

    fn remove_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
    ) -> Result<(), tor_provider::Error> {
        self.client_auth_keys.remove(service_id);
        Ok(())
    }

    fn connect(
        &mut self,
        service_id: &V3OnionServiceId,
        virt_port: u16,
        _circuit: Option<CircuitToken>,
    ) -> Result<OnionStream, tor_provider::Error> {
        let client_auth = self.client_auth_keys.get(service_id);

        match MOCK_TOR_NETWORK.lock() {
            Ok(mut mock_tor_network) => {
                Ok(mock_tor_network.connect_to_onion(service_id, virt_port, client_auth)?)
            }
            Err(_) => unreachable!("another thread panicked while holding mock tor network's lock"),
        }
    }

    fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorized_clients: Option<&[X25519PublicKey]>,
    ) -> Result<OnionListener, tor_provider::Error> {
        // convert inputs to relevant types
        let service_id = V3OnionServiceId::from_private_key(private_key);
        let onion_addr = OnionAddr::V3(OnionAddrV3::new(service_id.clone(), virt_port));
        let authorized_clients: Vec<X25519PublicKey> = match authorized_clients {
            Some(keys) => keys.into(),
            None => Default::default(),
        };

        // try to bind to a local address, let OS pick our port
        let socket_addr = SocketAddr::from(([127, 0, 0, 1], 0u16));
        let listener = TcpListener::bind(socket_addr).map_err(Error::TcpListenerBindFailed)?;
        let socket_addr = listener
            .local_addr()
            .map_err(Error::TcpListenerLocalAddrFailed)?;

        // register the onion service with the mock tor network
        match MOCK_TOR_NETWORK.lock() {
            Ok(mut mock_tor_network) => mock_tor_network.start_onion(
                service_id.clone(),
                virt_port,
                authorized_clients,
                socket_addr,
            ),
            Err(_) => unreachable!("another thread panicked while holding mock tor network's lock"),
        }

        // init flag for signaling when listener goes out of scope so we can tear down onion service
        let is_active = Arc::new(atomic::AtomicBool::new(true));
        self.onion_services
            .push((onion_addr.clone(), Arc::clone(&is_active)));

        // onion service published event
        self.events
            .push(TorEvent::OnionServicePublished { service_id });

        let onion_listener = Box::new(MockOnionListener {
            listener,
            is_active,
            onion_addr,
        });

        Ok(OnionListener { onion_listener })
    }

    fn generate_token(&mut self) -> CircuitToken {
        0usize
    }

    fn release_token(&mut self, _token: CircuitToken) {}
}

impl Drop for MockTorClient {
    fn drop(&mut self) {
        // remove all our onion services
        match MOCK_TOR_NETWORK.lock() {
            Ok(mut mock_tor_network) => {
                for entry in self.onion_services.iter() {
                    let onion_addr = &entry.0;
                    mock_tor_network.stop_onion(onion_addr);
                }
            }
            Err(_) => unreachable!("another thread panicked while holding mock tor network's lock"),
        }
    }
}
