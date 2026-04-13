use anyhow::Result;
use std::time::Duration;
use url::Url;

/// Default poll interval for the Exolix rate endpoint.
pub const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Connect to the Exolix REST API and poll it for XMR/BTC rate updates.
///
/// Unlike the websocket-based feeds, Exolix only exposes a REST rate endpoint,
/// so we emulate a "stream of updates" by polling on a fixed interval. The
/// reconnection/backoff machinery in [`crate::ticker`] transparently reuses
/// this stream shape.
///
/// See: <https://exolix.com/developers>
pub fn connect(
    rest_url: Url,
    api_key: Option<String>,
    client: reqwest::Client,
) -> Result<PriceUpdates> {
    crate::ticker::connect(
        "Exolix",
        ExolixParams {
            rest_url,
            api_key,
            client,
        },
        connection::new,
    )
}

pub type PriceUpdates = crate::ticker::PriceUpdates<wire::PriceUpdate>;
pub type PriceUpdate = crate::ticker::PriceUpdate<wire::PriceUpdate>;
pub type Error = crate::ticker::Error;

#[derive(Clone)]
pub struct ExolixParams {
    pub rest_url: Url,
    pub api_key: Option<String>,
    pub client: reqwest::Client,
}

pub(crate) mod connection {
    use super::{ExolixParams, POLL_INTERVAL, wire};
    use anyhow::{Context, Result};
    use futures::StreamExt;
    use futures::stream::{self, BoxStream};
    use std::convert::Infallible;
    use std::sync::Arc;

    pub async fn new(
        params: Arc<ExolixParams>,
    ) -> Result<BoxStream<'static, Result<wire::PriceUpdate, Infallible>>> {
        // Do a synchronous first fetch so connection failures (bad key,
        // wrong URL) surface immediately to the ticker's backoff machinery
        // instead of being buried behind a 30s sleep. The successful sample
        // is emitted as the very first stream item so subscribers leave
        // `NotYetAvailable` without waiting for the poll interval.
        let initial = fetch_rate(&params)
            .await
            .context("Failed initial Exolix rate fetch")?;

        tracing::debug!("Connected to Exolix REST API");

        enum State {
            First(wire::PriceUpdate, Arc<ExolixParams>),
            Polling(Arc<ExolixParams>),
        }

        let stream = stream::unfold(State::First(initial, params), |state| async move {
            match state {
                State::First(update, params) => Some((Ok(update), State::Polling(params))),
                State::Polling(params) => {
                    tokio::time::sleep(POLL_INTERVAL).await;
                    // Per-poll failures must NOT tear down the whole feed.
                    // Websocket feeds only reconnect on transport loss; a
                    // single bad REST response (429, 500, decode error)
                    // should be logged and retried on the next tick. We
                    // therefore skip item-errors by recursing the unfold
                    // until we get a healthy sample.
                    loop {
                        match fetch_rate(&params).await {
                            Ok(update) => {
                                return Some((Ok(update), State::Polling(params)));
                            }
                            Err(err) => {
                                tracing::warn!(
                                    error = %err,
                                    "Exolix poll failed, will retry after next interval",
                                );
                                tokio::time::sleep(POLL_INTERVAL).await;
                            }
                        }
                    }
                }
            }
        })
        .boxed();

        Ok(stream)
    }

    async fn fetch_rate(params: &ExolixParams) -> Result<wire::PriceUpdate, FetchError> {
        let mut url = params.rest_url.clone();
        url.query_pairs_mut()
            .append_pair("coinFrom", "XMR")
            .append_pair("networkFrom", "XMR")
            .append_pair("coinTo", "BTC")
            .append_pair("networkTo", "BTC")
            .append_pair("amount", "1")
            .append_pair("rateType", "float");

        let mut request = params.client.get(url).header("Accept", "application/json");
        if let Some(key) = params.api_key.as_deref() {
            request = request.header("Authorization", key);
        }

        let response = request
            .send()
            .await
            .map_err(FetchError::Request)?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .map_err(FetchError::BodyRead)?;
            return Err(FetchError::Status { status, body });
        }

        let bytes = response
            .bytes()
            .await
            .map_err(FetchError::BodyRead)?;
        let body: wire::RateResponse =
            serde_json::from_slice(&bytes).map_err(FetchError::Decode)?;
        wire::PriceUpdate::try_from(body).map_err(FetchError::Parse)
    }

    #[derive(Debug, thiserror::Error)]
    pub enum FetchError {
        #[error("Exolix HTTP request failed")]
        Request(#[source] reqwest::Error),
        #[error("Failed to read Exolix response body")]
        BodyRead(#[source] reqwest::Error),
        #[error("Exolix returned non-success status {status}: {body}")]
        Status {
            status: reqwest::StatusCode,
            body: String,
        },
        #[error("Failed to decode Exolix JSON response")]
        Decode(#[source] serde_json::Error),
        #[error("Invalid Exolix rate payload")]
        Parse(#[from] wire::Error),
    }

}

pub mod wire {
    use bitcoin::amount::ParseAmountError;
    use rust_decimal::Decimal;
    use serde::Deserialize;

    /// Raw response from `GET /api/v2/rate`.
    ///
    /// Only the fields we care about are captured.
    #[derive(Debug, Deserialize)]
    pub struct RateResponse {
        /// Rate as BTC received per 1 XMR sent (we query `amount=1`).
        pub rate: Decimal,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct PriceUpdate {
        pub ask: bitcoin::Amount,
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("Exolix returned a non-positive rate: {0}")]
        NonPositive(Decimal),
        #[error("Failed to parse Exolix rate {rate} as a Bitcoin amount")]
        AmountParse {
            rate: Decimal,
            #[source]
            source: ParseAmountError,
        },
    }

    impl TryFrom<RateResponse> for PriceUpdate {
        type Error = Error;

        fn try_from(value: RateResponse) -> Result<Self, Error> {
            if value.rate <= Decimal::ZERO {
                return Err(Error::NonPositive(value.rate));
            }
            // Route through the decimal string representation to avoid
            // binary-float drift. This matches how kraken/kucoin parse
            // their wire values.
            let rendered = value.rate.to_string();
            let ask = bitcoin::Amount::from_str_in(&rendered, bitcoin::Denomination::Bitcoin)
                .map_err(|source| Error::AmountParse {
                    rate: value.rate,
                    source,
                })?;
            Ok(PriceUpdate { ask })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_rate_response() {
            let body = r#"{"fromAmount":1,"toAmount":0.00468629,"rate":0.00468629,"message":null,"minAmount":0.14233428,"withdrawMin":0.00000624,"maxAmount":2000,"priceImpact":"0"}"#;
            let response: RateResponse = serde_json::from_str(body).unwrap();
            let update: PriceUpdate = response.try_into().unwrap();
            assert_eq!(update.ask.to_sat(), 468_629);
        }

        #[test]
        fn parses_rate_response_with_high_precision() {
            // More than 8 decimal places of BTC would not fit in sats and
            // must fail cleanly rather than being silently rounded.
            let body = r#"{"rate":0.123456789}"#;
            let response: RateResponse = serde_json::from_str(body).unwrap();
            assert!(PriceUpdate::try_from(response).is_err());
        }

        #[test]
        fn rejects_zero_rate() {
            let body = r#"{"rate":0}"#;
            let response: RateResponse = serde_json::from_str(body).unwrap();
            assert!(PriceUpdate::try_from(response).is_err());
        }

        #[test]
        fn rejects_negative_rate() {
            let body = r#"{"rate":-0.00468629}"#;
            let response: RateResponse = serde_json::from_str(body).unwrap();
            assert!(PriceUpdate::try_from(response).is_err());
        }
    }
}
