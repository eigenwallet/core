use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt, TryStreamExt};
use serde::Deserialize;
use std::convert::TryFrom;
use url::Url;

/// Connect to Kraken websocket API for a constant stream of rate updates.
///
/// If the connection fails, it will automatically be re-established.
///
/// price_ticker_ws_url_kraken must point to a websocket server that follows the kraken
/// price ticker protocol
/// See: https://docs.kraken.com/websockets/
pub fn connect(price_ticker_ws_url_kraken: Url) -> Result<PriceUpdates> {
    crate::ticker::connect("Kraken", price_ticker_ws_url_kraken, connection::new)
}

pub type PriceUpdates = crate::ticker::PriceUpdates<wire::PriceUpdate>;

pub type Error = crate::ticker::Error;

/// Kraken websocket connection module.
///
/// Responsible for establishing a connection to the Kraken websocket API and
/// transforming the received websocket frames into a stream of rate updates.
/// The connection may fail in which case it is simply terminated and the stream
/// ends.
mod connection {
    use super::*;
    use crate::kraken::wire;
    use futures::stream::BoxStream;
    use tokio_tungstenite::tungstenite;

    pub async fn new(ws_url: Url) -> Result<BoxStream<'static, Result<wire::PriceUpdate, Error>>> {
        let (mut rate_stream, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .context("Failed to connect to Kraken websocket API")?;

        rate_stream
            .send(SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD.into())
            .await?;

        let stream = rate_stream.err_into().try_filter_map(parse_message).boxed();

        Ok(stream)
    }

    /// Parse a websocket message into a [`wire::PriceUpdate`].
    ///
    /// Messages which are not actually ticker updates are ignored and result in
    /// `None` being returned. In the context of a [`TryStream`], these will
    /// simply be filtered out.
    async fn parse_message(msg: tungstenite::Message) -> Result<Option<wire::PriceUpdate>, Error> {
        let msg = match msg {
            tungstenite::Message::Text(msg) => msg,
            tungstenite::Message::Close(close_frame) => {
                if let Some(tungstenite::protocol::CloseFrame { code, reason }) = close_frame {
                    tracing::error!(
                        "Kraken rate stream was closed with code {} and reason: {}",
                        code,
                        reason
                    );
                } else {
                    tracing::error!("Kraken rate stream was closed without code and reason");
                }

                return Err(Error::ConnectionClosed);
            }
            msg => {
                tracing::trace!(
                    "Kraken rate stream returned non text message that will be ignored: {}",
                    msg
                );

                return Ok(None);
            }
        };

        let update = match serde_json::from_str::<wire::Event>(&msg) {
            Ok(wire::Event::SystemStatus) => {
                tracing::debug!("Connected to Kraken websocket API");

                return Ok(None);
            }
            Ok(wire::Event::SubscriptionStatus) => {
                tracing::debug!("Subscribed to updates for ticker");

                return Ok(None);
            }
            Ok(wire::Event::Heartbeat) => {
                return Ok(None);
            }
            // if the message is not an event, it is a ticker update or an unknown event
            Err(_) => match serde_json::from_str::<wire::PriceUpdate>(&msg) {
                Ok(ticker) => ticker,
                Err(error) => {
                    tracing::warn!(%msg, "Failed to deserialize message as ticker update. Error {:#}", error);
                    return Ok(None);
                }
            },
        };

        Ok(Some(update))
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("The Kraken server closed the websocket connection")]
        ConnectionClosed,
        #[error("Failed to read message from websocket stream")]
        WebSocket(#[from] tungstenite::Error),
        #[error("Failed to parse rate from websocket message")]
        Parse(#[from] wire::Error),
    }

    const SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD: &str = r#"
    { "event": "subscribe",
      "pair": [ "XMR/XBT" ],
      "subscription": {
        "name": "ticker"
      }
    }"#;
}

/// Kraken websocket API wire module.
///
/// Responsible for parsing websocket text messages to events and rate updates.
mod wire {
    use super::*;
    use bitcoin::amount::ParseAmountError;
    use serde_json::Value;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(tag = "event")]
    pub enum Event {
        #[serde(rename = "systemStatus")]
        SystemStatus,
        #[serde(rename = "heartbeat")]
        Heartbeat,
        #[serde(rename = "subscriptionStatus")]
        SubscriptionStatus,
    }

    #[derive(Clone, Debug, thiserror::Error)]
    pub enum Error {
        #[error("Data field is missing")]
        DataFieldMissing,
        #[error("Ask Rate Element is of unexpected type")]
        UnexpectedAskRateElementType,
        #[error("Ask Rate Element is missing")]
        MissingAskRateElementType,
        #[error("Failed to parse Bitcoin amount")]
        BitcoinParseAmount(#[from] ParseAmountError),
    }

    /// Represents an update within the price ticker.
    #[derive(Clone, Debug, Deserialize)]
    #[serde(try_from = "TickerUpdate")]
    pub struct PriceUpdate {
        pub ask: bitcoin::Amount,
    }

    #[derive(Debug, Deserialize)]
    #[serde(transparent)]
    pub struct TickerUpdate(Vec<TickerField>);

    #[allow(unused)]
    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    pub enum TickerField {
        Data(TickerData),
        Metadata(Value),
    }

    #[derive(Debug, Deserialize)]
    pub struct TickerData {
        #[serde(rename = "a")]
        ask: Vec<RateElement>,
    }

    #[allow(unused)]
    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    pub enum RateElement {
        Text(String),
        Number(u64),
    }

    impl TryFrom<TickerUpdate> for PriceUpdate {
        type Error = Error;

        fn try_from(value: TickerUpdate) -> Result<Self, Error> {
            let data = value
                .0
                .iter()
                .find_map(|field| match field {
                    TickerField::Data(data) => Some(data),
                    TickerField::Metadata(_) => None,
                })
                .ok_or(Error::DataFieldMissing)?;
            let ask = data.ask.first().ok_or(Error::MissingAskRateElementType)?;
            let ask = match ask {
                RateElement::Text(ask) => {
                    bitcoin::Amount::from_str_in(ask, ::bitcoin::Denomination::Bitcoin)?
                }
                _ => return Err(Error::UnexpectedAskRateElementType),
            };

            Ok(PriceUpdate { ask })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn can_deserialize_system_status_event() {
            let event = r#"{"connectionID":14859574189081089471,"event":"systemStatus","status":"online","version":"1.8.1"}"#;

            let event = serde_json::from_str::<Event>(event).unwrap();

            assert_eq!(event, Event::SystemStatus)
        }

        #[test]
        fn can_deserialize_subscription_status_event() {
            let event = r#"{"channelID":980,"channelName":"ticker","event":"subscriptionStatus","pair":"XMR/XBT","status":"subscribed","subscription":{"name":"ticker"}}"#;

            let event = serde_json::from_str::<Event>(event).unwrap();

            assert_eq!(event, Event::SubscriptionStatus)
        }

        #[test]
        fn deserialize_ticker_update() {
            let message = r#"[980,{"a":["0.00440700",7,"7.35318535"],"b":["0.00440200",7,"7.57416678"],"c":["0.00440700","0.22579000"],"v":["273.75489000","4049.91233351"],"p":["0.00446205","0.00441699"],"t":[123,1310],"l":["0.00439400","0.00429900"],"h":["0.00450000","0.00450000"],"o":["0.00449100","0.00433700"]},"ticker","XMR/XBT"]"#;

            let _ = serde_json::from_str::<TickerUpdate>(message).unwrap();
        }
    }
}
