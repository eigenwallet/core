use anyhow::{Context, Result};
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_env_filter("debug").finish(),
    )?;

    let price_ticker_ws_url_bitfinex = Url::parse("wss://api-pub.bitfinex.com/ws/2")?;
    let mut ticker = swap_feed::bitfinex::connect(price_ticker_ws_url_bitfinex)
        .context("Failed to connect to bitfinex")?;

    loop {
        match ticker.wait_for_next_update().await? {
            Ok(update) => println!("Price update: {}", update.ask),
            Err(e) => println!("Error: {e:#}"),
        }
    }
}
