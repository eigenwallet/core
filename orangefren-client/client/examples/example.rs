use std::{path::PathBuf, str::FromStr};

use anyhow::Context;
use client::{Client, TradeStatusType};
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
            monero::Address::from_str("4Ag3EtLQ9MJGCPKzBWgjFMEVMhUc1DuNv16eTnZoWCYFfXXGvda4QBxXWYKeNv4B5T4G9TgDeceFa2okuspRUF866Xa5dKS"
            )
        .expect("Not a valid address"))
        .await?;

    let mut updates = client.watch_status(trade_id.clone()).await;
    while let Some(status) = updates.next().await {
        tracing::info!("status: {:?}", status);
        tracing::info!(
            "deposit address: {}",
            client.deposit_address(trade_id.clone()).await?
        );
        if status.status_type == TradeStatusType::Initial {
            break;
        }
    }

    let client2 = Client::new(data_dir.clone())
        .await
        .context("Error creating a client")?;

    for trade in client2.all_trades().await {
        tracing::info!("Trades from the loaded client: {}", trade);
    }

    if let Some((info, status)) = client2.trade_state_by_id(trade_id).await {
        tracing::info!("got info {}, state{:?}", info, status);
    }
    Ok(())
}
