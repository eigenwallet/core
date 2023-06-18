// standard
use std::io::{Read, Write};
use std::net::TcpStream;
use std::ops::{Deref, DerefMut};

// internal crates
use crate::tor_crypto::*;

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
    stream: TcpStream,
    onion_addr: Option<V3OnionServiceId>,
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
    pub fn new(stream: TcpStream, onion_addr: Option<V3OnionServiceId>) -> Self {
        Self { stream, onion_addr }
    }

    pub fn onion_addr(&self) -> Option<V3OnionServiceId> {
        self.onion_addr.clone()
    }

    pub fn try_clone(&self) -> Result<Self, std::io::Error> {
        Ok(Self {
            stream: self.stream.try_clone()?,
            onion_addr: self.onion_addr.clone(),
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
