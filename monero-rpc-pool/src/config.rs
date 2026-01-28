use monero_address::Network;
use std::path::PathBuf;
use swap_tor::TorBackend;

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub tor_client: TorBackend,
    pub network: Network,
}

impl Config {
    pub fn new_with_port(host: String, port: u16, data_dir: PathBuf, network: Network) -> Self {
        Self::new_with_port_and_tor_client(host, port, data_dir, TorBackend::None, network)
    }

    pub fn new_with_port_and_tor_client(
        host: String,
        port: u16,
        data_dir: PathBuf,
        tor_client: TorBackend,
        network: Network,
    ) -> Self {
        Self {
            host,
            port,
            data_dir,
            tor_client,
            network,
        }
    }

    pub fn new_random_port(data_dir: PathBuf, network: Network) -> Self {
        Self::new_random_port_with_tor_client(data_dir, TorBackend::None, network)
    }

    pub fn new_random_port_with_tor_client(
        data_dir: PathBuf,
        tor_client: TorBackend,
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
