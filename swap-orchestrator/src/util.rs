use std::{path::PathBuf, process::Stdio};

use anyhow::{Context, Result};
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

#[allow(async_fn_in_trait)]
pub trait CommandExt {
    async fn exec_piped(&mut self) -> anyhow::Result<std::process::ExitStatus>;
}

impl CommandExt for tokio::process::Command {
    async fn exec_piped(&mut self) -> anyhow::Result<std::process::ExitStatus> {
        self.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?
            .wait()
            .await
            .context("Failed to execute command")
    }
}
