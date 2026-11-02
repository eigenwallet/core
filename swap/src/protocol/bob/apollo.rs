//! Apollo: encrypted on-chain backup of Bob's swap state.
//!
//! The Apollo transaction spends the Apollo output of the lock transaction and burns all of it as the fee.
//! Its only output is an `OP_RETURN` that carries the encrypted [`Apollo`].
//! The encryption key comes from Bob's seed, so Bob can find all Apollo transactions
//! in his Bitcoin wallet and continue the swaps with only the seed.

use anyhow::{Context, Result, anyhow, bail, ensure};
use bitcoin::script::Instruction;
use bitcoin::{Transaction, TxOut, Txid, Weight};
use bitcoin_wallet::primitives::ScriptStatus;
use bitcoin_wallet::{BitcoinWallet, burn_output_to_op_return};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use libp2p::PeerId;
use rand::RngCore;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use swap_core::monero::{BlockHeight, LabeledMoneroAddress, MoneroAddressPool};
use swap_machine::bob::{ApolloProgress, BobState, State3, State3Apollo, SwapSecretKeys};
use uuid::Uuid;

pub use manager::{ApolloManagerConfig, ApolloManagerHandle, ApolloStatus, spawn_apollo_manager};

use crate::cli::api::tauri_bindings::{TauriEmitter, TauriHandle};
use crate::common::retry;
use crate::protocol::{Database, PendingApollo, RecoveredSwap};
use crate::seed::Seed;

mod manager;

const APOLLO_VERSION: u8 = 1;
const NONCE_SIZE: usize = 12;
const APOLLO_RECOVERY_SYNC_MAX_ELAPSED_TIME: Duration = Duration::from_secs(10 * 60);
const APOLLO_RECOVERY_SYNC_MAX_INTERVAL: Duration = Duration::from_secs(30);
/// Time between two recovery runs after the first one at startup.
/// A new Apollo transaction can appear in the wallet at any time, for example from another device.
const APOLLO_RECOVERY_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Nodes that relay at most 80 bytes of `OP_RETURN` data (Bitcoin Core < 30, Bitcoin Knots)
/// treat a larger Apollo transaction as non-standard.
pub const TX_APOLLO_MAX_BLOB_SIZE: usize = 1500;

const TX_APOLLO_TARGET_BLOCKS: u32 = 3;

/// Weight of an Apollo transaction that carries a blob of `blob_size` bytes.
///
/// It has one P2WPKH input and one `OP_RETURN` output.
pub fn tx_apollo_weight(blob_size: usize) -> Weight {
    const NON_WITNESS_BYTES_WITHOUT_BLOB: u64 = 4 // version
        + 1 // input count
        + 41 // input
        + 1 // output count
        + 8 + 3 + 1 + 3 // OP_RETURN output without the blob
        + 4; // lock time
    const WITNESS_WEIGHT: u64 = 2 // segwit marker and flag
        + 108; // P2WPKH witness

    Weight::from_wu((NON_WITNESS_BYTES_WITHOUT_BLOB + blob_size as u64) * 4 + WITNESS_WEIGHT)
}

/// Upper bound of the bytes of an encrypted Apollo that do not depend on the Monero receive pool.
/// It covers version, nonce, tag, swap id, peer id, block height and the largest [`State3Apollo`]
/// (three bech32m addresses, full refund signatures, all optional fields set).
/// [`PreparedApollo::encrypt`] stays the authoritative size check.
const MAX_BLOB_SIZE_WITHOUT_MONERO_RECEIVE_POOL: usize = 1080;

/// Maximum number of Monero receive addresses of a swap. Four addresses do not fit in the Apollo.
const MAX_MONERO_RECEIVE_POOL_ENTRIES: usize = 3;

/// Fails if the Monero receive pool is too large for the Apollo to fit into [`TX_APOLLO_MAX_BLOB_SIZE`].
/// Call it before the user commits to a swap.
pub fn assert_monero_receive_pool_fits(pool: &MoneroAddressPool) -> Result<()> {
    ensure!(
        pool.iter().count() <= MAX_MONERO_RECEIVE_POOL_ENTRIES,
        "A swap can have at most {MAX_MONERO_RECEIVE_POOL_ENTRIES} Monero receive addresses"
    );
    let pool_size = bincode::serialized_size(&encode_monero_receive_pool(pool))
        .context("Failed to measure the Monero receive pool")? as usize;
    let max_pool_size = TX_APOLLO_MAX_BLOB_SIZE - MAX_BLOB_SIZE_WITHOUT_MONERO_RECEIVE_POOL;

    ensure!(
        pool_size <= max_pool_size,
        "The Monero receive pool is too large: it takes {pool_size} bytes but at most {max_pool_size} fit. Use fewer or shorter addresses"
    );

    Ok(())
}

pub fn tx_apollo_max_weight() -> Weight {
    tx_apollo_weight(TX_APOLLO_MAX_BLOB_SIZE)
}

/// The fee of the Apollo transaction for a blob of any allowed size.
///
/// Bob estimates it once when a swap starts. It is the value of the Apollo output of the lock
/// transaction, and the Apollo transaction reads the fee back from that output.
pub async fn estimate_tx_apollo_fee(bitcoin_wallet: &dyn BitcoinWallet) -> Result<bitcoin::Amount> {
    bitcoin_wallet
        .estimate_fee_for_target(tx_apollo_max_weight(), TX_APOLLO_TARGET_BLOCKS)
        .await
        .context("Failed to estimate Apollo transaction fee")
}

#[derive(Clone)]
pub struct ApolloEncryptionKey([u8; 32]);

impl ApolloEncryptionKey {
    pub fn new(key: [u8; 32]) -> Self {
        Self(key)
    }

    fn cipher(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(Key::from_slice(&self.0))
    }
}

impl std::fmt::Debug for ApolloEncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApolloEncryptionKey(..)")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Apollo {
    swap_id: Uuid,
    alice_peer_id: Vec<u8>,
    monero_wallet_restore_blockheight: u64,
    monero_receive_pool: Vec<ApolloMoneroAddress>,
    state3: State3Apollo,
}

/// [`MoneroAddressPool`] stores its percentages as `Decimal`, which `bincode`
/// cannot decode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ApolloMoneroAddress {
    address: Option<String>,
    percentage: String,
}

fn encode_monero_receive_pool(pool: &MoneroAddressPool) -> Vec<ApolloMoneroAddress> {
    pool.iter()
        .map(|labeled_address| ApolloMoneroAddress {
            address: labeled_address.address().map(|address| address.to_string()),
            percentage: labeled_address.percentage().to_string(),
        })
        .collect()
}

/// Restores payout destinations and percentages with empty display labels.
fn decode_monero_receive_pool(pool: Vec<ApolloMoneroAddress>) -> Result<MoneroAddressPool> {
    let labeled_addresses = pool
        .into_iter()
        .map(|entry| {
            let percentage = Decimal::from_str(&entry.percentage)
                .context("Apollo contains an invalid percentage")?;

            let Some(address) = entry.address else {
                return LabeledMoneroAddress::with_internal_address(percentage, String::new());
            };
            let address = monero_address::MoneroAddress::from_str_with_unchecked_network(&address)
                .context("Apollo contains an invalid Monero address")?;

            LabeledMoneroAddress::with_address(address, percentage, String::new())
        })
        .collect::<Result<Vec<_>>>()?;

    let pool = MoneroAddressPool::new(labeled_addresses);
    pool.assert_sum_to_one()?;
    Ok(pool)
}

impl Apollo {
    pub fn new(
        swap_id: Uuid,
        alice_peer_id: PeerId,
        monero_wallet_restore_blockheight: BlockHeight,
        monero_receive_pool: MoneroAddressPool,
        state3: &State3,
        secret_keys: &SwapSecretKeys,
    ) -> Result<Self> {
        monero_receive_pool.assert_sum_to_one()?;
        Ok(Self {
            swap_id,
            alice_peer_id: alice_peer_id.to_bytes(),
            monero_wallet_restore_blockheight: monero_wallet_restore_blockheight.height,
            monero_receive_pool: encode_monero_receive_pool(&monero_receive_pool),
            state3: state3.apollo(secret_keys)?,
        })
    }

    /// Layout: version (1 byte) || nonce (12 bytes) || ciphertext with tag.
    pub fn encrypt(&self, key: &ApolloEncryptionKey) -> Result<PreparedApollo> {
        let plaintext = bincode::serialize(self).context("Failed to encode Apollo")?;

        PreparedApollo::encrypt(&plaintext, key)
    }

    /// Returns `None` if the blob is not an Apollo encrypted with `key`.
    pub fn decrypt(key: &ApolloEncryptionKey, blob: &[u8]) -> Result<Option<Self>> {
        let Some((&version, rest)) = blob.split_first() else {
            return Ok(None);
        };
        if version != APOLLO_VERSION || rest.len() < NONCE_SIZE {
            return Ok(None);
        }
        let (nonce, ciphertext) = rest.split_at(NONCE_SIZE);

        let Ok(plaintext) = key.cipher().decrypt(Nonce::from_slice(nonce), ciphertext) else {
            return Ok(None);
        };

        let backup = bincode::deserialize(&plaintext)
            .context("Apollo is authentic but cannot be decoded")?;

        Ok(Some(backup))
    }

    async fn into_recovered_swap(
        self,
        tx: Transaction,
        bitcoin_wallet: &dyn BitcoinWallet,
        seed: &Seed,
    ) -> Result<RecoveredSwap, backoff::Error<anyhow::Error>> {
        use backoff::Error::{Permanent, Transient};
        let transient = |error| Transient {
            err: error,
            retry_after: None,
        };

        let swap_id = self.swap_id;
        let alice_peer_id = PeerId::from_bytes(&self.alice_peer_id)
            .context("Apollo contains an invalid peer ID")
            .map_err(Permanent)?;
        let monero_receive_pool =
            decode_monero_receive_pool(self.monero_receive_pool).map_err(Permanent)?;
        let [input] = tx.input.as_slice() else {
            return Err(Permanent(anyhow!(
                "Apollo transaction {} has {} inputs, expected one",
                tx.compute_txid(),
                tx.input.len()
            )));
        };
        let tx_lock = fetch_transaction(bitcoin_wallet, input.previous_output.txid)
            .await
            .map_err(transient)?;
        ensure_lock_is_seen(
            bitcoin_wallet,
            swap_id,
            &tx_lock,
            input.previous_output.vout,
        )
        .await
        .map_err(transient)?;
        let mut tx_lock_prevouts = Vec::with_capacity(tx_lock.input.len());
        for tx_lock_input in &tx_lock.input {
            tx_lock_prevouts.push(
                fetch_prevout(bitcoin_wallet, tx_lock_input.previous_output)
                    .await
                    .map_err(transient)?,
            );
        }

        let state3 = State3::from_apollo(
            self.state3,
            seed.derive_swap_secret_keys(swap_id),
            tx_lock,
            tx_lock_prevouts,
            input.previous_output.vout,
        )
        .map_err(Permanent)?;
        state3
            .verify_alice_signatures()
            .context("Apollo does not match the seed-derived keys")
            .map_err(Permanent)?;

        Ok(RecoveredSwap {
            swap_id,
            alice_peer_id,
            monero_receive_pool,
            state: BobState::BtcLocked {
                state3,
                monero_wallet_restore_blockheight: BlockHeight {
                    height: self.monero_wallet_restore_blockheight,
                },
            },
            apollo: ApolloProgress::Published(tx),
        })
    }

    pub fn swap_id(&self) -> Uuid {
        self.swap_id
    }
}

/// Confirmations after which an Apollo transaction counts as confirmed.
/// A reorg is unlikely to evict it after that, and nothing checks a confirmed Apollo transaction again.
const TX_APOLLO_MIN_CONFIRMATIONS: u32 = 3;

/// Encrypted and size-checked recovery data, prepared before locking Bitcoin.
pub struct PreparedApollo(Vec<u8>);

impl PreparedApollo {
    fn encrypt(plaintext: &[u8], key: &ApolloEncryptionKey) -> Result<Self> {
        let mut nonce = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut nonce);

        let ciphertext = key
            .cipher()
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .map_err(|_| anyhow::anyhow!("Failed to encrypt Apollo"))?;

        let mut blob = Vec::with_capacity(1 + NONCE_SIZE + ciphertext.len());
        blob.push(APOLLO_VERSION);
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ciphertext);

        if blob.len() > TX_APOLLO_MAX_BLOB_SIZE {
            bail!(
                "Apollo has {} bytes, the maximum is {}",
                blob.len(),
                TX_APOLLO_MAX_BLOB_SIZE
            );
        }

        Ok(PreparedApollo(blob))
    }
}

/// Calls `handle` for every swap with a pending Apollo transaction (neither confirmed nor abandoned),
/// whatever the swap state is.
///
/// The function handles all valid rows before it returns an error.
/// A corrupt row, or a row that `handle` rejects, does not stop the other rows, and the error names all failed rows.
async fn for_each_pending_apollo(
    db: &(dyn Database + Send + Sync),
    mut handle: impl FnMut(Uuid, &ApolloProgress) -> Result<()>,
) -> Result<()> {
    let PendingApollo { ready, corrupt } = db
        .pending_apollo_progress()
        .await
        .context("Failed to load the pending Apollo progress")?;

    let mut failures: Vec<String> = corrupt
        .into_iter()
        .map(|(swap_id, error)| format!("{swap_id}: {error:#}"))
        .collect();
    for (swap_id, progress) in &ready {
        if let Err(error) = handle(*swap_id, progress) {
            failures.push(format!("{swap_id}: {error:#}"));
        }
    }
    ensure!(
        failures.is_empty(),
        "Invalid Apollo progress of {} swap(s): {}",
        failures.len(),
        failures.join("; ")
    );

    Ok(())
}

/// Saves the first progress row of a swap before Bob broadcasts the lock transaction.
/// A row for the same Apollo transaction can exist already (for example after a crash, with later progress).
/// The function keeps that row.
pub async fn persist_constructed_apollo(
    db: &(dyn Database + Send + Sync),
    swap_id: Uuid,
    apollo_tx: &Transaction,
) -> Result<()> {
    if db
        .insert_apollo_if_absent(swap_id, ApolloProgress::Constructed(apollo_tx.clone()))
        .await
        .context("Failed to persist Apollo before locking Bitcoin")?
    {
        return Ok(());
    }

    let existing = db
        .get_apollo_progress(swap_id)
        .await
        .context("Failed to load the persisted Apollo")?;
    ensure!(
        existing.as_ref().map(ApolloProgress::tx) == Some(apollo_tx),
        "Swap {swap_id} already has a different Apollo transaction"
    );

    Ok(())
}

/// Retries a database call until it succeeds, so a transient error does not stop the publisher.
/// A row that cannot be decoded never becomes decodable, so the function returns that error.
async fn retry_db<T, Fut: Future<Output = Result<T>>>(
    description: &str,
    op: impl Fn() -> Fut,
) -> Result<T> {
    retry(
        description,
        || async {
            op().await.map_err(|error| {
                if error.chain().any(|cause| cause.is::<serde_json::Error>()) {
                    backoff::Error::permanent(error)
                } else {
                    backoff::Error::transient(error)
                }
            })
        },
        None,
        None,
    )
    .await
}

/// Whether a conflicting transaction with `min_confirmations` replaced the lock transaction that the
/// Apollo transaction spends. Abandoning is final, so an unconfirmed replacement is not enough.
async fn lock_conflicted(
    bitcoin_wallet: &dyn BitcoinWallet,
    tx_apollo: &Transaction,
    min_confirmations: u32,
) -> Result<bool> {
    let [input] = tx_apollo.input.as_slice() else {
        bail!("Apollo transaction must have exactly one input");
    };

    bitcoin_wallet
        .is_transaction_conflicted(input.previous_output.txid, min_confirmations)
        .await
}

/// Hands the outputs spent by the pending Apollo transactions to `reserve`.
/// The wallet must not spend them, or the Apollo transaction becomes invalid.
///
/// If a row is invalid, the function reserves the outputs of all valid rows and then fails.
pub async fn reserve_pending_apollo_outpoints(
    db: &(dyn Database + Send + Sync),
    reserve: impl FnOnce(Vec<bitcoin::OutPoint>),
) -> Result<()> {
    let mut outpoints = Vec::new();
    let result = for_each_pending_apollo(db, |_, progress| {
        let tx = progress.tx();
        let [input] = tx.input.as_slice() else {
            bail!(
                "Apollo transaction {} has {} inputs, expected one",
                tx.compute_txid(),
                tx.input.len()
            );
        };
        outpoints.push(input.previous_output);
        Ok(())
    })
    .await;
    reserve(outpoints);

    result
}

/// Broadcasts Apollo unless the server reports the required confirmation count.
/// Returns the current status.
///
/// The confirmation depth comes from the Electrum server. The txid check does not prove it.
/// This is an accepted limitation.
async fn ensure_tx_apollo_published(
    bitcoin_wallet: &dyn BitcoinWallet,
    tx_apollo: &Transaction,
    required_confirmations: u32,
) -> Result<ScriptStatus> {
    // Watch the input's previous output script.
    // Indexers can omit the OP_RETURN output.
    let [input] = tx_apollo.input.as_slice() else {
        bail!("Apollo transaction must have exactly one input");
    };
    let prevout = fetch_prevout(bitcoin_wallet, input.previous_output).await?;
    let watch = (tx_apollo.compute_txid(), prevout.script_pubkey);
    let status = bitcoin_wallet
        .status_of_script(&watch)
        .await
        .context("Failed to get status of Apollo transaction")?;
    if has_required_confirmations(bitcoin_wallet, tx_apollo, status, required_confirmations).await?
    {
        return Ok(status);
    }

    let broadcast = bitcoin_wallet
        .broadcast_without_subscription(tx_apollo.clone(), "Apollo")
        .await
        .context("Failed to publish Apollo transaction");
    match status {
        ScriptStatus::Unseen | ScriptStatus::Retrying => {
            broadcast?;
        }
        // Servers reject transactions they already know. This is expected here.
        ScriptStatus::InMempool | ScriptStatus::Confirmed(_) => {
            if let Err(error) = broadcast {
                tracing::debug!(?error, "Apollo rebroadcast was not accepted");
            }
        }
    }

    let status = bitcoin_wallet
        .status_of_script(&watch)
        .await
        .context("Failed to get status of Apollo transaction")?;
    // The function returns a status with the required depth only after this check fetches the transaction
    has_required_confirmations(bitcoin_wallet, tx_apollo, status, required_confirmations).await?;

    Ok(status)
}

/// Whether `status` has the required confirmations.
/// The status is a claim of the server about its script history, so a deep status must come
/// with the transaction itself. `fetch_transaction` checks that transaction against its txid.
async fn has_required_confirmations(
    bitcoin_wallet: &dyn BitcoinWallet,
    tx_apollo: &Transaction,
    status: ScriptStatus,
    required_confirmations: u32,
) -> Result<bool> {
    let ScriptStatus::Confirmed(confirmed) = status else {
        return Ok(false);
    };
    if !confirmed.meets_target(required_confirmations) {
        return Ok(false);
    }

    fetch_transaction(bitcoin_wallet, tx_apollo.compute_txid())
        .await
        .context("Failed to fetch the confirmed Apollo transaction")?;

    Ok(true)
}

/// Builds the Apollo transaction from the signed lock transaction, so Bob can save both before he broadcasts either.
/// The function does not register the lock transaction in the wallet.
/// The Apollo transaction burns the whole Apollo output, so its fee is the value of that output.
pub async fn build_tx_apollo(
    bitcoin_wallet: &dyn BitcoinWallet,
    tx_lock: &Transaction,
    output_vout: u32,
    prepared: PreparedApollo,
) -> Result<Transaction> {
    let PreparedApollo(blob) = prepared;
    let blob_size = blob.len();

    let psbt = burn_output_to_op_return(tx_lock, output_vout, blob)
        .context("Failed to build Apollo transaction")?;
    let fee = psbt
        .fee()
        .context("Failed to compute the Apollo transaction fee")?;
    let tx_apollo = bitcoin_wallet
        .sign_and_finalize(psbt)
        .await
        .context("Failed to sign Apollo transaction")?;

    tracing::info!(txid = %tx_apollo.compute_txid(), blob_size, %fee, "Built Apollo transaction");

    Ok(tx_apollo)
}

fn op_return_data(tx: &Transaction) -> Option<&[u8]> {
    tx.output
        .iter()
        .filter(|output| output.script_pubkey.is_op_return())
        .find_map(|output| match output.script_pubkey.instructions().nth(1) {
            Some(Ok(Instruction::PushBytes(data))) => Some(data.as_bytes()),
            _ => None,
        })
}

/// The result of one scan for Apollo transactions.
#[derive(Debug, Default)]
pub struct ApolloRecovery {
    /// Swaps that were saved by this scan
    pub recovered: Vec<Uuid>,
    /// Candidates that failed with a permanent error, or that are invalid.
    /// Recovery is incomplete while this is not empty.
    pub unresolved: Vec<Txid>,
}

/// Finds all Apollo transactions in the Bitcoin wallet and saves the swaps that are not in the database.
///
/// The candidates run concurrently, so a slow candidate does not delay the others.
/// A temporary error (Electrum, lock not yet seen, database) is retried until it succeeds.
/// An invalid backup is logged and skipped at once. Nothing is saved for a failed candidate.
/// The failed candidates are returned in `unresolved`, so a completed scan is not mistaken for a full recovery.
///
/// Calls `on_recovered` for each swap right after it is saved.
pub async fn recover_swaps_from_apollo_transactions(
    bitcoin_wallet: &dyn BitcoinWallet,
    seed: &Seed,
    db: &(dyn Database + Send + Sync),
    on_recovered: impl Fn(Uuid),
) -> Result<ApolloRecovery> {
    let key = seed.derive_apollo_encryption_key();
    let (key, on_recovered) = (&key, &on_recovered);

    let outcomes = futures::future::join_all(
        bitcoin_wallet
            .wallet_spending_transactions()
            .await
            .into_iter()
            .map(|tx| async move {
                let txid = tx.compute_txid();
                let result = retry(
                    "Recovering a swap from an Apollo candidate",
                    || recover_candidate(bitcoin_wallet, seed, db, key, &tx),
                    // Temporary errors are retried forever, a permanent error ends the retries
                    None,
                    APOLLO_RECOVERY_SYNC_MAX_INTERVAL,
                )
                .await;
                match result {
                    Ok(Some(swap_id)) => {
                        tracing::info!(%swap_id, %txid, "Recovered swap from Apollo transaction");
                        on_recovered(swap_id);
                        (Some(swap_id), None)
                    }
                    Ok(None) => (None, None),
                    Err(error) => {
                        tracing::warn!(%txid, ?error, "Failed to recover swap from Apollo candidate");
                        (None, Some(txid))
                    }
                }
            }),
    )
    .await;

    let mut recovery = ApolloRecovery::default();
    for (recovered, unresolved) in outcomes {
        recovery.recovered.extend(recovered);
        recovery.unresolved.extend(unresolved);
    }

    Ok(recovery)
}

/// Runs [`recover_after_sync`] as a detached task, once after startup and then every [`APOLLO_RECOVERY_INTERVAL`].
/// The task only inserts recovered swaps into the database and then wakes the Apollo manager.
/// It stops when the Apollo manager is shut down.
/// Only the GUI uses this task.
///
/// Swaps that are in the database already are skipped by a cheap lookup.
///
/// The sync comes first (with retries), because a restored wallet knows its Apollo transactions
/// only after a sync. If the sync keeps failing, that run stops instead of running with a stale wallet.
pub fn spawn_apollo_recovery(
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    seed: Seed,
    db: Arc<dyn Database + Send + Sync>,
    tauri_handle: Option<TauriHandle>,
    apollo: ApolloManagerHandle,
) {
    let shutdown = apollo.shutdown_token();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(APOLLO_RECOVERY_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            // The first tick completes at once, so the first run happens at startup
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = interval.tick() => {}
            }
            // Candidates retry forever, so a run can last longer than the interval.
            // A dropped run is safe because every step is idempotent. Each swap is inserted
            // in one conditional transaction and the manager wake only reads the database.
            let run = tokio::time::timeout(
                APOLLO_RECOVERY_INTERVAL,
                recover_after_sync(bitcoin_wallet.clone(), &seed, db.clone(), tauri_handle.clone(), &apollo),
            );
            tokio::select! {
                _ = shutdown.cancelled() => return,
                result = run => {
                    if result.is_err() {
                        tracing::warn!("Apollo recovery is still retrying some candidates, starting a new scan");
                    }
                }
            }
        }
    });
}

pub async fn recover_after_sync(
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    seed: &Seed,
    db: Arc<dyn Database + Send + Sync>,
    tauri_handle: Option<TauriHandle>,
    apollo: &ApolloManagerHandle,
) {
    if let Err(error) = retry(
        "Syncing Bitcoin wallet before Apollo recovery",
        || async {
            bitcoin_wallet
                .sync()
                .await
                .map_err(backoff::Error::transient)
        },
        APOLLO_RECOVERY_SYNC_MAX_ELAPSED_TIME,
        APOLLO_RECOVERY_SYNC_MAX_INTERVAL,
    )
    .await
    {
        tracing::warn!(
            ?error,
            "Skipping Apollo recovery: Bitcoin wallet sync failed"
        );
        return;
    }

    // Show and publish each recovered swap at once. A candidate that never resolves must not hold them back.
    let on_recovered = |swap_id| {
        tauri_handle.emit_swap_state_change_event(swap_id);
        apollo.wake();
    };

    match recover_swaps_from_apollo_transactions(
        bitcoin_wallet.as_ref(),
        seed,
        db.as_ref(),
        on_recovered,
    )
    .await
    {
        Ok(recovery) => {
            if !recovery.recovered.is_empty() {
                tracing::info!(recovered = ?recovery.recovered, "Recovered swaps from Apollo transactions");
            }
            if !recovery.unresolved.is_empty() {
                tracing::warn!(unresolved = ?recovery.unresolved, "Apollo recovery is incomplete: some candidates could not be recovered, they are retried on the next run");
            }
        }
        Err(error) => tracing::warn!(?error, "Apollo recovery failed"),
    }
}

async fn recover_candidate(
    bitcoin_wallet: &dyn BitcoinWallet,
    seed: &Seed,
    db: &(dyn Database + Send + Sync),
    key: &ApolloEncryptionKey,
    tx: &Transaction,
) -> Result<Option<Uuid>, backoff::Error<anyhow::Error>> {
    let transient = |error| backoff::Error::Transient {
        err: error,
        retry_after: None,
    };

    let Some(blob) = op_return_data(tx) else {
        return Ok(None);
    };
    let Some(apollo) = Apollo::decrypt(key, blob).map_err(backoff::Error::permanent)? else {
        return Ok(None);
    };
    let swap_id = apollo.swap_id;

    if db.has_swap(swap_id).await.map_err(transient)? {
        return Ok(None);
    }

    let recovered_swap = apollo
        .into_recovered_swap(tx.clone(), bitcoin_wallet, seed)
        .await?;

    if !db
        .insert_recovered_swap(recovered_swap)
        .await
        .map_err(transient)?
    {
        return Ok(None);
    }
    // Reserve the output for the Apollo transaction in this session too, until that transaction is confirmed.
    // `into_recovered_swap` already checked that the transaction has one input.
    bitcoin_wallet.reserve_outpoints(vec![tx.input[0].previous_output]);

    Ok(Some(swap_id))
}

/// Whether the lock transaction is in the mempool or confirmed.
/// The script is the one of the lock output that the Apollo transaction spends.
async fn lock_is_seen(
    bitcoin_wallet: &dyn BitcoinWallet,
    lock_txid: Txid,
    lock_script: bitcoin::ScriptBuf,
) -> Result<bool> {
    let status = bitcoin_wallet
        .status_of_script(&(lock_txid, lock_script))
        .await
        .context("Failed to get status of lock transaction")?;

    Ok(matches!(
        status,
        ScriptStatus::InMempool | ScriptStatus::Confirmed(_)
    ))
}

/// Fails unless the lock transaction is in the mempool or confirmed.
/// Recovery then does not restore a swap whose lock transaction was never mined, replaced or double-spent.
/// The function saves nothing, and recovery retries the candidate, because the lock may only be late.
async fn ensure_lock_is_seen(
    bitcoin_wallet: &dyn BitcoinWallet,
    swap_id: Uuid,
    tx_lock: &Transaction,
    lock_vout: u32,
) -> Result<()> {
    let lock_txid = tx_lock.compute_txid();
    let lock_output = tx_lock
        .output
        .get(usize::try_from(lock_vout)?)
        .with_context(|| format!("Lock transaction {lock_txid} has no output {lock_vout}"))?;

    if !lock_is_seen(bitcoin_wallet, lock_txid, lock_output.script_pubkey.clone()).await? {
        tracing::warn!(%swap_id, %lock_txid, "Not restoring swap from Apollo: lock transaction is neither in the mempool nor confirmed");
        bail!("Lock transaction {lock_txid} of swap {swap_id} is not in the mempool or confirmed");
    }

    Ok(())
}

async fn fetch_transaction(bitcoin_wallet: &dyn BitcoinWallet, txid: Txid) -> Result<Transaction> {
    let tx = bitcoin_wallet
        .get_raw_transaction(txid)
        .await?
        .with_context(|| format!("Transaction {txid} not found"))?;

    // A malicious Electrum server must not be able to substitute another transaction
    let fetched_txid = tx.compute_txid();
    if fetched_txid != txid {
        bail!("Requested transaction {txid} but the server returned {fetched_txid}");
    }

    Ok((*tx).clone())
}

async fn fetch_prevout(
    bitcoin_wallet: &dyn BitcoinWallet,
    outpoint: bitcoin::OutPoint,
) -> Result<TxOut> {
    let tx = fetch_transaction(bitcoin_wallet, outpoint.txid).await?;

    tx.output
        .get(outpoint.vout as usize)
        .cloned()
        .with_context(|| format!("Output {outpoint} not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_payload_size_limit_includes_version_nonce_and_tag() {
        let key = ApolloEncryptionKey::new([7; 32]);
        let plaintext = vec![42; TX_APOLLO_MAX_BLOB_SIZE - 1 - NONCE_SIZE - 16];
        let prepared = PreparedApollo::encrypt(&plaintext, &key).unwrap();
        assert_eq!(prepared.0.len(), TX_APOLLO_MAX_BLOB_SIZE);
        let decrypted = key
            .cipher()
            .decrypt(
                Nonce::from_slice(&prepared.0[1..1 + NONCE_SIZE]),
                &prepared.0[1 + NONCE_SIZE..],
            )
            .unwrap();
        assert_eq!(decrypted, plaintext);
        assert!(PreparedApollo::encrypt(&vec![0; plaintext.len() + 1], &key).is_err());
    }

    #[test]
    fn monero_receive_pool_survives_bincode_roundtrip_without_labels() {
        let address = monero_address::MoneroAddress::from_str_with_unchecked_network(
            "4AdUndXHHZ6cfufTMvppY6JwXNouMBzSkbLYfpAV5Usx3skxNgYeYTRj5UzqtReoS44qo9mtmXCqY45DJ852K5Jv2684Rge",
        )
        .unwrap();
        let pool = MoneroAddressPool::new(vec![
            LabeledMoneroAddress::with_address(address, Decimal::new(75, 2), "external".into())
                .unwrap(),
            LabeledMoneroAddress::with_internal_address(Decimal::new(25, 2), "internal".into())
                .unwrap(),
        ]);

        let encoded = bincode::serialize(&encode_monero_receive_pool(&pool)).unwrap();
        let decoded = decode_monero_receive_pool(bincode::deserialize(&encoded).unwrap()).unwrap();

        assert_eq!(decoded.iter().count(), pool.iter().count());
        for (decoded, original) in decoded.iter().zip(pool.iter()) {
            assert_eq!(decoded.address(), original.address());
            assert_eq!(decoded.percentage(), original.percentage());
            assert_eq!(decoded.label(), "");
        }
    }

    const FIXED_SWAP_ID: Uuid = Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);

    fn fixed_peer_id() -> PeerId {
        // Peers use Ed25519 (38 bytes). secp256k1 adds one byte, which the
        // slack in `MAX_BLOB_SIZE_WITHOUT_MONERO_RECEIVE_POOL` covers.
        libp2p::identity::Keypair::ed25519_from_bytes([9; 32])
            .unwrap()
            .public()
            .to_peer_id()
    }

    fn monero_address(address_type: monero_address::AddressType) -> monero_address::MoneroAddress {
        let key = |n: u8| {
            let mut bytes = [0; 32];
            bytes[0] = n;
            monero_oxide_ext::PrivateKey::from_slice(&bytes).unwrap()
        };
        monero_address::MoneroAddress::new(
            monero_address::Network::Mainnet,
            address_type,
            monero_oxide_ext::PublicKey::from_private_key(&key(1)).decompress(),
            monero_oxide_ext::PublicKey::from_private_key(&key(2)).decompress(),
        )
    }

    fn legacy_address() -> monero_address::MoneroAddress {
        monero_address(monero_address::AddressType::Legacy)
    }

    /// The longest address form: a guaranteed featured subaddress with a payment ID.
    fn longest_address() -> monero_address::MoneroAddress {
        monero_address(monero_address::AddressType::Featured {
            subaddress: true,
            payment_id: Some([7; 8]),
            guaranteed: true,
        })
    }

    fn fixed_apollo(largest: bool, monero_receive_pool: Vec<ApolloMoneroAddress>) -> Apollo {
        Apollo {
            swap_id: FIXED_SWAP_ID,
            alice_peer_id: fixed_peer_id().to_bytes(),
            monero_wallet_restore_blockheight: 3_000_000,
            monero_receive_pool,
            state3: swap_machine::bob::apollo_test_support::state3_apollo(largest),
        }
    }

    fn ordinary_pool() -> Vec<ApolloMoneroAddress> {
        vec![
            ApolloMoneroAddress {
                address: Some(legacy_address().to_string()),
                percentage: "0.75".into(),
            },
            ApolloMoneroAddress {
                address: None,
                percentage: "0.25".into(),
            },
        ]
    }

    /// Three of the longest addresses with the longest percentages that the pool check still accepts.
    fn largest_pool_entries() -> Vec<(monero_address::MoneroAddress, Decimal)> {
        let percentages =
            [3_333_333_333, 3_333_333_333, 3_333_333_334].map(|n| Decimal::new(n, 10));
        assert!(percentages.iter().all(|p| p.to_string().len() == 12));
        percentages.map(|p| (longest_address(), p)).to_vec()
    }

    fn pool_from(entries: Vec<(monero_address::MoneroAddress, Decimal)>) -> MoneroAddressPool {
        MoneroAddressPool::new(
            entries
                .into_iter()
                .map(|(address, percentage)| {
                    LabeledMoneroAddress::with_address(address, percentage, String::new()).unwrap()
                })
                .collect(),
        )
    }

    /// Pins the plaintext layout. If this test fails, the `bincode` layout changed and the code
    /// cannot decode Apollos from earlier versions. Bump `APOLLO_VERSION` and keep decoding
    /// the old version. Do not only update the constant.
    #[test]
    fn apollo_plaintext_matches_golden_vector() {
        const GOLDEN: &str = concat!(
            "1000000000000000123456789abcdef0123456789abcdef02600000000000000002408011220fd1724385aa0c75b64fb",
            "78cd602fa1d991fdebf76b13c58ed702eac835e9f618c0c62d00000000000200000000000000015f0000000000000034",
            "34795158666b575a4e6d4a38516752664657547a6d4a38516752664657547a6d4a38516752664657547a6d4a43416f66",
            "3770795561693351363878796f69653341534b39734254584e7565393579684737504537524c73347271774454410400",
            "000000000000302e3735000400000000000000302e3235031b84c5567b126440995d3ed5aaba0565d71e1834604819ff",
            "9c17f5e9d5dd078f2f1132ca61ab38dff00f2fea3228f24c6c71d58085b80e47e19515cb27e8d0470362c0a046dacce8",
            "6ddd0343c6d3c7c79c2208ba0d9c9cf24a6d046d21d21f90f72000000000000000060000000000000000000000000000",
            "000000000000000000000000000000000079df0d8648700000011027000000000000900000002001000001240000003e",
            "000000000000006263317030786c78766c68656d6a613663346471763232756170637471757066686c786d3968387a33",
            "6b3265373271346b3968637a3776717a6b356a6a303e000000000000006263317030786c78766c68656d6a6136633464",
            "71763232756170637471757066686c786d3968387a336b3265373271346b3968637a3776717a6b356a6a308ffaf4df6a",
            "6bc68a9e50f36123c26c76947c4f8ef6b01f5cd029db6cca76fba10180f9ad93ad47c6b3d04aafc184f7a5fedbbba739",
            "fb5a848683d00dc4b992eb0000000002af1e205ad1f729c51d933d83a58a0a2e8b99ee88b5e7cc83a855b0b6a06d6089",
            "03512f409f0c3a8de7d162cd7203ecc0478b81761b079f53035fccc60c83d73d0c9ca39c8f1f452f237a39abf3ede9cf",
            "cd8ab2e7839da12beaae922fc9aebcfca83d9174dd4d104a6b6238ab566c8383cad9dbe3d59160497be672aff5a94011",
            "5b8a5e5ad912d82e9cc5f1fc3ac6cdd57f5eb9efc69c5337030b33deb4564065ba8ffaf4df6a6bc68a9e50f36123c26c",
            "76947c4f8ef6b01f5cd029db6cca76fba10180f9ad93ad47c6b3d04aafc184f7a5fedbbba739fb5a848683d00dc4b992",
            "ebe903000000000000ea0300000000000001ec0300000000000001ed030000000000000000f003000000000000f10300",
            "0000000000010000003e000000000000006263317030786c78766c68656d6a6136633464717632327561706374717570",
            "66686c786d3968387a336b3265373271346b3968637a3776717a6b356a6a30",
        );
        let apollo = fixed_apollo(false, ordinary_pool());
        let plaintext = bincode::serialize(&apollo).unwrap();

        assert_eq!(hex::encode(&plaintext), GOLDEN);
        assert_eq!(bincode::deserialize::<Apollo>(&plaintext).unwrap(), apollo);
    }

    #[test]
    fn apollo_decrypts_to_the_encrypted_value_for_the_right_key_only() {
        let key = ApolloEncryptionKey::new([7; 32]);
        let apollo = fixed_apollo(false, ordinary_pool());

        // The nonce is random, so two encryptions differ but decrypt to the same Apollo
        let first = apollo.encrypt(&key).unwrap();
        let second = apollo.encrypt(&key).unwrap();
        assert_ne!(first.0, second.0);
        assert_eq!(first.0[0], APOLLO_VERSION);
        for blob in [&first.0, &second.0] {
            assert_eq!(Apollo::decrypt(&key, blob).unwrap(), Some(apollo.clone()));
        }

        let other_key = ApolloEncryptionKey::new([8; 32]);
        assert_eq!(Apollo::decrypt(&other_key, &first.0).unwrap(), None);
        let mut other_version = first.0.clone();
        other_version[0] = APOLLO_VERSION + 1;
        assert_eq!(Apollo::decrypt(&key, &other_version).unwrap(), None);
    }

    #[test]
    fn worst_case_apollo_fits_into_the_blob_limit() {
        let key = ApolloEncryptionKey::new([7; 32]);
        let pool = pool_from(largest_pool_entries());
        let encoded_pool = encode_monero_receive_pool(&pool);
        let pool_size = bincode::serialized_size(&encoded_pool).unwrap() as usize;

        let apollo = fixed_apollo(true, encoded_pool);
        let blob = apollo.encrypt(&key).unwrap().0;
        let size_without_pool = blob.len() - pool_size;

        assert!(
            blob.len() <= TX_APOLLO_MAX_BLOB_SIZE,
            "worst-case Apollo has {} bytes ({size_without_pool} without the receive pool, \
             {pool_size} for the pool), the maximum is {TX_APOLLO_MAX_BLOB_SIZE}",
            blob.len()
        );
        assert!(
            size_without_pool <= MAX_BLOB_SIZE_WITHOUT_MONERO_RECEIVE_POOL,
            "everything except the receive pool takes {size_without_pool} bytes, but \
             MAX_BLOB_SIZE_WITHOUT_MONERO_RECEIVE_POOL is only \
             {MAX_BLOB_SIZE_WITHOUT_MONERO_RECEIVE_POOL}"
        );
        assert!(assert_monero_receive_pool_fits(&pool).is_ok());
        assert_eq!(Apollo::decrypt(&key, &blob).unwrap(), Some(apollo));

        // Three of the longest addresses with the longest `Decimal` strings (28 decimal
        // places) are too large. The check must reject them up front.
        let too_long = Decimal::from_i128_with_scale(3_333_333_333_333_333_333_333_333_333, 28);
        assert_eq!(too_long.to_string().len(), 30);
        let too_large = pool_from(vec![(longest_address(), too_long); 3]);
        assert!(assert_monero_receive_pool_fits(&too_large).is_err());
    }

    #[test]
    fn receive_pool_check_accepts_three_entries_and_rejects_four() {
        let entry = |percentage: Decimal| (legacy_address(), percentage);
        let three = pool_from(vec![
            entry(Decimal::new(5, 1)),
            entry(Decimal::new(25, 2)),
            entry(Decimal::new(25, 2)),
        ]);
        assert!(assert_monero_receive_pool_fits(&three).is_ok());

        let four = pool_from(vec![entry(Decimal::new(25, 2)); 4]);
        let error = assert_monero_receive_pool_fits(&four).unwrap_err();
        assert!(error.to_string().contains("at most 3"), "{error}");
    }
}
