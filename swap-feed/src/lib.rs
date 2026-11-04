pub mod bitfinex;
pub mod exolix;
pub mod kraken;
pub mod kucoin;
pub mod rate;
pub mod traits;

use url::Url;

// Re-exports for convenience
pub use kraken::{Error as KrakenError, PriceUpdates, connect};
pub use rate::{ExchangeRate, FixedRate, Rate};
pub use traits::LatestRate;

mod ticker;

// Core functions
pub fn connect_kraken(url: Url, proxy: Option<String>) -> anyhow::Result<kraken::PriceUpdates> {
    kraken::connect(url, proxy)
}

pub fn connect_bitfinex(url: Url, proxy: Option<String>) -> anyhow::Result<bitfinex::PriceUpdates> {
    bitfinex::connect(url, proxy)
}

pub fn connect_kucoin(
    url: Url,
    client: reqwest::Client,
) -> anyhow::Result<kucoin::PriceUpdates> {
    kucoin::connect(url, client)
}

pub fn connect_exolix(
    url: Url,
    api_key: String,
    poll_interval: std::time::Duration,
    client: reqwest::Client,
) -> anyhow::Result<exolix::PriceUpdates> {
    exolix::connect(url, api_key, poll_interval, client)
}
