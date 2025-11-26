use anyhow::bail;

use crate::{
    command, flag,
    util::{probe_docker, probe_maker_config},
};

pub async fn start() -> anyhow::Result<()> {
    if !matches!(probe_maker_config().await, Some(Ok(_))) {
        bail!("No valid maker config.toml file found. Please run `orchestrator init` first.");
    }

    probe_docker().await?;

    command!("docker", flag!("compose"), flag!("up"), flag!("-d"))
        .exec_piped(true)
        .await
        .map(|_| ())
}
