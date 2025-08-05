mod asb;
mod compose;
mod electrs;
mod images;

use std::path::PathBuf;
use swap_env::prompt as config_prompt;

use crate::compose::ASB_DATA_DIR;

fn main() {
    let (bitcoin_network, monero_network) = prompt::network();

    use swap_env::{defaults::GetDefaults, env::Mainnet, env::Testnet};

    let defaults = match (bitcoin_network, monero_network) {
        (bitcoin::Network::Bitcoin, monero::Network::Mainnet) => {
            Mainnet::get_config_file_defaults().expect("defaults to be available")
        }
        (bitcoin::Network::Testnet, monero::Network::Stagenet) => {
            Testnet::get_config_file_defaults().expect("defaults to be available")
        }
        _ => panic!("Unsupported Bitcoin / Monero network combination"),
    };

    let build_type = prompt::build_type();
    let min_buy_btc = config_prompt::min_buy_amount().expect("Failed to prompt for min buy amount");
    let max_buy_btc = config_prompt::max_buy_amount().expect("Failed to prompt for max buy amount");
    let ask_spread = config_prompt::ask_spread().expect("Failed to prompt for ask spread");
    let rendezvous_points =
        config_prompt::rendezvous_points().expect("Failed to prompt for rendezvous points");
    let tor_hidden_service =
        config_prompt::tor_hidden_service().expect("Failed to prompt for tor hidden service");
    let listen_addresses = config_prompt::listen_addresses(&defaults.listen_address_tcp)
        .expect("Failed to prompt for listen addresses");

    use crate::compose::{
        IntoSpec, OrchestratorDirectories, OrchestratorImage, OrchestratorImages,
        OrchestratorInput, OrchestratorNetworks, OrchestratorPorts,
    };
    use swap_env::config::{Bitcoin, Config, Data, Maker, Monero, Network, TorConf};
    use url::Url;

    let recipe = OrchestratorInput {
        ports: OrchestratorPorts {
            monerod_rpc: 38081,
            bitcoind_rpc: 18332,
            bitcoind_p2p: 18333,
            electrs: 60001,
            asb_libp2p: 9839,
            asb_rpc_port: 9944,
        },
        networks: OrchestratorNetworks {
            monero: monero_network,
            bitcoin: bitcoin_network,
        },
        images: OrchestratorImages {
            monerod: OrchestratorImage::Registry(images::MONEROD_IMAGE.to_string()),
            electrs: OrchestratorImage::Registry(images::ELECTRS_IMAGE.to_string()),
            bitcoind: OrchestratorImage::Registry(images::BITCOIND_IMAGE.to_string()),
            asb: match build_type {
                BuildType::Source => {
                    OrchestratorImage::Build(images::ASB_IMAGE_FROM_SOURCE.clone())
                }
                BuildType::Prebuilt => OrchestratorImage::Registry(images::ASB_IMAGE.to_string()),
            },
            asb_controller: match build_type {
                BuildType::Source => {
                    OrchestratorImage::Build(images::ASB_CONTROLLER_IMAGE_FROM_SOURCE.clone())
                }
                BuildType::Prebuilt => panic!("Prebuilt ASB controller image is not supported"),
            },
        },
        directories: OrchestratorDirectories {
            asb_data_dir: PathBuf::from(ASB_DATA_DIR),
        },
    };

    let electrs_url = Url::parse(&format!("electrs:{}", recipe.ports.electrs))
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
            electrum_rpc_urls: vec![electrs_url],
            network: bitcoin_network,
            target_block: defaults.bitcoin_confirmation_target,
            use_mempool_space_fee_estimation: defaults.use_mempool_space_fee_estimation,
            // This means that we will use the default set in swap-env/src/env.rs
            finality_confirmations: None,
        },
        monero: Monero {
            daemon_url: monerod_daemon_url,
            network: monero_network,
            // We disable the monero node pool because we are using a self-hosted node
            monero_node_pool: false,
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

    // Write the compose to ./docker-compose.yml
    // Write the config to ./config.toml
    let compose = recipe.to_spec();
    std::fs::write("./docker-compose.yml", compose).expect("Failed to write docker-compose.yml");
    std::fs::write(
        "./config.toml",
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

mod prompt {
    use dialoguer::{theme::ColorfulTheme, Select};

    use crate::BuildType;

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
}
