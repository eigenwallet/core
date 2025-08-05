mod cli;
mod repl;

use anyhow::Context;
use clap::Parser;
use cli::{Cli, Cmd};
use swap_controller_api::AsbApiClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let client = jsonrpsee::http_client::HttpClientBuilder::default().build(&cli.url)?;

    match cli.cmd {
        None => repl::run(client, dispatch).await,
        Some(cmd) => dispatch(cmd, client.clone()).await,
    }?;

    Ok(())
}

async fn dispatch(cmd: Cmd, client: impl AsbApiClient) -> anyhow::Result<()> {
    match cmd {
        Cmd::CheckConnection => match client.check_connection().await {
            Ok(()) => println!("Connected"),
            Err(e) => println!("Connection failed: {:?}", e),
        },
    }
    Ok(())
}
