use anyhow::{self, Context};
use reqwest::header::{ACCEPT, HeaderMap, USER_AGENT};
use serde::de::Error;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

pub use generated_client;

#[derive(Debug, Error)]
pub enum OrangeFrenError {
    #[error("unknown currency symbol: {0}")]
    UnknownCurrency(String),
    #[error("Path creation failed failed: {0}")]
    PathCreateError(String),
}

// {
//     let client = Client::new();

//     let trade_id = client.create_trade(..);

//     while let Some(status) == client.watch_status(trade_id).await.next.await() {

//     }
// }

// trait Currency {
//     type Amount;
//     type Address;
// }

// struct Bitcoin;

// impl Currency for Bitcoin  {
//     type Address = bitcoin::Address;
//     type Amount = bitcoin::Amount;

//     fn as_str() -> &'static str;
// }

// fn create_trade<From: Currency, To: Currency>(from_amount: From::Amount, to_address: To::Address) -> Trade {
//     let currency = From::as_str(); // "XMR"
//     let amount = from_amount.to_float()

//         // (f64, "XMR")
// }

#[derive(Clone, Debug)]
enum TradeStatusType {
    Initial,
    Confirming,
    Exchanging,
    Success,
    Refunded,
    Failed,
    Expired,
    Unrecognized,
}

impl From<generated_client::models::trade_state::Type> for TradeStatusType {
    fn from(v: generated_client::models::trade_state::Type) -> Self {
        use generated_client::models::trade_state as api;
        match v {
            api::Type::Initial => Self::Initial,
            api::Type::Confirming => Self::Confirming,
            api::Type::Exchanging => Self::Exchanging,
            api::Type::Success => Self::Success,
            api::Type::Refunded => Self::Refunded,
            api::Type::Failed => Self::Failed,
            api::Type::Expired => Self::Expired,
            api::Type::Unrecognized => Self::Unrecognized,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TradeStatus {
    status_type: TradeStatusType,
    is_terminal: bool,
    description: String,
    valid_for: Duration,
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct TradeId(Uuid);

#[derive(Clone, Debug)]
pub struct Trade {
    id: TradeId,
    from_currency: Currency,
    to_currency: Currency,
    status: TradeStatus,
}

#[derive(Clone)]
pub struct Client {
    trades: HashMap<TradeId, Vec<TradeStatus>>,
    config: generated_client::apis::configuration::Configuration,
    //db: sqlite...,
}

impl Client {
    pub fn new() -> Self {
        let mut config = generated_client::apis::configuration::Configuration::new();

        config.base_path = "https://intercambio.app".to_string();

        Self {
            trades: HashMap::new(),
            config,
        }
    }

    pub async fn new_trade(
        &mut self,
        from_amount: bitcoin::Amount,
        to_address: monero::Address,
    ) -> Result<TradeId, anyhow::Error> {
        let mut path_request = generated_client::models::CreatePathRequest::new(
            from_amount.to_btc(),
            "BTC".to_string(),
            to_address.to_string(),
            "XMR".to_string(),
        );

        path_request.priority =
            Some(generated_client::models::create_path_request::Priority::default());

        path_request.service_variety =
            Some(generated_client::models::create_path_request::ServiceVariety::default());

        let res = generated_client::apis::default_api::api_eigenwallet_create_path_post(
            &self.config,
            path_request,
        )
        .await;

        let create_path_response = match res {
            Ok(ok) => ok,
            Err(generated_client::apis::Error::ResponseError(r)) => {
                return Err(anyhow::anyhow!(
                    "create-path HTTP {}: {}",
                    r.status,
                    r.content
                ));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("create-path request failed: {e}"));
            }
        };

        let path_uuid = create_path_response
            .path_uuid
            .context("Error getting uuid")?;

        println!("Got uuid: {}", path_uuid);

        let path_uuid = Uuid::from_str(path_uuid.as_str()).context("Error parsing uuid")?;
        let trade_uuid = TradeId(path_uuid);

        let status = self
            .get_status(trade_uuid.clone())
            .await
            .context("Error getting status")?;

        self.trades
            .entry(trade_uuid.clone())
            .or_default()
            .push(status);

        Ok(trade_uuid)
    }

    async fn get_status(&self, trade_id: TradeId) -> Result<TradeStatus, anyhow::Error> {
        println!("Looking for trade id {}", trade_id.0.to_string());
        let path_response =
            generated_client::apis::default_api::api_eigenwallet_get_path_path_uuid_post(
                &self.config,
                &trade_id.0.to_string(),
            )
            .await
            .context("Error getting the initial path responce")?;

        let path_state = path_response.state;

        match path_response.trades {
            Some(trades) => {
                if trades.len() > 1 {
                    anyhow::bail!("expected not more than 1 trade, got {}", trades.len());
                }

                let last_trade = trades.last().context("Last trade not found")?;
                let trade_state = last_trade.state.as_ref();

                Ok(TradeStatus {
                    status_type: trade_state.r#type.into(),
                    description: trade_state.description.clone(),
                    is_terminal: trade_state.r#final,
                    valid_for: Duration::from_secs(trade_state.valid_for as u64),
                })
            }
            None => {
                if path_state.r#type == generated_client::models::path_state::Type::NotFound {
                    return Err(anyhow::anyhow!("Path not found"));
                } else {
                    println!(
                        "Path state: {}, type {:?}",
                        path_state.description, path_state.r#type
                    );

                    Ok(TradeStatus {
                        status_type: TradeStatusType::Unrecognized,
                        description: path_state.description.clone(),
                        is_terminal: path_state.r#final,
                        valid_for: Duration::from_millis(1),
                    })
                }
            }
        }
    }

    pub async fn watch_status(&mut self, id: TradeId) -> ReceiverStream<TradeStatus> {
        let (tx, rx) = mpsc::channel(32);
        let client = self.clone();
        tokio::spawn(async move {
            loop {
                let status = match client.get_status(id.clone()).await {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("poll error: {e:#}");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        let error_status = TradeStatus {
                            status_type: TradeStatusType::Failed,
                            is_terminal: true,
                            description: e.to_string(),
                            valid_for: Duration::from_secs(30),
                        };

                        if tx.send(error_status.clone()).await.is_err() {
                            break;
                        };

                        error_status
                    }
                };
                if tx.send(status.clone()).await.is_err() {
                    break;
                }
                if status.is_terminal {
                    break;
                }
                tokio::time::sleep(status.valid_for / 2).await;
            }
        });
        ReceiverStream::new(rx)
    }

    async fn store(&self) {
        // trade (path_uuid, timestamp, from_currency, to_currency, from amoujtn ,,..., raw json response)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Currency {
    Xmr,
    Btc,
}

impl Currency {
    pub const fn as_str(self) -> &'static str {
        match self {
            Currency::Xmr => "XMR",
            Currency::Btc => "BTC",
        }
    }
}

impl TryFrom<&generated_client::models::Currency> for Currency {
    type Error = OrangeFrenError;
    fn try_from(c: &generated_client::models::Currency) -> Result<Self, Self::Error> {
        match c.symbol.as_str() {
            "XMR" => Ok(Currency::Xmr),
            "BTC" => Ok(Currency::Btc),
            other => Err(OrangeFrenError::UnknownCurrency(other.to_string())),
        }
    }
}
