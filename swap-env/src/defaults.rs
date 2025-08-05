use crate::env::{Mainnet, Testnet};
use anyhow::{Context, Result};
use libp2p::Multiaddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use swap_fs::{system_config_dir, system_data_dir};
use url::Url;

pub const DEFAULT_MIN_BUY_AMOUNT: f64 = 0.002f64;
pub const DEFAULT_MAX_BUY_AMOUNT: f64 = 0.02f64;
pub const DEFAULT_SPREAD: f64 = 0.02f64;

pub const KRAKEN_PRICE_TICKER_WS_URL: &str = "wss://ws.kraken.com";

pub fn default_rendezvous_points() -> Vec<Multiaddr> {
    vec![
        "/dns4/discover.unstoppableswap.net/tcp/8888/p2p/12D3KooWA6cnqJpVnreBVnoro8midDL9Lpzmg8oJPoAGi7YYaamE".parse().unwrap(),
        "/dns4/discover2.unstoppableswap.net/tcp/8888/p2p/12D3KooWGRvf7qVQDrNR5nfYD6rKrbgeTi9x8RrbdxbmsPvxL4mw".parse().unwrap(),
        "/dns4/darkness.su/tcp/8888/p2p/12D3KooWFQAgVVS9t9UgL6v1sLprJVM7am5hFK7vy9iBCCoCBYmU".parse().unwrap(),
        "/dns4/eigen.center/tcp/8888/p2p/12D3KooWS5RaYJt4ANKMH4zczGVhNcw5W214e2DDYXnjs5Mx5zAT".parse().unwrap(),
        "/dns4/swapanarchy.cfd/tcp/8888/p2p/12D3KooWRtyVpmyvwzPYXuWyakFbRKhyXGrjhq6tP7RrBofpgQGp".parse().unwrap(),
    ]
}

pub trait GetDefaults {
    fn get_config_file_defaults() -> Result<Defaults>;
}

pub struct Defaults {
    pub config_path: PathBuf,
    pub data_dir: PathBuf,
    pub listen_address_tcp: Multiaddr,
    pub electrum_rpc_url: Url,
    pub monero_daemon_address: Url,
    pub price_ticker_ws_url: Url,
    pub bitcoin_confirmation_target: u16,
    pub use_mempool_space_fee_estimation: bool,
}

impl GetDefaults for Mainnet {
    fn get_config_file_defaults() -> Result<Defaults> {
        let defaults = Defaults {
            config_path: default_asb_config_dir()?
                .join("mainnet")
                .join("config.toml"),
            data_dir: default_asb_data_dir()?.join("mainnet"),
            listen_address_tcp: Multiaddr::from_str("/ip4/0.0.0.0/tcp/9939")?,
            electrum_rpc_url: Url::parse("ssl://blockstream.info:700")?,
            monero_daemon_address: Url::parse("http://nthpyro.dev:18089")?,
            price_ticker_ws_url: Url::parse(KRAKEN_PRICE_TICKER_WS_URL)?,
            bitcoin_confirmation_target: 1,
            use_mempool_space_fee_estimation: true,
        };

        Ok(defaults)
    }
}

impl GetDefaults for Testnet {
    fn get_config_file_defaults() -> Result<Defaults> {
        let defaults = Defaults {
            config_path: default_asb_config_dir()?
                .join("testnet")
                .join("config.toml"),
            data_dir: default_asb_data_dir()?.join("testnet"),
            listen_address_tcp: Multiaddr::from_str("/ip4/0.0.0.0/tcp/9939")?,
            electrum_rpc_url: Url::parse("ssl://electrum.blockstream.info:60002")?,
            monero_daemon_address: Url::parse("http://node.sethforprivacy.com:38089")?,
            price_ticker_ws_url: Url::parse(KRAKEN_PRICE_TICKER_WS_URL)?,
            bitcoin_confirmation_target: 1,
            use_mempool_space_fee_estimation: true,
        };

        Ok(defaults)
    }
}

fn default_asb_config_dir() -> Result<PathBuf> {
    system_config_dir()
        .map(|dir| Path::join(&dir, "asb"))
        .context("Could not generate default config file path")
}

fn default_asb_data_dir() -> Result<PathBuf> {
    system_data_dir()
        .map(|dir| Path::join(&dir, "asb"))
        .context("Could not generate default config file path")
}
