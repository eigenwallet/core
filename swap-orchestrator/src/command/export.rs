use anyhow::Context;

pub async fn export() -> anyhow::Result<()> {
    let volume_path = crate::docker::get_volume_path("asb-data")
        .await
        .context("Couldn't get the path to the ASB data volume")?;

    println!("ASB data volume path: {}", volume_path.display());

    Ok(())
}
