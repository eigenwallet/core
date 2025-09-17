use dialoguer::{Select, theme::ColorfulTheme};
use swap_env::prompt as config_prompt;
use url::Url;

#[derive(Debug)]
pub enum BuildType {
    Source,
    Prebuilt,
}

#[derive(Clone)]
pub enum MoneroNodeType {
    Included,    // Run a Monero node
    Pool,        // Use the Monero Remote Node Pool with built in defaults
    Remote(Url), // Use a specific remote Monero node
}

pub enum ElectrumServerType {
    Included,         // Run a Bitcoin node and Electrum server
    Remote(Vec<Url>), // Use a specific remote Electrum server
}

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
        .with_prompt("Do you want to include a Monero node or use an existing node/remote node?")
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
