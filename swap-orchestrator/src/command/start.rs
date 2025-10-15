use anyhow::bail;

use crate::{
    command, flag,
    util::{CommandExt, probe_docker, probe_maker_config},
};

pub async fn start() -> anyhow::Result<()> {
    if !matches!(probe_maker_config().await, Some(Ok(_))) {
        bail!("No valid maker config.toml file found. Please run `orchestrator init` first.");
    }

    probe_docker().await?;

    let mut command = command!("docker", flag!("compose"), flag!("up"), flag!("-d"))
        .to_tokio_command()
        .expect("non-empty command");

    command.exec_piped().await.map(|_| ())
}
