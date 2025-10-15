use std::{fs::File, io::Write, path::PathBuf};

use crate::command::prompt::{self, ElectrumServerType, MoneroNodeType};
use crate::docker::containers::add_maker_services;
use crate::{docker::compose::ComposeConfig, util::unix_epoch_secs};
use anyhow::{Context, Result, bail};
use dialoguer::{Select, theme::ColorfulTheme};
use swap_env::config::{Bitcoin, Data, Maker, Monero, Network, TorConf};
use swap_env::defaults::GetDefaults;
use swap_env::{
    config::Config,
    env::{Mainnet, Testnet},
    prompt as config_prompt,
};

pub async fn init(
    bitcoin_network: bitcoin::Network,
    monero_network: monero::Network,
) -> anyhow::Result<()> {
    // Read the already existing config, if it's there
    let existing_config: Option<Result<Config>> =
        match swap_env::config::read_config(PathBuf::from(crate::CONFIG_PATH)) {
            Ok(Ok(config)) => Some(Ok(config)),
            Ok(Err(_)) => None,
            Err(err) => Some(Err(err)),
        };

    // Run the wizard and generate the configs, if necessary.
    // If the maker config already exists, it will be returned as-is and only the docker config re-generated.
    let (maker_config, compose_config) =
        setup_wizard(existing_config, bitcoin_network, monero_network)
            .context("Couldn't execute swap wizard")?;

    // Write the configs to their files.
    let maker_config_stringified = toml::to_string(&maker_config)?;
    File::create(crate::CONFIG_PATH)
        .context("Can't create maker config.toml file")?
        .write_all(maker_config_stringified.as_bytes())?;
    let compose_config_stringified = compose_config.build();
    File::create(crate::DOCKER_COMPOSE_PATH)
        .context("Can't create docker-compose.yml file")?
        .write_all(compose_config_stringified.as_bytes())?;

    Ok(())
}

/// Take a possibly already existing config.toml and (if necessary) go through the wizard steps necessary to
/// (if necessary) generate it and the docker-compose.yml
///
/// # Returns
/// The complete maker config.toml and docker compose config.
fn setup_wizard(
    existing_config: Option<Result<Config>>,
    bitcoin_network: bitcoin::Network,
    monero_network: monero::Network,
) -> Result<(Config, ComposeConfig)> {
    // If we already have a valid config, just use it and deduce the monero/bitcoin settings
    if let Some(Ok(config)) = existing_config {
        // If the config points to our local electrs node, we must have previously created it
        let create_full_bitcoin_node = config
            .bitcoin
            .electrum_rpc_urls
            .iter()
            .any(|url| url.as_str().contains("tcp://electrs:"));
        // Same for monero
        let create_full_monero_node = config
            .monero
            .daemon_url
            .as_ref()
            .is_some_and(|url| url.as_str().contains("http://monerod:"));

        let name = compose_name(config.bitcoin.network, config.monero.network)?;
        let mut compose = ComposeConfig::new(name);
        crate::docker::containers::add_maker_services(
            &mut compose,
            config.bitcoin.network,
            config.monero.network,
            create_full_bitcoin_node,
            create_full_monero_node,
        );

        return Ok((config, compose));
    }

    // If we have an invalid config we offer to procede as if there was no config and rename the old one
    if let Some(Err(err)) = existing_config {
        println!(
            "Error: We couldn't parse your existing config.toml file (`{}`)",
            err
        );

        let proposed_filename = format!("config.toml.invalid-{}", unix_epoch_secs());

        let choice = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("How do you want to procede?")
            .item(format!(
                "Start wizard from scratch and rename my existing `config.toml` to `{proposed_filename}`"
            ))
            .item("Abort and leave my `config.toml` alone")
            .interact()?;

        if choice != 0 {
            println!("Stopping wizard. Goodbye!");
            bail!("User doesn't want to procede.")
        }

        std::fs::rename(crate::CONFIG_PATH, &proposed_filename)?;
        println!("Renamed your old config to `{proposed_filename}`.")
    }

    let defaults = match (bitcoin_network, monero_network) {
        (bitcoin::Network::Bitcoin, monero::Network::Mainnet) => {
            Mainnet::get_config_file_defaults()?
        }
        (bitcoin::Network::Testnet, monero::Network::Stagenet) => {
            Testnet::get_config_file_defaults()?
        }
        (a, b) => bail!("unsupported network combo (bitocoin={a}, monero={b:?}"),
    };

    // At this point we either have no or an invalid config, so we do the whole wizard.

    // Maker questions (spread, max, min etc)
    let min_buy = config_prompt::min_buy_amount()?;
    let max_buy = config_prompt::max_buy_amount()?;
    let markup = config_prompt::ask_spread()?;
    // Networking: rendezvous points, hidden service, etc.
    let rendezvous_points = config_prompt::rendezvous_points()?;
    let hidden_service = config_prompt::tor_hidden_service()?;
    let listen_addresses = config_prompt::listen_addresses(&defaults.listen_address_tcp)?;
    // Monero and Electrum node types (local vs remote)
    let monero_node_type = prompt::monero_node_type();
    let electrum_node_type = prompt::electrum_server_type(&defaults.electrum_rpc_urls);
    // Whether to tip the devs
    let tip = config_prompt::developer_tip()?;

    // Derive docker compose config from
    let create_full_bitcoin_node = matches!(electrum_node_type, ElectrumServerType::Included);
    let create_full_monero_node = matches!(monero_node_type, MoneroNodeType::Included);

    let mut compose = ComposeConfig::new(compose_name(bitcoin_network, monero_network)?);
    let (asb_data, compose_electrs_url, compose_monerd_rpc_url) = add_maker_services(
        &mut compose,
        bitcoin_network,
        monero_network,
        create_full_bitcoin_node,
        create_full_monero_node,
    );

    let actual_electrum_rpc_urls = match electrum_node_type {
        ElectrumServerType::Included => vec![compose_electrs_url],
        ElectrumServerType::Remote(remote_nodes) => remote_nodes,
    };
    // None means Monero RPC pool
    let actual_monerod_url = match monero_node_type {
        MoneroNodeType::Included => Some(compose_monerd_rpc_url),
        MoneroNodeType::Remote(remote_node) => Some(remote_node),
        MoneroNodeType::Pool => None,
    };

    let config = Config {
        data: Data {
            dir: asb_data.as_root_dir(),
        },
        maker: Maker {
            max_buy_btc: max_buy,
            min_buy_btc: min_buy,
            ask_spread: markup,
            external_bitcoin_redeem_address: None,
            price_ticker_ws_url: defaults.price_ticker_ws_url,
            developer_tip: tip,
        },
        bitcoin: Bitcoin {
            electrum_rpc_urls: actual_electrum_rpc_urls,
            target_block: defaults.bitcoin_confirmation_target,
            // None means use default from env.rs
            finality_confirmations: None,
            network: bitcoin_network,
            use_mempool_space_fee_estimation: defaults.use_mempool_space_fee_estimation,
        },
        monero: Monero {
            daemon_url: actual_monerod_url,
            // None means use default from env.rs
            finality_confirmations: None,
            network: monero_network,
        },
        network: Network {
            listen: listen_addresses,
            rendezvous_point: rendezvous_points,
            external_addresses: vec![],
        },
        tor: TorConf {
            register_hidden_service: hidden_service,
            ..Default::default()
        },
    };

    Ok((config, compose))
}

fn compose_name(
    bitcoin_network: bitcoin::Network,
    monero_network: monero::Network,
) -> Result<String> {
    let monero_network_str = match monero_network {
        monero::Network::Mainnet => "mainnet",
        monero::Network::Stagenet => "stagenet",
        _ => bail!("unknown monero network"),
    };
    let bitcoin_network_str = match bitcoin_network {
        bitcoin::Network::Bitcoin => "bitcoin",
        bitcoin::Network::Testnet => "testnet",
        _ => bail!("unknown bitcoin network"),
    };
    Ok(format!(
        "bitcoin_{bitcoin_network_str}_monero_{monero_network_str}"
    ))
}
