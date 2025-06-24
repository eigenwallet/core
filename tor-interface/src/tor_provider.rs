// standard
use std::any::Any;
use std::boxed::Box;
use std::convert::TryFrom;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::sync::{atomic, Arc, OnceLock};
#[cfg(unix)]
use std::os::unix::io::{IntoRawFd, RawFd};
#[cfg(windows)]
use std::os::windows::io::{IntoRawSocket, RawSocket};

// extern crates
use domain::base::name::Name;
use idna::uts46::{Hyphens, Uts46};
use idna::{domain_to_ascii_cow, AsciiDenyList};
use regex::Regex;
pub use socks::TcpOrUnixStream;

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

/// A version 3 onion service address.
///
/// Version 3 Onion Service addresses const of a [`crate::tor_crypto::V3OnionServiceId`] and a 16-bit port number.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OnionAddrV3 {
    pub(crate) service_id: V3OnionServiceId,
    pub(crate) virt_port: u16,
}

impl OnionAddrV3 {
    /// Create a new `OnionAddrV3` from a [`crate::tor_crypto::V3OnionServiceId`] and port number.
    pub fn new(service_id: V3OnionServiceId, virt_port: u16) -> OnionAddrV3 {
        OnionAddrV3 {
            service_id,
            virt_port,
        }
    }

    /// Return the service id associated with this onion address.
    pub fn service_id(&self) -> &V3OnionServiceId {
        &self.service_id
    }

    /// Return the port numebr associated with this onion address.
    pub fn virt_port(&self) -> u16 {
        self.virt_port
    }
}

impl std::fmt::Display for OnionAddrV3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.onion:{}", self.service_id, self.virt_port)
    }
}

/// An onion service address analog to [`std::net::SocketAddr`]
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

/// A domain name analog to `std::net::SocketAddr`
///
/// A `DomainAddr` must not end in ".onion"
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DomainAddr {
    domain: String,
    port: u16,
}

/// A `DomainAddr` has a domain name (scuh as `www.example.com`) and a port
impl DomainAddr {
    /// Returns the domain name associated with this domain address.
    pub fn domain(&self) -> &str {
        self.domain.as_ref()
    }

    /// Returns the port number associated with this domain address.
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

/// An enum representing the various types of addresses a [`TorProvider`] implementation may connect to.
#[derive(Clone, Debug)]
pub enum TargetAddr {
    /// An ip address and port
    Socket(std::net::SocketAddr),
    /// An onion-service id and virtual port
    OnionService(OnionAddr),
    /// A domain name and port
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

/// Various events possibly returned by a [`TorProvider`] implementation's `update()` method.
#[derive(Debug)]
pub enum TorEvent {
    /// A status update received connecting to the Tor Network.
    BootstrapStatus {
        /// A number from 0 to 100 for how through the bootstrap process the `TorProvider` is.
        progress: u32,
        /// A short string to identify the current phase of the bootstrap process.
        tag: String,
        /// A longer string with a summary of the current phase of the bootstrap process.
        summary: String,
    },
    /// Indicates successful connection to the Tor Network. The [`TorProvider::connect()`] and [`TorProvider::listener()`] methods may now be used.
    BootstrapComplete,
    /// Messages which may be useful for troubleshooting.
    LogReceived {
        /// A message
        line: String,
    },
    /// An onion-service has been published to the Tor Network and may now be reachable by clients.
    OnionServicePublished {
        /// The service-id of the onion-service which has been published.
        service_id: V3OnionServiceId,
    },
}

/// A `CircuitToken` is used to specify circuits used to connect to clearnet services.
pub type CircuitToken = usize;

//
// Onion Stream
//

#[cfg(unix)]
pub type OnionStreamIntoRaw = RawFd;
#[cfg(windows)]
pub type OnionStreamIntoRaw = RawSocket;

/// A wrapper around a [`TcpOrUnixStream`] with some Tor-specific customisations
///
/// An onion-listener can be constructed using the [`TorProvider::connect()`] method.
pub trait OnionStream: Send + Read + Write + std::fmt::Debug {
    /// Returns the target address of the remote peer of this onion connection.
    fn peer_addr(&self) -> Option<TargetAddr>;

    /// Returns the onion address of the local connection for an incoming onion-service connection. Returns `None` for outgoing connections.
    fn local_addr(&self) -> Option<OnionAddr>;

    /// Tries to clone the underlying connection and data. A simple pass-through to [`TcpOrUnixStream::try_clone()`].
    fn try_clone(&self) -> std::io::Result<Self> where Self: Sized;

    /// Moves the underlying `TcpOrUnixStream` into or out of nonblocking mode.
    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()>;

    /// Consume stream and return the underlying raw handle.
    fn into_raw(self) -> OnionStreamIntoRaw;
}

#[derive(Debug)]
pub struct TcpOrUnixOnionStream {
    pub(crate) stream: TcpOrUnixStream,
    pub(crate) local_addr: Option<OnionAddr>,
    pub(crate) peer_addr: Option<TargetAddr>,
}

impl Deref for TcpOrUnixOnionStream {
    type Target = TcpOrUnixStream;
    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl DerefMut for TcpOrUnixOnionStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stream
    }
}

impl From<TcpOrUnixOnionStream> for TcpOrUnixStream {
    fn from(onion_stream: TcpOrUnixOnionStream) -> Self {
        onion_stream.stream
    }
}

impl Read for TcpOrUnixOnionStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.stream.read(buf)
    }
    fn read_vectored(&mut self, bufs: &mut [std::io::IoSliceMut<'_>]) -> std::io::Result<usize> {
        self.stream.read_vectored(bufs)
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> std::io::Result<usize> {
        self.stream.read_to_end(buf)
    }
    fn read_to_string(&mut self, buf: &mut String) -> std::io::Result<usize> {
        self.stream.read_to_string(buf)
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        self.stream.read_exact(buf)
    }
}

impl Write for TcpOrUnixOnionStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stream.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        self.stream.write_vectored(bufs)
    }
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.stream.write_all(buf)
    }
    fn write_fmt(&mut self, fmt: std::fmt::Arguments<'_>) -> std::io::Result<()> {
        self.stream.write_fmt(fmt)
    }
}

impl OnionStream for TcpOrUnixOnionStream {
    fn peer_addr(&self) -> Option<TargetAddr> {
        self.peer_addr.clone()
    }

    fn local_addr(&self) -> Option<OnionAddr> {
        self.local_addr.clone()
    }

    fn try_clone(&self) -> std::io::Result<Self> where Self: Sized {
        Ok(Self {
            stream: self.stream.try_clone()?,
            local_addr: self.local_addr.clone(),
            peer_addr: self.peer_addr.clone(),
        })
    }

    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        self.stream.set_nonblocking(nonblocking)
    }

    fn into_raw(self) -> OnionStreamIntoRaw {
        #[cfg(unix)]
        return self.stream.into_raw_fd();
        #[cfg(windows)]
        return self.stream.into_raw_stream();
    }
}

pub struct BoxOnionStream {
    data: Box<dyn Any + Send>,

    peer_addr: fn(&Box<dyn Any + Send>) -> Option<TargetAddr>,
    local_addr: fn(&Box<dyn Any + Send>) -> Option<OnionAddr>,
    try_clone: fn(&Box<dyn Any + Send>) -> std::io::Result<Self>,
    set_nonblocking: fn(&Box<dyn Any + Send>, bool) -> std::io::Result<()>,
    into_raw: fn(Box<dyn Any + Send>) -> OnionStreamIntoRaw,

    read: fn(&mut Box<dyn Any + Send>, &mut [u8]) -> std::io::Result<usize>,
    read_vectored: fn(&mut Box<dyn Any + Send>, &mut [std::io::IoSliceMut<'_>]) -> std::io::Result<usize>,
    read_to_end: fn(&mut Box<dyn Any + Send>, &mut Vec<u8>) -> std::io::Result<usize>,
    read_to_string: fn(&mut Box<dyn Any + Send>, &mut String) -> std::io::Result<usize>,
    read_exact: fn(&mut Box<dyn Any + Send>, &mut [u8]) -> std::io::Result<()>,

    write: fn(&mut Box<dyn Any + Send>, &[u8]) -> std::io::Result<usize>,
    flush: fn(&mut Box<dyn Any + Send>) -> std::io::Result<()>,
    write_vectored: fn(&mut Box<dyn Any + Send>, &[std::io::IoSlice<'_>]) -> std::io::Result<usize>,
    write_all: fn(&mut Box<dyn Any + Send>, &[u8]) -> std::io::Result<()>,
    write_fmt: fn(&mut Box<dyn Any + Send>, std::fmt::Arguments<'_>) -> std::io::Result<()>,
}

impl std::fmt::Debug for BoxOnionStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.write_str("BoxOnionStream")
    }
}

impl BoxOnionStream {
    pub fn new<S: OnionStream + 'static>(s: S) -> Self {
        Self {
            data: Box::new(s),

            peer_addr: |slf| slf.downcast_ref::<S>().unwrap().peer_addr(),
            local_addr: |slf| slf.downcast_ref::<S>().unwrap().local_addr(),
            try_clone: |slf| slf.downcast_ref::<S>().unwrap().try_clone().map(BoxOnionStream::new),
            set_nonblocking: |slf, nonblocking| slf.downcast_ref::<S>().unwrap().set_nonblocking(nonblocking),
            into_raw: |slf| slf.downcast::<S>().unwrap().into_raw(),

            read: |slf, buf| slf.downcast_mut::<S>().unwrap().read(buf),
            read_vectored: |slf, bufs| slf.downcast_mut::<S>().unwrap().read_vectored(bufs),
            read_to_end: |slf, buf| slf.downcast_mut::<S>().unwrap().read_to_end(buf),
            read_to_string: |slf, buf| slf.downcast_mut::<S>().unwrap().read_to_string(buf),
            read_exact: |slf, buf| slf.downcast_mut::<S>().unwrap().read_exact(buf),

            write: |slf, buf| slf.downcast_mut::<S>().unwrap().write(buf),
            flush: |slf| slf.downcast_mut::<S>().unwrap().flush(),
            write_vectored: |slf, bufs| slf.downcast_mut::<S>().unwrap().write_vectored(bufs),
            write_all: |slf, buf| slf.downcast_mut::<S>().unwrap().write_all(buf),
            write_fmt: |slf, fmt| slf.downcast_mut::<S>().unwrap().write_fmt(fmt),
        }
    }
}

impl Read for BoxOnionStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        (self.read)(&mut self.data, buf)
    }
    fn read_vectored(&mut self, bufs: &mut [std::io::IoSliceMut<'_>]) -> std::io::Result<usize> {
        (self.read_vectored)(&mut self.data, bufs)
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> std::io::Result<usize> {
        (self.read_to_end)(&mut self.data, buf)
    }
    fn read_to_string(&mut self, buf: &mut String) -> std::io::Result<usize> {
        (self.read_to_string)(&mut self.data, buf)
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        (self.read_exact)(&mut self.data, buf)
    }
}

impl Write for BoxOnionStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        (self.write)(&mut self.data, buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        (self.flush)(&mut self.data)
    }
    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        (self.write_vectored)(&mut self.data, bufs)
    }
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        (self.write_all)(&mut self.data, buf)
    }
    fn write_fmt(&mut self, fmt: std::fmt::Arguments<'_>) -> std::io::Result<()> {
        (self.write_fmt)(&mut self.data, fmt)
    }
}

impl OnionStream for BoxOnionStream {
    fn peer_addr(&self) -> Option<TargetAddr> {
        (self.peer_addr)(&self.data)
    }

    fn local_addr(&self) -> Option<OnionAddr> {
        (self.local_addr)(&self.data)
    }

    fn try_clone(&self) -> std::io::Result<Self> where Self: Sized {
        (self.try_clone)(&self.data)
    }

    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        (self.set_nonblocking)(&self.data, nonblocking)
    }

    fn into_raw(self) -> OnionStreamIntoRaw {
        (self.into_raw)(self.data)
    }
}

//
// Onion Listener
//

/// A wrapper around a [`std::net::TcpListener`] with some Tor-specific customisations.
///
/// An onion-listener can be constructed using the [`TorProvider::listener()`] method.
pub trait OnionListener: Send {
    type Stream: OnionStream;

    /// Moves the underlying `TcpListener` into or out of nonblocking mode.
    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()>;

    /// Accept a new incoming connection from this listener.
    fn accept(&self) -> std::io::Result<Option<Self::Stream>>;

    /// Address this listener is listening on
    fn address(&self) -> &OnionAddr;
}

pub(crate) struct TcpOnionListenerBase(pub TcpListener, pub OnionAddr);

pub struct TcpOnionListener(pub(crate) TcpOnionListenerBase, pub(crate) Arc<atomic::AtomicBool>);

impl OnionListener for TcpOnionListenerBase {
    type Stream = TcpOrUnixOnionStream;

    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        self.0.set_nonblocking(nonblocking)
    }

    fn accept(&self) -> std::io::Result<Option<Self::Stream>> {
        match self.0.accept() {
            Ok((stream, _socket_addr)) => Ok(Some(TcpOrUnixOnionStream {
                stream: stream.into(),
                local_addr: Some(self.1.clone()),
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

    fn address(&self) -> &OnionAddr {
        &self.1
    }
}

impl OnionListener for TcpOnionListener {
    type Stream = TcpOrUnixOnionStream;

    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        self.0.set_nonblocking(nonblocking)
    }

    fn accept(&self) -> std::io::Result<Option<Self::Stream>> {
        self.0.accept()
    }

    fn address(&self) -> &OnionAddr {
        &self.0.address()
    }
}

impl TcpOnionListener {
    /// `TcpListener::try_clone()` the inner listener
    ///
    /// The lifetime of the hidden service itself is still bound to this object,
    /// but the resulting [`TcpListener`] may be polled/`accept`ed independently
    pub fn try_clone_inner(&self) -> std::io::Result<TcpListener> {
        self.0.0.try_clone()
    }
}

impl Drop for TcpOnionListener {
    fn drop(&mut self) {
        self.1.store(false, atomic::Ordering::Relaxed)
    }
}

pub struct BoxOnionListener {
    data: Box<dyn Any + Send>,

    set_nonblocking: fn(&Box<dyn Any + Send>, bool) -> std::io::Result<()>,
    accept: fn(&Box<dyn Any + Send>) -> std::io::Result<Option<<Self as OnionListener>::Stream>>,
    address: fn(&Box<dyn Any + Send>) -> &OnionAddr,
}

impl BoxOnionListener {
    pub fn new<L: OnionListener + 'static>(l: L) -> Self {
        Self {
            data: Box::new(l),

            set_nonblocking: |slf, nonblocking| slf.downcast_ref::<L>().unwrap().set_nonblocking(nonblocking),
            accept: |slf| slf.downcast_ref::<L>().unwrap().accept().map(|r| r.map(BoxOnionStream::new)),
            address: |slf| slf.downcast_ref::<L>().unwrap().address(),
        }
    }
}

impl OnionListener for BoxOnionListener {
    type Stream = BoxOnionStream;

    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        (self.set_nonblocking)(&self.data, nonblocking)
    }

    fn accept(&self) -> std::io::Result<Option<Self::Stream>> {
        (self.accept)(&self.data)
    }

    fn address(&self) -> &OnionAddr {
        (self.address)(&self.data)
    }
}

/// The `TorProvider` trait allows for high-level Tor Network functionality. Implementations ay connect to the Tor Network, anonymously connect to both clearnet and onion-service endpoints, and host onion-services.
pub trait TorProvider: Send {
    type Stream: OnionStream;
    type Listener: OnionListener;

    /// Process and return `TorEvent`s handled by this `TorProvider`.
    fn update(&mut self) -> Result<Vec<TorEvent>, Error>;
    /// Begin connecting to the Tor Network.
    fn bootstrap(&mut self) -> Result<(), Error>;
    /// Add v3 onion-service authorisation credentials, allowing this `TorProvider` to connect to an onion-service whose service-descriptor is encrypted using the assocciated x25519 public key.
    fn add_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
        client_auth: &X25519PrivateKey,
    ) -> Result<(), Error>;
    /// Remove a previously added client authorisation credential. This `TorProvider` will be unable to connect to the onion-service associated with the removed credentail.
    fn remove_client_auth(&mut self, service_id: &V3OnionServiceId) -> Result<(), Error>;
    /// Anonymously connect to the address specified by `target` over the Tor Network and return the associated [`OnionStream`].
    ///
    /// When conecting to clearnet targets, an optional [`CircuitToken`] may be used to enforce usage of different circuits through the Tor Network. If `circuit` is `None`, the default circuit is used.
    ///
    ///Connections made with different `CircuitToken`s are required to use different circuits through the Tor Network. However, connections made with identical `CircuitToken`s are *not* required to use identical circuits through the Tor Network.
    ///
    /// Specifying a circuit token when connecting to an onion-service has no effect on the resulting circuit.
    fn connect(
        &mut self,
        target: TargetAddr,
        circuit: Option<CircuitToken>,
    ) -> Result<Self::Stream, Error>;
    /// Anonymously start an onion-service and return the associated [`OnionListener`].
    ///
    ///The resulting onion-service will not be reachable by clients until [`TorProvider::update()`] returns a [`TorEvent::OnionServicePublished`] event. The optional `authorised_clients` parameter may be used to require client authorisation keys to connect to resulting onion-service. `bind_addr` may be used to force a specific address and port. For further information, see the Tor Project's onion-services [client-auth documentation](https://community.torproject.org/onion-services/advanced/client-auth).
    fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorised_clients: Option<&[X25519PublicKey]>,
        bind_addr: Option<SocketAddr>,
    ) -> Result<Self::Listener, Error>;
    /// Create a new [`CircuitToken`].
    fn generate_token(&mut self) -> CircuitToken;
    /// Releaes a previously generated [`CircuitToken`].
    fn release_token(&mut self, token: CircuitToken);
}


pub struct BoxTorProvider {
    data: Box<dyn Any + Send>,

    update: fn(&mut Box<dyn Any + Send>) -> Result<Vec<TorEvent>, Error>,
    bootstrap: fn(&mut Box<dyn Any + Send>) -> Result<(), Error>,
    add_client_auth: fn(&mut Box<dyn Any + Send>, &V3OnionServiceId, &X25519PrivateKey) -> Result<(), Error>,
    remove_client_auth: fn(&mut Box<dyn Any + Send>, &V3OnionServiceId) -> Result<(), Error>,
    connect: fn(&mut Box<dyn Any + Send>, TargetAddr, Option<CircuitToken>) -> Result<<Self as TorProvider>::Stream, Error>,
    listener: fn(&mut Box<dyn Any + Send>, &Ed25519PrivateKey, u16, Option<&[X25519PublicKey]>, Option<SocketAddr>) -> Result<<Self as TorProvider>::Listener, Error>,
    generate_token: fn(&mut Box<dyn Any + Send>) -> CircuitToken,
    release_token: fn(&mut Box<dyn Any + Send>, CircuitToken),
}

impl BoxTorProvider {
    pub fn new<P: TorProvider + Send + 'static>(p: P) -> Self {
        Self {
            data: Box::new(p),

            update: |slf| slf.downcast_mut::<P>().unwrap().update(),
            bootstrap: |slf| slf.downcast_mut::<P>().unwrap().bootstrap(),
            add_client_auth: |slf, service_id, client_auth| slf.downcast_mut::<P>().unwrap().add_client_auth(service_id, client_auth),
            remove_client_auth: |slf, service_id| slf.downcast_mut::<P>().unwrap().remove_client_auth(service_id),
            connect: |slf, target, circuit| slf.downcast_mut::<P>().unwrap().connect(target, circuit).map(BoxOnionStream::new),
            listener: |slf, private_key, virt_port, authorised_clients, bind_addr| slf.downcast_mut::<P>().unwrap().listener(private_key, virt_port, authorised_clients, bind_addr).map(BoxOnionListener::new),
            generate_token: |slf| slf.downcast_mut::<P>().unwrap().generate_token(),
            release_token: |slf, token| slf.downcast_mut::<P>().unwrap().release_token(token),
        }
    }
}

impl TorProvider for BoxTorProvider {
    type Stream = BoxOnionStream;
    type Listener = BoxOnionListener;

    fn update(&mut self) -> Result<Vec<TorEvent>, Error> {
        (self.update)(&mut self.data)
    }
    fn bootstrap(&mut self) -> Result<(), Error> {
        (self.bootstrap)(&mut self.data)
    }
    fn add_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
        client_auth: &X25519PrivateKey,
    ) -> Result<(), Error> {
        (self.add_client_auth)(&mut self.data, service_id, client_auth)
    }
    fn remove_client_auth(&mut self, service_id: &V3OnionServiceId) -> Result<(), Error> {
        (self.remove_client_auth)(&mut self.data, service_id)
    }
    fn connect(
        &mut self,
        target: TargetAddr,
        circuit: Option<CircuitToken>,
    ) -> Result<Self::Stream, Error> {
        (self.connect)(&mut self.data, target, circuit)
    }
    fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorised_clients: Option<&[X25519PublicKey]>,
        bind_addr: Option<SocketAddr>,
    ) -> Result<Self::Listener, Error> {
        (self.listener)(&mut self.data, private_key, virt_port, authorised_clients, bind_addr)
    }
    fn generate_token(&mut self) -> CircuitToken {
        (self.generate_token)(&mut self.data)
    }
    fn release_token(&mut self, token: CircuitToken) {
        (self.release_token)(&mut self.data, token)
    }
}
