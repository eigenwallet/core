use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Ok, Result};

use automerge::Change;
use libp2p::PeerId;
use sqlx::SqlitePool;
use tracing::info;

use crate::protocol::SerializedChange;

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)?;
            info!("Created server database directory: {}", data_dir.display());
        }

        let db_path = data_dir.join("changes");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePool::connect(&database_url).await?;

        let db = Self { pool };
        db.migrate().await?;

        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        info!("Server changes database migration completed");
        Ok(())
    }

    pub async fn get_peer_changes(&self, peer_id: PeerId) -> Result<Vec<SerializedChange>> {
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

        let mut changes = Vec::new();

        for row in rows.iter() {
            changes.push(SerializedChange::new(row.change.clone()));
        }
        

        Ok(changes)
    }

    pub async fn insert_peer_changes(&self, peer_id: PeerId, changes: Vec<SerializedChange>) -> Result<()> {
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