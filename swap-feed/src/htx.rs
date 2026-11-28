use anyhow::Result;
use std::time::Duration;
use url::Url;

/// Connect to the HTX REST API and poll it for XMR/BTC rate updates.
///
/// HTX has no XMR/BTC market. The rate is derived from the XMR/USDT and
/// BTC/USDT markets.
///
/// `rest_url` must point to the "latest aggregated ticker" endpoint
/// (`/market/detail/merged`).
pub fn connect(
    rest_url: Url,
    poll_interval: Duration,
    client: reqwest::Client,
) -> Result<PriceUpdates> {
    crate::ticker::connect(
        "HTX",
        HtxParams {
            rest_url,
            poll_interval,
            client,
        },
        connection::new,
    )
}

pub type PriceUpdates = crate::ticker::PriceUpdates<wire::PriceUpdate>;
pub type PriceUpdate = crate::ticker::PriceUpdate<wire::PriceUpdate>;
pub type Error = crate::ticker::Error;

#[derive(Clone)]
pub struct HtxParams {
    pub rest_url: Url,
    pub poll_interval: Duration,
    pub client: reqwest::Client,
}

pub(crate) mod connection {
    use super::{HtxParams, wire};
    use anyhow::Result;
    use backoff::{ExponentialBackoff, backoff::Backoff};
    use futures::StreamExt;
    use futures::stream::{self, BoxStream};
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::Duration;

    const BACKOFF_INITIAL: Duration = Duration::from_secs(3);
    const BACKOFF_MAX: Duration = Duration::from_secs(60);

    const XMR_USDT_SYMBOL: &str = "xmrusdt";
    const BTC_USDT_SYMBOL: &str = "btcusdt";

    fn new_backoff() -> ExponentialBackoff {
        ExponentialBackoff {
            initial_interval: BACKOFF_INITIAL,
            max_interval: BACKOFF_MAX,
            max_elapsed_time: None,
            ..ExponentialBackoff::default()
        }
    }

    pub async fn new(
        params: Arc<HtxParams>,
    ) -> Result<BoxStream<'static, Result<wire::PriceUpdate, Infallible>>> {
        // A failed poll is retried here with its own backoff.
        // The reconnect logic of the ticker is for a lost transport, which a REST API does not have.
        let stream = stream::unfold(
            (params, true, new_backoff()),
            |(params, first, mut backoff)| async move {
                if !first {
                    tokio::time::sleep(params.poll_interval).await;
                }
                loop {
                    match fetch_rate(&params).await {
                        Ok(update) => {
                            backoff.reset();
                            return Some((Ok(update), (params, false, backoff)));
                        }
                        Err(err) => {
                            let delay = backoff.next_backoff().unwrap_or(BACKOFF_MAX);
                            tracing::warn!(
                                error = %format!("{:#}", anyhow::Error::from(err)),
                                retry_in_ms = delay.as_millis() as u64,
                                "HTX poll failed, will retry",
                            );
                            tokio::time::sleep(delay).await;
                        }
                    }
                }
            },
        )
        .boxed();

        Ok(stream)
    }

    async fn fetch_rate(params: &HtxParams) -> Result<wire::PriceUpdate, FetchError> {
        let (xmr_usdt, btc_usdt) = tokio::try_join!(
            fetch_ticker(params, XMR_USDT_SYMBOL),
            fetch_ticker(params, BTC_USDT_SYMBOL)
        )?;

        Ok(wire::PriceUpdate::from_tickers(&xmr_usdt, &btc_usdt)?)
    }

    async fn fetch_ticker(params: &HtxParams, symbol: &str) -> Result<wire::Ticker, FetchError> {
        let response = params
            .client
            .get(params.rest_url.clone())
            .query(&[("symbol", symbol)])
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(FetchError::Request)?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.map_err(FetchError::BodyRead)?;
            return Err(FetchError::Status { status, body });
        }

        let bytes = response.bytes().await.map_err(FetchError::BodyRead)?;
        let body: wire::TickerResponse =
            serde_json::from_slice(&bytes).map_err(FetchError::Decode)?;

        Ok(body.tick)
    }

    #[derive(Debug, thiserror::Error)]
    pub enum FetchError {
        #[error("HTX HTTP request failed")]
        Request(#[source] reqwest::Error),
        #[error("Failed to read HTX response body")]
        BodyRead(#[source] reqwest::Error),
        #[error("HTX returned non-success status {status}: {body}")]
        Status {
            status: reqwest::StatusCode,
            body: String,
        },
        #[error("Failed to decode HTX JSON response")]
        Decode(#[source] serde_json::Error),
        #[error("Invalid HTX ticker payload")]
        Parse(#[from] wire::Error),
    }
}

pub mod wire {
    use bitcoin::amount::ParseAmountError;
    use rust_decimal::{Decimal, RoundingStrategy};
    use serde::Deserialize;

    /// Raw response from `GET /market/detail/merged`.
    #[derive(Debug, Deserialize)]
    pub struct TickerResponse {
        pub tick: Ticker,
    }

    /// Best bid and best ask of one market, each as `[price, size]`.
    #[derive(Debug, Deserialize)]
    pub struct Ticker {
        pub bid: (Decimal, Decimal),
        pub ask: (Decimal, Decimal),
    }

    const BTC_DECIMAL_PLACES: u32 = 8;

    #[derive(Clone, Debug, PartialEq)]
    pub struct PriceUpdate {
        pub ask: bitcoin::Amount,
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("HTX returned a non-positive price: {0}")]
        NonPositive(Decimal),
        #[error("XMR/BTC rate derived from HTX prices overflowed")]
        Overflow,
        #[error("Failed to parse HTX rate {rate} as a Bitcoin amount")]
        AmountParse {
            rate: Decimal,
            #[source]
            source: ParseAmountError,
        },
    }

    impl PriceUpdate {
        /// The BTC needed to buy 1 XMR through USDT.
        /// We sell BTC at the BTC/USDT bid and buy XMR at the XMR/USDT ask.
        pub fn from_tickers(xmr_usdt: &Ticker, btc_usdt: &Ticker) -> Result<Self, Error> {
            let (xmr_ask, _) = xmr_usdt.ask;
            let (btc_bid, _) = btc_usdt.bid;

            for price in [xmr_ask, btc_bid] {
                if price <= Decimal::ZERO {
                    return Err(Error::NonPositive(price));
                }
            }

            let rate = xmr_ask
                .checked_div(btc_bid)
                .ok_or(Error::Overflow)?
                .round_dp_with_strategy(BTC_DECIMAL_PLACES, RoundingStrategy::ToPositiveInfinity);
            let ask =
                bitcoin::Amount::from_str_in(&rate.to_string(), bitcoin::Denomination::Bitcoin)
                    .map_err(|source| Error::AmountParse { rate, source })?;

            Ok(PriceUpdate { ask })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn ticker(body: &str) -> Ticker {
            serde_json::from_str::<TickerResponse>(body).unwrap().tick
        }

        #[test]
        fn derives_xmr_btc_ask_from_usdt_markets() {
            let xmr_usdt = ticker(
                r#"{"ch":"market.xmrusdt.detail.merged","status":"ok","ts":1795870291521,"tick":{"id":215080877940,"version":215080877940,"open":438.35,"close":440.58,"low":396.07,"high":446.67,"amount":50023.81301087708,"vol":2.097931587265573E7,"count":54321,"bid":[440.05,0.01],"ask":[441.13,3.926]}}"#,
            );
            let btc_usdt = ticker(
                r#"{"ch":"market.btcusdt.detail.merged","status":"ok","ts":1795870292236,"tick":{"id":391908504397,"version":391908504397,"open":61489.07,"close":59236.3,"low":58809.81,"high":61644.4,"amount":3522.7057963658413,"vol":2.1115740693514544E8,"count":16448,"bid":[59242.21,2.181583],"ask":[59242.22,0.689471]}}"#,
            );

            let update = PriceUpdate::from_tickers(&xmr_usdt, &btc_usdt).unwrap();

            // 441.13 / 59242.21 = 0.00744621..., rounded up
            assert_eq!(update.ask.to_sat(), 744_622);
        }

        #[test]
        fn rejects_error_response() {
            let body = r#"{"ts":1795870278552,"status":"error","err-code":"invalid-parameter","err-msg":"invalid symbol"}"#;
            assert!(serde_json::from_str::<TickerResponse>(body).is_err());
        }

        #[test]
        fn rejects_zero_price() {
            let xmr_usdt = ticker(r#"{"tick":{"bid":[440.05,0.01],"ask":[441.13,3.926]}}"#);
            let btc_usdt = ticker(r#"{"tick":{"bid":[0,0],"ask":[59242.22,0.689471]}}"#);
            assert!(PriceUpdate::from_tickers(&xmr_usdt, &btc_usdt).is_err());
        }
    }
}
