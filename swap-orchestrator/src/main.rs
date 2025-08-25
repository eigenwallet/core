mod asb;
mod compose;
mod electrs;
mod images;

use crate::compose::{
    IntoSpec, OrchestratorDirectories, OrchestratorImage, OrchestratorImages, OrchestratorInput,
    OrchestratorNetworks, DOCKER_COMPOSE_FILE,
};
use std::path::PathBuf;
use swap_env::config::{Bitcoin, Config, Data, Maker, Monero, Network, TorConf};
use swap_env::prompt as config_prompt;
use swap_env::{defaults::GetDefaults, env::Mainnet, env::Testnet};
use url::Url;

use crate::compose::ASB_DATA_DIR;

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

    // TOOD: Allow pre-built images here
    //let build_type = prompt::build_type();

    let min_buy_btc = config_prompt::min_buy_amount().expect("Failed to prompt for min buy amount");
    let max_buy_btc = config_prompt::max_buy_amount().expect("Failed to prompt for max buy amount");
    let ask_spread = config_prompt::ask_spread().expect("Failed to prompt for ask spread");
    let rendezvous_points =
        config_prompt::rendezvous_points().expect("Failed to prompt for rendezvous points");
    let tor_hidden_service =
        config_prompt::tor_hidden_service().expect("Failed to prompt for tor hidden service");
    let listen_addresses = config_prompt::listen_addresses(&defaults.listen_address_tcp)
        .expect("Failed to prompt for listen addresses");
    let monero_node_type = prompt::monero_node_type();
    let electrum_server_type = prompt::electrum_server_type(&defaults.electrum_rpc_urls);

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
            monerod: OrchestratorImage::Registry(images::MONEROD_IMAGE.to_string()),
            electrs: OrchestratorImage::Registry(images::ELECTRS_IMAGE.to_string()),
            bitcoind: OrchestratorImage::Registry(images::BITCOIND_IMAGE.to_string()),
            // TODO: Allow pre-built images here
            asb: OrchestratorImage::Build(images::ASB_IMAGE_FROM_SOURCE.clone()),
            // TODO: Allow pre-built images here
            asb_controller: OrchestratorImage::Build(
                images::ASB_CONTROLLER_IMAGE_FROM_SOURCE.clone(),
            ),
        },
        directories: OrchestratorDirectories {
            asb_data_dir: PathBuf::from(ASB_DATA_DIR),
        },
    };

    let electrs_url = Url::parse(&format!("tcp://electrs:{}", recipe.ports.electrs))
        .expect("electrs url to be convertible to a valid url");

    let monerod_daemon_url = Url::parse(&format!("http://monerod:{}", recipe.ports.monerod_rpc))
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
                ElectrumServerType::Included => vec![electrs_url],
                ElectrumServerType::Remote(electrum_servers) => electrum_servers,
            },
            network: bitcoin_network,
            target_block: defaults.bitcoin_confirmation_target,
            use_mempool_space_fee_estimation: defaults.use_mempool_space_fee_estimation,
            // This means that we will use the default set in swap-env/src/env.rs
            finality_confirmations: None,
        },
        monero: Monero {
            daemon_url: match monero_node_type.clone() {
                MoneroNodeType::Included => Some(monerod_daemon_url),
                MoneroNodeType::Pool => None,
                MoneroNodeType::Remote(url) => Some(url),
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
        },
    };

    // Write the compose to ./docker-compose.yml and the config to ./config.toml
    let asb_config_path = recipe.directories.asb_config_path_on_host();
    let compose = recipe.to_spec();

    std::fs::write(DOCKER_COMPOSE_FILE, compose).expect("Failed to write docker-compose.yml");
    std::fs::write(
        asb_config_path,
        toml::to_string(&config).expect("Failed to write config.toml"),
    )
    .expect("Failed to write config.toml");

    println!();
    println!("Run `docker compose up -d` to start the services.");
}

#[derive(Debug)]
enum BuildType {
    Source,
    Prebuilt,
}

#[derive(Clone)]
enum MoneroNodeType {
    Included,    // Run a Monero node
    Pool,        // Use the Monero Remote Node Pool with built in defaults
    Remote(Url), // Use a specific remote Monero node
}

enum ElectrumServerType {
    Included,         // Run a Bitcoin node and Electrum server
    Remote(Vec<Url>), // Use a specific remote Electrum server
}

mod prompt {
    use dialoguer::{theme::ColorfulTheme, Select};
    use swap_env::prompt as config_prompt;
    use url::Url;

    use crate::{BuildType, ElectrumServerType, MoneroNodeType};

    pub fn network() -> (bitcoin::Network, monero::Network) {
        let network = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Which network do you want to run on?")
            .items(&[
                "Mainnet Bitcoin & Mainnet Monero",
                "Testnet Bitcoin & Stagenet Monero",
            ])
            .default(0)
            .interact()
            .expect("Failed to select network");

        match network {
            0 => (bitcoin::Network::Bitcoin, monero::Network::Mainnet),
            1 => (bitcoin::Network::Testnet, monero::Network::Stagenet),
            _ => unreachable!(),
        }
    }

    #[allow(dead_code)] // will be used in the future
    pub fn build_type() -> BuildType {
        let build_type = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("How do you want to build the Docker image for the ASB?")
            .items(&[
                "Build Docker image from source (can take >1h)",
                "Prebuild Docker image (pinned to a specific commit with SHA256 hash)",
            ])
            .default(0)
            .interact()
            .expect("Failed to select build type");

        match build_type {
            0 => BuildType::Source,
            1 => BuildType::Prebuilt,
            _ => unreachable!(),
        }
    }

    pub fn monero_node_type() -> MoneroNodeType {
        let node_choice = Select::with_theme(&ColorfulTheme::default())
            .with_prompt(
                "Do you want to include a Monero node or use an existing node/remote node?",
            )
            .items(&[
                "Include a full Monero node",
                "Use an existing node or remote node",
            ])
            .default(0)
            .interact()
            .expect("Failed to select node choice");

        match node_choice {
            0 => MoneroNodeType::Included,
            1 => {
                match config_prompt::monero_daemon_url()
                    .expect("Failed to prompt for Monero daemon URL")
                {
                    Some(url) => MoneroNodeType::Remote(url),
                    None => MoneroNodeType::Pool,
                }
            }
            _ => unreachable!(),
        }
    }

    pub fn electrum_server_type(default_electrum_urls: &Vec<Url>) -> ElectrumServerType {
        let electrum_server_type = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("How do you want to connect to the Bitcoin network?")
            .items(&[
                "Run a full Bitcoin node & Electrum server",
                "List of remote Electrum servers",
            ])
            .default(0)
            .interact()
            .expect("Failed to select electrum server type");

        match electrum_server_type {
            0 => ElectrumServerType::Included,
            1 => {
                println!("Okay, let's use remote Electrum servers!");

                let electrum_servers = config_prompt::electrum_rpc_urls(default_electrum_urls)
                    .expect("Failed to prompt for electrum servers");

                ElectrumServerType::Remote(electrum_servers)
            }
            _ => unreachable!(),
        }
    }
}
