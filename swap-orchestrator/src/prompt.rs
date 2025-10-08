use anyhow::bail;
use dialoguer::{Input, Select, theme::ColorfulTheme};
use swap_env::{
    config::Monero,
    prompt::{self as config_prompt, print_info_box},
};
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

#[allow(dead_code)] // will be used in the future
pub fn build_type() -> BuildType {
    let build_type = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("How do you want to obtain the maker Docker image?")
        .items(&[
            "Build from source (can take >1h depending on your machine)",
            "Use a prebuilt Docker image (pinned to a specific version using a SHA256 hash)",
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
        .with_prompt("How do you want to connect to the Monero blockchain?")
        .items(&[
            "Use a mix of default remote nodes (instant)",
            "Create a full Monero node (most private - but requires 1-2 days to sync and ~500GB of disk space)",
            "I already have a node (instant)",
        ])
        .default(0)
        .interact()
        .expect("Failed to select node choice");

    match node_choice {
        0 => MoneroNodeType::Pool,
        1 => MoneroNodeType::Included,
        2 => {
            let node: Url = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Enter the address of your node")
                .interact_text()
                .expect("user to enter url");

            MoneroNodeType::Remote(node)
        }
        _ => unreachable!(),
    }
}

pub fn electrum_server_type(default_electrum_urls: &Vec<Url>) -> ElectrumServerType {
    let theme = ColorfulTheme::default();
    let select = Select::with_theme(&theme)
        .with_prompt("How do you want to connect to the Bitcoin blockchain?")
        .items(&[
            "Use a mix of default Electrum servers (instant)",
            "Create a full Bitcoin node and Electrum server (most private - but requires 1-2 days to sync and ~500GB of disk space)",
            "Specify my own Electrum server (instant)",
            "Specify my own Electrum server in addition to the mix of default Electrum servers (instant)",
            "Print the list of the default Electrum servers"
        ])
        .default(0);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Choice {
        ElectrumPool,
        RunFullNode,
        CustomElectrumNode,
        CustomElectrumNodeAndPool,
        PrintPoolUrls,
    }

    impl TryFrom<usize> for Choice {
        type Error = anyhow::Error;

        fn try_from(value: usize) -> Result<Self, Self::Error> {
            Ok(match value {
                0 => Choice::ElectrumPool,
                1 => Choice::RunFullNode,
                2 => Choice::CustomElectrumNode,
                3 => Choice::CustomElectrumNodeAndPool,
                4 => Choice::PrintPoolUrls,
                5.. => bail!("invalid choice"),
            })
        }
    }

    let mut electrum_servers: Vec<Url> = Vec::new();

    let mut choice: Choice = select
        .clone()
        .interact()
        .expect("valid choice")
        .try_into()
        .expect("valid choice");

    // Keep printing the list until the user makes and actual choice
    while choice == Choice::PrintPoolUrls {
        print_info_box(default_electrum_urls.iter().map(Url::to_string));
        choice = select
            .clone()
            .interact()
            .expect("valid choice")
            .try_into()
            .expect("valid choice");
    }

    if matches!(
        choice,
        Choice::ElectrumPool | Choice::CustomElectrumNodeAndPool
    ) {
        electrum_servers.extend_from_slice(&default_electrum_urls);
    }

    if matches!(
        choice,
        Choice::CustomElectrumNode | Choice::CustomElectrumNodeAndPool
    ) {
        let url: Url = Input::with_theme(&theme)
            .with_prompt("Please enter the url of your own Electrum server")
            .interact_text()
            .expect("invalid input");
        electrum_servers.push(url);
    }

    match choice {
        Choice::RunFullNode => ElectrumServerType::Included,
        Choice::CustomElectrumNode | Choice::CustomElectrumNodeAndPool | Choice::ElectrumPool => {
            ElectrumServerType::Remote(electrum_servers)
        }
        Choice::PrintPoolUrls => unimplemented!(),
    }
}
