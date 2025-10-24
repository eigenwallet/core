use anyhow::{self, Context};
use bitcoin::hashes::Hash;
use std::os::macos::raw::stat;
use std::str::FromStr;
use std::time::Duration;
use std::{collections::HashMap, f64};
use thiserror::Error;
use tokio::io::DuplexStream;
use tokio::sync::mpsc;
use tokio_stream::{StreamExt as _, wrappers::ReceiverStream};
use uuid::Uuid;

pub use generated_client;

#[derive(Debug, Error)]
pub enum OrangeFrenError {
    #[error("unknown currency symbol: {0}")]
    UnknownCurrency(String),
    #[error("Path creation failed failed: {0}")]
    PathCreateError(String),
}

#[derive(Clone)]
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

#[derive(Clone)]
struct TradeStatus {
    id: TradeId,
    from_currency: Currency,
    to_currency: Currency,
    status: TradeStatusType,
    is_terminal: bool,
    description: String,
    valid_for: Duration,
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub struct TradeId(Uuid);

#[derive(Clone)]
pub struct Client {
    trades: HashMap<TradeId, Vec<TradeStatus>>,
    config: generated_client::apis::configuration::Configuration,
    //db: sqlite...,
}

impl Client {
    pub fn new() -> Client {
        let mut config = generated_client::apis::configuration::Configuration::new();

        config.base_path = "https://intercambio.app/apidocs/".to_string();

        Client {
            trades: HashMap::new(),
            config, //db:
        }
    }

    pub async fn new_trade(
        &mut self,
        from_amount: bitcoin::Amount,
        to_address: monero::Address,
    ) -> Result<TradeId, anyhow::Error> {
        let path_request = generated_client::models::CreatePathRequest::new(
            from_amount.to_btc(),
            "BTC".to_string(),
            to_address.as_hex(),
            "XMR".to_string(),
        );

        let create_path_responce =
            generated_client::apis::default_api::api_eigenwallet_create_path_get(
                &self.config,
                path_request,
            )
            .await
            .map_err(|e| {
                OrangeFrenError::PathCreateError(format!("Failed to send sign request: {}", e))
            })?;

        let path_uuid = create_path_responce.path_uuid.expect("Error getting uuid");

        let path_uuid = Uuid::from_str(path_uuid.as_str()).expect("Error parsing uuid");
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

    pub async fn get_status(&self, trade_id: TradeId) -> Result<TradeStatus, anyhow::Error> {
        // if let Some(self.trades.get(TradeId)) {

        // }

        // if letzter state nicht mehr gültig {
        // fetch new status
        // }
        //
        // else return last saved state

        let path_response =
            generated_client::apis::default_api::api_eigenwallet_get_path_path_uuid_get(
                &self.config,
                &trade_id.0.to_string(),
            )
            .await
            .context("Error getting the initial path responce")?;

        let trades = path_response.trades.context("No trades found")?;
        if trades.len() != 1 {
            anyhow::bail!("expected exactly 1 trade, got {}", trades.len());
        }

        let last_trade = trades.last().context("No trades found")?;
        let trade_state = last_trade.state.as_ref();

        Ok(TradeStatus {
            id: trade_id.clone(),
            from_currency: last_trade.from_currency.as_ref().try_into()?,
            to_currency: last_trade.to_currency.as_ref().try_into()?,
            status: trade_state.r#type.into(),
            description: trade_state.description.clone(),
            is_terminal: trade_state.r#final,
            valid_for: Duration::from_secs(trade_state.valid_for as u64),
        })
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
                        continue;
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
