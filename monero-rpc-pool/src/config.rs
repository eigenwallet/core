use monero_address::Network;
use std::path::PathBuf;

use crate::TorClientArc;

#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub tor_client: Option<TorClientArc>,
    pub network: Network,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("data_dir", &self.data_dir)
            .field("tor_client", &self.tor_client.is_some())
            .field("network", &self.network)
            .finish()
    }
}

impl Config {
    pub fn new_with_port(host: String, port: u16, data_dir: PathBuf, network: Network) -> Self {
        Self::new_with_port_and_tor_client(host, port, data_dir, None, network)
    }

    pub fn new_with_port_and_tor_client(
        host: String,
        port: u16,
        data_dir: PathBuf,
        tor_client: impl Into<Option<TorClientArc>>,
        network: Network,
    ) -> Self {
        Self {
            host,
            port,
            data_dir,
            tor_client: tor_client.into(),
            network,
        }
    }

    pub fn new_random_port(data_dir: PathBuf, network: Network) -> Self {
        Self::new_random_port_with_tor_client(data_dir, None, network)
    }

    pub fn new_random_port_with_tor_client(
        data_dir: PathBuf,
        tor_client: impl Into<Option<TorClientArc>>,
        network: Network,
    ) -> Self {
        Self::new_with_port_and_tor_client(
            "127.0.0.1".to_string(),
            0,
            data_dir,
            tor_client,
            network,
        )
    }
}
