// standard
use std::io::{Read, Write};
use std::net::TcpStream;
use std::ops::{Deref, DerefMut};

// internal crates
use crate::tor_crypto::*;

#[derive(Clone, Debug)]
pub enum OnionAddr {
    V3(V3OnionServiceId, u16),
}

impl std::fmt::Display for OnionAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OnionAddr::V3(service_id, port) => write!(f, "{}:{}", service_id, port),
        }
    }
}

#[derive(Clone, Debug)]
pub enum TargetAddr {
    Ip(std::net::SocketAddr),
    Domain(String, u16),
    OnionService(OnionAddr),
}

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

pub trait CircuitToken {}

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

pub trait OnionListener {
    fn set_nonblocking(&self, nonblocking: bool) -> Result<(), std::io::Error>;
    fn accept(&self) -> Result<Option<OnionStream>, std::io::Error>;
}

pub trait TorProvider<CT: CircuitToken, OL: OnionListener> {
    type Error;

    fn update(&mut self) -> Result<Vec<TorEvent>, Self::Error>;
    fn bootstrap(&mut self) -> Result<(), Self::Error>;
    fn add_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
        client_auth: &X25519PrivateKey,
    ) -> Result<(), Self::Error>;
    fn remove_client_auth(&mut self, service_id: &V3OnionServiceId) -> Result<(), Self::Error>;
    fn connect(
        &mut self,
        service_id: &V3OnionServiceId,
        virt_port: u16,
        circuit: Option<CT>,
    ) -> Result<OnionStream, Self::Error>;
    fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorized_clients: Option<&[X25519PublicKey]>,
    ) -> Result<OL, Self::Error>;
}
