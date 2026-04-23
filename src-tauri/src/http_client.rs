use anyhow::Result;
use std::time::Duration;
use tor_socks5::Subsystem;

/// Set on every GUI-originated HTTP request. CoinGecko's fiat price API
/// (see `fetchCurrencyPrice` in `src-gui/src/renderer/api.ts`) refuses
/// requests without a User-Agent, so we always announce ourselves.
const GUI_HTTP_USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/eigenwallet/core)"
);

pub fn build_http_client(url: &reqwest::Url, timeout: Duration) -> Result<reqwest::Client> {
    swap::common::http::build_http_client(url, timeout, Subsystem::Http, Some(GUI_HTTP_USER_AGENT))
}
