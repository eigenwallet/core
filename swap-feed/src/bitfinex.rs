use anyhow::{anyhow, Context, Result};
use futures::{SinkExt, StreamExt, TryStreamExt};
use serde::Deserialize;
use std::convert::{Infallible, TryFrom};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
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
    let (price_update, price_update_receiver) = watch::channel(Err(Error::NotYetAvailable));
    let price_update = Arc::new(price_update);

    tokio::spawn(async move {
        // The default backoff config is fine for us apart from one thing:
        // `max_elapsed_time`. If we don't get an error within this timeframe,
        // backoff won't actually retry the operation.
        let backoff = backoff::ExponentialBackoff {
            max_elapsed_time: None,
            ..backoff::ExponentialBackoff::default()
        };

        let result = backoff::future::retry_notify::<Infallible, _, _, _, _, _>(
            backoff,
            || {
                let price_update = price_update.clone();
                let price_ticker_ws_url_bitfinex = price_ticker_ws_url_bitfinex.clone();
                async move {
                    let mut stream = connection::new(price_ticker_ws_url_bitfinex).await?;

                    while let Some(update) = stream.try_next().await.map_err(to_backoff)? {
                        let send_result = price_update.send(Ok(update));

                        if send_result.is_err() {
                            return Err(backoff::Error::Permanent(anyhow!(
                                "receiver disconnected"
                            )));
                        }
                    }

                    Err(backoff::Error::transient(anyhow!("stream ended")))
                }
            },
            |error, next: Duration| {
                tracing::info!(
                    "Bitfinex websocket connection failed, retrying in {}ms. Error {:#}",
                    next.as_millis(),
                    error
                );
            },
        )
        .await;

        match result {
            Err(e) => {
                tracing::warn!("Rate updates incurred an unrecoverable error: {:#}", e);

                // in case the retries fail permanently, let the subscribers know
                price_update.send(Err(Error::PermanentFailure(e.into())))
            }
            Ok(never) => match never {},
        }
    });

    Ok(PriceUpdates {
        inner: price_update_receiver,
    })
}

#[derive(Clone, Debug)]
pub struct PriceUpdates {
    inner: watch::Receiver<PriceUpdate>,
}

impl PriceUpdates {
    pub async fn wait_for_next_update(&mut self) -> Result<PriceUpdate> {
        self.inner.changed().await?;

        Ok(self.inner.borrow().clone())
    }

    pub fn latest_update(&mut self) -> PriceUpdate {
        self.inner.borrow().clone()
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("Rate is not yet available")]
    NotYetAvailable,
    #[error("Permanently failed to retrieve rate from Bitfinex")]
    PermanentFailure(Arc<anyhow::Error>),
}

type PriceUpdate = Result<wire::PriceUpdate, Error>;

/// Maps a [`connection::Error`] to a backoff error, effectively defining our
/// retry strategy.
fn to_backoff(e: connection::Error) -> backoff::Error<anyhow::Error> {
    use backoff::Error::*;

    match e {
        // Connection closures and websocket errors will be retried
        connection::Error::ConnectionClosed => backoff::Error::transient(anyhow::Error::from(e)),
        connection::Error::WebSocket(_) => backoff::Error::transient(anyhow::Error::from(e)),

        // Failures while parsing a message are permanent because they most likely present a
        // programmer error
        connection::Error::Parse(_) => Permanent(anyhow::Error::from(e)),
    }
}

/// Bitfinex websocket connection module.
///
/// Responsible for establishing a connection to the Bitfinex websocket API and
/// transforming the received websocket frames into a stream of rate updates.
/// The connection may fail in which case it is simply terminated and the stream
/// ends.
mod connection {
    use super::*;
    use futures::stream::BoxStream;
    use tokio_tungstenite::tungstenite;

    pub async fn new(ws_url: Url) -> Result<BoxStream<'static, Result<wire::PriceUpdate, Error>>> {
        let (mut rate_stream, _) = tokio_tungstenite::connect_async(ws_url)
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
/// ```
/// $ websocat wss://api-pub.bitfinex.com/ws/2
/// {"event":"info","version":2,"serverId":"2307ff06-41db-4d2b-b3ce-220348811755","platform":{"status":1}}
/// {"event": "subscribe", "channel": "ticker", "symbol": "tXMRBTC" }
/// {"event":"subscribed","channel":"ticker","chanId":225000,"symbol":"tXMRBTC","pair":"XMRBTC"}
/// [225000,[0.003744,358.96223856,0.0037548,338.14332753,-0.0000834,-0.02175955,0.0037494,1284.13109312,0.0038328,0.0035223]]
/// [225000,"hb"]
/// ```
/// `[chanId,[BID,BID_SIZE,ASK,ASK_SIZE,DAILY_CHANGE,DAILY_CHANGE_RELATIVE,LAST_PRICE,VOLUME,HIGH,LOW]]`
mod wire {
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

    #[derive(Debug, Deserialize, PartialEq)]
    pub struct TradingEvent(u64, [f64; 10]);

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
            let [_bid, _bid_size, ask, _ask_size, _daily_change, _daily_change_relative, _last_price, _volume, _high, _low] =
                value.1;

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
