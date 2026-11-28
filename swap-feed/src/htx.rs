use anyhow::Result;
use url::Url;

/// Connect to the HTX REST API and poll it for XMR/BTC rate updates.
///
/// HTX has no XMR/BTC market. The rate is derived from the XMR/USDT and
/// BTC/USDT markets.
///
/// `rest_url` must point to the "latest aggregated ticker" endpoint
/// (`/market/detail/merged`).
pub fn connect(rest_url: Url, client: reqwest::Client) -> Result<PriceUpdates> {
    crate::ticker::connect("HTX", HtxParams { rest_url, client }, connection::new)
}

pub type PriceUpdates = crate::ticker::PriceUpdates<wire::PriceUpdate>;
pub type PriceUpdate = crate::ticker::PriceUpdate<wire::PriceUpdate>;
pub type Error = crate::ticker::Error;

#[derive(Clone)]
pub struct HtxParams {
    pub rest_url: Url,
    pub client: reqwest::Client,
}

pub(crate) mod connection {
    use super::{HtxParams, wire};
    use anyhow::Result;
    use futures::stream::BoxStream;
    use std::convert::Infallible;
    use std::sync::Arc;

    const XMR_USDT_SYMBOL: &str = "xmrusdt";
    const BTC_USDT_SYMBOL: &str = "btcusdt";

    #[derive(Debug, thiserror::Error)]
    pub enum PollError {
        #[error("Failed to fetch HTX ticker")]
        Fetch(#[from] crate::rest::FetchError),
        #[error("Invalid HTX ticker payload")]
        Parse(#[from] wire::Error),
    }

    pub async fn new(
        params: Arc<HtxParams>,
    ) -> Result<BoxStream<'static, Result<wire::PriceUpdate, Infallible>>> {
        Ok(crate::rest::poll("HTX", params, fetch_rate))
    }

    async fn fetch_rate(params: Arc<HtxParams>) -> Result<wire::PriceUpdate, PollError> {
        let (xmr_usdt, btc_usdt) = tokio::try_join!(
            fetch_ticker(&params, XMR_USDT_SYMBOL),
            fetch_ticker(&params, BTC_USDT_SYMBOL)
        )?;

        Ok(wire::PriceUpdate::from_tickers(&xmr_usdt, &btc_usdt)?)
    }

    async fn fetch_ticker(params: &HtxParams, symbol: &str) -> Result<wire::Ticker, PollError> {
        let request = params
            .client
            .get(params.rest_url.clone())
            .query(&[("symbol", symbol)]);
        let response: wire::TickerResponse = crate::rest::fetch_json(request).await?;

        Ok(response.into_ticker()?)
    }
}

pub mod wire {
    use rust_decimal::Decimal;
    use serde::Deserialize;
    use serde::de::IgnoredAny;

    /// Raw response from `GET /market/detail/merged`.
    /// HTX reports a rejected request in the body, with a success HTTP status.
    #[derive(Debug, Deserialize)]
    pub struct TickerResponse {
        pub status: String,
        pub tick: Option<Ticker>,
        #[serde(rename = "err-code")]
        pub err_code: Option<String>,
        #[serde(rename = "err-msg")]
        pub err_msg: Option<String>,
    }

    /// Best bid and best ask of one market, each as `[price, size]`.
    #[derive(Debug, Deserialize)]
    pub struct Ticker {
        pub bid: (Decimal, IgnoredAny),
        pub ask: (Decimal, IgnoredAny),
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct PriceUpdate {
        pub ask: bitcoin::Amount,
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error(
            "HTX rejected the request with status {status}: {} {}",
            code.as_deref().unwrap_or_default(),
            message.as_deref().unwrap_or_default()
        )]
        Rejected {
            status: String,
            code: Option<String>,
            message: Option<String>,
        },
        #[error("HTX sent no ticker")]
        TickerMissing,
        #[error("Failed to derive the XMR/BTC rate from HTX prices")]
        CrossRate(#[from] crate::cross_rate::Error),
    }

    impl TickerResponse {
        const STATUS_OK: &'static str = "ok";

        pub fn into_ticker(self) -> Result<Ticker, Error> {
            if self.status != Self::STATUS_OK {
                return Err(Error::Rejected {
                    status: self.status,
                    code: self.err_code,
                    message: self.err_msg,
                });
            }

            self.tick.ok_or(Error::TickerMissing)
        }
    }

    impl PriceUpdate {
        /// The BTC needed to buy 1 XMR through USDT.
        /// We sell BTC at the BTC/USDT bid and buy XMR at the XMR/USDT ask.
        pub fn from_tickers(xmr_usdt: &Ticker, btc_usdt: &Ticker) -> Result<Self, Error> {
            let (xmr_ask, _) = xmr_usdt.ask;
            let (btc_bid, _) = btc_usdt.bid;

            let ask = crate::cross_rate::cross_rate(xmr_ask, btc_bid)?;

            Ok(PriceUpdate { ask })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn response(body: &str) -> TickerResponse {
            serde_json::from_str(body).unwrap()
        }

        #[test]
        fn derives_xmr_btc_ask_from_usdt_markets() {
            let xmr_usdt = response(
                r#"{"ch":"market.xmrusdt.detail.merged","status":"ok","ts":1795870291521,"tick":{"id":215080877940,"version":215080877940,"open":438.35,"close":440.58,"low":396.07,"high":446.67,"amount":50023.81301087708,"vol":2.097931587265573E7,"count":54321,"bid":[440.05,0.01],"ask":[441.13,3.926]}}"#,
            )
            .into_ticker()
            .unwrap();
            let btc_usdt = response(
                r#"{"ch":"market.btcusdt.detail.merged","status":"ok","ts":1795870292236,"tick":{"id":391908504397,"version":391908504397,"open":61489.07,"close":59236.3,"low":58809.81,"high":61644.4,"amount":3522.7057963658413,"vol":2.1115740693514544E8,"count":16448,"bid":[59242.21,2.181583],"ask":[59242.22,0.689471]}}"#,
            )
            .into_ticker()
            .unwrap();

            let update = PriceUpdate::from_tickers(&xmr_usdt, &btc_usdt).unwrap();

            // 441.13 / 59242.21 = 0.00744621..., rounded up
            assert_eq!(update.ask.to_sat(), 744_622);
        }

        #[test]
        fn reports_rejected_request() {
            let body = r#"{"ts":1795870278552,"status":"error","err-code":"invalid-parameter","err-msg":"invalid symbol"}"#;

            let Err(Error::Rejected { code, message, .. }) = response(body).into_ticker() else {
                panic!("expected a rejected request");
            };

            assert_eq!(code.as_deref(), Some("invalid-parameter"));
            assert_eq!(message.as_deref(), Some("invalid symbol"));
        }

        #[test]
        fn rejects_ok_response_without_ticker() {
            assert!(matches!(
                response(r#"{"status":"ok"}"#).into_ticker(),
                Err(Error::TickerMissing)
            ));
        }

        #[test]
        fn rejects_zero_xmr_ask() {
            let xmr_usdt = response(r#"{"status":"ok","tick":{"bid":[0,0],"ask":[0,0]}}"#)
                .into_ticker()
                .unwrap();
            let btc_usdt =
                response(r#"{"status":"ok","tick":{"bid":[59242.21,2.1],"ask":[59242.22,0.6]}}"#)
                    .into_ticker()
                    .unwrap();

            assert!(matches!(
                PriceUpdate::from_tickers(&xmr_usdt, &btc_usdt),
                Err(Error::CrossRate(crate::cross_rate::Error::NonPositive(_)))
            ));
        }
    }
}
