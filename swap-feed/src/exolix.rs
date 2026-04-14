use anyhow::Result;
use std::time::Duration;
use url::Url;

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
    api_key: String,
    poll_interval: Duration,
    client: reqwest::Client,
) -> Result<PriceUpdates> {
    crate::ticker::connect(
        "Exolix",
        ExolixParams {
            rest_url,
            api_key,
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
pub struct ExolixParams {
    pub rest_url: Url,
    pub api_key: String,
    pub poll_interval: Duration,
    pub client: reqwest::Client,
}

pub(crate) mod connection {
    use super::{ExolixParams, wire};
    use anyhow::Result;
    use backoff::{ExponentialBackoff, backoff::Backoff};
    use futures::StreamExt;
    use futures::stream::{self, BoxStream};
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::Duration;

    /// Initial retry delay after a failed fetch.
    const BACKOFF_INITIAL: Duration = Duration::from_secs(3);
    /// Cap on the retry delay after repeated failures.
    const BACKOFF_MAX: Duration = Duration::from_secs(60);

    fn new_backoff() -> ExponentialBackoff {
        ExponentialBackoff {
            initial_interval: BACKOFF_INITIAL,
            max_interval: BACKOFF_MAX,
            max_elapsed_time: None,
            ..ExponentialBackoff::default()
        }
    }

    pub async fn new(
        params: Arc<ExolixParams>,
    ) -> Result<BoxStream<'static, Result<wire::PriceUpdate, Infallible>>> {
        tracing::debug!("Connected to Exolix REST API");

        // Fetch-then-sleep loop. Per-poll failures are handled locally
        // with an exponential backoff (3s → 60s) rather than being
        // propagated to the ticker's reconnect machinery — that layer is
        // designed for transport loss, not transient REST errors (429,
        // 500, decode error). On success the backoff resets.
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
                                error = %err,
                                retry_in_ms = delay.as_millis() as u64,
                                "Exolix poll failed, will retry",
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

    async fn fetch_rate(params: &ExolixParams) -> Result<wire::PriceUpdate, FetchError> {
        let request_body = wire::RateRequest::xmr_to_btc();

        let request = params
            .client
            .get(params.rest_url.clone())
            .query(&request_body)
            .header("Accept", "application/json")
            .header("Authorization", &params.api_key);

        let response = request.send().await.map_err(FetchError::Request)?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.map_err(FetchError::BodyRead)?;
            return Err(FetchError::Status { status, body });
        }

        let bytes = response.bytes().await.map_err(FetchError::BodyRead)?;
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
    use serde::{Deserialize, Serialize};

    /// Query parameters for `GET /api/v2/rate`.
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RateRequest {
        pub coin_from: String,
        pub network_from: String,
        pub coin_to: String,
        pub network_to: String,
        pub amount: Decimal,
        pub rate_type: RateType,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "lowercase")]
    pub enum RateType {
        Float,
        #[allow(dead_code)]
        Fixed,
    }

    impl RateRequest {
        /// Request the XMR -> BTC floating rate for a 1 XMR send amount.
        pub fn xmr_to_btc() -> Self {
            Self {
                coin_from: "XMR".to_string(),
                network_from: "XMR".to_string(),
                coin_to: "BTC".to_string(),
                network_to: "BTC".to_string(),
                amount: Decimal::ONE,
                rate_type: RateType::Float,
            }
        }
    }

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
