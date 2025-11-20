use std::{os::macos::raw::stat, path::PathBuf, str::FromStr};

use anyhow::Context;
use orangefren_client::{Client, TradeStatusType};
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

    let address = monero::Address::from_str(
        "4Ag3EtLQ9MJGCPKzBWgjFMEVMhUc1DuNv16eTnZoWCYFfXXGvda4QBxXWYKeNv4B5T4G9TgDeceFa2okuspRUF866Xa5dKS",
    )?;

    let mut client = Client::new(data_dir.clone())
        .await
        .context("Error creating a client")?;
    let (trade_id, _trade_info) = client
        .new_trade(bitcoin::Amount::from_sat(1), address)
        .await
        .expect("Not a valid address");

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

    let mut client2 = Client::new(data_dir.clone())
        .await
        .context("Error creating a client")?;

    for trade in client2.all_trades().await {
        tracing::info!("Trades from the loaded client: {}", trade);
    }

    let (recovered_info, recovered_path_id) =
        client2.recover_trade_by_withdraw_address(address).await?;

    tracing::info!("recovered info {}", recovered_info);

    let mut updates2 = client2.watch_status(recovered_path_id.clone()).await;

    while let Some(status) = updates2.next().await {
        tracing::info!("status: {:?}", status);
        tracing::info!(
            "deposit address: {}",
            client2.deposit_address(recovered_path_id.clone()).await?
        );
    }
    Ok(())
}
