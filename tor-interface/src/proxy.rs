// internal crates
use crate::tor_provider::TargetAddr;

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