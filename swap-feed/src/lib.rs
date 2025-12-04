pub mod bitfinex;
pub mod kraken;
pub mod kucoin;
pub mod rate;
pub mod traits;

// Re-exports for convenience
pub use kraken::{connect, Error as KrakenError, PriceUpdates};
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
