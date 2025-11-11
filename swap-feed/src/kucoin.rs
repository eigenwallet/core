use anyhow::{anyhow, Context, Result};
use futures::{SinkExt, StreamExt, TryStreamExt};
use serde::Deserialize;
use std::convert::{Infallible, TryFrom};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use url::Url;

/// Connect to KuCoin websocket API for a constant stream of rate updates.
///
/// If the connection fails, it will automatically be re-established.
///
/// price_ticker_rest_url_kucoin must point to an HTTP REST server that follows the kucoin
/// REST API
///
/// See: https://www.kucoin.com/docs-new/websocket-api/base-info/get-public-token-spot-margin
/// See: https://www.kucoin.com/docs-new/websocket-api/base-info/introduction
/// See: https://www.kucoin.com/docs-new/3470063w0?lang=en_US
pub fn connect(price_ticker_rest_url_kucoin: Url, client: reqwest::Client) -> Result<PriceUpdates> {
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
                let price_ticker_rest_url_kucoin = price_ticker_rest_url_kucoin.clone();
                let client = client.clone();
                async move {
                    let mut stream = connection::new(price_ticker_rest_url_kucoin, client).await?;

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
                    "KuCoin websocket connection failed, retrying in {}ms. Error {:#}",
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
                price_update.send(Err(Error::PermanentFailure))
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

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("Rate is not yet available")]
    NotYetAvailable,
    #[error("Permanently failed to retrieve rate from KuCoin")]
    PermanentFailure,
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
        connection::Error::Parse(_) | connection::Error::NoWebsocketServers => {
            Permanent(anyhow::Error::from(e))
        }
    }
}

/// KuCoin websocket connection module.
///
/// Responsible for getting the token and address for the KuCoin websocket API,
/// then establishing a connection to it and transforming the received websocket
/// frames into a stream of rate updates. The connection may fail in which case
/// it is simply terminated and the stream ends.
mod connection {
    use super::*;
    use futures::stream::BoxStream;
    use tokio_tungstenite::tungstenite;

    pub async fn new(
        rest_url: Url,
        client: reqwest::Client,
    ) -> Result<BoxStream<'static, Result<wire::PriceUpdate, Error>>> {
        let auth: wire::BulletPublicResponse = client
            .post(rest_url)
            .send()
            .await
            .context("Failed to call the KuCoin REST API")?
            .json()
            .await
            .context("KuCoin REST API returned invalid data")?;

        let mut ws_url = auth
            .data
            .instance_servers
            .iter()
            .find_map(|is| Url::parse(&is.endpoint).ok())
            .ok_or(Error::NoWebsocketServers)?;
        ws_url.set_query(Some(&format!("token={}", auth.data.token)[..]));
        tracing::debug!(%ws_url);

        let (mut rate_stream, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .context("Failed to connect to KuCoin websocket API")?;

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
                    tracing::debug!(
                        "KuCoin rate stream was closed with code {} and reason: {}",
                        code,
                        reason
                    );
                } else {
                    tracing::debug!("KuCoin rate stream was closed without code and reason");
                }

                return Err(Error::ConnectionClosed);
            }
            msg => {
                tracing::trace!(
                    "KuCoin rate stream returned non text message that will be ignored: {}",
                    msg
                );

                return Ok(None);
            }
        };

        let update = match serde_json::from_str::<wire::Event>(&msg) {
            Ok(wire::Event::Welcome) => {
                tracing::debug!("Connected to KuCoin websocket API");

                return Ok(None);
            }
            Ok(wire::Event::ACK) => {
                tracing::debug!("Subscribed to updates for ticker");

                return Ok(None);
            }
            Ok(wire::Event::Message { data }) => data.best_ask,
            Err(error) => {
                tracing::warn!(%msg, "Failed to deserialize message as ticker update. Error {:#}", error);
                return Ok(None);
            }
        };

        Ok(Some(update))
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("The KuCoin server closed the websocket connection")]
        ConnectionClosed,
        #[error("Failed to read message from websocket stream")]
        WebSocket(#[from] tungstenite::Error),
        #[error("Failed to parse rate from websocket message")]
        Parse(#[from] wire::Error),
        #[error("KuCoin didn't give us any Websocket servers")]
        NoWebsocketServers,
    }

    const SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD: &str =
        r#"{"type": "subscribe", "topic": "/market/ticker:XMR-BTC", "response": true}"#;
}

/// KuCoin websocket API wire module.
///
/// Responsible for parsing websocket text messages to events and rate updates.
///
/// https://www.kucoin.com/docs-new/3470063w0?lang=en_US
/// ```shell-session
/// $ websocat "wss://ws-api-spot.kucoin.com/"?token=...
/// {"id":"15vfeLTo3ii","type":"welcome"}
/// {"type": "subscribe", "topic": "/market/ticker:XMR-BTC", "response": true }
/// {"type":"ack"}
/// {"topic":"/market/ticker:XMR-BTC","type":"message","subject":"trade.ticker","data":{"bestAsk":"0.003622","bestAskSize":"0.01","bestBid":"0.003621","bestBidSize":"0.714","price":"0.003616","sequence":"1434695930","size":"0.714","time":1762893726426}}
/// {"topic":"/market/ticker:XMR-BTC","type":"message","subject":"trade.ticker","data":{"bestAsk":"0.003622","bestAskSize":"0.01","bestBid":"0.003621","bestBidSize":"0.714","price":"0.003616","sequence":"1434695936","size":"0.714","time":1762893726426}}
/// {"topic":"/market/ticker:XMR-BTC","type":"message","subject":"trade.ticker","data":{"bestAsk":"0.003622","bestAskSize":"0.01","bestBid":"0.003621","bestBidSize":"0.714","price":"0.003616","sequence":"1434695943","size":"0.714","time":1762893726426}}
/// ```
mod wire {
    use super::*;

    /// https://api.kucoin.com/api/v1/bullet-public
    #[derive(Debug, Deserialize, PartialEq)]
    pub struct BulletPublicResponse {
        pub data: BulletPublicResponseData,
    }
    #[derive(Debug, Deserialize, PartialEq)]
    pub struct BulletPublicResponseData {
        pub token: String,
        #[serde(rename = "instanceServers")]
        pub instance_servers: Vec<BulletPublicResponseDataInstanceServers>,
    }
    #[derive(Debug, Deserialize, PartialEq)]
    pub struct BulletPublicResponseDataInstanceServers {
        pub endpoint: String,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(tag = "type")]
    pub enum Event {
        #[serde(rename = "welcome")]
        Welcome,
        #[serde(rename = "ack")]
        ACK,
        #[serde(rename = "message")]
        Message { data: MessageEventData },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    pub struct MessageEventData {
        #[serde(rename = "bestAsk")]
        pub best_ask: PriceUpdate,
    }

    #[derive(Clone, Debug, Deserialize, PartialEq)]
    #[serde(try_from = "&str")]
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

    impl TryFrom<&str> for PriceUpdate {
        type Error = Error;

        fn try_from(best_ask: &str) -> Result<Self, Error> {
            let ask = bitcoin::Amount::from_str_in(&best_ask, bitcoin::Denomination::Bitcoin)?;

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
