// standard
use std::boxed::Box;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::ops::{Deref, DerefMut};

// internal crates
use crate::tor_crypto::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OnionAddrV3 {
    pub(crate) service_id: V3OnionServiceId,
    pub(crate) virt_port: u16,
}

impl OnionAddrV3 {
    pub fn new(service_id: V3OnionServiceId, virt_port: u16) -> OnionAddrV3 {
        OnionAddrV3 {
            service_id,
            virt_port,
        }
    }

    pub fn service_id(&self) -> &V3OnionServiceId {
        &self.service_id
    }

    pub fn virt_port(&self) -> u16 {
        self.virt_port
    }
}

impl std::fmt::Display for OnionAddrV3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.onion:{}", self.service_id, self.virt_port)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OnionAddr {
    V3(OnionAddrV3),
}

impl std::fmt::Display for OnionAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OnionAddr::V3(onion_addr) => onion_addr.fmt(f),
        }
    }
}

#[derive(Clone, Debug)]
pub enum TargetAddr {
    Ip(std::net::SocketAddr),
    Domain(String, u16),
    OnionService(OnionAddr),
}

impl From<(V3OnionServiceId, u16)> for TargetAddr {
    fn from(target_tuple: (V3OnionServiceId, u16)) -> Self {
        TargetAddr::OnionService(OnionAddr::V3(OnionAddrV3::new(target_tuple.0, target_tuple.1)))
    }
}

#[derive(Debug)]
pub enum TorEvent {
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

pub type CircuitToken = usize;

//
// OnionStream Implementation
//

pub struct OnionStream {
    pub(crate) stream: TcpStream,
    pub(crate) local_addr: Option<OnionAddr>,
    pub(crate) peer_addr: Option<TargetAddr>,
}

impl Deref for OnionStream {
    type Target = TcpStream;
    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl DerefMut for OnionStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stream
    }
}

impl From<OnionStream> for TcpStream {
    fn from(onion_stream: OnionStream) -> Self {
        onion_stream.stream
    }
}

impl Read for OnionStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        self.stream.read(buf)
    }
}

impl Write for OnionStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        self.stream.flush()
    }
}

impl OnionStream {
    pub fn peer_addr(&self) -> Option<TargetAddr> {
        self.peer_addr.clone()
    }

    pub fn local_addr(&self) -> Option<OnionAddr> {
        None
    }

    pub fn try_clone(&self) -> Result<Self, std::io::Error> {
        Ok(Self {
            stream: self.stream.try_clone()?,
            local_addr: self.local_addr.clone(),
            peer_addr: self.peer_addr.clone(),
        })
    }
}

pub trait OnionListenerImpl: Send {
    fn set_nonblocking(&self, nonblocking: bool) -> Result<(), std::io::Error>;
    fn accept(&self) -> Result<Option<OnionStream>, std::io::Error>;
}

pub struct OnionListener {
    pub(crate) onion_listener: Box<dyn OnionListenerImpl>,
}

impl OnionListener {
    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<(), std::io::Error> {
        self.onion_listener.set_nonblocking(nonblocking)
    }

    pub fn accept(&self) -> Result<Option<OnionStream>, std::io::Error> {
        self.onion_listener.accept()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Generic(String),
}

pub trait TorProvider: Send {
    fn update(&mut self) -> Result<Vec<TorEvent>, Error>;
    fn bootstrap(&mut self) -> Result<(), Error>;
    fn add_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
        client_auth: &X25519PrivateKey,
    ) -> Result<(), Error>;
    fn remove_client_auth(&mut self, service_id: &V3OnionServiceId) -> Result<(), Error>;
    fn connect(
        &mut self,
        target: TargetAddr,
        circuit: Option<CircuitToken>,
    ) -> Result<OnionStream, Error>;
    fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorized_clients: Option<&[X25519PublicKey]>,
    ) -> Result<OnionListener, Error>;
    fn generate_token(&mut self) -> CircuitToken;
    fn release_token(&mut self, token: CircuitToken);
}
