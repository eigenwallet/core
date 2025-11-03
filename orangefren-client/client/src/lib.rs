pub mod database;

use anyhow::{Context, Result};
use generated_client::models::path_response;
use sqlx::types::chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use tracing;

pub use database::Database;
pub use generated_client;

#[derive(Debug, Error)]
pub enum OrangeFrenError {
    #[error("unknown currency symbol: {0}")]
    UnknownCurrency(String),
    #[error("Path creation failed failed: {0}")]
    PathCreateError(String),
}

#[derive(Clone, Debug)]
enum TradeStatusType {
    Queued,
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

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct TradeInfo {
    from_currency: Currency,
    to_currency: Currency,
    from_network: Currency,
    to_network: Currency,
    withdraw_address: monero::Address,
    deposit_address: Option<bitcoin::Address>,
    raw_json: String,
}

#[derive(Clone)]
pub struct Client {
    trades: HashMap<TradeId, (TradeInfo, Vec<TradeStatus>)>,
    config: generated_client::apis::configuration::Configuration,
    db: Database,
}

impl Client {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        let mut config = generated_client::apis::configuration::Configuration::new();
        config.base_path = "https://intercambio.app".to_string();

        let db = Database::new(data_dir)
            .await
            .context("Error creating the trades db")?;

        Ok(Self {
            trades: HashMap::new(),
            config,
            db,
        })
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

        tracing::info!("Got uuid: {}", path_uuid);

        let path_uuid = Uuid::from_str(path_uuid.as_str()).context("Error parsing uuid")?;
        let trade_id = TradeId(path_uuid);

        let trade_info = self
            .wait_until_created(trade_id.clone())
            .await
            .context("Error creating the trade")?;

        self.trades
            .insert(trade_id.clone(), (trade_info.clone(), Vec::new()));

        self.db
            .insert_trade_info(trade_info, trade_id.clone())
            .await
            .context("Could not insert trade info into the db")?;

        Ok(trade_id)
    }

    pub fn trade_state_by_id(&self, trade_id: TradeId) -> Option<(TradeInfo, Vec<TradeStatus>)> {
        self.trades.get(&trade_id).cloned()
    }

    async fn wait_until_created(&self, trade_id: TradeId) -> Result<TradeInfo, anyhow::Error> {
        let delay = Duration::from_millis(250);
        let deadline = Instant::now() + Duration::from_secs(60);

        loop {
            let path_response =
                generated_client::apis::default_api::api_eigenwallet_get_path_path_uuid_post(
                    &self.config,
                    &trade_id.0.to_string(),
                )
                .await
                .context("Error getting path response")?;

            use generated_client::models::path_state::Type as State;

            match path_response.state.r#type {
                State::NotFound => anyhow::bail!("Path not found"),
                State::Error => anyhow::bail!("Error getting path response"),
                State::Queued => {
                    if Instant::now() >= deadline {
                        anyhow::bail!("Waiting for path to be created timed out");
                    }
                    tokio::time::sleep(delay).await;
                }
                State::Created => {
                    let trade_info = self
                        .get_trade_info(path_response)
                        .await
                        .context("Could not get trade info")?;

                    return Ok(trade_info);
                }
            }
        }
    }

    async fn get_trade_info(
        &self,
        path_response: generated_client::models::PathResponse,
    ) -> Result<TradeInfo, anyhow::Error> {
        match path_response.clone().trades {
            Some(trades) => {
                if trades.len() > 1 {
                    anyhow::bail!("expected not more than 1 trade, got {}", trades.len());
                }

                let last_trade = trades.last().context("Last trade not found")?;

                tracing::info!("bitcoin addr: {}", last_trade.deposit_address);
                let bitcoin_addr = if last_trade.deposit_address != "None" {
                    Some(
                        bitcoin::Address::from_str(last_trade.deposit_address.as_str())
                            .context("Could not parse bitcoin address")?
                            .assume_checked(),
                    )
                } else {
                    None
                };

                let raw_json = serde_json::to_string(&path_response).context("Serde error")?;

                let trade_info = TradeInfo {
                    from_currency: Currency::try_from(last_trade.from_currency.as_ref())?,
                    to_currency: Currency::try_from(last_trade.to_currency.as_ref())?,
                    from_network: last_trade.from_currency.network.clone().try_into()?,
                    to_network: last_trade.from_currency.network.clone().try_into()?,
                    withdraw_address: monero::Address::from_str(
                        last_trade.withdrawal_address.as_str(),
                    )?,
                    deposit_address: bitcoin_addr,
                    raw_json: raw_json,
                };

                Ok(trade_info)
            }
            None => {
                anyhow::bail!("No trades found");
            }
        }
    }

    async fn get_trade_status(&self, trade_id: TradeId) -> Result<TradeStatus, anyhow::Error> {
        let path_response =
            generated_client::apis::default_api::api_eigenwallet_get_path_path_uuid_post(
                &self.config,
                &trade_id.0.to_string(),
            )
            .await
            .context("Error getting path responce")?;

        match path_response.trades {
            Some(trades) => {
                if trades.len() > 1 {
                    anyhow::bail!("expected not more than 1 trade, got {}", trades.len());
                }

                let last_trade = trades.last().context("Last trade not found")?;
                let trade_state = last_trade.state.as_ref();

                let trade_status = TradeStatus {
                    status_type: trade_state.r#type.into(),
                    description: trade_state.description.clone(),
                    is_terminal: trade_state.r#final,
                    valid_for: Duration::from_secs(trade_state.valid_for as u64),
                };

                Ok(trade_status)
            }
            None => {
                anyhow::bail!("No trades found");
            }
        }
    }

    pub async fn watch_status(&self, trade: TradeId) -> ReceiverStream<TradeStatus> {
        let (tx, rx) = mpsc::channel(32);
        let client = self.clone();

        tokio::spawn(async move {
            loop {
                let status = match client.get_trade_status(trade.clone()).await {
                    Ok(s) => s,
                    Err(e) => {
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

    pub async fn store(&mut self, status: TradeStatus) -> Result<(), anyhow::Error> {
        for trade in self.trades.clone() {
            let trade_key = trade.0;

            let trade_info = trade.1.0.clone();
            let entry = self
                .trades
                .entry(trade_key)
                .or_insert_with(|| (trade_info, Vec::new()));

            entry.1.push(status.clone());
        }

        Ok(())
    }

    pub async fn load_from_db(&mut self) -> Result<(), anyhow::Error> {
        let trades = self.db.get_trades().await?;

        for (id, info) in trades {
            self.trades
                .entry(id)
                .and_modify(|(old_info, _statuses)| *old_info = info.clone())
                .or_insert((info, Vec::new()));
        }

        Ok(())
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

impl TryFrom<String> for Currency {
    type Error = OrangeFrenError;
    fn try_from(c: String) -> Result<Self, Self::Error> {
        match c.as_str() {
            "XMR" => Ok(Currency::Xmr),
            "BTC" => Ok(Currency::Btc),
            other => Err(OrangeFrenError::UnknownCurrency(other.to_string())),
        }
    }
}
