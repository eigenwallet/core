// standard
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::OnceLock;

// extern crates
use regex::Regex;

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