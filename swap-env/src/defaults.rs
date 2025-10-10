use crate::env::{Mainnet, Testnet};
use anyhow::{Context, Result};
use libp2p::Multiaddr;
use rust_decimal::Decimal;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use swap_fs::{system_config_dir, system_data_dir};
use url::Url;

/*
Here's the GPG signature of the donation address.

Signed by the public key present in `utils/gpg_keys/binarybaron.asc`

-----BEGIN PGP SIGNED MESSAGE-----
Hash: SHA512

87QwQmWZQwS6RvuprCqWuJgmystL8Dw6BCx8SrrCjVJhZYGc5s6kf9A2awfFfStvEGCGeNTBNqLGrHzH6d4gi7jLM2aoq9o is our donation address for Github (signed by binarybaron)
-----BEGIN PGP SIGNATURE-----

iHUEARYKAB0WIQQ1qETX9LVbxE4YD/GZt10+FHaibgUCaJTWlQAKCRCZt10+FHai
bhasAQDGrAkZu+FFwDZDUEZzrIVS42he+GeMiS+ykpXyL5I7RQD/dXCR3f39zFsK
1A7y45B3a8ZJYTzC7bbppg6cEnCoWQE=
=j+Vz
-----END PGP SIGNATURE-----
*/
pub const DEFAULT_DEVELOPER_TIP_ADDRESS_MAINNET: &str = "87QwQmWZQwS6RvuprCqWuJgmystL8Dw6BCx8SrrCjVJhZYGc5s6kf9A2awfFfStvEGCGeNTBNqLGrHzH6d4gi7jLM2aoq9o";
pub const DEFAULT_DEVELOPER_TIP_ADDRESS_STAGENET: &str = "54ZYC5tgGRoKMJDLviAcJF2aHittSZGGkFZE6wCLkuAdUyHaaiQrjTxeSyfvxycn3yiexL4YNqdUmHuaReAk6JD4DQssQcF";

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
        "/dns4/rendezvous.observer/tcp/8888/p2p/12D3KooWMjceGXrYuGuDMGrfmJxALnSDbK4km6s1i1sJEgDTgGQa".parse().unwrap(),
        "/dns4/aswap.click/tcp/8888/p2p/12D3KooWQzW52mdsLHTMu1EPiz3APumG6vGwpCuyy494MAQoEa5X".parse().unwrap(),
        "/dns4/getxmr.st/tcp/8888/p2p/12D3KooWHHwiz6WDThPT8cEurstomg3kDSxzL2L8pwxfyX2fpxVk".parse().unwrap()
    ]
}

pub fn default_electrum_servers_mainnet() -> Vec<Url> {
    vec![
        Url::parse("ssl://electrum.blockstream.info:50002")
            .expect("default electrum server url to be valid"),
        Url::parse("tcp://electrum.blockstream.info:50001")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://bitcoin.stackwallet.com:50002")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://b.1209k.com:50002").expect("default electrum server url to be valid"),
        Url::parse("tcp://electrum.coinucopia.io:50001")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://mainnet.foundationdevices.com:50002")
            .expect("default electrum server url to be valid"),
        Url::parse("tcp://bitcoin.lu.ke:50001").expect("default electrum server url to be valid"),
        Url::parse("tcp://se-mma-crypto-payments-001.mullvad.net:50001")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://electrum.coinfinity.co:50002")
            .expect("default electrum server url to be valid"),
    ]
}

pub fn default_electrum_servers_testnet() -> Vec<Url> {
    vec![
        Url::parse("ssl://ax101.blockeng.ch:60002")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://blackie.c3-soft.com:57006")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://v22019051929289916.bestsrv.de:50002")
            .expect("default electrum server url to be valid"),
        Url::parse("tcp://v22019051929289916.bestsrv.de:50001")
            .expect("default electrum server url to be valid"),
        Url::parse("tcp://electrum.blockstream.info:60001")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://electrum.blockstream.info:60002")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://blockstream.info:993").expect("default electrum server url to be valid"),
        Url::parse("tcp://blockstream.info:143").expect("default electrum server url to be valid"),
        Url::parse("ssl://testnet.qtornado.com:51002")
            .expect("default electrum server url to be valid"),
        Url::parse("tcp://testnet.qtornado.com:51001")
            .expect("default electrum server url to be valid"),
        Url::parse("tcp://testnet.aranguren.org:51001")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://testnet.aranguren.org:51002")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://testnet.qtornado.com:50002")
            .expect("default electrum server url to be valid"),
        Url::parse("ssl://bitcoin.devmole.eu:5010")
            .expect("default electrum server url to be valid"),
        Url::parse("tcp://bitcoin.devmole.eu:5000")
            .expect("default electrum server url to be valid"),
    ]
}

pub trait GetDefaults {
    fn get_config_file_defaults() -> Result<Defaults>;
}

pub struct Defaults {
    pub config_path: PathBuf,
    pub data_dir: PathBuf,
    pub listen_address_tcp: Multiaddr,
    pub electrum_rpc_urls: Vec<Url>,
    pub price_ticker_ws_url: Url,
    pub bitcoin_confirmation_target: u16,
    pub use_mempool_space_fee_estimation: bool,
    pub developer_tip: Decimal,
}

impl GetDefaults for Mainnet {
    fn get_config_file_defaults() -> Result<Defaults> {
        let defaults = Defaults {
            config_path: default_asb_config_dir()?
                .join("mainnet")
                .join("config.toml"),
            data_dir: default_asb_data_dir()?.join("mainnet"),
            listen_address_tcp: Multiaddr::from_str("/ip4/0.0.0.0/tcp/9939")?,
            electrum_rpc_urls: default_electrum_servers_mainnet(),
            price_ticker_ws_url: Url::parse(KRAKEN_PRICE_TICKER_WS_URL)?,
            bitcoin_confirmation_target: 1,
            use_mempool_space_fee_estimation: true,
            developer_tip: Decimal::ZERO,
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
            electrum_rpc_urls: default_electrum_servers_testnet(),
            price_ticker_ws_url: Url::parse(KRAKEN_PRICE_TICKER_WS_URL)?,
            bitcoin_confirmation_target: 1,
            use_mempool_space_fee_estimation: true,
            developer_tip: Decimal::ZERO,
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
