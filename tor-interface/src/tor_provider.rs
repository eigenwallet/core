// standard
use std::any::Any;
use std::boxed::Box;
use std::convert::TryFrom;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::sync::OnceLock;

// extern crates
use domain::base::name::Name;
use idna::uts46::{Hyphens, Uts46};
use idna::{domain_to_ascii_cow, AsciiDenyList};
use regex::Regex;

// internal crates
use crate::tor_crypto::*;


/// Various `tor_provider` errors.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to parse '{0}' as {1}")]
    /// Failure parsing some string into a type
    ParseFailure(String, String),

    #[error("{0}")]
    /// Other miscellaneous error
    Generic(String),
}

//
// OnionAddr
//

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

impl FromStr for OnionAddr {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        static ONION_SERVICE_PATTERN: OnceLock<Regex> = OnceLock::new();
        let onion_service_pattern = ONION_SERVICE_PATTERN.get_or_init(|| {
            Regex::new(r"(?m)^(?P<service_id>[a-z2-7]{56})\.onion:(?P<port>[1-9][0-9]{0,4})$")
                .unwrap()
        });

        if let Some(caps) = onion_service_pattern.captures(s.to_lowercase().as_ref()) {
            let service_id = caps
                .name("service_id")
                .expect("missing service_id group")
                .as_str()
                .to_lowercase();
            let port = caps.name("port").expect("missing port group").as_str();
            if let (Ok(service_id), Ok(port)) = (
                V3OnionServiceId::from_string(service_id.as_ref()),
                u16::from_str(port),
            ) {
                return Ok(OnionAddr::V3(OnionAddrV3::new(service_id, port)));
            }
        }
        Err(Self::Err::ParseFailure(s.to_string(), "OnionAddr".to_string()))
    }
}

impl std::fmt::Display for OnionAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OnionAddr::V3(onion_addr) => onion_addr.fmt(f),
        }
    }
}

//
// DomainAddr
//

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DomainAddr {
    domain: String,
    port: u16,
}

impl DomainAddr {
    pub fn domain(&self) -> &str {
        self.domain.as_ref()
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl std::fmt::Display for DomainAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let uts46: Uts46 = Default::default();
        let (ui_str, _err) = uts46.to_user_interface(
            self.domain.as_str().as_bytes(),
            AsciiDenyList::URL,
            Hyphens::Allow,
            |_, _, _| -> bool { false },
        );
        write!(f, "{}:{}", ui_str, self.port)
    }
}

impl TryFrom<(String, u16)> for DomainAddr {
    type Error = Error;

    fn try_from(value: (String, u16)) -> Result<Self, Self::Error> {
        let (domain, port) = (&value.0, value.1);
        if let Ok(domain) = domain_to_ascii_cow(domain.as_bytes(), AsciiDenyList::URL) {
            let domain = domain.to_string();
            if let Ok(domain) = Name::<Vec<u8>>::from_str(domain.as_ref()) {
                let domain = domain.to_string();
                if !domain.ends_with(".onion") {
                    return Ok(Self {
                        domain,
                        port,
                    });
                }
            }
        }
        Err(Self::Error::ParseFailure(format!(
            "{}:{}",
            domain, port
        ), "DomainAddr".to_string()))
    }
}

impl FromStr for DomainAddr {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        static DOMAIN_PATTERN: OnceLock<Regex> = OnceLock::new();
        let domain_pattern = DOMAIN_PATTERN
            .get_or_init(|| Regex::new(r"(?m)^(?P<domain>.*):(?P<port>[1-9][0-9]{0,4})$").unwrap());
        if let Some(caps) = domain_pattern.captures(s) {
            let domain = caps
                .name("domain")
                .expect("missing domain group")
                .as_str()
                .to_string();
            let port = caps.name("port").expect("missing port group").as_str();
            if let Ok(port) = u16::from_str(port) {
                return Self::try_from((domain, port));
            }
        }
        Err(Self::Err::ParseFailure(s.to_string(), "DomainAddr".to_string()))
    }
}

//
// TargetAddr
//

#[derive(Clone, Debug)]
pub enum TargetAddr {
    Socket(std::net::SocketAddr),
    OnionService(OnionAddr),
    Domain(DomainAddr),
}

impl From<(V3OnionServiceId, u16)> for TargetAddr {
    fn from(target_tuple: (V3OnionServiceId, u16)) -> Self {
        TargetAddr::OnionService(OnionAddr::V3(OnionAddrV3::new(
            target_tuple.0,
            target_tuple.1,
        )))
    }
}

impl FromStr for TargetAddr {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(socket_addr) = SocketAddr::from_str(s) {
            return Ok(TargetAddr::Socket(socket_addr));
        } else if let Ok(onion_addr) = OnionAddr::from_str(s) {
            return Ok(TargetAddr::OnionService(onion_addr));
        } else if let Ok(domain_addr) = DomainAddr::from_str(s) {
            return Ok(TargetAddr::Domain(domain_addr));
        }
        Err(Self::Err::ParseFailure(s.to_string(), "TargetAddr".to_string()))
    }
}

impl std::fmt::Display for TargetAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetAddr::Socket(socket_addr) => socket_addr.fmt(f),
            TargetAddr::OnionService(onion_addr) => onion_addr.fmt(f),
            TargetAddr::Domain(domain_addr) => domain_addr.fmt(f),
        }
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
        self.local_addr.clone()
    }

    pub fn try_clone(&self) -> Result<Self, std::io::Error> {
        Ok(Self {
            stream: self.stream.try_clone()?,
            local_addr: self.local_addr.clone(),
            peer_addr: self.peer_addr.clone(),
        })
    }
}

//
// Onion Listener
//


pub struct OnionListener {
    pub(crate) listener: TcpListener,
    pub(crate) onion_addr: OnionAddr,
    pub(crate) data: Option<Box<dyn Any + Send>>,
    pub(crate) drop: Option<Box<dyn FnMut(Box<dyn Any>) + Send>>,
}

impl OnionListener {
    pub(crate) fn new<T: 'static + Send>(
        listener: TcpListener,
        onion_addr: OnionAddr,
        data: T,
        mut drop: impl FnMut(T) + 'static + Send) -> Self {
        // marshall our data into an Any
        let data: Option<Box<dyn Any + Send>> = Some(Box::new(data));
        // marhsall our drop into a function which takes an Any
        let drop: Option<Box<dyn FnMut(Box<dyn Any>) + Send>>  = Some(Box::new(move |data: Box<dyn std::any::Any>| {
            // encapsulate extracting our data from the Any
            if let Ok(data) = data.downcast::<T>() {
                // and call our provided drop
                drop(*data);
            }
        }));

        Self{
            listener,
            onion_addr,
            data,
            drop,
        }
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<(), std::io::Error> {
        self.listener.set_nonblocking(nonblocking)
    }

    pub fn accept(&self) -> Result<Option<OnionStream>, std::io::Error> {
        match self.listener.accept() {
            Ok((stream, _socket_addr)) => Ok(Some(OnionStream {
                stream,
                local_addr: Some(self.onion_addr.clone()),
                peer_addr: None,
            })),
            Err(err) => {
                if err.kind() == std::io::ErrorKind::WouldBlock {
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
        if let (Some(data), Some(mut drop)) = (self.data.take(), self.drop.take()) {
            drop(data)
        }
    }
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
