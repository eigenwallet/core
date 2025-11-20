use anyhow::{Context, Result};
use std::path::PathBuf;
use std::str::FromStr;
use uuid::Uuid;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    types::chrono::Utc,
};

use tracing::info;

use crate::{PathId, TradeInfo, TradeStatus, TradeStatusType};

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)?;
            info!(data_dir = %data_dir.display(), "Created an orangefren database directory");
        }

        let db_path = data_dir.join("trades");
        let connect_options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .connect_with(connect_options)
            .await?;

        let db = Self { pool };
        db.migrate().await?;

        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        info!("Trades database migration completed");
        Ok(())
    }

    pub async fn insert_trade_info(
        &self,
        trade_info: TradeInfo,
        path_id: PathId,
    ) -> Result<(), anyhow::Error> {
        let now = Utc::now().to_rfc3339();

        let from_currency = trade_info.from_currency.as_str().to_string();
        let to_currency = trade_info.to_currency.as_str().to_string();
        let from_network = trade_info.from_network.as_str().to_string();
        let to_network = trade_info.to_network.as_str().to_string();
        let withdraw_address = trade_info.withdraw_address.to_string();
        let deposit_address = trade_info.deposit_address.to_string();
        let path_uuid = &path_id.id.to_string();

        let to_amount = trade_info.to_amount.as_xmr();
        let raw_json = trade_info.raw_json.clone();

        sqlx::query!(
            r#"
            INSERT INTO trades (
                path_uuid,
                timestamp,
                from_currency,
                from_network,
                to_currency,
                to_network,
                withdraw_address,
                deposit_address,
                to_amount,
                raw_json
                ) values (
                ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
                );
            "#,
            path_uuid,
            now,
            from_currency,
            from_network,
            to_currency,
            to_network,
            withdraw_address,
            deposit_address,
            to_amount,
            raw_json
        )
        .execute(&self.pool)
        .await
        .context("insert_trade_info(): failed to insert trade info")?;

        Ok(())
    }

    pub async fn get_trades(&mut self) -> Result<Vec<(PathId, TradeInfo)>, anyhow::Error> {
        let mut info = Vec::new();
        let rows = sqlx::query!(
            r#"
            SELECT
                path_uuid,
                from_currency,
                from_network,
                to_currency,
                to_network,
                withdraw_address,
                deposit_address,
                to_amount,
                raw_json
            FROM trades
            ORDER BY id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("load_from_db(): failed to fetch rows")?;

        for row in rows {
            let path_id = PathId {
                id: Uuid::parse_str(&row.path_uuid)
                    .with_context(|| format!("invalid UUID in path_uuid: {}", row.path_uuid))?,
            };

            let deposit_address = bitcoin::Address::from_str(row.deposit_address.as_str())
                .context("Could not parse bitcoin address")?
                .assume_checked();

            info.push((
                path_id.clone(),
                TradeInfo {
                    to_amount: monero::Amount::from_xmr(row.to_amount).context("Could not parse to amount")?,
                    from_currency: row.from_currency.clone().try_into()?,
                    to_currency: row.to_currency.clone().try_into()?,
                    from_network: row.from_network.clone().try_into()?,
                    to_network: row.to_network.clone().try_into()?,
                    withdraw_address: monero::Address::from_str(row.withdraw_address.as_str())?,
                    deposit_address: deposit_address,
                    raw_json: row.raw_json,
                },
            ));
        }

        Ok(info)
    }

    pub async fn latest_trade_id_by_withdraw_address(
        &self,
        address: monero::Address,
    ) -> Result<PathId, anyhow::Error> {
        let address_str = address.to_string();

        let row = sqlx::query!(
            r#"
            SELECT path_uuid
            FROM trades
            WHERE withdraw_address = ?
            ORDER BY timestamp DESC, id DESC
            LIMIT 1
            "#,
            address_str
        )
        .fetch_one(&self.pool)
        .await
        .context("Could not find the path")?;

        let trade_id = PathId {
            id: Uuid::from_str(row.path_uuid.as_str()).context("Could not initialize the uuid")?,
        };
        Ok(trade_id)
    }

    pub async fn insert_trade_state(
        &self,
        trade_state: TradeStatus,
        path_id: PathId,
    ) -> Result<(), anyhow::Error> {
        let now = Utc::now().to_rfc3339();
        let path_uuid = &path_id.id.to_string();

        let status_type = format!("{:?}", trade_state.status_type);
        let is_terminal = if trade_state.is_terminal { 1 } else { 0 };
        let valid_for_secs = trade_state.valid_for.as_secs() as i64;

        sqlx::query!(
            r#"
            INSERT INTO trade_states (
                path_uuid,
                timestamp,
                status_type,
                is_terminal,
                description,
                valid_for_secs
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
            path_uuid,
            now,
            status_type,
            is_terminal,
            trade_state.description,
            valid_for_secs
        )
        .execute(&self.pool)
        .await
        .context("insert_trade_state(): failed to insert trade state")?;

        Ok(())
    }

    pub async fn get_trade_states(&self, path_id: PathId) -> Result<Vec<TradeStatus>, anyhow::Error> {
        let path_uuid = path_id.id.to_string();
        
        let rows = sqlx::query!(
            r#"
            SELECT
                status_type,
                is_terminal,
                description,
                valid_for_secs
            FROM trade_states
            WHERE path_uuid = ?
            ORDER BY id ASC
            "#,
            path_uuid
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to fetch states")?;

        let mut states = Vec::new();
        for row in rows {
            
            let status_type = match row.status_type.as_str() {
                "Queued" => TradeStatusType::Queued,
                "Initial" => TradeStatusType::Initial,
                "Confirming" => TradeStatusType::Confirming,
                "Exchanging" => TradeStatusType::Exchanging,
                "Success" => TradeStatusType::Success,
                "Refunded" => TradeStatusType::Refunded,
                "Failed" => TradeStatusType::Failed,
                "Expired" => TradeStatusType::Expired,
                _ => TradeStatusType::Unrecognized,
            };

            states.push(TradeStatus {
                status_type,
                is_terminal: row.is_terminal != 0,
                description: row.description,
                valid_for: std::time::Duration::from_secs(row.valid_for_secs as u64),
            });
        }

        Ok(states)
    }
}
