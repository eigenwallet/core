use std::path::PathBuf;

use anyhow::{Context, bail};
use tokio::{fs::File, io::AsyncReadExt};

use crate::util::probe_docker;

pub mod compose;
pub mod containers;
pub mod images;

/// Get the path of a volume in the docker compose config.
/// Errors if the volume or the docker-compose.yml file doesn't exist.
pub async fn get_volume_path(volume_name: &str) -> anyhow::Result<PathBuf> {
    probe_docker().await?;

    let mut compose_config_string = String::new();
    File::open(crate::DOCKER_COMPOSE_PATH)
        .await
        .context("Failed to open docker-compose.yml. Are you in the right directory?")?
        .read_to_string(&mut compose_config_string)
        .await?;
    let compose_config: compose_spec::Compose = serde_yaml::from_str(&compose_config_string)?;

    if !compose_config.volumes.keys().any(|key| key == volume_name) {
        bail!("Volume {volume_name} not found in docker-compose.yml");
    }

    let project_name = compose_config
        .name
        .context("docker-compose.yml doesn't have a name")?
        .to_string();

    Ok(PathBuf::from(format!(
        "/var/lib/docker/volumes/{project_name}_{volume_name}/_data"
    )))
}
