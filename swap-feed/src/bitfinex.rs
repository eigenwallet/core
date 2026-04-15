use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt, TryStreamExt};
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::convert::TryFrom;
use std::fmt;
use url::Url;

/// Connect to Bitfinex websocket API for a constant stream of rate updates.
///
/// If the connection fails, it will automatically be re-established.
///
/// price_ticker_ws_url_bitfinex must point to a websocket server that follows the bitfinex
/// price ticker protocol version 2
/// See: https://docs.bitfinex.com/docs/ws-public
/// See: https://docs.bitfinex.com/reference/ws-public-ticker
pub fn connect(price_ticker_ws_url_bitfinex: Url) -> Result<PriceUpdates> {
    crate::ticker::connect("Bitfinex", price_ticker_ws_url_bitfinex, connection::new)
}

pub type PriceUpdates = crate::ticker::PriceUpdates<wire::PriceUpdate>;
pub type PriceUpdate = crate::ticker::PriceUpdate<wire::PriceUpdate>;
pub type Error = crate::ticker::Error;

/// Bitfinex websocket connection module.
///
/// Responsible for establishing a connection to the Bitfinex websocket API and
/// transforming the received websocket frames into a stream of rate updates.
/// The connection may fail in which case it is simply terminated and the stream
/// ends.
mod connection {
    use super::*;
    use futures::stream::BoxStream;
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite;

    pub async fn new(
        ws_url: Arc<Url>,
    ) -> Result<BoxStream<'static, Result<wire::PriceUpdate, Error>>> {
        let (mut rate_stream, _) = tokio_tungstenite::connect_async(&*ws_url)
            .await
            .context("Failed to connect to Bitfinex websocket API")?;

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
                        "Bitfinex rate stream was closed with code {} and reason: {}",
                        code,
                        reason
                    );
                } else {
                    tracing::error!("Bitfinex rate stream was closed without code and reason");
                }

                return Err(Error::ConnectionClosed);
            }
            msg => {
                tracing::trace!(
                    "Bitfinex rate stream returned non text message that will be ignored: {}",
                    msg
                );

                return Ok(None);
            }
        };

        let update = match serde_json::from_str::<wire::ObjectEvent>(&msg) {
            Ok(wire::ObjectEvent::Info) => {
                tracing::debug!("Connected to Bitfinex websocket API");

                return Ok(None);
            }
            Ok(wire::ObjectEvent::Subscribed) => {
                tracing::debug!("Subscribed to updates for ticker");

                return Ok(None);
            }
            // if the message is not an object-wrapped event, it is a heartbeat, ticker update, or something unknown
            Err(_) => match serde_json::from_str::<wire::HeartbeatEvent>(&msg) {
                Ok(_) => {
                    return Ok(None);
                }
                Err(_) => match serde_json::from_str::<wire::PriceUpdate>(&msg) {
                    Ok(ticker) => ticker,
                    Err(error) => {
                        tracing::warn!(%msg, "Failed to deserialize message as ticker update. Error {:#}", error);
                        return Ok(None);
                    }
                },
            },
        };

        Ok(Some(update))
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("The Bitfinex server closed the websocket connection")]
        ConnectionClosed,
        #[error("Failed to read message from websocket stream")]
        WebSocket(#[from] tungstenite::Error),
        #[error("Failed to parse rate from websocket message")]
        Parse(#[from] wire::Error),
    }

    const SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD: &str =
        r#"{"event": "subscribe", "channel": "ticker", "symbol": "tXMRBTC"}"#;
}

/// Bitfinex websocket API wire module.
///
/// Responsible for parsing websocket text messages to events and rate updates.
///
/// https://docs.bitfinex.com/reference/ws-public-ticker
/// ```shell-session
/// $ websocat wss://api-pub.bitfinex.com/ws/2
/// {"event":"info","version":2,"serverId":"2307ff06-41db-4d2b-b3ce-220348811755","platform":{"status":1}}
/// {"event": "subscribe", "channel": "ticker", "symbol": "tXMRBTC" }
/// {"event":"subscribed","channel":"ticker","chanId":225000,"symbol":"tXMRBTC","pair":"XMRBTC"}
/// [225000,[0.003744,358.96223856,0.0037548,338.14332753,-0.0000834,-0.02175955,0.0037494,1284.13109312,0.0038328,0.0035223]]
/// [225000,"hb"]
/// ```
/// `[chanId,[BID,BID_SIZE,ASK,ASK_SIZE,DAILY_CHANGE,DAILY_CHANGE_RELATIVE,LAST_PRICE,VOLUME,HIGH,LOW]]`
pub mod wire {
    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(tag = "event")]
    pub enum ObjectEvent {
        #[serde(rename = "info")]
        Info,
        #[serde(rename = "subscribed")]
        Subscribed,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    pub struct HeartbeatEvent(u64, pub String);

    /// Bitfinex may append new fields to the ticker array at any time (often as a trailing
    /// `null` placeholder), per their WS spec: "message lengths should never be hardcoded".
    /// We read exactly the 10 documented fields and silently discard any trailing elements.
    #[derive(Debug, PartialEq)]
    pub struct TradingEvent(pub u64, pub [f64; 10]);

    impl<'de> Deserialize<'de> for TradingEvent {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct TradingEventVisitor;

            impl<'de> Visitor<'de> for TradingEventVisitor {
                type Value = TradingEvent;

                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("a [chan_id, [..10 floats..]] array with optional trailing fields")
                }

                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<TradingEvent, A::Error> {
                    let chan_id: u64 = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                    let values: PriceValues = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                    // drain any extra trailing elements Bitfinex may have appended
                    while seq.next_element::<de::IgnoredAny>()?.is_some() {}
                    Ok(TradingEvent(chan_id, values.0))
                }
            }

            deserializer.deserialize_seq(TradingEventVisitor)
        }
    }

    /// The 10 documented ticker floats, tolerating (and ignoring) any trailing fields
    /// Bitfinex may append in the future.
    #[derive(Debug, PartialEq)]
    struct PriceValues([f64; 10]);

    impl<'de> Deserialize<'de> for PriceValues {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct PriceValuesVisitor;

            impl<'de> Visitor<'de> for PriceValuesVisitor {
                type Value = PriceValues;

                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("an array of at least 10 floats")
                }

                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<PriceValues, A::Error> {
                    let mut values = [0.0f64; 10];
                    for (i, slot) in values.iter_mut().enumerate() {
                        *slot = seq
                            .next_element()?
                            .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                    }
                    while seq.next_element::<de::IgnoredAny>()?.is_some() {}
                    Ok(PriceValues(values))
                }
            }

            deserializer.deserialize_seq(PriceValuesVisitor)
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    #[serde(try_from = "TradingEvent")]
    pub struct PriceUpdate {
        pub ask: bitcoin::Amount,
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("Failed to parse JSON message")]
        JsonParseError(#[from] serde_json::Error),
        #[error("Failed to parse Bitcoin amount")]
        BitcoinParseAmount(#[from] bitcoin::amount::ParseAmountError),
    }

    impl TryFrom<TradingEvent> for PriceUpdate {
        type Error = Error;

        fn try_from(value: TradingEvent) -> Result<Self, Error> {
            let [
                _bid,
                _bid_size,
                ask,
                _ask_size,
                _daily_change,
                _daily_change_relative,
                _last_price,
                _volume,
                _high,
                _low,
            ] = value.1;

            let ask = bitcoin::Amount::from_btc(ask)?;

            Ok(PriceUpdate { ask })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn can_deserialize_system_info_event() {
            let event = r#"{"event":"info","version":2,"serverId":"2307ff06-41db-4d2b-b3ce-220348811755","platform":{"status":1}}"#;

            let event = serde_json::from_str::<ObjectEvent>(event).unwrap();

            assert_eq!(event, ObjectEvent::Info)
        }

        #[test]
        fn can_deserialize_subscribed_event() {
            let event = r#"{"event":"subscribed","channel":"ticker","chanId":225000,"symbol":"tXMRBTC","pair":"XMRBTC"}"#;

            let event = serde_json::from_str::<ObjectEvent>(event).unwrap();

            assert_eq!(event, ObjectEvent::Subscribed)
        }

        #[test]
        fn can_deserialize_heartbeat_event() {
            let event = r#"[225000,"hb"]"#;

            let event = serde_json::from_str::<HeartbeatEvent>(event).unwrap();

            assert_eq!(event, HeartbeatEvent(225000, "hb".to_string()))
        }

        #[test]
        fn can_deserialize_trading_event_with_trailing_null() {
            // Bitfinex appended an extra `null` field to the ticker array. Parsing must
            // still succeed and the 10 documented values must be preserved.
            let message = r#"[1958338,[0.004619,531.52417965,0.0046467,356.07460428,-0.0000349,-0.00749909,0.004619,416.68683558,0.0047546,0.004571,null]]"#;

            let event = serde_json::from_str::<TradingEvent>(message).unwrap();

            assert_eq!(
                event,
                TradingEvent(
                    1958338,
                    [
                        0.004619,
                        531.52417965,
                        0.0046467,
                        356.07460428,
                        -0.0000349,
                        -0.00749909,
                        0.004619,
                        416.68683558,
                        0.0047546,
                        0.004571,
                    ]
                )
            );
        }

        #[test]
        fn can_deserialize_trading_event() {
            let message = r#"[225000,[0.003744,358.96223856,0.0037548,338.14332753,-0.0000834,-0.02175955,0.0037494,1284.13109312,0.0038328,0.0035223]]"#;

            let event = serde_json::from_str::<TradingEvent>(message).unwrap();

            assert_eq!(
                event,
                TradingEvent(
                    225000,
                    [
                        0.003744,
                        358.96223856,
                        0.0037548,
                        338.14332753,
                        -0.0000834,
                        -0.02175955,
                        0.0037494,
                        1284.13109312,
                        0.0038328,
                        0.0035223
                    ]
                )
            )
        }
    }
}
