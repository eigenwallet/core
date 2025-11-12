pub mod database;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;
use std::path::{Ancestors, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{Mutex, mpsc};
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TradeStatusType {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TradeStatus {
    pub status_type: TradeStatusType,
    pub is_terminal: bool,
    pub description: String,
    pub valid_for: Duration,
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct PathId(Uuid);

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct TradeInfo {
    from_currency: Currency,
    to_currency: Currency,
    from_network: Currency,
    to_network: Currency,
    //TODO: put the path id in the TradeInfo,
    withdraw_address: monero::Address,
    deposit_address: Option<bitcoin::Address>,
    raw_json: String,
}

impl fmt::Display for TradeInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Trade:")?;
        writeln!(
            f,
            "  from: {:?} on {:?}",
            self.from_currency, self.from_network
        )?;
        writeln!(f, "  to:   {:?} on {:?}", self.to_currency, self.to_network)?;
        writeln!(f, "  withdraw → {}", self.withdraw_address)?;
        match &self.deposit_address {
            Some(addr) => writeln!(f, "  deposit  → {}", addr)?,
            None => writeln!(f, "  deposit  → (none)")?,
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Client {
    trades: Arc<Mutex<HashMap<PathId, (TradeInfo, Vec<TradeStatus>)>>>,
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

        let mut client = Self {
            trades: Arc::new(Mutex::new(HashMap::new())),
            config,
            db,
        };

        client.load_from_db().await?;

        Ok(client)
    }

    pub async fn all_trades(&self) -> Vec<TradeInfo> {
        let map = self.trades.lock().await;
        map.values().map(|(info, _statuses)| info.clone()).collect()
    }

    pub async fn new_trade(
        &mut self,
        from_amount: bitcoin::Amount,
        to_address: monero::Address,
    ) -> Result<PathId, anyhow::Error> {
        if to_address.network == monero::Network::Stagenet
            || to_address.network == monero::Network::Testnet
        {
            anyhow::bail!("Only Monero mainnet is supported");
        }

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
        let path_id = PathId(path_uuid);

        let trade_info = self
            .wait_until_created(path_id.clone())
            .await
            .context("Error creating the trade")?;

        self.db
            .insert_trade_info(trade_info, path_id.clone())
            .await
            .context("Could not insert trade info into the db")?;

        Ok(path_id)
    }

    pub async fn last_trade_state_by_id(
        &self,
        path_id: PathId,
    ) -> Result<(TradeInfo, Vec<TradeStatus>), anyhow::Error> {
        let map = self.trades.lock().await;
        if let Some(trade) = map.get(&path_id).cloned() {
            Ok(trade)
        } else {
            anyhow::bail!("No trades found for id")
        }
    }

    pub async fn deposit_address(
        &mut self,
        trade_id: PathId,
    ) -> Result<bitcoin::Address, anyhow::Error> {
        let trade_state = self.last_trade_state_by_id(trade_id).await?;
        if let Some(address) = trade_state.0.deposit_address {
            Ok(address)
        } else {
            anyhow::bail!("No address in the path response");
        }
    }

    async fn wait_until_created(&self, path_id: PathId) -> Result<TradeInfo, anyhow::Error> {
        let delay = Duration::from_millis(250);
        let deadline = Instant::now() + Duration::from_secs(60);

        loop {
            let path_response =
                generated_client::apis::default_api::api_eigenwallet_get_path_path_uuid_post(
                    &self.config,
                    &path_id.0.to_string(),
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

                    {
                        let mut map = self.trades.lock().await;
                        map.insert(path_id.clone(), (trade_info.clone(), Vec::new()));
                    }

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
                    to_network: last_trade.to_currency.network.clone().try_into()?,
                    withdraw_address: monero::Address::from_str(
                        last_trade.withdrawal_address.as_str(),
                    )?,
                    deposit_address: bitcoin_addr,
                    //path_id: path_response.path_uuid
                    raw_json: raw_json,
                };

                Ok(trade_info)
            }
            None => {
                anyhow::bail!("No trades found");
            }
        }
    }

    async fn fetch_trade_status(&self, trade_id: PathId) -> Result<TradeStatus, anyhow::Error> {
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

    pub async fn recover_trade_by_withdraw_address(
        &self,
        address: monero::Address,
    ) -> Result<(TradeInfo, ReceiverStream<TradeStatus>), anyhow::Error> {
        let path_id = self.db.latest_trade_id_by_withdraw_address(address).await?;
        let trade = self.wait_until_created(path_id.clone()).await?;
        let stream = self.watch_status(path_id).await;

        Ok((trade, stream))
    }

    pub async fn watch_status(&self, path_id: PathId) -> ReceiverStream<TradeStatus> {
        let (tx, rx) = mpsc::channel(32);
        let client = self.clone();
        let path_id_cloned = path_id;

        tokio::spawn(async move {
            loop {
                let status = match client.fetch_trade_status(path_id_cloned.clone()).await {
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

                if client
                    .store(status.clone(), path_id_cloned.clone())
                    .await
                    .is_err()
                {
                    break;
                }

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

    async fn store(&self, status: TradeStatus, path_id: PathId) -> Result<(), anyhow::Error> {
        let mut map = self.trades.lock().await;
        if let Some((_, statuses)) = map.get_mut(&path_id) {
            statuses.push(status);
            Ok(())
        } else {
            anyhow::bail!("Trade not found");
        }
    }

    pub async fn load_from_db(&mut self) -> Result<(), anyhow::Error> {
        let rows = self.db.get_trades().await?;

        let mut map = self.trades.lock().await;

        for (id, info) in rows {
            match map.entry(id) {
                Entry::Occupied(mut e) => {
                    e.get_mut().0 = info;
                }
                Entry::Vacant(e) => {
                    e.insert((info, Vec::new()));
                }
            }
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
