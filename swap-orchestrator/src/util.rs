use std::path::PathBuf;

use anyhow::Result;
use swap_env::config::Config;

use crate::{command, flag};

/// Get the number of seconds since unix epoch.
pub fn unix_epoch_secs() -> u64 {
    std::time::UNIX_EPOCH
        .elapsed()
        .expect("unix epoch to be elapsed")
        .as_secs()
}

/// Probe that docker compose is available and the daemon is running.
pub async fn probe_docker() -> Result<()> {
    // Just a random docker command which requires the daemon to be running.
    match command!("docker", flag!("compose"), flag!("ps"))
        .to_tokio_command()
        .expect("non-empty command")
        .output()
        .await
    {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(anyhow::anyhow!(
            "Docker compose is not available. Are you sure it's installed and running?\n\nerror: {}",
            String::from_utf8(output.stderr).unwrap_or_else(|_| "unknown error".to_string())
        )),
        Err(err) => Err(anyhow::anyhow!(
            "Failed to probe docker compose. Are you sure it's installed and running?\n\nerror: {}",
            err
        )),
    }
}

/// Check whether there's a valid maker config.toml file in the current directory.
/// `None` if there isn't, `Some(Err(err))` if there is but it's invalid, `Some(Ok(config))` if there is and it's valid.
pub async fn probe_maker_config() -> Option<anyhow::Result<Config>> {
    // Read the already existing config, if it's there
    match swap_env::config::read_config(PathBuf::from(crate::CONFIG_PATH)) {
        Ok(Ok(config)) => Some(Ok(config)),
        Ok(Err(_)) => None,
        Err(err) => Some(Err(err)),
    }
}

/// Determine whether we should create a full Bitcoin node in the docker compose.
/// Returns true if the config points to the local electrs node.
pub fn should_create_full_bitcoin_node(config: &Config) -> bool {
    config
        .bitcoin
        .electrum_rpc_urls
        .iter()
        .any(|url| url.as_str().contains("tcp://electrs:"))
}

/// Determine whether we should create a full Monero node in the docker compose.
/// Returns true if the config points to the local monerod node.
pub fn should_create_full_monero_node(config: &Config) -> bool {
    config
        .monero
        .daemon_url
        .as_ref()
        .is_some_and(|url| url.as_str().contains("http://monerod:"))
}

/// Generate the docker compose project name based on the Bitcoin and Monero networks.
pub fn compose_name(
    bitcoin_network: bitcoin::Network,
    monero_network: monero::Network,
) -> Result<String> {
    let monero_network_str = match monero_network {
        monero::Network::Mainnet => "mainnet",
        monero::Network::Stagenet => "stagenet",
        _ => anyhow::bail!("unknown monero network"),
    };
    let bitcoin_network_str = match bitcoin_network {
        bitcoin::Network::Bitcoin => "mainnet",
        bitcoin::Network::Testnet => "testnet",
        _ => anyhow::bail!("unknown bitcoin network"),
    };
    Ok(format!(
        "{bitcoin_network_str}_monero_{monero_network_str}_bitcoin"
    ))
}
