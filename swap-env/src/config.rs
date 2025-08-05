use crate::defaults::GetDefaults;
use crate::env::{Mainnet, Testnet};
use crate::prompt;
use anyhow::{bail, Context, Result};
use config::ConfigError;
use libp2p::core::Multiaddr;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use swap_fs::ensure_directory_exists;
use url::Url;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub data: Data,
    pub network: Network,
    pub bitcoin: Bitcoin,
    pub monero: Monero,
    pub tor: TorConf,
    pub maker: Maker,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Data {
    pub dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    #[serde(deserialize_with = "swap_serde::libp2p::multiaddresses::deserialize")]
    pub listen: Vec<Multiaddr>,
    #[serde(
        default,
        deserialize_with = "swap_serde::libp2p::multiaddresses::deserialize"
    )]
    pub rendezvous_point: Vec<Multiaddr>,
    #[serde(
        default,
        deserialize_with = "swap_serde::libp2p::multiaddresses::deserialize"
    )]
    pub external_addresses: Vec<Multiaddr>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bitcoin {
    #[serde(deserialize_with = "swap_serde::electrum::urls::deserialize")]
    pub electrum_rpc_urls: Vec<Url>,
    pub target_block: u16,
    pub finality_confirmations: Option<u32>,
    #[serde(with = "swap_serde::bitcoin::network")]
    pub network: bitcoin::Network,
    #[serde(default = "default_use_mempool_space_fee_estimation")]
    pub use_mempool_space_fee_estimation: bool,
}

fn default_use_mempool_space_fee_estimation() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Monero {
    pub daemon_url: Url,
    pub finality_confirmations: Option<u64>,
    #[serde(with = "swap_serde::monero::network")]
    pub network: monero::Network,
    #[serde(default = "default_monero_node_pool")]
    pub monero_node_pool: bool,
}

fn default_monero_node_pool() -> bool {
    false
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TorConf {
    pub register_hidden_service: bool,
    pub hidden_service_num_intro_points: u8,
}

impl Default for TorConf {
    fn default() -> Self {
        Self {
            register_hidden_service: true,
            hidden_service_num_intro_points: 5,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Maker {
    #[serde(with = "::bitcoin::amount::serde::as_btc")]
    pub min_buy_btc: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_btc")]
    pub max_buy_btc: bitcoin::Amount,
    pub ask_spread: Decimal,
    pub price_ticker_ws_url: Url,
    #[serde(default, with = "swap_serde::bitcoin::address_serde::option")]
    pub external_bitcoin_redeem_address: Option<bitcoin::Address>,
}

impl Config {
    pub fn read<D>(config_file: D) -> Result<Self, ConfigError>
    where
        D: AsRef<OsStr>,
    {
        let config_file = Path::new(&config_file);

        let config = config::Config::builder()
            .add_source(config::File::from(config_file))
            .add_source(
                config::Environment::with_prefix("ASB")
                    .separator("__")
                    .list_separator(","),
            )
            .build()?;

        config.try_into()
    }
}

impl TryFrom<config::Config> for Config {
    type Error = config::ConfigError;

    fn try_from(value: config::Config) -> Result<Self, Self::Error> {
        value.try_deserialize()
    }
}

#[derive(thiserror::Error, Debug, Clone, Copy)]
#[error("config not initialized")]
pub struct ConfigNotInitialized;

pub fn read_config(config_path: PathBuf) -> Result<Result<Config, ConfigNotInitialized>> {
    if config_path.exists() {
        tracing::info!(
            path = %config_path.display(),
            "Reading config file",
        );
    } else {
        return Ok(Err(ConfigNotInitialized {}));
    }

    let file = Config::read(&config_path)
        .with_context(|| format!("Failed to read config file at {}", config_path.display()))?;

    Ok(Ok(file))
}

pub fn initial_setup(config_path: PathBuf, config: Config) -> Result<()> {
    let toml = toml::to_string(&config)?;

    ensure_directory_exists(config_path.as_path())?;
    fs::write(&config_path, toml)?;

    tracing::info!(
        path = %config_path.as_path().display(),
        "Initial setup complete, config file created",
    );
    Ok(())
}

pub fn query_user_for_initial_config_with_network(
    bitcoin_network: bitcoin::Network,
    monero_network: monero::Network,
) -> Result<Config> {
    let defaults = match bitcoin_network {
        bitcoin::Network::Bitcoin => Mainnet::get_config_file_defaults()?,
        bitcoin::Network::Testnet => Testnet::get_config_file_defaults()?,
        _ => bail!("Unsupported bitcoin network"),
    };

    let data_dir = prompt::data_directory(&defaults.data_dir)?;
    let target_block = prompt::bitcoin_confirmation_target(defaults.bitcoin_confirmation_target)?;
    let listen_addresses = prompt::listen_addresses(&defaults.listen_address_tcp)?;
    let electrum_rpc_urls = prompt::electrum_rpc_urls(&defaults.electrum_rpc_url)?;
    let monero_daemon_url = prompt::monero_daemon_url(&defaults.monero_daemon_address)?;
    let register_hidden_service = prompt::tor_hidden_service()?;
    let min_buy = prompt::min_buy_amount()?;
    let max_buy = prompt::max_buy_amount()?;
    let ask_spread = prompt::ask_spread()?;
    let rendezvous_points = prompt::rendezvous_points()?;

    println!();

    Ok(Config {
        data: Data { dir: data_dir },
        network: Network {
            listen: listen_addresses,
            rendezvous_point: rendezvous_points,
            external_addresses: vec![],
        },
        bitcoin: Bitcoin {
            electrum_rpc_urls,
            target_block,
            finality_confirmations: None,
            network: bitcoin_network,
            use_mempool_space_fee_estimation: true,
        },
        monero: Monero {
            daemon_url: monero_daemon_url,
            finality_confirmations: None,
            network: monero_network,
            monero_node_pool: false,
        },
        tor: TorConf {
            register_hidden_service,
            ..Default::default()
        },
        maker: Maker {
            min_buy_btc: min_buy,
            max_buy_btc: max_buy,
            ask_spread,
            price_ticker_ws_url: defaults.price_ticker_ws_url,
            external_bitcoin_redeem_address: None,
        },
    })
}

pub fn query_user_for_initial_config(testnet: bool) -> Result<Config> {
    let (bitcoin_network, monero_network) = if testnet {
        let bitcoin_network = bitcoin::Network::Testnet;
        let monero_network = monero::Network::Stagenet;
        (bitcoin_network, monero_network)
    } else {
        let bitcoin_network = bitcoin::Network::Bitcoin;
        let monero_network = monero::Network::Mainnet;
        (bitcoin_network, monero_network)
    };

    query_user_for_initial_config_with_network(bitcoin_network, monero_network)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    // these tests are run serially since env vars affect the whole process
    #[test]
    #[serial]
    fn config_roundtrip_testnet() {
        let temp_dir = tempdir().unwrap().path().to_path_buf();
        let config_path = Path::join(&temp_dir, "config.toml");

        let defaults = Testnet::get_config_file_defaults().unwrap();

        let expected = Config {
            data: Data {
                dir: Default::default(),
            },
            bitcoin: Bitcoin {
                electrum_rpc_urls: vec![defaults.electrum_rpc_url],
                target_block: defaults.bitcoin_confirmation_target,
                finality_confirmations: None,
                network: bitcoin::Network::Testnet,
                use_mempool_space_fee_estimation: true,
            },
            network: Network {
                listen: vec![defaults.listen_address_tcp],
                rendezvous_point: vec![],
                external_addresses: vec![],
            },
            monero: Monero {
                daemon_url: defaults.monero_daemon_address,
                finality_confirmations: None,
                network: monero::Network::Stagenet,
                monero_node_pool: false,
            },
            tor: Default::default(),
            maker: Maker {
                min_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MIN_BUY_AMOUNT).unwrap(),
                max_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MAX_BUY_AMOUNT).unwrap(),
                ask_spread: Decimal::from_f64(DEFAULT_SPREAD).unwrap(),
                price_ticker_ws_url: defaults.price_ticker_ws_url,
                external_bitcoin_redeem_address: None,
            },
        };

        initial_setup(config_path.clone(), expected.clone()).unwrap();
        let actual = read_config(config_path).unwrap().unwrap();

        assert_eq!(expected, actual);
    }

    #[test]
    #[serial]
    fn config_roundtrip_mainnet() {
        let temp_dir = tempdir().unwrap().path().to_path_buf();
        let config_path = Path::join(&temp_dir, "config.toml");

        let defaults = Mainnet::get_config_file_defaults().unwrap();

        let expected = Config {
            data: Data {
                dir: Default::default(),
            },
            bitcoin: Bitcoin {
                electrum_rpc_urls: vec![defaults.electrum_rpc_url],
                target_block: defaults.bitcoin_confirmation_target,
                finality_confirmations: None,
                network: bitcoin::Network::Bitcoin,
                use_mempool_space_fee_estimation: true,
            },
            network: Network {
                listen: vec![defaults.listen_address_tcp],
                rendezvous_point: vec![],
                external_addresses: vec![],
            },
            monero: Monero {
                daemon_url: defaults.monero_daemon_address,
                finality_confirmations: None,
                network: monero::Network::Mainnet,
                monero_node_pool: false,
            },
            tor: Default::default(),
            maker: Maker {
                min_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MIN_BUY_AMOUNT).unwrap(),
                max_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MAX_BUY_AMOUNT).unwrap(),
                ask_spread: Decimal::from_f64(DEFAULT_SPREAD).unwrap(),
                price_ticker_ws_url: defaults.price_ticker_ws_url,
                external_bitcoin_redeem_address: None,
            },
        };

        initial_setup(config_path.clone(), expected.clone()).unwrap();
        let actual = read_config(config_path).unwrap().unwrap();

        assert_eq!(expected, actual);
    }

    #[test]
    #[serial]
    fn env_override() {
        let temp_dir = tempfile::tempdir().unwrap().path().to_path_buf();
        let config_path = Path::join(&temp_dir, "config.toml");

        let defaults = Mainnet::get_config_file_defaults().unwrap();

        let dir = PathBuf::from("/tmp/dir");
        std::env::set_var("ASB__DATA__DIR", dir.clone());
        let addr1 = "/dns4/example.com/tcp/9939";
        let addr2 = "/ip4/1.2.3.4/tcp/9940";
        let external_addresses = vec![addr1.parse().unwrap(), addr2.parse().unwrap()];
        let listen = external_addresses.clone();
        std::env::set_var(
            "ASB__NETWORK__EXTERNAL_ADDRESSES",
            format!("{},{}", addr1, addr2),
        );
        std::env::set_var("ASB__NETWORK__LISTEN", format!("{},{}", addr1, addr2));

        let expected = Config {
            data: Data { dir },
            bitcoin: Bitcoin {
                electrum_rpc_urls: vec![defaults.electrum_rpc_url],
                target_block: defaults.bitcoin_confirmation_target,
                finality_confirmations: None,
                network: bitcoin::Network::Bitcoin,
                use_mempool_space_fee_estimation: true,
            },
            network: Network {
                listen,
                rendezvous_point: vec![],
                external_addresses,
            },
            monero: Monero {
                daemon_url: defaults.monero_daemon_address,
                finality_confirmations: None,
                network: monero::Network::Mainnet,
                monero_node_pool: false,
            },
            tor: Default::default(),
            maker: Maker {
                min_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MIN_BUY_AMOUNT).unwrap(),
                max_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MAX_BUY_AMOUNT).unwrap(),
                ask_spread: Decimal::from_f64(DEFAULT_SPREAD).unwrap(),
                price_ticker_ws_url: defaults.price_ticker_ws_url,
                external_bitcoin_redeem_address: None,
            },
        };

        initial_setup(config_path.clone(), expected.clone()).unwrap();
        let actual = read_config(config_path).unwrap().unwrap();

        assert_eq!(expected, actual);
        std::env::remove_var("ASB__DATA__DIR");
        std::env::remove_var("ASB__NETWORK__EXTERNAL_ADDRESSES");
        std::env::remove_var("ASB__NETWORK__LISTEN");
    }
}
