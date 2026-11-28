use anyhow::{Context, Result};
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_env_filter("debug").finish(),
    )?;

    let rest_url = Url::parse("https://api.htx.com/market/detail/merged")?;
    let mut ticker = swap_feed::htx::connect(rest_url, reqwest::Client::new())
        .context("Failed to connect to HTX")?;

    loop {
        match ticker.wait_for_next_update().await? {
            Ok(update) => println!("Price update: {}", update.1.ask),
            Err(e) => println!("Error: {e:#}"),
        }
    }
}
