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
    use futures::stream::BoxStream;
    use std::convert::Infallible;
    use std::sync::Arc;

    #[derive(Debug, thiserror::Error)]
    pub enum PollError {
        #[error("Failed to fetch Hyperliquid prices")]
        Fetch(#[from] crate::rest::FetchError),
        #[error("Invalid Hyperliquid price payload")]
        Parse(#[from] wire::Error),
    }

    pub async fn new(
        params: Arc<HyperliquidParams>,
    ) -> Result<BoxStream<'static, Result<wire::PriceUpdate, Infallible>>> {
        Ok(crate::rest::poll("Hyperliquid", params, fetch_rate))
    }

    async fn fetch_rate(params: Arc<HyperliquidParams>) -> Result<wire::PriceUpdate, PollError> {
        let request = params
            .client
            .post(params.rest_url.clone())
            .json(&wire::InfoRequest::meta_and_asset_ctxs());
        let response: wire::MetaAndAssetCtxs = crate::rest::fetch_json(request).await?;

        Ok(wire::PriceUpdate::try_from(response)?)
    }
}

pub mod wire {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Serialize};

    const XMR_PERPETUAL: &str = "XMR";
    const BTC_PERPETUAL: &str = "BTC";

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
        #[error("Hyperliquid sent no asset context for the {0} perpetual")]
        AssetCtxMissing(&'static str),
        #[error("Hyperliquid sent no mark price for the {0} perpetual")]
        MarkPriceMissing(&'static str),
        #[error("Hyperliquid sent no oracle price for the {0} perpetual")]
        OraclePriceMissing(&'static str),
        #[error("Failed to derive the XMR/BTC rate from Hyperliquid prices")]
        CrossRate(#[from] crate::cross_rate::Error),
    }

    impl MetaAndAssetCtxs {
        fn asset_ctx(&self, name: &'static str) -> Result<&AssetCtx, Error> {
            let MetaAndAssetCtxs(meta, asset_ctxs) = self;

            let index = meta
                .universe
                .iter()
                .position(|perpetual| perpetual.name == name)
                .ok_or(Error::PerpetualMissing(name))?;

            asset_ctxs.get(index).ok_or(Error::AssetCtxMissing(name))
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

            let ask = crate::cross_rate::cross_rate(xmr_mark, btc_oracle)?;

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
        fn rejects_response_with_fewer_asset_ctxs_than_perpetuals() {
            let MetaAndAssetCtxs(meta, mut asset_ctxs) = serde_json::from_str(RESPONSE).unwrap();
            asset_ctxs.pop();

            assert!(matches!(
                PriceUpdate::try_from(MetaAndAssetCtxs(meta, asset_ctxs)),
                Err(Error::AssetCtxMissing(XMR_PERPETUAL))
            ));
        }

        #[test]
        fn rejects_missing_xmr_mark_price() {
            let response: MetaAndAssetCtxs =
                serde_json::from_str(&RESPONSE.replace("\"440.35\"", "null")).unwrap();

            assert!(matches!(
                PriceUpdate::try_from(response),
                Err(Error::MarkPriceMissing(XMR_PERPETUAL))
            ));
        }

        #[test]
        fn rejects_missing_btc_oracle_price() {
            let response: MetaAndAssetCtxs =
                serde_json::from_str(&RESPONSE.replace("\"59221.9\"", "null")).unwrap();

            assert!(matches!(
                PriceUpdate::try_from(response),
                Err(Error::OraclePriceMissing(BTC_PERPETUAL))
            ));
        }

        #[test]
        fn rejects_zero_xmr_mark_price() {
            let response: MetaAndAssetCtxs =
                serde_json::from_str(&RESPONSE.replace("\"440.35\"", "\"0.0\"")).unwrap();

            assert!(matches!(
                PriceUpdate::try_from(response),
                Err(Error::CrossRate(crate::cross_rate::Error::NonPositive(_)))
            ));
        }
    }
}
