// standard
use std::boxed::Box;
use std::convert::TryFrom;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::OnceLock;

// extern crates
use domain::base::name::Name;
use idna::uts46::{Hyphens, Uts46};
use idna::{domain_to_ascii_cow, AsciiDenyList};
use regex::Regex;

// internal crates
use crate::tor_crypto::*;

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

#[derive(thiserror::Error, Debug)]
pub enum OnionAddrParseError {
    #[error("Failed to parse '{0}' as OnionAddr")]
    Generic(String),
}

impl FromStr for OnionAddr {
    type Err = OnionAddrParseError;
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
        Err(Self::Err::Generic(s.to_string()))
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

#[derive(thiserror::Error, Debug)]
pub enum DomainAddrParseError {
    #[error("Unable to parse '{0}' as DomainAddr")]
    Generic(String),
}

impl TryFrom<(String, u16)> for DomainAddr {
    type Error = DomainAddrParseError;

    fn try_from(value: (String, u16)) -> Result<Self, Self::Error> {
        let (domain, port) = (&value.0, value.1);
        if let Ok(domain) = domain_to_ascii_cow(domain.as_bytes(), AsciiDenyList::URL) {
            let domain = domain.to_string();
            if let Ok(domain) = Name::<Vec<u8>>::from_str(domain.as_ref()) {
                return Ok(Self {
                    domain: domain.to_string(),
                    port,
                });
            }
        }
        Err(DomainAddrParseError::Generic(format!(
            "{}:{}",
            domain, port
        )))
    }
}

impl FromStr for DomainAddr {
    type Err = DomainAddrParseError;
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
        Err(DomainAddrParseError::Generic(s.to_string()))
    }
}

//
// TargetAddr
//

#[derive(Clone, Debug)]
pub enum TargetAddr {
    Ip(std::net::SocketAddr),
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

#[derive(thiserror::Error, Debug)]
pub enum TargetAddrParseError {
    #[error("Unable to parse '{0}' as TargetAddr")]
    Generic(String),
}

impl FromStr for TargetAddr {
    type Err = TargetAddrParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(socket_addr) = SocketAddr::from_str(s) {
            return Ok(TargetAddr::Ip(socket_addr));
        } else if let Ok(onion_addr) = OnionAddr::from_str(s) {
            return Ok(TargetAddr::OnionService(onion_addr));
        } else if let Ok(domain_addr) = DomainAddr::from_str(s) {
            if !domain_addr.domain().ends_with(".onion") {
                return Ok(TargetAddr::Domain(domain_addr));
            }
        }
        Err(TargetAddrParseError::Generic(s.to_string()))
    }
}

impl std::fmt::Display for TargetAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetAddr::Ip(socket_addr) => socket_addr.fmt(f),
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

//
// Onion Listener
//

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

//
// ProxyConfig
//

#[derive(thiserror::Error, Debug)]
pub enum ProxyConfigError {
    #[error("{0}")]
    Generic(String),
}

#[derive(Clone, Debug)]
pub struct Socks4ProxyConfig {
    pub(crate) address: TargetAddr,
}

impl Socks4ProxyConfig {
    pub fn new(address: TargetAddr) -> Result<Self, ProxyConfigError> {
        let port = match &address {
            TargetAddr::Ip(addr) => addr.port(),
            TargetAddr::Domain(addr) => addr.port(),
            TargetAddr::OnionService(_) => {
                return Err(ProxyConfigError::Generic(
                    "proxy address may not be onion service".to_string(),
                ))
            }
        };
        if port == 0 {
            return Err(ProxyConfigError::Generic("proxy port not be 0".to_string()));
        }

        Ok(Self { address })
    }
}

#[derive(Clone, Debug)]
pub struct Socks5ProxyConfig {
    pub(crate) address: TargetAddr,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
}

impl Socks5ProxyConfig {
    pub fn new(
        address: TargetAddr,
        username: Option<String>,
        password: Option<String>,
    ) -> Result<Self, ProxyConfigError> {
        let port = match &address {
            TargetAddr::Ip(addr) => addr.port(),
            TargetAddr::Domain(addr) => addr.port(),
            TargetAddr::OnionService(_) => {
                return Err(ProxyConfigError::Generic(
                    "proxy address may not be onion service".to_string(),
                ))
            }
        };
        if port == 0 {
            return Err(ProxyConfigError::Generic("proxy port not be 0".to_string()));
        }

        // username must be less than 255 bytes
        if let Some(username) = &username {
            if username.len() > 255 {
                return Err(ProxyConfigError::Generic(
                    "socks5 username must be <= 255 bytes".to_string(),
                ));
            }
        }
        // password must be less than 255 bytes
        if let Some(password) = &password {
            if password.len() > 255 {
                return Err(ProxyConfigError::Generic(
                    "socks5 password must be <= 255 bytes".to_string(),
                ));
            }
        }

        Ok(Self {
            address,
            username,
            password,
        })
    }
}

#[derive(Clone, Debug)]
pub struct HttpsProxyConfig {
    pub(crate) address: TargetAddr,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
}

impl HttpsProxyConfig {
    pub fn new(
        address: TargetAddr,
        username: Option<String>,
        password: Option<String>,
    ) -> Result<Self, ProxyConfigError> {
        let port = match &address {
            TargetAddr::Ip(addr) => addr.port(),
            TargetAddr::Domain(addr) => addr.port(),
            TargetAddr::OnionService(_) => {
                return Err(ProxyConfigError::Generic(
                    "proxy address may not be onion service".to_string(),
                ))
            }
        };
        if port == 0 {
            return Err(ProxyConfigError::Generic("proxy port not be 0".to_string()));
        }

        // username may not contain ':' character (per RFC 2617)
        if let Some(username) = &username {
            if username.contains(':') {
                return Err(ProxyConfigError::Generic(
                    "username may not contain ':' character".to_string(),
                ));
            }
        }

        Ok(Self {
            address,
            username,
            password,
        })
    }
}

#[derive(Clone, Debug)]
pub enum ProxyConfig {
    Socks4(Socks4ProxyConfig),
    Socks5(Socks5ProxyConfig),
    Https(HttpsProxyConfig),
}

impl From<Socks4ProxyConfig> for ProxyConfig {
    fn from(config: Socks4ProxyConfig) -> Self {
        ProxyConfig::Socks4(config)
    }
}

impl From<Socks5ProxyConfig> for ProxyConfig {
    fn from(config: Socks5ProxyConfig) -> Self {
        ProxyConfig::Socks5(config)
    }
}

impl From<HttpsProxyConfig> for ProxyConfig {
    fn from(config: HttpsProxyConfig) -> Self {
        ProxyConfig::Https(config)
    }
}

//
// PluggableTransportConfig
//

#[derive(Clone, Debug)]
pub struct PluggableTransportConfig {
    transports: Vec<String>,
    path_to_binary: PathBuf,
    options: Vec<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum PluggableTransportConfigError {
    #[error("pluggable transport name '{0}' is invalid")]
    TransportNameInvalid(String),
    #[error("unable to use '{0}' as pluggable transport binary path, {1}")]
    BinaryPathInvalid(String, String),
}

// per the PT spec: https://github.com/Pluggable-Transports/Pluggable-Transports-spec/blob/main/releases/PTSpecV1.0/pt-1_0.txt
static TRANSPORT_PATTERN: OnceLock<Regex> = OnceLock::new();
fn init_transport_pattern() -> Regex {
    Regex::new(r"(?m)^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap()
}

impl PluggableTransportConfig {
    pub fn new(
        transports: Vec<String>,
        path_to_binary: PathBuf,
    ) -> Result<Self, PluggableTransportConfigError> {
        let transport_pattern = TRANSPORT_PATTERN.get_or_init(init_transport_pattern);
        // validate each transport
        for transport in &transports {
            if !transport_pattern.is_match(&transport) {
                return Err(PluggableTransportConfigError::TransportNameInvalid(
                    transport.clone(),
                ));
            }
        }

        // pluggable transport path must be absolute so we can fix it up for individual
        // TorProvider implementations
        if !path_to_binary.is_absolute() {
            return Err(PluggableTransportConfigError::BinaryPathInvalid(
                format!("{:?}", path_to_binary.display()),
                "must be an absolute path".to_string(),
            ));
        }

        Ok(Self {
            transports,
            path_to_binary,
            options: Default::default(),
        })
    }

    pub fn transports(&self) -> &Vec<String> {
        &self.transports
    }

    pub fn path_to_binary(&self) -> &PathBuf {
        &self.path_to_binary
    }

    pub fn options(&self) -> &Vec<String> {
        &self.options
    }

    pub fn add_option(&mut self, arg: String) {
        self.options.push(arg);
    }
}

//
// BridgeSettings
//

#[derive(Clone, Debug)]
pub struct BridgeLine {
    transport: String,
    address: SocketAddr,
    fingerprint: String,
    keyvalues: Vec<(String, String)>,
}

#[derive(thiserror::Error, Debug)]
pub enum BridgeLineError {
    #[error("bridge line '{0}' missing transport")]
    TransportMissing(String),

    #[error("bridge line '{0}' missing address")]
    AddressMissing(String),

    #[error("bridge line '{0}' missing fingerprint")]
    FingerprintMissing(String),

    #[error("transport name '{0}' is invalid")]
    TransportNameInvalid(String),

    #[error("address '{0}' cannot be parsed as IP:PORT")]
    AddressParseFailed(String),

    #[error("key=value '{0}' is invalid")]
    KeyValueInvalid(String),

    #[error("bridge address port must not be 0")]
    AddressPortInvalid,

    #[error("fingerprint '{0}' is invalid")]
    FingerprintInvalid(String),
}

impl BridgeLine {
    pub fn new(
        transport: String,
        address: SocketAddr,
        fingerprint: String,
        keyvalues: Vec<(String, String)>,
    ) -> Result<BridgeLine, BridgeLineError> {
        let transport_pattern = TRANSPORT_PATTERN.get_or_init(init_transport_pattern);

        // transports have a particular pattern
        if !transport_pattern.is_match(&transport) {
            return Err(BridgeLineError::TransportNameInvalid(transport));
        }

        // port can't be 0
        if address.port() == 0 {
            return Err(BridgeLineError::AddressPortInvalid);
        }

        static BRIDGE_FINGERPRINT_PATTERN: OnceLock<Regex> = OnceLock::new();
        let bridge_fingerprint_pattern = BRIDGE_FINGERPRINT_PATTERN
            .get_or_init(|| Regex::new(r"(?m)^[0-9a-fA-F]{40}$").unwrap());

        // fingerprint should be a sha1 hash
        if !bridge_fingerprint_pattern.is_match(&fingerprint) {
            return Err(BridgeLineError::FingerprintInvalid(fingerprint));
        }

        // validate key-values
        for (key, value) in &keyvalues {
            if key.contains(' ') || key.contains('=') || key.len() == 0 {
                return Err(BridgeLineError::KeyValueInvalid(format!("{key}={value}")));
            }
        }

        Ok(Self {
            transport,
            address,
            fingerprint,
            keyvalues,
        })
    }

    pub fn transport(&self) -> &String {
        &self.transport
    }

    pub fn address(&self) -> &SocketAddr {
        &self.address
    }

    pub fn fingerprint(&self) -> &String {
        &self.fingerprint
    }

    pub fn keyvalues(&self) -> &Vec<(String, String)> {
        &self.keyvalues
    }

    #[cfg(feature = "legacy-tor-provider")]
    pub fn as_legacy_tor_setconf_value(&self) -> String {
        let transport = &self.transport;
        let address = self.address.to_string();
        let fingerprint = self.fingerprint.to_string();
        let keyvalues: Vec<String> = self
            .keyvalues
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        let keyvalues = keyvalues.join(" ");

        format!("{transport} {address} {fingerprint} {keyvalues}")
    }
}

impl FromStr for BridgeLine {
    type Err = BridgeLineError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut tokens = s.split(' ');
        // get transport name
        let transport = if let Some(transport) = tokens.next() {
            transport
        } else {
            return Err(BridgeLineError::TransportMissing(s.to_string()));
        };
        // get bridge address
        let address = if let Some(address) = tokens.next() {
            if let Ok(address) = SocketAddr::from_str(address) {
                address
            } else {
                return Err(BridgeLineError::AddressParseFailed(address.to_string()));
            }
        } else {
            return Err(BridgeLineError::AddressMissing(s.to_string()));
        };
        // get the bridge fingerprint
        let fingerprint = if let Some(fingerprint) = tokens.next() {
            fingerprint
        } else {
            return Err(BridgeLineError::FingerprintMissing(s.to_string()));
        };

        // get the bridge options
        static BRIDGE_OPTION_PATTERN: OnceLock<Regex> = OnceLock::new();
        let bridge_option_pattern = BRIDGE_OPTION_PATTERN
            .get_or_init(|| Regex::new(r"(?m)^(?<key>[^=]+)=(?<value>.*)$").unwrap());

        let mut keyvalues: Vec<(String, String)> = Default::default();
        while let Some(keyvalue) = tokens.next() {
            if let Some(caps) = bridge_option_pattern.captures(&keyvalue) {
                let key = caps
                    .name("key")
                    .expect("missing key group")
                    .as_str()
                    .to_string();
                let value = caps
                    .name("value")
                    .expect("missing value group")
                    .as_str()
                    .to_string();
                keyvalues.push((key, value));
            } else {
                return Err(BridgeLineError::KeyValueInvalid(keyvalue.to_string()));
            }
        }

        BridgeLine::new(
            transport.to_string(),
            address,
            fingerprint.to_string(),
            keyvalues,
        )
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
