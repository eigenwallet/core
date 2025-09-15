use std::{path::PathBuf, str::FromStr};

use anyhow::{Result};

use libp2p::PeerId;
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePoolOptions}, SqlitePool};
use tracing::info;

use crate::protocol::EncryptedChange;

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)?;
            info!(data_dir = %data_dir.display(), "Created server database directory");
        }

        let db_path = data_dir.join("changes");
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
        info!("Server database migration completed");
        Ok(())
    }

    pub async fn get_peer_changes(&self, peer_id: PeerId) -> Result<Vec<EncryptedChange>> {
        let peer_id = peer_id.to_string();
        
        let rows = sqlx::query!(
            r#"
            SELECT change
            FROM change
            WHERE peer_id = ?
            ORDER BY id DESC
            "#,
            peer_id
        )
        .fetch_all(&self.pool)
        .await?;

        let changes = rows.iter().map(|row| EncryptedChange::new(row.change.clone())).collect();

        Ok(changes)
    }

    pub async fn insert_peer_changes(&self, peer_id: PeerId, changes: Vec<EncryptedChange>) -> Result<()> {
        let peer_id = peer_id.to_string();
    
        for change in changes {
            let serialized = change.to_bytes();
            sqlx::query!(
                r#"
                INSERT or IGNORE INTO change (peer_id, change)
                VALUES (?, ?)
                "#,
                peer_id,
                serialized
            )
            .execute(&self.pool)
            .await?;
        }
    
        Ok(())
    }
}