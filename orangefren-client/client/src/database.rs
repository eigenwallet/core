use anyhow::Result;
use std::path::PathBuf;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use tracing::info;

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
        info!("Client database migration completed");
        Ok(())
    }
}
