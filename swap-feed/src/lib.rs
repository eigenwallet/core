pub mod bitfinex;
pub mod exolix;
pub mod kraken;
pub mod kucoin;
pub mod rate;
pub mod traits;

// Re-exports for convenience
pub use kraken::{Error as KrakenError, PriceUpdates, connect};
pub use rate::{ExchangeRate, FixedRate, Rate};
pub use traits::LatestRate;

mod ticker;

// Core functions
pub fn connect_kraken(url: url::Url) -> anyhow::Result<kraken::PriceUpdates> {
    kraken::connect(url)
}

pub fn connect_bitfinex(url: url::Url) -> anyhow::Result<bitfinex::PriceUpdates> {
    bitfinex::connect(url)
}

pub fn connect_kucoin(
    url: url::Url,
    client: reqwest::Client,
) -> anyhow::Result<kucoin::PriceUpdates> {
    kucoin::connect(url, client)
}

pub fn connect_exolix(
    url: url::Url,
    api_key: String,
    poll_interval: std::time::Duration,
    client: reqwest::Client,
) -> anyhow::Result<exolix::PriceUpdates> {
    exolix::connect(url, api_key, poll_interval, client)
}
