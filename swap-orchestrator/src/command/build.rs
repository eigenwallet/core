use crate::{command, flag};

pub async fn build() -> anyhow::Result<()> {
    println!("Pulling the latest Docker images...");
    command!("docker", flag!("compose"), flag!("pull"))
        .exec_piped(false)
        .await?;

    println!("Building the Docker images... (this might take a while - up to a few hours depending on your machine)");
    command!(
        "docker",
        flag!("compose"),
        flag!("build"),
        flag!("--no-cache")
    )
    .exec_piped(true)
    .await?;

    println!("Done!");

    Ok(())
}
