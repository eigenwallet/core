// internal crates
use crate::tor_provider::TargetAddr;

#[derive(thiserror::Error, Debug)]
/// Error type for the proxy module
pub enum ProxyConfigError {
    #[error("{0}")]
    /// An error returned when constructing a proxy configuration with invalid parameters
    Generic(String),
}

#[derive(Clone, Debug)]
/// Configuration for a SOCKS4 proxy
pub struct Socks4ProxyConfig {
    pub(crate) address: TargetAddr,
}

impl Socks4ProxyConfig {
    /// Construct a new `Socks4ProxyConfig`. The `address` argument must not be a [`crate::tor_provider::TargetAddr::OnionService`] and its port must not be 0.
    pub fn new(address: TargetAddr) -> Result<Self, ProxyConfigError> {
        let port = match &address {
            TargetAddr::Socket(addr) => addr.port(),
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
/// Configuration for a SOCKS5 proxy
pub struct Socks5ProxyConfig {
    pub(crate) address: TargetAddr,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
}

impl Socks5ProxyConfig {
    /// Construct a new `Socks5ProxyConfig`. The `address` argument must not be a  [`crate::tor_provider::TargetAddr::OnionService`] and its port must not be 0. The `username` and `password` arguments, if present, must each be less than 256 bytes long.
    pub fn new(
        address: TargetAddr,
        username: Option<String>,
        password: Option<String>,
    ) -> Result<Self, ProxyConfigError> {
        let port = match &address {
            TargetAddr::Socket(addr) => addr.port(),
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
/// Configuration for an HTTP CONNECT proxy (`HTTPSProxy` in c-tor torrc configuration)
pub struct HttpsProxyConfig {
    pub(crate) address: TargetAddr,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
}

impl HttpsProxyConfig {
    /// Construct a new `HttpsProxyConfig`. The `address` argument must not be a [`crate::tor_provider::TargetAddr::OnionService`] and its port must not be 0. The `username` argument, if present, must not contain the `:` (colon) character.
    pub fn new(
        address: TargetAddr,
        username: Option<String>,
        password: Option<String>,
    ) -> Result<Self, ProxyConfigError> {
        let port = match &address {
            TargetAddr::Socket(addr) => addr.port(),
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
/// An enum representing a possible proxy server configuration with address and possible credentials.
pub enum ProxyConfig {
    /// A SOCKS4 proxy
    Socks4(Socks4ProxyConfig),
    /// A SOCKS5 proxy
    Socks5(Socks5ProxyConfig),
    /// An HTTP CONNECT proxy
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
