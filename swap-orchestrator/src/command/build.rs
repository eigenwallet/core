use crate::util::CommandExt;
use crate::{command, flag};

pub async fn build() -> anyhow::Result<()> {
    println!("Pulling the latest Docker images...");
    let mut command = command!("docker", flag!("compose"), flag!("pull")).to_tokio_command()?;
    command.exec_piped().await?;

    println!("Building the Docker images... (this might take a while)");
    let mut command = command!(
        "docker",
        flag!("compose"),
        flag!("build"),
        flag!("--no-cache")
    )
    .to_tokio_command()?;
    command.exec_piped().await?;

    println!("Done!");

    Ok(())
}
