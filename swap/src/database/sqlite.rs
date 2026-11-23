use crate::cli::api::tauri_bindings::TauriEmitter;
use crate::cli::api::tauri_bindings::TauriHandle;
use crate::database::Swap;
use crate::monero::LabeledMoneroAddress;
use crate::monero::MoneroAddressPool;
use crate::monero::TransferProof;
use crate::protocol::{Database, PendingApollo, RecoveredSwap, State};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use sqlx::sqlite::{Sqlite, SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{ConnectOptions, Pool, SqliteExecutor};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use swap_machine::bob::ApolloProgress;
use time::OffsetDateTime;
use uuid::Uuid;

use super::AccessMode;

/// Number of connections opened eagerly at startup and held for the process
/// lifetime, reserving the database's file descriptors before other subsystems
/// can exhaust the limit.
const DATABASE_POOL_SIZE: u32 = 32;

pub struct SqliteDatabase {
    pool: Pool<Sqlite>,
    tauri_handle: Option<TauriHandle>,
}

impl SqliteDatabase {
    pub async fn open(path: impl AsRef<Path>, access_mode: AccessMode) -> Result<Self>
    where
        Self: std::marker::Sized,
    {
        let read_only = matches!(access_mode, AccessMode::ReadOnly);

        let path_str = format!("sqlite:{}", path.as_ref().display());

        let options = SqliteConnectOptions::from_str(&path_str)?.read_only(read_only);
        let options = options.disable_statement_logging();

        // Warm and hold the pool at startup so the database's file descriptors are
        // reserved up front and survive later fd pressure from other subsystems.
        let pool = SqlitePoolOptions::new()
            .min_connections(DATABASE_POOL_SIZE)
            .max_connections(DATABASE_POOL_SIZE)
            .max_lifetime(None)
            .idle_timeout(None)
            .connect_with(options.to_owned())
            .await?;
        let mut sqlite = Self {
            pool,
            tauri_handle: None,
        };

        if !read_only {
            sqlite.run_migrations().await?;
        }

        Ok(sqlite)
    }

    pub fn with_tauri_handle(mut self, tauri_handle: impl Into<Option<TauriHandle>>) -> Self {
        self.tauri_handle = tauri_handle.into();
        self
    }

    async fn run_migrations(&mut self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations")
            .set_ignore_missing(true)
            .run(&self.pool)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Database for SqliteDatabase {
    async fn insert_recovered_swap(&self, recovered: RecoveredSwap) -> Result<bool> {
        let swap_id = recovered.swap_id;
        let id = swap_id.to_string();
        let peer_id = recovered.alice_peer_id.to_string();
        let state = serde_json::to_string(&Swap::from(State::Bob(recovered.state)))?;
        let addresses = recovered
            .monero_receive_pool
            .iter()
            .map(|entry| {
                Ok((
                    entry.address().map(|address| address.to_string()),
                    entry
                        .percentage()
                        .to_f64()
                        .context("Invalid recovered payout percentage")?,
                    entry.label().to_owned(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let apollo = serde_json::to_string(&recovered.apollo)?;
        let entered_at = OffsetDateTime::now_utc().to_string();

        // All steps run in one transaction, so a failure leaves nothing behind.
        // The transaction inserts, in this order:
        // 1. Which peer we did the swap with. This write comes first, so concurrent recoveries run one after the other.
        // 2. The Monero receive pool of the swap.
        // 3. The swap state.
        // 4. The Apollo progress.
        // If the swap has a state already, the transaction rolls back and inserts nothing.
        let mut transaction = self.pool.begin().await?;
        // 1. Peer id
        sqlx::query(
            "INSERT INTO peers (swap_id, peer_id) VALUES (?, ?) ON CONFLICT(swap_id) DO NOTHING",
        )
        .bind(&id)
        .bind(&peer_id)
        .execute(&mut *transaction)
        .await?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM swap_states WHERE swap_id = ?)")
                .bind(&id)
                .fetch_one(&mut *transaction)
                .await?;
        if exists {
            transaction.rollback().await?;
            return Ok(false);
        }

        // 2. Monero receive pool. Replace only orphaned metadata, left by an interrupted older recovery.
        sqlx::query("UPDATE peers SET peer_id = ? WHERE swap_id = ?")
            .bind(peer_id)
            .bind(&id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM monero_addresses WHERE swap_id = ?")
            .bind(&id)
            .execute(&mut *transaction)
            .await?;
        for (address, percentage, label) in addresses {
            sqlx::query("INSERT INTO monero_addresses (swap_id, address, percentage, label) VALUES (?, ?, ?, ?)")
                .bind(&id).bind(address).bind(percentage).bind(label)
                .execute(&mut *transaction).await?;
        }
        // 3. State
        sqlx::query("INSERT INTO swap_states (swap_id, entered_at, state) VALUES (?, ?, ?)")
            .bind(&id)
            .bind(&entered_at)
            .bind(state)
            .execute(&mut *transaction)
            .await?;
        // 4. Apollo progress
        sqlx::query(
            "INSERT INTO apollo_progress (swap_id, progress, created_at) VALUES (?, ?, ?)",
        )
        .bind(&id)
        .bind(apollo)
        .bind(&entered_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    async fn insert_peer_id(&self, swap_id: Uuid, peer_id: PeerId) -> Result<()> {
        insert_peer_id(&self.pool, swap_id, peer_id).await
    }

    async fn get_peer_id(&self, swap_id: Uuid) -> Result<PeerId> {
        let swap_id = swap_id.to_string();

        let row = sqlx::query!(
            r#"
        SELECT peer_id
        FROM peers
        WHERE swap_id = ?
        "#,
            swap_id
        )
        .fetch_one(&self.pool)
        .await?;

        let peer_id = PeerId::from_str(&row.peer_id)?;
        Ok(peer_id)
    }

    async fn insert_monero_address_pool(
        &self,
        swap_id: Uuid,
        address: MoneroAddressPool,
    ) -> Result<()> {
        let swap_id = swap_id.to_string();

        for labeled_address in address.iter() {
            let address_str = labeled_address.address().map(|address| address.to_string());
            let percentage_f64 = labeled_address
                .percentage()
                .to_f64()
                .expect("Decimal should convert to f64");
            let label_str = labeled_address.label();

            sqlx::query!(
                r#"
            insert into monero_addresses (
                swap_id,
                address,
                percentage,
                label
                ) values (?, ?, ?, ?);
            "#,
                swap_id,
                address_str,
                percentage_f64,
                label_str
            )
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    async fn get_monero_address_pool(&self, swap_id: Uuid) -> Result<MoneroAddressPool> {
        let swap_id = swap_id.to_string();

        let row = sqlx::query!(
            r#"
        SELECT address, percentage, label
        FROM monero_addresses
        WHERE swap_id = ?
        "#,
            swap_id
        )
        .fetch_all(&self.pool)
        .await?;

        if row.is_empty() {
            return Err(anyhow!(
                "No Monero address pool found for swap ID: {}",
                swap_id
            ));
        }

        let addresses = row
            .iter()
            .map(|row| -> Result<LabeledMoneroAddress> {
                let address = row
                    .address
                    .as_ref()
                    .map(|address| {
                        monero_address::MoneroAddress::from_str_with_unchecked_network(&address)
                    })
                    .transpose()?;
                let percentage = Decimal::from_f64(row.percentage).expect("Invalid percentage");
                let label = row.label.clone();

                match address {
                    Some(address) => LabeledMoneroAddress::with_address(address, percentage, label)
                        .map_err(|e| anyhow::anyhow!("Invalid percentage in database: {}", e)),
                    None => LabeledMoneroAddress::with_internal_address(percentage, label)
                        .map_err(|e| anyhow::anyhow!("Invalid percentage in database: {}", e)),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(MoneroAddressPool::new(addresses))
    }

    async fn get_monero_addresses(&self) -> Result<Vec<monero_address::MoneroAddress>> {
        let rows =
            sqlx::query!("SELECT DISTINCT address FROM monero_addresses WHERE address IS NOT NULL")
                .fetch_all(&self.pool)
                .await?;

        let addresses = rows
            .iter()
            .filter_map(|row| row.address.as_ref().and_then(|address| monero_address::MoneroAddress::from_str_with_unchecked_network(address).inspect_err(|e| tracing::error!(%address, error = ?e, "Failed to parse monero address")).ok()))
            .collect::<Vec<_>>();

        Ok(addresses)
    }

    async fn insert_address(&self, peer_id: PeerId, address: Multiaddr) -> Result<()> {
        let peer_id = peer_id.to_string();
        let address = address.to_string();

        sqlx::query!(
            r#"
        insert or ignore into peer_addresses (
            peer_id,
            address
            ) values (?, ?);
        "#,
            peer_id,
            address
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_addresses(&self, peer_id: PeerId) -> Result<Vec<Multiaddr>> {
        let peer_id = peer_id.to_string();

        let rows = sqlx::query!(
            r#"
        SELECT DISTINCT address
        FROM peer_addresses
        WHERE peer_id = ?
        "#,
            peer_id,
        )
        .fetch_all(&self.pool)
        .await?;

        let addresses = rows
            .iter()
            .map(|row| {
                let multiaddr = Multiaddr::from_str(&row.address)?;
                Ok(multiaddr)
            })
            .collect::<Result<Vec<Multiaddr>>>();

        addresses
    }

    async fn get_all_peer_addresses(&self) -> Result<Vec<(PeerId, Vec<Multiaddr>)>> {
        let rows = sqlx::query!("SELECT peer_id, address FROM peer_addresses")
            .fetch_all(&self.pool)
            .await?;

        let mut peer_map: std::collections::HashMap<PeerId, Vec<Multiaddr>> =
            std::collections::HashMap::new();

        for row in rows.iter() {
            match (
                PeerId::from_str(&row.peer_id),
                Multiaddr::from_str(&row.address),
            ) {
                (Ok(peer_id), Ok(multiaddr)) => {
                    peer_map.entry(peer_id).or_default().push(multiaddr);
                }
                (Err(e), _) => {
                    tracing::warn!(
                        peer_id = %row.peer_id,
                        error = %e,
                        "Failed to parse peer ID, skipping entry"
                    );
                }
                (_, Err(e)) => {
                    tracing::warn!(
                        address = %row.address,
                        error = %e,
                        "Failed to parse multiaddr, skipping entry"
                    );
                }
            }
        }

        Ok(peer_map.into_iter().collect())
    }

    async fn get_swap_start_date(&self, swap_id: Uuid) -> Result<String> {
        let swap_id = swap_id.to_string();

        let row = sqlx::query!(
            r#"
                SELECT entered_at as start_date
                FROM swap_states
                WHERE swap_id = ?
                ORDER BY id ASC
                LIMIT 1
                "#,
            swap_id
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(row.start_date)
    }

    async fn insert_latest_state(&self, swap_id: Uuid, state: State) -> Result<()> {
        insert_latest_state(&self.pool, swap_id, state).await?;

        // Emit event to Tauri, the frontend will then send another request to get the latest state
        // This is why we don't send the state here
        self.tauri_handle.emit_swap_state_change_event(swap_id);

        Ok(())
    }

    async fn get_apollo_progress(&self, swap_id: Uuid) -> Result<Option<ApolloProgress>> {
        let progress: Option<String> = sqlx::query_scalar(
            "SELECT progress FROM apollo_progress WHERE swap_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(swap_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        progress
            .map(|progress| serde_json::from_str(&progress))
            .transpose()
            .context("Failed to decode the Apollo progress")
    }

    async fn insert_apollo_if_absent(
        &self,
        swap_id: Uuid,
        progress: ApolloProgress,
    ) -> Result<bool> {
        // One statement, so no other writer can slip in between the check and the write
        let result = sqlx::query(
            "INSERT INTO apollo_progress (swap_id, progress, created_at)
             SELECT ?1, ?2, ?3
             WHERE NOT EXISTS (SELECT 1 FROM apollo_progress WHERE swap_id = ?1)",
        )
        .bind(swap_id.to_string())
        .bind(serde_json::to_string(&progress)?)
        .bind(OffsetDateTime::now_utc().to_string())
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(false);
        }

        self.tauri_handle.emit_swap_state_change_event(swap_id);

        Ok(true)
    }

    async fn append_apollo_progress(
        &self,
        swap_id: Uuid,
        progress: ApolloProgress,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO apollo_progress (swap_id, progress, created_at) VALUES (?, ?, ?)",
        )
        .bind(swap_id.to_string())
        .bind(serde_json::to_string(&progress)?)
        .bind(OffsetDateTime::now_utc().to_string())
        .execute(&self.pool)
        .await?;

        self.tauri_handle.emit_swap_state_change_event(swap_id);

        Ok(())
    }

    async fn pending_apollo_progress(&self) -> Result<PendingApollo> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT swap_id, progress FROM apollo_progress
             WHERE id IN (SELECT MAX(id) FROM apollo_progress GROUP BY swap_id)",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut pending = PendingApollo::default();
        for (swap_id, progress) in rows {
            let decoded = Uuid::from_str(&swap_id)
                .map_err(anyhow::Error::from)
                .and_then(|id| Ok((id, serde_json::from_str::<ApolloProgress>(&progress)?)));
            match decoded {
                Ok((_, progress)) if progress.is_complete() => {}
                Ok((id, progress)) => pending.ready.push((id, progress)),
                Err(error) => pending.corrupt.push((swap_id, error)),
            }
        }

        Ok(pending)
    }

    async fn get_state(&self, swap_id: Uuid) -> Result<State> {
        let swap_id = swap_id.to_string();
        let row = sqlx::query!(
            r#"
           SELECT state
           FROM swap_states
           WHERE swap_id = ?
           ORDER BY id desc
           LIMIT 1;

        "#,
            swap_id
        )
        .fetch_all(&self.pool)
        .await?;

        let row = row
            .first()
            .context(format!("No state in database for swap: {}", swap_id))?;
        let swap: Swap = serde_json::from_str(&row.state)?;

        Ok(swap.into())
    }

    async fn all(&self) -> Result<Vec<(PeerId, Uuid, State)>> {
        let rows = sqlx::query!(
            r#"
            SELECT s.swap_id, s.state, p.peer_id
            FROM (
                SELECT max(id), swap_id, state
                FROM swap_states
                GROUP BY swap_id
            ) s
            INNER JOIN peers p ON s.swap_id = p.swap_id
        "#
        )
        .fetch_all(&self.pool)
        .await?;

        let result = rows
            .iter()
            .filter_map(|row| {
                let swap_id = match Uuid::from_str(&row.swap_id) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!(swap_id = %row.swap_id, error = ?e, "Failed to parse UUID");
                        return None;
                    }
                };
                let peer_id = match PeerId::from_str(&row.peer_id) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!(%swap_id, error = ?e, "Failed to parse PeerId");
                        return None;
                    }
                };
                let state = match serde_json::from_str::<Swap>(&row.state) {
                    Ok(a) => State::from(a),
                    Err(e) => {
                        tracing::error!(%swap_id, error = ?e, "Failed to deserialize state");
                        return None;
                    }
                };

                Some((peer_id, swap_id, state))
            })
            .collect();

        Ok(result)
    }

    async fn get_alice_swaps_with_pending_tx_lock(
        &self,
        updated_since: i64,
    ) -> Result<Vec<(Uuid, State)>> {
        // Must match AliceState::pending_tx_lock.
        let rows = sqlx::query!(
            r#"
            -- Find the latest ids on the covering (swap_id, id) index first, so only
            -- the latest state of each swap is read from the table.
            WITH latest_ids AS (
                SELECT max(id) AS id
                FROM swap_states
                GROUP BY swap_id
            ), latest AS (
                SELECT s.id, s.swap_id, s.state, s.entered_at
                FROM latest_ids l
                JOIN swap_states s ON s.id = l.id
            ), normalized AS (
                SELECT *, CASE
                    WHEN entered_at NOT GLOB '* +00:00:00' THEN NULL
                    WHEN substr(entered_at, 13, 1) = ':'
                        THEN substr(entered_at, 1, 11) || '0' || substr(entered_at, 12, length(entered_at) - 21)
                    ELSE substr(entered_at, 1, length(entered_at) - 10)
                END AS utc_time
                FROM latest
            )
            SELECT s.swap_id, s.state, s.entered_at,
                   CAST(strftime('%s', s.utc_time) AS INTEGER) AS "entered_at_unix?: i64"
            FROM normalized s
            -- CASE skips JSON parsing entirely for old rows. Rows with an invalid
            -- timestamp or malformed recent JSON are returned, so that they are logged
            -- and skipped below instead of failing the whole query.
            WHERE CASE
                WHEN strftime('%s', s.utc_time) IS NULL THEN 1
                WHEN CAST(strftime('%s', s.utc_time) AS INTEGER) < ? THEN 0
                WHEN json_valid(s.state) = 0 THEN 1
                ELSE json_extract(s.state, '$.Alice.Started') IS NOT NULL
                  OR json_extract(s.state, '$.Alice.BtcLockTransactionSeen') IS NOT NULL
            END
        "#,
            updated_since,
        )
        .fetch_all(&self.pool)
        .await?;

        // Log and skip unreadable rows instead of failing the whole call.
        let result = rows
            .iter()
            .filter_map(|row| {
                if row.entered_at_unix.is_none() {
                    tracing::error!(swap_id = %row.swap_id, entered_at = %row.entered_at, "Skipping swap state with an invalid UTC timestamp");
                    return None;
                }
                let swap_id = match Uuid::from_str(&row.swap_id) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!(swap_id = %row.swap_id, error = ?e, "Skipping swap state with an invalid swap id");
                        return None;
                    }
                };
                let state = match serde_json::from_str::<Swap>(&row.state) {
                    Ok(state) => State::from(state),
                    Err(e) => {
                        tracing::error!(%swap_id, error = ?e, "Skipping swap state that does not deserialize");
                        return None;
                    }
                };

                Some((swap_id, state))
            })
            .collect();

        Ok(result)
    }

    /// Same as [`all`] except paginated, and returns both the first and last
    /// recorded state per swap. `Alice::Started` is ignored when picking the
    /// first state, so swaps whose lock transaction was never seen are
    /// filtered out, as are swaps that never progressed past `SafelyAborted`.
    /// `limit` and `offset` must each fit into `i32`.
    async fn all_paginated(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<(PeerId, Uuid, State, State)>> {
        // sqlite only accepts signed integers, but we want to expose a
        // sensible api
        let limit = i32::try_from(limit)?;
        let offset = i32::try_from(offset)?;

        let rows = sqlx::query!(
            r#"
            SELECT
                peers.peer_id,
                oldest.swap_id,
                oldest.state       AS "first_state!: String",
                newest.state       AS "last_state!: String",
                oldest.entered_at  AS start_date
            FROM (SELECT swap_id, state, entered_at, MIN(id) AS id
                  FROM swap_states
                  -- Unreadable rows must not fail the whole listing
                  WHERE CASE
                      WHEN json_valid(state) THEN json_extract(state, '$.Alice.Started') IS NULL
                      ELSE 1
                  END
                  GROUP BY swap_id) oldest
            JOIN (SELECT swap_id, state, MAX(id) AS id
                  FROM swap_states GROUP BY swap_id) newest
                ON newest.swap_id = oldest.swap_id
            JOIN peers ON peers.swap_id = oldest.swap_id
            WHERE NOT (
                json_extract(oldest.state, '$.Alice.Done') IS 'SafelyAborted'
                AND json_extract(newest.state, '$.Alice.Done') IS 'SafelyAborted'
            )
            ORDER BY oldest.id
            LIMIT ?
            OFFSET ?
        "#,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await?;

        let result = rows
            .iter()
            .filter_map(|row| {
                let swap_id = match Uuid::from_str(&row.swap_id) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!(swap_id = %row.swap_id, error = ?e, "Failed to parse UUID");
                        return None;
                    }
                };
                let peer_id = match PeerId::from_str(&row.peer_id) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!(%swap_id, error = ?e, "Failed to parse PeerId");
                        return None;
                    }
                };
                let first_state = match serde_json::from_str::<Swap>(&row.first_state) {
                    Ok(a) => State::from(a),
                    Err(e) => {
                        tracing::error!(%swap_id, error = ?e, "Failed to deserialize first state");
                        return None;
                    }
                };
                let last_state = match serde_json::from_str::<Swap>(&row.last_state) {
                    Ok(a) => State::from(a),
                    Err(e) => {
                        tracing::error!(%swap_id, error = ?e, "Failed to deserialize last state");
                        return None;
                    }
                };

                Some((peer_id, swap_id, first_state, last_state))
            })
            .collect();

        Ok(result)
    }

    async fn get_states(&self, swap_id: Uuid) -> Result<Vec<State>> {
        let swap_id = swap_id.to_string();

        // TODO: We should use query! instead of query here to allow for at-compile-time validation
        // I didn't manage to generate the mappings for the query! macro because of problems with sqlx-cli
        let rows = sqlx::query!(
            r#"
           SELECT state
           FROM swap_states
           WHERE swap_id = ?
           ORDER BY id ASC
        "#,
            swap_id
        )
        .fetch_all(&self.pool)
        .await?;

        let result = rows
            .iter()
            .map(|row| {
                let state_str: &str = &row.state;

                let state = match serde_json::from_str::<Swap>(state_str) {
                    Ok(a) => Ok(State::from(a)),
                    Err(e) => Err(e),
                }?;
                Ok(state)
            })
            .collect::<Result<Vec<State>>>();

        result
    }

    async fn insert_buffered_transfer_proof(
        &self,
        swap_id: Uuid,
        proof: TransferProof,
    ) -> Result<()> {
        let swap_id = swap_id.to_string();
        let proof = serde_json::to_string(&proof)?;

        sqlx::query!(
            r#"
            INSERT INTO buffered_transfer_proofs (
                swap_id,
                proof
                ) VALUES (?, ?);
        "#,
            swap_id,
            proof
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_buffered_transfer_proof(&self, swap_id: Uuid) -> Result<Option<TransferProof>> {
        let swap_id = swap_id.to_string();

        let row = sqlx::query!(
            r#"
           SELECT proof
           FROM buffered_transfer_proofs
           WHERE swap_id = ?
            "#,
            swap_id
        )
        .fetch_all(&self.pool)
        .await?;

        if row.is_empty() {
            return Ok(None);
        }

        let proof_str = &row[0].proof;
        let proof = serde_json::from_str(proof_str)?;

        Ok(Some(proof))
    }

    async fn has_swap(&self, swap_id: Uuid) -> Result<bool> {
        has_swap(&self.pool, swap_id).await
    }

    async fn insert_new_swap(
        &self,
        swap_id: Uuid,
        peer_id: PeerId,
        state: State,
        timeout: Duration,
    ) -> Result<()> {
        // Everything up to the commit is bounded. Dropping the transaction rolls it back.
        let transaction = tokio::time::timeout(timeout, async {
            let mut transaction = self.pool.begin().await?;

            // Writing first takes the write lock right away. Reading first would take a
            // read lock that can fail to upgrade when another connection writes.
            insert_peer_id(&mut *transaction, swap_id, peer_id)
                .await
                .context("Failed to save peer-id in database")?;
            if has_swap(&mut *transaction, swap_id).await? {
                bail!("UUID is already in use");
            }
            insert_latest_state(&mut *transaction, swap_id, state)
                .await
                .context("Failed to save the initial swap state in database")?;

            Ok(transaction)
        })
        .await
        .context("Persisting the new swap timed out")??;

        // Not bounded by the timeout, because we must know whether it happened. On a busy
        // database SQLite gives up after its busy timeout and rolls the transaction back.
        transaction
            .commit()
            .await
            .context("Failed to commit the new swap")?;

        self.tauri_handle.emit_swap_state_change_event(swap_id);

        Ok(())
    }
}

async fn insert_peer_id<'e>(
    executor: impl SqliteExecutor<'e>,
    swap_id: Uuid,
    peer_id: PeerId,
) -> Result<()> {
    let swap_id = swap_id.to_string();
    let peer_id = peer_id.to_string();

    sqlx::query!(
        r#"
        insert into peers (
            swap_id,
            peer_id
            ) values (?, ?);
        "#,
        swap_id,
        peer_id
    )
    .execute(executor)
    .await?;

    Ok(())
}

async fn insert_latest_state<'e>(
    executor: impl SqliteExecutor<'e>,
    swap_id: Uuid,
    state: State,
) -> Result<()> {
    let entered_at = OffsetDateTime::now_utc();

    let swap = serde_json::to_string(&Swap::from(state))?;
    let entered_at = entered_at.to_string();
    let swap_id_str = swap_id.to_string();

    sqlx::query!(
        r#"
            insert into swap_states (
                swap_id,
                entered_at,
                state
                ) values (?, ?, ?);
        "#,
        swap_id_str,
        entered_at,
        swap
    )
    .execute(executor)
    .await?;

    Ok(())
}

async fn has_swap<'e>(executor: impl SqliteExecutor<'e>, swap_id: Uuid) -> Result<bool> {
    let swap_id = swap_id.to_string();

    let row = sqlx::query!(
        r#"
            SELECT 1 as "found!: i64"
            FROM swap_states
            WHERE swap_id = ?
            LIMIT 1
            "#,
        swap_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.is_some())
}

impl SqliteDatabase {
    /// Like [`Database::all`] but only returns swaps whose latest state
    /// update happened within the last `freshness_hours`.
    ///
    /// Normalize the UTC timestamps written by `OffsetDateTime::to_string()` in
    /// SQL (including single-digit hours) before filtering or decoding state JSON.
    pub async fn all_fresh(&self, freshness_hours: u64) -> Result<Vec<(PeerId, Uuid, State)>> {
        let seconds = i64::try_from(freshness_hours)?
            .checked_mul(3600)
            .context("Freshness interval overflow")?;
        let cutoff = OffsetDateTime::now_utc()
            .unix_timestamp()
            .checked_sub(seconds)
            .context("Freshness cutoff overflow")?;
        self.all_fresh_since(cutoff).await
    }

    async fn all_fresh_since(&self, cutoff: i64) -> Result<Vec<(PeerId, Uuid, State)>> {
        let rows = sqlx::query!(
            r#"
            -- Find the latest ids on the covering (swap_id, id) index first, so only
            -- the latest state of each swap is read from the table.
            WITH latest_ids AS (
                SELECT max(id) AS id
                FROM swap_states
                GROUP BY swap_id
            ), latest AS (
                SELECT s.id, s.swap_id, s.state, s.entered_at
                FROM latest_ids l
                JOIN swap_states s ON s.id = l.id
            ), normalized AS (
                SELECT *, CASE
                    WHEN entered_at NOT GLOB '* +00:00:00' THEN NULL
                    WHEN substr(entered_at, 13, 1) = ':'
                        THEN substr(entered_at, 1, 11) || '0' || substr(entered_at, 12, length(entered_at) - 21)
                    ELSE substr(entered_at, 1, length(entered_at) - 10)
                END AS utc_time
                FROM latest
            )
            -- Parse the entire normalized timestamp so malformed fractional
            -- seconds cannot be hidden by truncation. Epoch seconds discard
            -- the fractional part only after the timestamp has been parsed.
            SELECT s.swap_id, s.state, p.peer_id, s.entered_at,
                   CAST(strftime('%s', s.utc_time) AS INTEGER) AS "entered_at_unix?: i64"
            FROM normalized s
            INNER JOIN peers p ON s.swap_id = p.swap_id
            WHERE strftime('%s', s.utc_time) IS NULL
               OR CAST(strftime('%s', s.utc_time) AS INTEGER) >= ?
            "#,
            cutoff,
        )
        .fetch_all(&self.pool)
        .await?;

        // Log and skip unreadable rows instead of failing the whole call.
        let result = rows
            .iter()
            .filter_map(|row| {
                if row.entered_at_unix.is_none() {
                    tracing::error!(swap_id = %row.swap_id, entered_at = %row.entered_at, "Skipping swap state with an invalid UTC timestamp");
                    return None;
                }
                let swap_id = match Uuid::from_str(&row.swap_id) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!(swap_id = %row.swap_id, error = ?e, "Skipping swap state with an invalid swap id");
                        return None;
                    }
                };
                let peer_id = match PeerId::from_str(&row.peer_id) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!(%swap_id, error = ?e, "Skipping swap with an invalid peer id");
                        return None;
                    }
                };
                let state = match serde_json::from_str::<Swap>(&row.state) {
                    Ok(state) => State::from(state),
                    Err(e) => {
                        tracing::error!(%swap_id, error = ?e, "Skipping swap state that does not deserialize");
                        return None;
                    }
                };

                Some((peer_id, swap_id, state))
            })
            .collect();

        Ok(result)
    }
}

#[async_trait]
impl crate::network::wormhole::PeerTrust for SqliteDatabase {
    async fn peers_with_financially_relevant_swap(
        &self,
        freshness_hours: u64,
    ) -> Result<Vec<PeerId>> {
        use std::collections::HashSet;

        let swaps = self.all_fresh(freshness_hours).await?;
        let peers = swaps
            .into_iter()
            .filter_map(|(peer_id, _, state)| {
                let State::Alice(alice_state) = state else {
                    return None;
                };
                alice_state.is_at_or_past_btc_locked().then_some(peer_id)
            })
            .collect::<HashSet<_>>();

        Ok(peers.into_iter().collect())
    }
}

#[async_trait]
impl crate::network::wormhole::WormholeStore for SqliteDatabase {
    async fn store_wormhole(&self, peer: PeerId, address: Multiaddr, active: bool) -> Result<()> {
        let peer_id = peer.to_string();
        let address = address.to_string();

        sqlx::query!(
            r#"
            INSERT INTO wormholes (peer_id, address, active)
            VALUES (?, ?, ?)
            ON CONFLICT (peer_id) DO UPDATE SET address = excluded.address, active = excluded.active
            "#,
            peer_id,
            address,
            active,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_wormhole(&self, peer: PeerId) -> Result<Option<(Multiaddr, bool)>> {
        let peer_id = peer.to_string();

        let row = sqlx::query!(
            r#"
            SELECT address, active
            FROM wormholes
            WHERE peer_id = ?
            "#,
            peer_id,
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let address = Multiaddr::from_str(&row.address)
            .map_err(|e| anyhow::anyhow!("Invalid multiaddr in wormholes table: {e}"))?;

        Ok(Some((address, row.active)))
    }

    async fn get_all_wormholes(&self) -> Result<Vec<(PeerId, Multiaddr)>> {
        let rows = sqlx::query!(
            r#"
            SELECT peer_id, address
            FROM wormholes
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.iter()
            .map(|row| {
                let peer_id = PeerId::from_str(&row.peer_id)
                    .map_err(|e| anyhow::anyhow!("Invalid peer_id in wormholes table: {e}"))?;
                let address = Multiaddr::from_str(&row.address)
                    .map_err(|e| anyhow::anyhow!("Invalid multiaddr in wormholes table: {e}"))?;
                Ok((peer_id, address))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::alice::{AliceState, test_state3};
    use crate::protocol::bob::BobState;
    use std::fs::File;
    use tempfile::{TempDir, tempdir};

    fn recovered_swap(swap_id: Uuid) -> Result<RecoveredSwap> {
        Ok(RecoveredSwap {
            swap_id,
            alice_peer_id: PeerId::random(),
            monero_receive_pool: MoneroAddressPool::new(vec![
                LabeledMoneroAddress::with_internal_address(Decimal::ONE, String::new())?,
            ]),
            state: BobState::SafelyAborted,
            apollo: ApolloProgress::Published(empty_tx()),
        })
    }

    fn empty_tx() -> bitcoin::Transaction {
        bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![],
            output: vec![],
        }
    }

    #[tokio::test]
    async fn recovery_rolls_back_all_records_and_can_retry() -> Result<()> {
        let db = setup_test_db().await?;
        let recovered = recovered_swap(Uuid::new_v4())?;
        sqlx::query("CREATE TRIGGER fail_recovery BEFORE INSERT ON swap_states BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
            .execute(&db.pool).await?;
        assert!(db.insert_recovered_swap(recovered.clone()).await.is_err());
        assert!(!db.has_swap(recovered.swap_id).await?);
        for table in [
            "peers",
            "monero_addresses",
            "swap_states",
            "apollo_progress",
        ] {
            let count: i64 =
                sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE swap_id = ?"))
                    .bind(recovered.swap_id.to_string())
                    .fetch_one(&db.pool)
                    .await?;
            assert_eq!(count, 0, "partial recovery left rows in {table}");
        }
        sqlx::query("DROP TRIGGER fail_recovery")
            .execute(&db.pool)
            .await?;
        assert!(db.insert_recovered_swap(recovered.clone()).await?);
        assert_eq!(
            db.get_peer_id(recovered.swap_id).await?,
            recovered.alice_peer_id
        );
        assert_eq!(
            db.get_monero_address_pool(recovered.swap_id).await?,
            recovered.monero_receive_pool
        );
        assert_eq!(
            db.get_state(recovered.swap_id).await?,
            State::Bob(recovered.state)
        );
        assert_eq!(
            db.get_apollo_progress(recovered.swap_id).await?,
            Some(recovered.apollo)
        );
        Ok(())
    }

    #[tokio::test]
    async fn recovery_rolls_back_the_state_if_the_apollo_row_fails() -> Result<()> {
        let db = setup_test_db().await?;
        let recovered = recovered_swap(Uuid::new_v4())?;
        sqlx::query("CREATE TRIGGER fail_apollo BEFORE INSERT ON apollo_progress BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
            .execute(&db.pool).await?;
        assert!(db.insert_recovered_swap(recovered.clone()).await.is_err());
        assert!(!db.has_swap(recovered.swap_id).await?);
        assert_eq!(db.get_apollo_progress(recovered.swap_id).await?, None);
        Ok(())
    }

    #[tokio::test]
    async fn recovery_replaces_orphaned_metadata_but_preserves_existing_swaps() -> Result<()> {
        let db = setup_test_db().await?;
        let recovered = recovered_swap(Uuid::new_v4())?;
        db.insert_peer_id(recovered.swap_id, PeerId::random())
            .await?;
        db.insert_monero_address_pool(recovered.swap_id, recovered.monero_receive_pool.clone())
            .await?;
        db.insert_monero_address_pool(recovered.swap_id, recovered.monero_receive_pool.clone())
            .await?;
        assert!(db.insert_recovered_swap(recovered.clone()).await?);
        assert_eq!(
            db.get_peer_id(recovered.swap_id).await?,
            recovered.alice_peer_id
        );
        assert_eq!(
            db.get_monero_address_pool(recovered.swap_id)
                .await?
                .iter()
                .count(),
            1
        );
        let mut duplicate = recovered.clone();
        duplicate.alice_peer_id = PeerId::random();
        assert!(!db.insert_recovered_swap(duplicate).await?);
        assert_eq!(
            db.get_peer_id(recovered.swap_id).await?,
            recovered.alice_peer_id
        );
        assert_eq!(db.get_states(recovered.swap_id).await?.len(), 1);
        assert_eq!(
            db.get_apollo_progress(recovered.swap_id).await?,
            Some(recovered.apollo)
        );
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_recovery_inserts_once() -> Result<()> {
        let db = setup_test_db().await?;
        let recovered = recovered_swap(Uuid::new_v4())?;
        let id = recovered.swap_id;
        let (left, right) = tokio::join!(
            db.insert_recovered_swap(recovered.clone()),
            db.insert_recovered_swap(recovered)
        );
        assert_ne!(left?, right?);
        assert_eq!(db.get_states(id).await?.len(), 1);
        assert_eq!(db.get_monero_address_pool(id).await?.iter().count(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn insert_apollo_if_absent_inserts_only_if_absent() -> Result<()> {
        let db = setup_test_db().await?;
        let id = Uuid::new_v4();
        let constructed = ApolloProgress::Constructed(empty_tx());

        assert_eq!(db.get_apollo_progress(id).await?, None);
        assert!(
            db.insert_apollo_if_absent(id, constructed.clone())
                .await?
        );
        assert_eq!(db.get_apollo_progress(id).await?, Some(constructed.clone()));

        // A second insert-if-absent changes nothing
        let published = ApolloProgress::Published(empty_tx());
        assert!(!db.insert_apollo_if_absent(id, published).await?);
        assert_eq!(db.get_apollo_progress(id).await?, Some(constructed));
        Ok(())
    }

    #[tokio::test]
    async fn append_apollo_progress_appends_a_row_per_transition() -> Result<()> {
        let db = setup_test_db().await?;
        let id = Uuid::new_v4();
        let constructed = ApolloProgress::Constructed(empty_tx());
        let published = ApolloProgress::Published(empty_tx());
        let confirmed = ApolloProgress::Confirmed(empty_tx());

        assert!(db.insert_apollo_if_absent(id, constructed).await?);
        db.append_apollo_progress(id, published).await?;
        db.append_apollo_progress(id, confirmed.clone()).await?;

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM apollo_progress WHERE swap_id = ?")
                .bind(id.to_string())
                .fetch_one(&db.pool)
                .await?;
        assert_eq!(count, 3);
        assert_eq!(db.get_apollo_progress(id).await?, Some(confirmed));
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_apollo_inserts_have_exactly_one_winner() -> Result<()> {
        let db = std::sync::Arc::new(setup_test_db().await?);
        let id = Uuid::new_v4();

        let writers = (0..16).map(|i| {
            let db = db.clone();
            let progress = if i % 2 == 0 {
                ApolloProgress::Published(empty_tx())
            } else {
                ApolloProgress::Abandoned(empty_tx())
            };
            tokio::spawn(async move { db.insert_apollo_if_absent(id, progress).await })
        });
        // All writers are spawned before any is awaited, so that they overlap
        let writers: Vec<_> = writers.collect();
        let mut winners = 0;
        for writer in writers {
            winners += usize::from(writer.await??);
        }

        assert_eq!(winners, 1);
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM apollo_progress WHERE swap_id = ?")
                .bind(id.to_string())
                .fetch_one(&db.pool)
                .await?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[tokio::test]
    async fn pending_apollo_progress_skips_complete_rows_and_reports_undecodable_ones() -> Result<()>
    {
        let db = setup_test_db().await?;
        let constructed = (Uuid::new_v4(), ApolloProgress::Constructed(empty_tx()));
        let published = (Uuid::new_v4(), ApolloProgress::Published(empty_tx()));
        let confirmed = (Uuid::new_v4(), ApolloProgress::Confirmed(empty_tx()));
        let abandoned = (Uuid::new_v4(), ApolloProgress::Abandoned(empty_tx()));
        for (id, progress) in [&constructed, &published, &confirmed, &abandoned] {
            db.insert_apollo_if_absent(*id, progress.clone()).await?;
        }
        sqlx::query(
            "INSERT INTO apollo_progress (swap_id, progress, created_at) VALUES (?, 'garbage', '')",
        )
        .bind(Uuid::new_v4().to_string())
        .execute(&db.pool)
        .await?;

        let PendingApollo {
            ready: mut pending,
            corrupt,
        } = db.pending_apollo_progress().await?;
        pending.sort_by_key(|(id, _)| *id);
        let mut expected = vec![constructed, published];
        expected.sort_by_key(|(id, _)| *id);

        assert_eq!(pending, expected);
        assert_eq!(corrupt.len(), 1);
        Ok(())
    }

    fn tx_spending(vout: u32, inputs: usize) -> bitcoin::Transaction {
        bitcoin::Transaction {
            input: (0..inputs)
                .map(|_| bitcoin::TxIn {
                    previous_output: bitcoin::OutPoint::new(
                        bitcoin::Txid::from_raw_hash(bitcoin::hashes::Hash::all_zeros()),
                        vout,
                    ),
                    ..Default::default()
                })
                .collect(),
            ..empty_tx()
        }
    }

    #[tokio::test]
    async fn persisting_apollo_before_the_broadcast_accepts_the_same_tx_only() -> Result<()> {
        use crate::protocol::bob::apollo::persist_constructed_apollo;
        let db = setup_test_db().await?;
        let id = Uuid::new_v4();
        let tx = tx_spending(1, 1);

        persist_constructed_apollo(&db, id, &tx).await?;
        assert_eq!(
            db.get_apollo_progress(id).await?,
            Some(ApolloProgress::Constructed(tx.clone()))
        );

        // A resumed swap saves the first row again
        persist_constructed_apollo(&db, id, &tx).await?;

        // The same call after the publisher appended a row must not undo that row
        let published = ApolloProgress::Published(tx.clone());
        db.append_apollo_progress(id, published.clone()).await?;
        persist_constructed_apollo(&db, id, &tx).await?;
        assert_eq!(db.get_apollo_progress(id).await?, Some(published.clone()));

        let error = persist_constructed_apollo(&db, id, &tx_spending(2, 1))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("different Apollo"), "{error:#}");
        assert_eq!(db.get_apollo_progress(id).await?, Some(published));
        Ok(())
    }

    /// Startup handles pending rows with the same iteration, whatever the swap state is.
    /// The test uses a swap with no state, a finished swap and an unfinished swap.
    #[tokio::test]
    async fn pending_apollo_outpoints_are_reserved_for_good_rows_before_failing_on_bad_ones()
    -> Result<()> {
        use crate::protocol::bob::apollo::reserve_pending_apollo_outpoints;
        let db = setup_test_db().await?;
        let (no_state, aborted, locked) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        db.insert_latest_state(aborted, State::Bob(BobState::SafelyAborted))
            .await?;
        for (vout, id) in [(1, no_state), (2, aborted), (3, locked)] {
            db.insert_apollo_if_absent(id, ApolloProgress::Published(tx_spending(vout, 1)))
                .await?;
        }
        let two_inputs = Uuid::new_v4();
        db.insert_apollo_if_absent(two_inputs, ApolloProgress::Constructed(tx_spending(4, 2)))
        .await?;
        let confirmed = Uuid::new_v4();
        db.insert_apollo_if_absent(confirmed, ApolloProgress::Confirmed(tx_spending(5, 1)))
        .await?;
        sqlx::query("INSERT INTO apollo_progress (swap_id, progress, created_at) VALUES ('corrupt-id', 'garbage', '')")
            .execute(&db.pool)
            .await?;

        let mut reserved = Vec::new();
        let error = reserve_pending_apollo_outpoints(&db, |outpoints| reserved = outpoints)
            .await
            .unwrap_err();

        let mut vouts: Vec<u32> = reserved.iter().map(|outpoint| outpoint.vout).collect();
        vouts.sort();
        assert_eq!(vouts, vec![1, 2, 3]);
        let message = format!("{error:#}");
        assert!(message.contains("corrupt-id"), "{message}");
        assert!(message.contains(&two_inputs.to_string()), "{message}");
        assert!(!message.contains(&confirmed.to_string()), "{message}");
        Ok(())
    }

    #[tokio::test]
    async fn test_insert_and_load_state() {
        let db = setup_test_db().await.unwrap();

        let state_1 = State::Alice(AliceState::BtcRedeemed);
        let swap_id_1 = Uuid::new_v4();

        db.insert_latest_state(swap_id_1, state_1).await.unwrap();

        let state_1 = State::Alice(AliceState::BtcRedeemed);

        db.insert_latest_state(swap_id_1, state_1.clone())
            .await
            .unwrap();

        let state_1_loaded = db.get_state(swap_id_1).await.unwrap();

        assert_eq!(state_1, state_1_loaded);
    }

    #[tokio::test]
    async fn test_retrieve_all_latest_states() {
        let db = setup_test_db().await.unwrap();

        let peer_id_1 = PeerId::random();
        let peer_id_2 = PeerId::random();
        let state_1 = State::Alice(AliceState::BtcRedeemed);
        let state_2 = State::Alice(AliceState::SafelyAborted);
        let state_3 = State::Bob(BobState::SafelyAborted);
        let swap_id_1 = Uuid::new_v4();
        let swap_id_2 = Uuid::new_v4();

        db.insert_peer_id(swap_id_1, peer_id_1).await.unwrap();
        db.insert_peer_id(swap_id_2, peer_id_2).await.unwrap();

        db.insert_latest_state(swap_id_1, state_1.clone())
            .await
            .unwrap();
        db.insert_latest_state(swap_id_1, state_2.clone())
            .await
            .unwrap();
        db.insert_latest_state(swap_id_2, state_3.clone())
            .await
            .unwrap();

        let latest_loaded = db.all().await.unwrap();

        assert_eq!(latest_loaded.len(), 2);

        assert!(latest_loaded.contains(&(peer_id_1, swap_id_1, state_2)));
        assert!(latest_loaded.contains(&(peer_id_2, swap_id_2, state_3)));

        assert!(!latest_loaded.contains(&(peer_id_1, swap_id_1, state_1)));
    }

    #[tokio::test]
    async fn test_insert_and_load_monero_address_pool() -> Result<()> {
        use crate::monero::{LabeledMoneroAddress, MoneroAddressPool};
        use rust_decimal::Decimal;

        let db = setup_test_db().await?;

        let swap_id = Uuid::new_v4();

        // Create multiple labeled addresses with valid percentages that sum to 1
        let address1 = monero_address::MoneroAddress::from_str_with_unchecked_network(
            "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a",
        )?; // Stagenet address
        let address2 = monero_address::MoneroAddress::from_str_with_unchecked_network(
            "44Ato7HveWidJYUAVw5QffEcEtSH1DwzSP3FPPkHxNAS4LX9CqgucphTisH978FLHE34YNEx7FcbBfQLQUU8m3NUC4VqsRa",
        )?; // Mainnet address
        let address3 = monero_address::MoneroAddress::from_str_with_unchecked_network(
            "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a",
        )?; // Same as address1 for simplicity

        let labeled_addresses = vec![
            LabeledMoneroAddress::with_address(address1, Decimal::new(5, 1), "Primary".to_string())
                .map_err(|e| anyhow!(e))?, // 0.5
            LabeledMoneroAddress::with_address(
                address2,
                Decimal::new(3, 1),
                "Secondary".to_string(),
            )
            .map_err(|e| anyhow!(e))?, // 0.3
            LabeledMoneroAddress::with_address(
                address3,
                Decimal::new(2, 1),
                "Tertiary".to_string(),
            )
            .map_err(|e| anyhow!(e))?, // 0.2
        ];

        let address_pool = MoneroAddressPool::new(labeled_addresses);

        // Insert the address pool
        db.insert_monero_address_pool(swap_id, address_pool.clone())
            .await?;

        // Load the address pool back
        let loaded_address_pool = db.get_monero_address_pool(swap_id).await?;

        // Verify they are equal
        assert_eq!(address_pool.addresses(), loaded_address_pool.addresses());
        assert_eq!(
            address_pool.percentages(),
            loaded_address_pool.percentages()
        );

        // Verify each labeled address individually
        let original_addresses: Vec<_> = address_pool.iter().collect();
        let loaded_addresses: Vec<_> = loaded_address_pool.iter().collect();

        assert_eq!(original_addresses.len(), loaded_addresses.len());

        for (orig, loaded) in original_addresses.iter().zip(loaded_addresses.iter()) {
            assert_eq!(orig.address(), loaded.address());
            assert_eq!(orig.percentage(), loaded.percentage());
            assert_eq!(orig.label(), loaded.label());
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_insert_and_load_multiaddr() -> Result<()> {
        let db = setup_test_db().await?;

        let peer_id = PeerId::random();
        let multiaddr1 = "/ip4/127.0.0.1".parse::<Multiaddr>()?;
        let multiaddr2 = "/ip4/127.0.0.2".parse::<Multiaddr>()?;

        db.insert_address(peer_id, multiaddr1.clone()).await?;
        db.insert_address(peer_id, multiaddr2.clone()).await?;

        let loaded_multiaddr = db.get_addresses(peer_id).await?;

        assert!(loaded_multiaddr.contains(&multiaddr1));
        assert!(loaded_multiaddr.contains(&multiaddr2));
        assert_eq!(loaded_multiaddr.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_insert_and_load_peer_id() -> Result<()> {
        let db = setup_test_db().await?;

        let peer_id = PeerId::random();
        let multiaddr1 = "/ip4/127.0.0.1".parse::<Multiaddr>()?;
        let multiaddr2 = "/ip4/127.0.0.2".parse::<Multiaddr>()?;

        db.insert_address(peer_id, multiaddr1.clone()).await?;
        db.insert_address(peer_id, multiaddr2.clone()).await?;

        let loaded_multiaddr = db.get_addresses(peer_id).await?;

        assert!(loaded_multiaddr.contains(&multiaddr1));
        assert!(loaded_multiaddr.contains(&multiaddr2));
        assert_eq!(loaded_multiaddr.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn fresh_swaps_normalize_hours_and_filter_before_decoding() -> Result<()> {
        let db = setup_test_db().await?;
        // Test midnight, single-digit and two-digit hours with fractional seconds.
        for hour in [0, 1, 9, 10, 23] {
            sqlx::query("DELETE FROM swap_states")
                .execute(&db.pool)
                .await?;
            sqlx::query("DELETE FROM peers").execute(&db.pool).await?;
            let cutoff = OffsetDateTime::from_unix_timestamp(1_735_689_600 + hour * 3600)?;
            let mut expected = Vec::new();
            for offset in [-1, 0, 1] {
                let id = Uuid::new_v4();
                db.insert_peer_id(id, PeerId::random()).await?;
                let timestamp =
                    (cutoff + time::Duration::seconds(offset) + time::Duration::milliseconds(123))
                        .to_string();
                let state = if offset < 0 {
                    "this stale JSON must never be decoded"
                } else {
                    r#"{"Alice":{"Done":"SafelyAborted"}}"#
                };
                sqlx::query(
                    "INSERT INTO swap_states (swap_id, entered_at, state) VALUES (?, ?, ?)",
                )
                .bind(id.to_string())
                .bind(timestamp)
                .bind(state)
                .execute(&db.pool)
                .await?;
                if offset >= 0 {
                    expected.push(id);
                }
            }
            let fresh = db.all_fresh_since(cutoff.unix_timestamp()).await?;
            assert_eq!(fresh.len(), expected.len());
            assert!(fresh.iter().all(|(_, id, _)| expected.contains(id)));
            // A newer row supersedes a fresh one even if its timestamp is older.
            sqlx::query("INSERT INTO swap_states (swap_id, entered_at, state) VALUES (?, ?, ?)")
                .bind(expected[0].to_string())
                .bind((cutoff - time::Duration::seconds(1)).to_string())
                .bind("stale JSON")
                .execute(&db.pool)
                .await?;
            assert_eq!(db.all_fresh_since(cutoff.unix_timestamp()).await?.len(), 1);
        }
        Ok(())
    }

    #[tokio::test]
    async fn pending_lock_checks_filter_timestamps_before_reading_json() -> Result<()> {
        let db = setup_test_db().await?;
        for hour in [0, 1, 9, 10, 23] {
            let cutoff = OffsetDateTime::from_unix_timestamp(1_735_689_600 + hour * 3600)?;
            for variant in ["Started", "BtcLockTransactionSeen"] {
                for offset in [-1, 0, 1] {
                    sqlx::query("DELETE FROM swap_states")
                        .execute(&db.pool)
                        .await?;
                    let id = Uuid::new_v4();
                    let timestamp = cutoff
                        + time::Duration::seconds(offset)
                        + time::Duration::milliseconds(123);
                    // Recent pending states must reach deserialization (and are skipped
                    // because state3 is missing); stale states must not even reach SQL's
                    // JSON parser. Neither query requires a peers-table record.
                    let state = if offset < 0 {
                        "this stale JSON must never be parsed".to_owned()
                    } else {
                        format!(r#"{{"Alice":{{"{variant}":{{}}}}}}"#)
                    };
                    sqlx::query(
                        "INSERT INTO swap_states (swap_id, entered_at, state) VALUES (?, ?, ?)",
                    )
                    .bind(id.to_string())
                    .bind(timestamp.to_string())
                    .bind(state)
                    .execute(&db.pool)
                    .await?;
                    assert!(
                        db.get_alice_swaps_with_pending_tx_lock(cutoff.unix_timestamp())
                            .await?
                            .is_empty()
                    );
                }
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn pending_lock_checks_skip_recent_malformed_json() -> Result<()> {
        let db = setup_test_db().await?;
        sqlx::query("INSERT INTO swap_states (swap_id, entered_at, state) VALUES (?, ?, ?)")
            .bind(Uuid::new_v4().to_string())
            .bind(OffsetDateTime::now_utc().to_string())
            .bind("this recent JSON is malformed")
            .execute(&db.pool)
            .await?;
        let cutoff = OffsetDateTime::now_utc() - time::Duration::hours(1);
        assert!(
            db.get_alice_swaps_with_pending_tx_lock(cutoff.unix_timestamp())
                .await?
                .is_empty()
        );
        Ok(())
    }

    #[tokio::test]
    async fn pending_lock_checks_do_not_resurrect_older_states() -> Result<()> {
        let db = setup_test_db().await?;
        let cutoff = OffsetDateTime::from_unix_timestamp(1_735_689_600)?;
        for (latest_timestamp, latest_state) in [
            (cutoff, r#"{"Alice":{"Done":"SafelyAborted"}}"#),
            (cutoff - time::Duration::seconds(1), "stale JSON"),
        ] {
            let id = Uuid::new_v4();
            for (timestamp, state) in [
                (cutoff, r#"{"Alice":{"BtcLockTransactionSeen":{}}}"#),
                (latest_timestamp, latest_state),
            ] {
                sqlx::query(
                    "INSERT INTO swap_states (swap_id, entered_at, state) VALUES (?, ?, ?)",
                )
                .bind(id.to_string())
                .bind(timestamp.to_string())
                .bind(state)
                .execute(&db.pool)
                .await?;
            }
            assert!(
                db.get_alice_swaps_with_pending_tx_lock(cutoff.unix_timestamp())
                    .await?
                    .is_empty()
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn pending_lock_checks_return_valid_pending_swaps() -> Result<()> {
        let db = setup_test_db().await?;
        let wallet = bitcoin_wallet::TestWalletBuilder::new(1_000_000)
            .build()
            .await;
        let cutoff = OffsetDateTime::now_utc() - time::Duration::hours(1);

        let started = State::Alice(AliceState::Started {
            state3: Box::new(test_state3(&wallet).await?),
        });
        let seen = State::Alice(AliceState::BtcLockTransactionSeen {
            state3: Box::new(test_state3(&wallet).await?),
        });
        let (started_id, seen_id) = (Uuid::new_v4(), Uuid::new_v4());
        db.insert_latest_state(started_id, started.clone()).await?;
        db.insert_latest_state(seen_id, seen.clone()).await?;

        // Recorded before the cutoff.
        let stale = State::Alice(AliceState::Started {
            state3: Box::new(test_state3(&wallet).await?),
        });
        sqlx::query("INSERT INTO swap_states (swap_id, entered_at, state) VALUES (?, ?, ?)")
            .bind(Uuid::new_v4().to_string())
            .bind((cutoff - time::Duration::seconds(1)).to_string())
            .bind(serde_json::to_string(&Swap::from(stale))?)
            .execute(&db.pool)
            .await?;

        let mut pending = db
            .get_alice_swaps_with_pending_tx_lock(cutoff.unix_timestamp())
            .await?;
        pending.sort_by_key(|(id, _)| *id != started_id);
        assert_eq!(pending, vec![(started_id, started), (seen_id, seen)]);
        Ok(())
    }

    #[tokio::test]
    async fn paginated_swaps_are_listed_once_their_lock_transaction_was_seen() -> Result<()> {
        let db = setup_test_db().await?;
        let wallet = bitcoin_wallet::TestWalletBuilder::new(1_000_000)
            .build()
            .await;
        let state3 = Box::new(test_state3(&wallet).await?);
        let started = State::Alice(AliceState::Started {
            state3: state3.clone(),
        });
        let seen = State::Alice(AliceState::BtcLockTransactionSeen {
            state3: state3.clone(),
        });
        let locked = State::Alice(AliceState::BtcLocked { state3 });
        let aborted = State::Alice(AliceState::SafelyAborted);

        let (legacy, locked_id, pending, never_locked, unreadable) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for (id, states) in [
            // Created before Alice persisted swaps at registration
            (legacy, vec![&seen, &locked]),
            (locked_id, vec![&started, &seen, &locked]),
            (pending, vec![&started]),
            (never_locked, vec![&started, &aborted]),
            (unreadable, vec![&started, &seen]),
        ] {
            db.insert_peer_id(id, PeerId::random()).await?;
            for state in states {
                db.insert_latest_state(id, state.clone()).await?;
            }
        }
        // An unreadable row between readable ones must not fail the listing
        sqlx::query("INSERT INTO swap_states (swap_id, entered_at, state) VALUES (?, ?, ?)")
            .bind(unreadable.to_string())
            .bind(OffsetDateTime::now_utc().to_string())
            .bind("{not json")
            .execute(&db.pool)
            .await?;
        db.insert_latest_state(unreadable, locked.clone()).await?;

        let listed: Vec<_> = db
            .all_paginated(10, 0)
            .await?
            .into_iter()
            .map(|(_, id, first, last)| (id, first, last))
            .collect();
        assert_eq!(
            listed,
            vec![
                (legacy, seen.clone(), locked.clone()),
                (locked_id, seen.clone(), locked.clone()),
                (unreadable, seen, locked),
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn fresh_swaps_skip_malformed_timestamps() -> Result<()> {
        let db = setup_test_db().await?;
        let id = Uuid::new_v4();
        db.insert_peer_id(id, PeerId::random()).await?;
        for timestamp in [
            "not a timestamp",
            "2025-01-01 4:30:00 +01:00:00",
            "2025-99-01 4:30:00 +00:00:00",
            "2025-01-01 4:30:00.garbage +00:00:00",
            "2025-01-01 4:30:00.123 invalid +00:00:00",
        ] {
            sqlx::query("INSERT INTO swap_states (swap_id, entered_at, state) VALUES (?, ?, ?)")
                .bind(id.to_string())
                .bind(timestamp)
                .bind(r#"{"Alice":{"Done":"SafelyAborted"}}"#)
                .execute(&db.pool)
                .await?;
            assert!(db.all_fresh(24).await?.is_empty());
            assert!(
                db.get_alice_swaps_with_pending_tx_lock(OffsetDateTime::now_utc().unix_timestamp())
                    .await?
                    .is_empty()
            );
        }
        assert!(db.all_fresh(u64::MAX).await.is_err());
        Ok(())
    }

    async fn setup_test_db() -> Result<SqliteDatabase> {
        let dir: TempDir = tempdir().unwrap();
        let temp_db = dir.path().join("tempdb");

        // file has to exist in order to connect with sqlite
        File::create(&temp_db).unwrap();

        // keep the directory alive for the duration of the test
        let _db_dir = dir.keep();

        let db = SqliteDatabase::open(temp_db, AccessMode::ReadWrite).await?;

        Ok(db)
    }
}
