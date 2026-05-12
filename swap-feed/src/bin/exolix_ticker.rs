use anyhow::{Context, Result};
use url::Url;

/// Hand-test binary for the Exolix price feed.
///
/// Usage: `exolix_ticker <API_KEY>`
/// Alternatively, set `EXOLIX_API_KEY` in the environment.
#[tokio::main]
async fn main() -> Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_env_filter("debug").finish(),
    )?;

    let api_key = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("EXOLIX_API_KEY").ok())
        .context("Exolix API key required: pass as first arg or set EXOLIX_API_KEY")?;

    let rest_url = Url::parse("https://exolix.com/api/v2/rate")?;
    let mut ticker = swap_feed::exolix::connect(
        rest_url,
        api_key,
        std::time::Duration::from_secs(10),
        reqwest::Client::new(),
    )
    .context("Failed to connect to Exolix")?;

    loop {
        match ticker.wait_for_next_update().await? {
            Ok(update) => println!("Price update: {}", update.1.ask),
            Err(e) => println!("Error: {e:#}"),
        }
    }
}
