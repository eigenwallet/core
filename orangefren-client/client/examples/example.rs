use std::{path::PathBuf, str::FromStr};

use anyhow::Context;
use client::Client;
use tokio_stream::StreamExt;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    pub data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .init();

    let data_dir = Cli::parse().data_dir;

    let mut client = Client::new(data_dir.clone())
        .await
        .context("Error creating a client")?;
    let trade_id = client
        .new_trade(
            bitcoin::Amount::from_sat(1),
            monero::Address::from_str("53xRhkZNYwV6q1AgN7NT7ee5rZK8LsJxX2TkF2G2jyJmiSESQaVJrHW9nvP7aN9xGyCRuiJf9uL53GeRCu4ECvLA8fM8spF"
            )
        .expect("Not a valid address"))
        .await?;

    let mut updates = client.watch_status(trade_id.clone()).await;
    while let Some(status) = updates.next().await {
        tracing::info!("status: {:?}", status);
        client.store(status).await?;
    }

    let mut client2 = Client::new(data_dir.clone())
        .await
        .context("Error creating a client")?;

    client2.load_from_db().await?;

    if let Some((info, status)) = client2.trade_state_by_id(trade_id) {
        tracing::info!("got info {:?}, state{:?}", info, status);
    }
    Ok(())
}
