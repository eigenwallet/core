// standard
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::OnceLock;

// extern crates
use regex::Regex;

#[derive(Clone, Debug)]
/// Configuration for a pluggable-transport
pub struct PluggableTransportConfig {
    transports: Vec<String>,
    path_to_binary: PathBuf,
    options: Vec<String>,
}

#[derive(thiserror::Error, Debug)]
/// Error returned on failure to construct a [`PluggableTransportConfig`]
pub enum PluggableTransportConfigError {
    #[error("pluggable transport name '{0}' is invalid")]
    /// transport names must be a valid C identifier
    TransportNameInvalid(String),
    #[error("unable to use '{0}' as pluggable transport binary path, {1}")]
    /// configuration only allows aboslute paths to binaries
    BinaryPathInvalid(String, String),
}

// per the PT spec: https://github.com/Pluggable-Transports/Pluggable-Transports-spec/blob/main/releases/PTSpecV1.0/pt-1_0.txt
static TRANSPORT_PATTERN: OnceLock<Regex> = OnceLock::new();
fn init_transport_pattern() -> Regex {
    Regex::new(r"(?m)^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap()
}

/// Configuration struct for a pluggable-transport which conforms to the v1.0 pluggable-transport [specification](https://github.com/Pluggable-Transports/Pluggable-Transports-spec/blob/main/releases/PTSpecV1.0/pt-1_0.txt)
impl PluggableTransportConfig {
    /// Construct a new `PluggableTransportConfig`. Each `transport` string must be a [valid C identifier](https://github.com/Pluggable-Transports/Pluggable-Transports-spec/blob/c92e59a9fa6ba11c181f4c5ec9d533eaa7d9d7f3/releases/PTSpecV1.0/pt-1_0.txt#L144) while `path_to_binary` must be an absolute path.
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

    /// Get a reference to this `PluggableTransportConfig`'s list of transports.
    pub fn transports(&self) -> &Vec<String> {
        &self.transports
    }

    /// Get a reference to this `PluggableTransportConfig`'s `PathBuf` containing the absolute path to the pluggable-transport binary.
    pub fn path_to_binary(&self) -> &PathBuf {
        &self.path_to_binary
    }

    /// Get a reference to this `PluggableTransportConfig`'s list of command-line options
    pub fn options(&self) -> &Vec<String> {
        &self.options
    }

    /// Add a command-line option used to invoke this pluggable-transport.
    pub fn add_option(&mut self, arg: String) {
        self.options.push(arg);
    }
}

/// Configuration for a bridge line to be used with a pluggable-transport
#[derive(Clone, Debug)]
pub struct BridgeLine {
    transport: String,
    address: SocketAddr,
    fingerprint: String,
    keyvalues: Vec<(String, String)>,
}

#[derive(thiserror::Error, Debug)]
/// Error returned on failure to construct a [`BridgeLine`]
pub enum BridgeLineError {
    #[error("bridge line '{0}' missing transport")]
    /// Provided bridge line missing transport
    TransportMissing(String),

    #[error("bridge line '{0}' missing address")]
    /// Provided bridge line missing address
    AddressMissing(String),

    #[error("bridge line '{0}' missing fingerprint")]
    /// Provided bridge line missing fingerprint
    FingerprintMissing(String),

    #[error("transport name '{0}' is invalid")]
    /// Invalid transport name (must be a valid C identifier)
    TransportNameInvalid(String),

    #[error("address '{0}' cannot be parsed as IP:PORT")]
    /// Provided bridge line's address not parseable
    AddressParseFailed(String),

    #[error("key=value '{0}' is invalid")]
    /// A key/value pair in invalid format
    KeyValueInvalid(String),

    #[error("bridge address port must not be 0")]
    /// Invalid bridge address port
    AddressPortInvalid,

    #[error("fingerprint '{0}' is invalid")]
    /// Fingerprint is not parseable (must be length 40 base16 string)
    FingerprintInvalid(String),
}

/// A `BridgeLine` contains the information required to connect to a bridge through the means of a particular pluggable-transport (defined in a `PluggableTransportConfi`). For more information, see:
/// - [https://tb-manual.torproject.org/bridges/](https://tb-manual.torproject.org/bridges/)
impl BridgeLine {
    /// Construct a new `BridgeLine` from its constiuent parts. The `transport` argument must be a valid C identifier and must have an associated `transport` defined in an associated `PluggableTransportConfig`. The `address` must have a non-zero port. The `fingerprint` is a length 40 base16-encoded string. Finally, the keys in the `keyvalues` list must not contain space (` `) or equal (`=`) characters.
    ///
    /// In practice, bridge lines are distributed as entire strings so most consumers of these APIs are not likely to need this particular function.
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

    /// Get a reference to this `BridgeLine`'s transport field.
    pub fn transport(&self) -> &String {
        &self.transport
    }

    /// Get a reference to this `BridgeLine`'s address field.
    pub fn address(&self) -> &SocketAddr {
        &self.address
    }

    /// Get a reference to this `BridgeLine`'s fingerprint field.
    pub fn fingerprint(&self) -> &String {
        &self.fingerprint
    }

    /// Get a reference to this `BridgeLine`'s key/values field.
    pub fn keyvalues(&self) -> &Vec<(String, String)> {
        &self.keyvalues
    }

    #[cfg(feature = "legacy-tor-provider")]
    /// Serialise this `BridgeLine` to the value set via `SETCONF Bridge...` legacy c-tor control-port command.
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