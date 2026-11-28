use anyhow::Result;
use url::Url;

/// Connect to the Hyperliquid REST API and poll it for XMR/BTC rate updates.
///
/// Hyperliquid has no XMR/BTC market. The rate is the mark price of the XMR
/// perpetual divided by the oracle price of the BTC perpetual.
///
/// `rest_url` must point to the info endpoint (`/info`).
pub fn connect(rest_url: Url, client: reqwest::Client) -> Result<PriceUpdates> {
    crate::ticker::connect(
        "Hyperliquid",
        HyperliquidParams { rest_url, client },
        connection::new,
    )
}

pub type PriceUpdates = crate::ticker::PriceUpdates<wire::PriceUpdate>;
pub type PriceUpdate = crate::ticker::PriceUpdate<wire::PriceUpdate>;
pub type Error = crate::ticker::Error;

#[derive(Clone)]
pub struct HyperliquidParams {
    pub rest_url: Url,
    pub client: reqwest::Client,
}

pub(crate) mod connection {
    use super::{HyperliquidParams, wire};
    use anyhow::Result;
    use futures::StreamExt;
    use futures::stream::{self, BoxStream};
    use std::sync::Arc;
    use std::time::Duration;

    const POLL_INTERVAL: Duration = Duration::from_secs(10);

    /// A failed poll ends the stream with an error. The ticker then retries with backoff.
    pub async fn new(
        params: Arc<HyperliquidParams>,
    ) -> Result<BoxStream<'static, Result<wire::PriceUpdate, FetchError>>> {
        let stream = stream::unfold((params, true), |(params, first)| async move {
            if !first {
                tokio::time::sleep(POLL_INTERVAL).await;
            }

            Some((fetch_rate(&params).await, (params, false)))
        })
        .boxed();

        Ok(stream)
    }

    async fn fetch_rate(params: &HyperliquidParams) -> Result<wire::PriceUpdate, FetchError> {
        let response = params
            .client
            .post(params.rest_url.clone())
            .json(&wire::InfoRequest::meta_and_asset_ctxs())
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
        let body: wire::MetaAndAssetCtxs =
            serde_json::from_slice(&bytes).map_err(FetchError::Decode)?;

        Ok(wire::PriceUpdate::try_from(body)?)
    }

    #[derive(Debug, thiserror::Error)]
    pub enum FetchError {
        #[error("Hyperliquid HTTP request failed")]
        Request(#[source] reqwest::Error),
        #[error("Failed to read Hyperliquid response body")]
        BodyRead(#[source] reqwest::Error),
        #[error("Hyperliquid returned non-success status {status}: {body}")]
        Status {
            status: reqwest::StatusCode,
            body: String,
        },
        #[error("Failed to decode Hyperliquid JSON response")]
        Decode(#[source] serde_json::Error),
        #[error("Invalid Hyperliquid price payload")]
        Parse(#[from] wire::Error),
    }
}

pub mod wire {
    use bitcoin::amount::ParseAmountError;
    use rust_decimal::{Decimal, RoundingStrategy};
    use serde::{Deserialize, Serialize};

    const XMR_PERPETUAL: &str = "XMR";
    const BTC_PERPETUAL: &str = "BTC";

    const BTC_DECIMAL_PLACES: u32 = 8;

    /// Body of `POST /info`.
    #[derive(Debug, Serialize)]
    pub struct InfoRequest {
        r#type: &'static str,
    }

    impl InfoRequest {
        pub fn meta_and_asset_ctxs() -> Self {
            Self {
                r#type: "metaAndAssetCtxs",
            }
        }
    }

    /// Response to the `metaAndAssetCtxs` request.
    /// The context at index `i` belongs to the perpetual at index `i` of the universe.
    #[derive(Debug, Deserialize)]
    pub struct MetaAndAssetCtxs(pub Meta, pub Vec<AssetCtx>);

    #[derive(Debug, Deserialize)]
    pub struct Meta {
        pub universe: Vec<Perpetual>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Perpetual {
        pub name: String,
    }

    /// Hyperliquid sends `null` for a price that a perpetual does not have.
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct AssetCtx {
        pub mark_px: Option<Decimal>,
        pub oracle_px: Option<Decimal>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct PriceUpdate {
        pub ask: bitcoin::Amount,
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("Hyperliquid did not list the {0} perpetual")]
        PerpetualMissing(&'static str),
        #[error("Hyperliquid sent no mark price for the {0} perpetual")]
        MarkPriceMissing(&'static str),
        #[error("Hyperliquid sent no oracle price for the {0} perpetual")]
        OraclePriceMissing(&'static str),
        #[error("Hyperliquid returned a non-positive price: {0}")]
        NonPositive(Decimal),
        #[error("XMR/BTC rate derived from Hyperliquid prices overflowed")]
        Overflow,
        #[error("Failed to parse Hyperliquid rate {rate} as a Bitcoin amount")]
        AmountParse {
            rate: Decimal,
            #[source]
            source: ParseAmountError,
        },
    }

    impl MetaAndAssetCtxs {
        fn asset_ctx(&self, name: &'static str) -> Result<&AssetCtx, Error> {
            let MetaAndAssetCtxs(meta, asset_ctxs) = self;

            meta.universe
                .iter()
                .position(|perpetual| perpetual.name == name)
                .and_then(|index| asset_ctxs.get(index))
                .ok_or(Error::PerpetualMissing(name))
        }
    }

    impl TryFrom<MetaAndAssetCtxs> for PriceUpdate {
        type Error = Error;

        fn try_from(value: MetaAndAssetCtxs) -> Result<Self, Error> {
            let xmr_mark = value
                .asset_ctx(XMR_PERPETUAL)?
                .mark_px
                .ok_or(Error::MarkPriceMissing(XMR_PERPETUAL))?;
            let btc_oracle = value
                .asset_ctx(BTC_PERPETUAL)?
                .oracle_px
                .ok_or(Error::OraclePriceMissing(BTC_PERPETUAL))?;

            for price in [xmr_mark, btc_oracle] {
                if price <= Decimal::ZERO {
                    return Err(Error::NonPositive(price));
                }
            }

            let rate = xmr_mark
                .checked_div(btc_oracle)
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

        const RESPONSE: &str = r#"[
            {"universe":[
                {"szDecimals":5,"name":"BTC","maxLeverage":40,"marginTableId":56},
                {"szDecimals":2,"name":"OLD","maxLeverage":3,"isDelisted":true},
                {"szDecimals":3,"name":"XMR","maxLeverage":5}
            ],"marginTables":[],"collateralToken":0},
            [
                {"funding":"-0.0000048526","oraclePx":"59221.9","markPx":"59189.9","midPx":"59189.5","impactPxs":["59189.0","59190.0"]},
                {"funding":"0.0","oraclePx":"1.0","markPx":null,"midPx":null,"impactPxs":null},
                {"funding":"-0.0004540407","oraclePx":"440.321","markPx":"440.35","midPx":"441.11","impactPxs":["440.806","441.329"]}
            ]
        ]"#;

        #[test]
        fn derives_xmr_btc_ask_from_xmr_mark_and_btc_oracle() {
            let response: MetaAndAssetCtxs = serde_json::from_str(RESPONSE).unwrap();
            let update = PriceUpdate::try_from(response).unwrap();

            // 440.35 / 59221.9 = 0.00743559..., rounded up
            assert_eq!(update.ask.to_sat(), 743_560);
        }

        #[test]
        fn rejects_response_without_xmr_perpetual() {
            let response: MetaAndAssetCtxs =
                serde_json::from_str(&RESPONSE.replace("\"XMR\"", "\"XRP\"")).unwrap();

            assert!(matches!(
                PriceUpdate::try_from(response),
                Err(Error::PerpetualMissing(XMR_PERPETUAL))
            ));
        }

        #[test]
        fn rejects_zero_price() {
            let response: MetaAndAssetCtxs =
                serde_json::from_str(&RESPONSE.replace("\"59221.9\"", "\"0.0\"")).unwrap();

            assert!(matches!(
                PriceUpdate::try_from(response),
                Err(Error::NonPositive(_))
            ));
        }
    }
}
