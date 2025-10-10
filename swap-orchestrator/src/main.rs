mod compose;
mod containers;
mod images;
mod prompt;

use crate::compose::{
    IntoSpec, OrchestratorDirectories, OrchestratorImage, OrchestratorImages, OrchestratorInput,
    OrchestratorNetworks, ASB_DATA_DIR, DOCKER_COMPOSE_FILE,
};
use std::path::PathBuf;
use swap_env::config::{
    Bitcoin, Config, ConfigNotInitialized, Data, Maker, Monero, Network, TorConf,
};
use swap_env::prompt as config_prompt;
use swap_env::{defaults::GetDefaults, env::Mainnet, env::Testnet};
use url::Url;

fn main() {
    let (bitcoin_network, monero_network) = prompt::network();

    let defaults = match (bitcoin_network, monero_network) {
        (bitcoin::Network::Bitcoin, monero::Network::Mainnet) => {
            Mainnet::get_config_file_defaults().expect("defaults to be available")
        }
        (bitcoin::Network::Testnet, monero::Network::Stagenet) => {
            Testnet::get_config_file_defaults().expect("defaults to be available")
        }
        _ => panic!("Unsupported Bitcoin / Monero network combination"),
    };

    let recipe = OrchestratorInput {
        ports: OrchestratorNetworks {
            monero: monero_network,
            bitcoin: bitcoin_network,
        }
        .into(),
        networks: OrchestratorNetworks {
            monero: monero_network,
            bitcoin: bitcoin_network,
        },
        images: OrchestratorImages {
            // TODO: These containers should be conditonally removed / disabled,
            // depending on if they are used by the asb
            monerod: OrchestratorImage::Registry(images::MONEROD_IMAGE.to_string()),
            electrs: OrchestratorImage::Registry(images::ELECTRS_IMAGE.to_string()),
            bitcoind: OrchestratorImage::Registry(images::BITCOIND_IMAGE.to_string()),
            // TODO: Allow pre-built images here
            asb: OrchestratorImage::Build(images::ASB_IMAGE_FROM_SOURCE.clone()),
            // TODO: Allow pre-built images here
            asb_controller: OrchestratorImage::Build(
                images::ASB_CONTROLLER_IMAGE_FROM_SOURCE.clone(),
            ),
            asb_tracing_logger: OrchestratorImage::Registry(
                images::ASB_TRACING_LOGGER_IMAGE.to_string(),
            ),
        },
        directories: OrchestratorDirectories {
            asb_data_dir: PathBuf::from(ASB_DATA_DIR),
        },
    };

    // If the config file already exists and be de-serialized,
    // we give the user the ability to skip the setup for the "asb config" (config.toml)
    //
    // The "asb config" is distinctly different from the [`monero_node_type`] and [`electrum_server_type`]
    // since these are also required to decide on the structure of the `docker-compose.yml` file
    enum ConfigExistence {
        PresentAndValid,
        Missing,
        PresentButInvalid(anyhow::Error),
    }

    let asb_config_state = match swap_env::config::read_config(
        recipe.directories.asb_config_path_on_host_as_path_buf(),
    ) {
        Ok(Ok(_)) => ConfigExistence::PresentAndValid,
        Ok(Err(ConfigNotInitialized)) => ConfigExistence::Missing,
        Err(err) => ConfigExistence::PresentButInvalid(err),
    };

    // None, means to do nothing because we already have a valid file at the correct location
    // Some(None) means to generate a config from scratch
    // Some(Some(PathBuf)) means to move the file at the location of the config file to the path
    let should_prompt_config_wizard = match asb_config_state {
        // Config is present and valid => we do not need to prompt a wizard
        ConfigExistence::PresentAndValid => None,
        // Config file is missing => force wizard
        ConfigExistence::Missing => Some(None),
        // Config is present but invalid => Ask user to rename old config file and generate new one
        ConfigExistence::PresentButInvalid(err) => {
            println!("The asb config is present but it is invalid. We were unable to parse it.");
            println!("{:?}", err);
            println!("Do you want to re-generate your asb config from scratch?");

            let unix_epoch = unix_epoch_secs();

            let renamed_file_name = format!(
                "{}.backup_at_{}",
                recipe
                    .directories
                    .asb_config_path_on_host_as_path_buf()
                    .file_name()
                    .expect("asb config file to have filename")
                    .to_str()
                    .expect("asb config file to fit into non OsStr"),
                unix_epoch
            );

            println!(
                "Your previous (invalid) config will be renamed to {}",
                renamed_file_name
            );

            let renamed_path = recipe
                .directories
                .asb_config_path_on_host_as_path_buf()
                .with_file_name(renamed_file_name);

            Some(Some(renamed_path))
        }
    };

    // If the config is invalid or doesn't exist, we prompt the user
    if let Some(should_move_old_file) = should_prompt_config_wizard {
        let min_buy_btc =
            config_prompt::min_buy_amount().expect("Failed to prompt for min buy amount");
        let max_buy_btc =
            config_prompt::max_buy_amount().expect("Failed to prompt for max buy amount");
        let ask_spread = config_prompt::ask_spread().expect("Failed to prompt for ask spread");
        let rendezvous_points =
            config_prompt::rendezvous_points().expect("Failed to prompt for rendezvous points");
        let tor_hidden_service =
            config_prompt::tor_hidden_service().expect("Failed to prompt for tor hidden service");
        let listen_addresses = config_prompt::listen_addresses(&defaults.listen_address_tcp)
            .expect("Failed to prompt for listen addresses");
        let monero_node_type = prompt::monero_node_type();
        let electrum_server_type = prompt::electrum_server_type(&defaults.electrum_rpc_urls);
        let developer_tip =
            config_prompt::developer_tip().expect("Failed to prompt for developer tip");

        let electrs_url = Url::parse(&format!("tcp://electrs:{}", recipe.ports.electrs))
            .expect("electrs url to be convertible to a valid url");
        let monerod_daemon_url =
            Url::parse(&format!("http://monerod:{}", recipe.ports.monerod_rpc))
                .expect("monerod daemon url to be convertible to a valid url");

        let config = Config {
            data: Data {
                dir: recipe.directories.asb_data_dir.clone(),
            },
            network: Network {
                listen: listen_addresses,
                rendezvous_point: rendezvous_points,
                external_addresses: vec![],
            },
            bitcoin: Bitcoin {
                electrum_rpc_urls: match electrum_server_type {
                    // If user chose the included option, we will use the electrs url from the container
                    prompt::ElectrumServerType::Included => vec![electrs_url],
                    prompt::ElectrumServerType::Remote(electrum_servers) => electrum_servers,
                },
                network: bitcoin_network,
                target_block: defaults.bitcoin_confirmation_target,
                use_mempool_space_fee_estimation: defaults.use_mempool_space_fee_estimation,
                // This means that we will use the default set in swap-env/src/env.rs
                finality_confirmations: None,
            },
            monero: Monero {
                daemon_url: match monero_node_type.clone() {
                    prompt::MoneroNodeType::Included => Some(monerod_daemon_url),
                    prompt::MoneroNodeType::Pool => None,
                    prompt::MoneroNodeType::Remote(url) => Some(url),
                },
                network: monero_network,
                // This means that we will use the default set in swap-env/src/env.rs
                finality_confirmations: None,
            },
            tor: TorConf {
                register_hidden_service: tor_hidden_service,
                ..Default::default()
            },
            maker: Maker {
                min_buy_btc,
                max_buy_btc,
                ask_spread,
                price_ticker_ws_url: defaults.price_ticker_ws_url,
                external_bitcoin_redeem_address: None,
                developer_tip,
            },
        };

        // If there was an invalid config file previously, we rename it
        if let Some(move_invalid_config_to) = should_move_old_file {
            std::fs::rename(
                recipe.directories.asb_config_path_on_host(),
                move_invalid_config_to,
            )
            .expect("to be able to move old invalid config file");
        }

        // Write the asb config to the host, the config will then be mounted into the `asb` docker container
        let asb_config_path = recipe.directories.asb_config_path_on_host();

        std::fs::write(
            asb_config_path,
            toml::to_string(&config).expect("Failed to write config.toml"),
        )
        .expect("Failed to write config.toml");
    }

    // Write the compose to ./docker-compose.yml
    let compose = recipe.to_spec();
    std::fs::write(DOCKER_COMPOSE_FILE, compose).expect("Failed to write docker-compose.yml");

    println!();
    println!("Run `docker compose up -d` to start the services.");
}

fn unix_epoch_secs() -> u64 {
    std::time::UNIX_EPOCH
        .elapsed()
        .expect("unix epoch to be elapsed")
        .as_secs()
}
