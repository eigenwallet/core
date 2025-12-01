use std::{io::{Write, stdout}, process::exit};

use crate::{command, flag, util::{is_compose_container_running, is_docker_compose_running}};

pub async fn controller() -> anyhow::Result<()> {
    if !is_docker_compose_running().await {
        println!("Docker Compose is not running. Start it first and try again.");
        exit(1);
    }

    if !is_compose_container_running("asb-controller").await {
        println!("ASB controller is not running. Start it by running `orchestrator start`.");
        exit(1);
    }
    
    let cmd = command!(
        "docker",
        flag!("compose"),
        flag!("attach"),
        flag!("asb-controller")
    );

    // Prompt for confirmation
    cmd.confirm()?;

    // Print fake prompt before attaching (the real prompt won't appear immediately)
    print!("Entering ASB controller. Type \"help\" for a list of commands\nasb> ");
    stdout().flush()?;

    // Execute without confirmation (we already confirmed above)
    cmd.exec_piped(false).await?;

    Ok(())
}
