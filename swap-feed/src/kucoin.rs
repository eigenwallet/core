use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt, TryStreamExt};
use serde::Deserialize;
use std::convert::TryFrom;
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
    crate::ticker::connect(
        "KuCoin",
        (price_ticker_rest_url_kucoin, client),
        connection::new,
    )
}

pub type PriceUpdates = crate::ticker::PriceUpdates<wire::PriceUpdate>;

pub type Error = crate::ticker::Error;

/// KuCoin websocket connection module.
///
/// Responsible for getting the token and address for the KuCoin websocket API,
/// then establishing a connection to it and transforming the received websocket
/// frames into a stream of rate updates. The connection may fail in which case
/// it is simply terminated and the stream ends.
mod connection {
    use super::*;
    use futures::stream::BoxStream;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;
    use tokio_tungstenite::tungstenite;

    pub async fn new(
        rest_url_client: Arc<(Url, reqwest::Client)>,
    ) -> Result<BoxStream<'static, Result<wire::PriceUpdate, Error>>> {
        let &(ref rest_url, ref client) = &*rest_url_client;

        let auth: wire::BulletPublicResponse = client
            .post(rest_url.clone())
            .send()
            .await
            .context(
                "Failed to call the KuCoin REST API to acquire auth token and websocket servers",
            )?
            .json()
            .await
            .context("KuCoin REST API returned invalid data")?;

        let (mut ws_url, ping_interval_ms) = auth
            .data
            .instance_servers
            .iter()
            .find_map(|is| {
                Url::parse(&is.endpoint)
                    .ok()
                    .map(|u| (u, is.ping_interval_ms))
            })
            .ok_or(Error::NoWebsocketServers)?;
        // https://www.kucoin.com/docs-new/websocket-api/base-info/introduction#3-create-connection
        ws_url.set_query(Some(&format!("token={}", auth.data.token)[..]));
        // The real time-out is about double pingInterval, we get the pre-halved value from the API
        let ping_interval = Duration::from_millis(ping_interval_ms);
        tracing::debug!(%ws_url, ?ping_interval, "KuCoin REST API returned valid websocket URL");

        let (rate_stream, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .context("Failed to connect to KuCoin websocket API")?;

        let (to_kucoin, rate_stream) = rate_stream.split();
        let to_kucoin = Arc::new(Mutex::new(to_kucoin));

        let stream = rate_stream
            .err_into()
            .try_filter_map(move |msg| parse_message(msg, to_kucoin.clone(), ping_interval))
            .boxed();

        Ok(stream)
    }

    type ToKucoin = futures::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tungstenite::Message,
    >;

    /// Parse a websocket message into a [`wire::PriceUpdate`].
    ///
    /// Messages which are not actually ticker updates are ignored and result in
    /// `None` being returned. In the context of a [`TryStream`], these will
    /// simply be filtered out.
    async fn parse_message(
        msg: tungstenite::Message,
        to_kucoin: Arc<Mutex<ToKucoin>>,
        ping_interval: Duration,
    ) -> Result<Option<wire::PriceUpdate>, Error> {
        let msg = match msg {
            tungstenite::Message::Text(msg) => msg,
            tungstenite::Message::Close(close_frame) => {
                if let Some(tungstenite::protocol::CloseFrame { code, reason }) = close_frame {
                    tracing::error!(
                        "KuCoin rate stream was closed with code {} and reason: {}",
                        code,
                        reason
                    );
                } else {
                    tracing::error!("KuCoin rate stream was closed without code and reason");
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

                to_kucoin
                    .lock()
                    .await
                    .send(SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD.into())
                    .await?;
                tokio::spawn(async move {
                    let mut ping_timer = tokio::time::interval_at(
                        tokio::time::Instant::now() + ping_interval,
                        ping_interval,
                    );
                    loop {
                        ping_timer.tick().await;
                        tracing::debug!("Renewing KuCoin ticker server lease");
                        to_kucoin
                            .lock()
                            .await
                            .send(PING_PAYLOAD.into())
                            .await
                            .expect("Renewing KuCoin lease");
                    }
                });

                return Ok(None);
            }
            Ok(wire::Event::ACK) => {
                tracing::debug!("Subscribed to updates for ticker");

                return Ok(None);
            }
            Ok(wire::Event::Pong) => {
                tracing::debug!("Renewed KuCoin ticker server lease");

                return Ok(None);
            }
            Ok(wire::Event::Error { data }) => {
                tracing::error!("KuCoin error: {}. Reconnecting.", data);

                // We most likely got here because the ping thread died to remote hangup => I/O error.
                // If we don't, this will kill it on next iteration.
                let _ = to_kucoin.lock().await.close().await;

                return Err(Error::RemoteError(data));
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
        #[error("KuCoin remote error: {0}")]
        RemoteError(String),
    }

    const SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD: &str =
        r#"{"type": "subscribe", "topic": "/market/ticker:XMR-BTC", "response": true}"#;
    const PING_PAYLOAD: &str = r#"{"type": "ping"}"#;
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
///
/// We must send `{"type":"ping"}` every `pingInterval` to get `{"type":"pong","timestamp":1762983181128413}`.
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
        #[serde(rename = "pingInterval")]
        pub ping_interval_ms: u64,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(tag = "type")]
    pub enum Event {
        #[serde(rename = "welcome")]
        Welcome,
        #[serde(rename = "ack")]
        ACK,
        #[serde(rename = "pong")]
        Pong,
        #[serde(rename = "error")]
        Error { data: String },
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
        fn can_deserialize_welcome_event() {
            let event = r#"{"id":"15xO8l89LYO","type":"welcome"}"#;

            let event = serde_json::from_str::<Event>(event).unwrap();

            assert_eq!(event, Event::Welcome)
        }

        #[test]
        fn can_deserialize_subscribed_event() {
            let event = r#"{"type":"ack"}"#;

            let event = serde_json::from_str::<Event>(event).unwrap();

            assert_eq!(event, Event::ACK)
        }

        #[test]
        fn can_deserialize_pong_event() {
            let event = r#"{"type":"pong","timestamp":1762983181128413}"#;

            let event = serde_json::from_str::<Event>(event).unwrap();

            assert_eq!(event, Event::Pong)
        }

        #[test]
        fn can_deserialize_message_event() {
            let event = r#"{"topic":"/market/ticker:XMR-BTC","type":"message","subject":"trade.ticker","data":{"bestAsk":"0.003751","bestAskSize":"2.512","bestBid":"0.003748","bestBidSize":"0.208","price":"0.00375","sequence":"1437643854","size":"0.607","time":1762983159347}}"#;

            let event = serde_json::from_str::<Event>(event).unwrap();

            let Event::Message {
                data:
                    MessageEventData {
                        best_ask: PriceUpdate { ask },
                    },
            } = event
            else {
                panic!("bad variant")
            };
            assert_eq!(
                ask,
                bitcoin::Amount::from_str_in("0.003751", bitcoin::Denomination::Bitcoin).unwrap()
            );
        }
    }
}
