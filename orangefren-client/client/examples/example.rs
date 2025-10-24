use std::str::FromStr;

use client::Client;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let mut client = Client::new();
    let trade_id = client
        .new_trade(
            bitcoin::Amount::from_sat(1),
            monero::Address::from_str("53xRhkZNYwV6q1AgN7NT7ee5rZK8LsJxX2TkF2G2jyJmiSESQaVJrHW9nvP7aN9xGyCRuiJf9uL53GeRCu4ECvLA8fM8spF"
            )
        .expect("Not a valid address"))
        .await?;

    let mut updates = client.watch_status(trade_id).await; // no .await needed here
    while let Some(status) = updates.next().await {
        println!("status: {:?}", status);
    }
    Ok(())
}
