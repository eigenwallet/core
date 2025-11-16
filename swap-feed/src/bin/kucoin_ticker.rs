use anyhow::{Context, Result};
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_env_filter("debug").finish(),
    )?;

    let price_ticker_rest_url_kucoin = Url::parse("https://api.kucoin.com/api/v1/bullet-public")?;
    let mut ticker =
        swap_feed::kucoin::connect(price_ticker_rest_url_kucoin, reqwest::Client::new())
            .context("Failed to connect to kucoin")?;

    loop {
        match ticker.wait_for_next_update().await? {
            Ok(update) => println!("Price update: {}", update.1.ask),
            Err(e) => println!("Error: {e:#}"),
        }
    }
}
