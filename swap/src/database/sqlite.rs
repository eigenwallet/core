use crate::cli::api::tauri_bindings::TauriEmitter;
use crate::cli::api::tauri_bindings::TauriHandle;
use crate::database::Swap;
use crate::monero::LabeledMoneroAddress;
use crate::monero::MoneroAddressPool;
use crate::monero::TransferProof;
use crate::protocol::{Database, State};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use sqlx::sqlite::{Sqlite, SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{ConnectOptions, Pool};
use std::path::Path;
use std::str::FromStr;
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
    async fn insert_peer_id(&self, swap_id: Uuid, peer_id: PeerId) -> Result<()> {
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
        .execute(&self.pool)
        .await?;

        Ok(())
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
        .execute(&self.pool)
        .await?;

        // Emit event to Tauri, the frontend will then send another request to get the latest state
        // This is why we don't send the state here
        self.tauri_handle.emit_swap_state_change_event(swap_id);

        Ok(())
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
        // Include the supported serialized Started variant for compatibility with
        // stored/imported states, matching AliceState::pending_tx_lock. The current
        // runner first persists BtcLockTransactionSeen and reserves inputs until then.
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

        // A row that cannot be read must not stop every new swap setup. Alice wrote
        // these rows herself, so a bad row is corruption, not an attack.
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
    /// recorded state per swap. Swaps that never progressed past
    /// `SafelyAborted` are filtered out. `limit` and `offset` must each fit
    /// into `i32`.
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
                  FROM swap_states GROUP BY swap_id) oldest
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
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.is_some())
    }
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

        rows.iter()
            .map(|row| {
                // Invalid timestamps must not silently hide a swap from this query.
                row.entered_at_unix.with_context(|| {
                    format!(
                        "Invalid UTC timestamp {:?} for swap {}",
                        row.entered_at, row.swap_id
                    )
                })?;
                let swap_id = Uuid::from_str(&row.swap_id).context("Invalid swap UUID")?;
                let peer_id = PeerId::from_str(&row.peer_id).context("Invalid swap peer ID")?;
                let state = serde_json::from_str::<Swap>(&row.state)
                    .with_context(|| format!("Failed to deserialize state of swap {swap_id}"))?;
                Ok((peer_id, swap_id, State::from(state)))
            })
            .collect()
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
    use crate::protocol::alice::AliceState;
    use crate::protocol::bob::BobState;
    use std::fs::File;
    use tempfile::{TempDir, tempdir};

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
    async fn fresh_swaps_report_malformed_timestamps() -> Result<()> {
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
            assert!(
                db.all_fresh(24)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("Invalid UTC timestamp")
            );
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
