mod compose;
mod containers;
mod images;
mod prompt;

use anyhow::{Result, anyhow, bail};
use dialoguer::Select;
use dialoguer::theme::ColorfulTheme;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use swap_env::config::{
    Bitcoin, Config, ConfigNotInitialized, Data, Maker, Monero, Network, TorConf,
};
use swap_env::defaults::{
    Defaults, default_electrum_servers_mainnet, default_electrum_servers_testnet,
};
use swap_env::prompt::{self as config_prompt, print_info_box};
use swap_env::{defaults::GetDefaults, env::Mainnet, env::Testnet};
use swap_orchestrator::compose::{Command, ComposeConfig, Flag, ImageSource, Mount, Service};
use url::Url;

const CONFIG_PATH: &str = "config.toml";

fn main() {
    // Default to mainnet, switch to testnet when `--testnet` flag is provided
    let (mut bitcoin_network, mut monero_network) =
        (bitcoin::Network::Bitcoin, monero::Network::Mainnet);

    for arg in std::env::args() {
        match arg.as_str() {
            "--help" => {
                println!(
                    "Look at our documentation: https://github.com/eigenwallet/core/blob/master/swap-orchestrator/README.md"
                );
                return;
            }
            "--testnet" => {
                println!(
                    "Detected `--testnet` flag, switching to Bitcoin Testnet3 and Monero Stagenet"
                );
                bitcoin_network = bitcoin::Network::Testnet;
                monero_network = monero::Network::Stagenet;
            }
            _ => (),
        }
    }

    let defaults = match (bitcoin_network, monero_network) {
        (bitcoin::Network::Bitcoin, monero::Network::Mainnet) => {
            Mainnet::get_config_file_defaults().expect("defaults to be available")
        }
        (bitcoin::Network::Testnet, monero::Network::Stagenet) => {
            Testnet::get_config_file_defaults().expect("defaults to be available")
        }
        _ => panic!("Unsupported Bitcoin / Monero network combination"),
    };

    let existing_config: Option<anyhow::Result<Config>> =
        match swap_env::config::read_config(PathBuf::from(CONFIG_PATH)) {
            Ok(Ok(config)) => Some(Ok(config)),
            Ok(Err(ConfigNotInitialized)) => None,
            Err(err) => Some(Err(anyhow!(err))),
        };

    let (config, create_full_bitcoin_node, create_full_monero_node) =
        setup_wizard(existing_config, defaults).unwrap();
    {
        let mut compose = ComposeConfig::default();

        containers::add_maker_services(
            &mut compose,
            bitcoin_network,
            monero_network,
            create_full_bitcoin_node,
            create_full_monero_node,
        );

        let yml_config = compose.build();

        File::create("docker-compose.yml")
            .unwrap()
            .write_all(yml_config.as_bytes())
            .unwrap();
    }
}

/// Take a possibly already existing config.toml and (if necessary) go through the wizard steps necessary to
/// (if necessary) generate it and the docker-compose.yml
///
/// # Returns
/// The complete config, whether to create a full bitcoin/electrum node and whether to create a full monero node
fn setup_wizard(
    existing_config: Option<Result<Config>>,
    defaults: Defaults,
) -> Result<(Config, bool, bool)> {
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

        return Ok((config, create_full_bitcoin_node, create_full_monero_node));
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

        std::fs::rename(CONFIG_PATH, &proposed_filename)?;
        println!("Renamed your old config to `{proposed_filename}`.")
    }

    // At this point we either have no or an invalid config, so we do the whole wizard.
    println!("Starting the wizard.");

    // we need
    //  - monero node type
    //  - electrum node type
    //  - min buy
    //  - max buy
    //  - markup
    //  - rendezvous points
    //  - hidden service
    //  - listen addresses
    //  - tip

    let min_buy = config_prompt::min_buy_amount()?;
    let max_buy = config_prompt::max_buy_amount()?;
    let markup = config_prompt::ask_spread()?;
    let rendezvous_points = config_prompt::rendezvous_points()?;
    let hidden_service = config_prompt::tor_hidden_service()?;
    let listen_addresses = config_prompt::listen_addresses(&defaults.listen_address_tcp)?;

    let monero_node_type = prompt::monero_node_type();
    let electrum_node_type = prompt::electrum_server_type(&defaults.electrum_rpc_urls);

    let tip = config_prompt::developer_tip()?;

    bail!("unimplemented")
}

fn unix_epoch_secs() -> u64 {
    std::time::UNIX_EPOCH
        .elapsed()
        .expect("unix epoch to be elapsed")
        .as_secs()
}
