use anyhow::{Context, Result};
use swap_feed::LatestRate;
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_env_filter("debug").finish(),
    )?;

    let price_ticker_ws_url_kraken = Url::parse("wss://ws.kraken.com")?;
    let kraken_ticker = swap_feed::connect_kraken(price_ticker_ws_url_kraken)
        .context("Failed to connect to kraken")?;

    let price_ticker_ws_url_bitfinex = Url::parse("wss://api-pub.bitfinex.com/ws/2")?;
    let bitfinex_ticker = swap_feed::connect_bitfinex(price_ticker_ws_url_bitfinex)
        .context("Failed to connect to bitfinex")?;

    let mut combo = swap_feed::ExchangeRate::new(
        rust_decimal::Decimal::ZERO,
        Some(kraken_ticker),
        Some(bitfinex_ticker),
        None,
        None,
        std::time::Duration::from_secs(10 * 60),
    )?;

    let mut timer = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut prev_rate = Ok(swap_feed::Rate::ZERO);
    loop {
        timer.tick().await;
        let rate = combo.latest_rate();
        if rate.as_ref().map_err(|e| e.to_string())
            != prev_rate
                .as_ref()
                .map_err(|e: &swap_feed::rate::Error| e.to_string())
        {
            tracing::debug!(?rate);
            prev_rate = rate;
        }
    }
}
