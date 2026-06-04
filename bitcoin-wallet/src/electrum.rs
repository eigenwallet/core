//! Electrum backend for the Bitcoin wallet, built on [`electrum_streaming_client`] via the async
//! [`ElectrumBalancer`].
//!
//! [`Client`] mirrors the previous `bdk_electrum`-backed client: it tracks watched script
//! histories and the chain tip (for [`Watchable`] status), broadcasts to every server, and
//! estimates fees. [`SyncGlue`] re-ports the `bdk_electrum` chain-sync logic (full scan / sync,
//! transaction & anchor caching, Merkle-proof-validated confirmation anchors and re-org-aware
//! checkpoint construction) so that the produced [`FullScanResponse`]/[`SyncResponse`] can be
//! applied to a `bdk_wallet::Wallet` exactly as before.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use bdk_core::spk_client::{
    FullScanRequest, FullScanResponse, SpkWithExpectedTxids, SyncRequest, SyncResponse,
};
use bdk_core::{BlockId, CheckPoint, ConfirmationBlockTime, TxUpdate};
use bdk_wallet::KeychainKind;
use bitcoin::{BlockHash, FeeRate, OutPoint, ScriptBuf, Transaction, Txid, block::Header};
use electrum_pool::{Connection, ElectrumBalancer, Error};
use electrum_streaming_client::request::{
    EstimateFee, GetFeeHistogram, GetHistory, GetTx, GetTxMerkle, Header as HeaderReq, Headers,
    HeadersSubscribe, RelayFee,
};
use electrum_streaming_client::response;
use tokio::sync::{Mutex as TokioMutex, RwLock as TokioRwLock};

use crate::primitives::{Confirmed, EstimateFeeRate, ScriptStatus, Watchable};
use crate::{BlockHeight, RpcErrorCode, extract_rpc_error_code};

/// We include a chain suffix of a certain length for the purpose of robustness.
const CHAIN_SUFFIX_LENGTH: u32 = 8;

/// One Electrum history entry for a script: a transaction id and its Electrum height
/// (`> 0` confirmed at that block height, `0`/`-1` unconfirmed).
#[derive(Debug, Clone, Copy)]
struct HistoryEntry {
    txid: Txid,
    height: i64,
}

impl From<&response::Tx> for HistoryEntry {
    fn from(tx: &response::Tx) -> Self {
        Self {
            txid: tx.txid(),
            height: tx.electrum_height(),
        }
    }
}

/// In-memory caches shared across all server connections, mirroring `BdkElectrumClient`.
#[derive(Default)]
pub(crate) struct Caches {
    txs: SyncMutex<HashMap<Txid, Arc<Transaction>>>,
    headers: SyncMutex<HashMap<u32, Header>>,
    anchors: SyncMutex<HashMap<(Txid, BlockHash), ConfirmationBlockTime>>,
}

/// Electrum client wrapping the load balancer plus watched-script state.
#[derive(Clone)]
pub struct Client {
    /// The underlying load balancer over all configured Electrum servers.
    pub(crate) inner: Arc<ElectrumBalancer>,
    /// Transaction/header/anchor caches used by the chain-sync glue.
    pub(crate) caches: Arc<Caches>,
    /// Last-known merged history for each watched script.
    script_history: Arc<TokioRwLock<BTreeMap<ScriptBuf, Vec<HistoryEntry>>>>,
    /// Active subscriptions, deduplicated by `(txid, script)`.
    pub(crate) subscriptions: Arc<TokioMutex<HashMap<(Txid, ScriptBuf), crate::Subscription>>>,
    /// Time of the last `update_state`.
    last_sync: Arc<SyncMutex<Instant>>,
    /// How often `update_state` actually refreshes.
    sync_interval: Duration,
    /// Monotonic latest known block height.
    latest_block_height: Arc<SyncMutex<BlockHeight>>,
}

impl Client {
    /// Create a new client over the given Electrum servers.
    pub async fn new(electrum_rpc_urls: &[String], sync_interval: Duration) -> Result<Self> {
        let balancer = ElectrumBalancer::new(electrum_rpc_urls.to_vec())
            .map_err(|e| anyhow!("Failed to create Electrum balancer: {e}"))?;
        let initial_last_sync = Instant::now()
            .checked_sub(sync_interval)
            .ok_or_else(|| anyhow!("failed to set last sync time"))?;

        Ok(Self {
            inner: Arc::new(balancer),
            caches: Arc::new(Caches::default()),
            script_history: Arc::new(TokioRwLock::new(BTreeMap::new())),
            subscriptions: Arc::new(TokioMutex::new(HashMap::new())),
            last_sync: Arc::new(SyncMutex::new(initial_last_sync)),
            sync_interval,
            latest_block_height: Arc::new(SyncMutex::new(BlockHeight::from(0))),
        })
    }

    /// Refresh watched-script histories and the chain tip if the sync interval has elapsed (or
    /// `force`).
    pub async fn update_state(&self, force: bool) -> Result<()> {
        if !force {
            let last_sync = *self.last_sync.lock().expect("last_sync mutex poisoned");
            if Instant::now().duration_since(last_sync) < self.sync_interval {
                return Ok(());
            }
        }

        self.update_script_histories().await?;
        self.update_block_height().await?;

        *self.last_sync.lock().expect("last_sync mutex poisoned") = Instant::now();

        Ok(())
    }

    /// Refresh a single script's history and the chain tip, ignoring the sync-interval throttle.
    pub async fn update_state_single(&self, script: &dyn Watchable) -> Result<()> {
        self.update_script_history_for(script.script()).await?;
        self.update_block_height().await?;
        Ok(())
    }

    async fn update_block_height(&self) -> Result<()> {
        let latest = self
            .inner
            .request("block_headers_subscribe", HeadersSubscribe)
            .await
            .context("Failed to fetch latest block header")?;
        let latest_block_height = BlockHeight::from(latest.height);

        let mut current = self
            .latest_block_height
            .lock()
            .expect("latest_block_height mutex poisoned");
        if latest_block_height > *current {
            tracing::trace!(
                block_height = u32::from(latest_block_height),
                "Got notification for new block"
            );
            *current = latest_block_height;
        }

        Ok(())
    }

    async fn update_script_histories(&self) -> Result<()> {
        let scripts: Vec<ScriptBuf> = self.script_history.read().await.keys().cloned().collect();
        if scripts.is_empty() {
            return Ok(());
        }

        let mut any_success = false;
        let mut last_error = None;
        for script in scripts {
            match self.update_script_history_for(script).await {
                Ok(()) => any_success = true,
                Err(e) => last_error = Some(e),
            }
        }

        if !any_success {
            if let Some(e) = last_error {
                return Err(e);
            }
        }

        Ok(())
    }

    /// Refresh a single script's history by merging the responses of all servers (highest height
    /// wins per txid). Succeeds if at least one server responds.
    pub async fn update_script_history(&self, script: &dyn Watchable) -> Result<()> {
        self.update_script_history_for(script.script()).await
    }

    async fn update_script_history_for(&self, script: ScriptBuf) -> Result<()> {
        let results = self.inner.script_get_history_all(script.clone()).await;

        let mut all_entries = Vec::new();
        let mut any_success = false;
        let mut first_error = None;
        for result in results {
            match result {
                Ok(history) => {
                    any_success = true;
                    all_entries.extend(history.iter().map(HistoryEntry::from));
                }
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
            }
        }

        if !any_success {
            if let Some(e) = first_error {
                return Err(anyhow::Error::new(e));
            }
        }

        self.script_history
            .write()
            .await
            .insert(script, merge_history(all_entries));

        Ok(())
    }

    /// Broadcast a transaction to all servers in parallel, caching it on first acceptance.
    pub async fn transaction_broadcast_all(
        &self,
        transaction: &Transaction,
    ) -> Result<Vec<Result<Txid, Error>>> {
        let results = self.inner.broadcast_all(transaction.clone()).await;

        if results.iter().any(|r| r.is_ok()) {
            self.caches
                .txs
                .lock()
                .expect("tx cache poisoned")
                .insert(transaction.compute_txid(), Arc::new(transaction.clone()));
        }

        Ok(results)
    }

    /// Compute the [`ScriptStatus`] of the given watchable transaction.
    pub async fn status_of_script(
        &self,
        script: &dyn Watchable,
        force: bool,
    ) -> Result<ScriptStatus> {
        let (script_buf, txid) = script.script_and_txid();

        let is_first_time = {
            let mut history = self.script_history.write().await;
            if history.contains_key(&script_buf) {
                false
            } else {
                history.insert(script_buf.clone(), vec![]);
                true
            }
        };

        if is_first_time || force {
            self.update_state_single(script).await?;
        } else {
            self.update_state(false).await?;
        }

        let history_guard = self.script_history.read().await;
        let history = history_guard.get(&script_buf);

        let history_of_tx: Vec<&HistoryEntry> = history
            .into_iter()
            .flatten()
            .filter(|entry| entry.txid == txid)
            .collect();

        let [rest @ .., last] = history_of_tx.as_slice() else {
            return Ok(ScriptStatus::Unseen);
        };

        if !rest.is_empty() {
            tracing::warn!(%txid, "Found multiple history entries for the same txid. Ignoring all but the last one.");
        }

        let latest_block_height = *self
            .latest_block_height
            .lock()
            .expect("latest_block_height mutex poisoned");

        match last.height {
            ..=0 => Ok(ScriptStatus::InMempool),
            height => Ok(ScriptStatus::Confirmed(
                Confirmed::from_inclusion_and_latest_block(
                    u32::try_from(height)?,
                    u32::from(latest_block_height),
                ),
            )),
        }
    }

    /// Fetch a transaction from any server. `Ok(None)` if the servers report it does not exist.
    pub async fn get_tx(&self, txid: Txid) -> Result<Option<Arc<Transaction>>> {
        match self.inner.request("get_raw_transaction", GetTx { txid }).await {
            Ok(full) => {
                let tx = Arc::new(full.tx);
                self.caches
                    .txs
                    .lock()
                    .expect("tx cache poisoned")
                    .insert(txid, tx.clone());
                Ok(Some(tx))
            }
            Err(multi_error) => {
                if multi_error.any(is_tx_not_found) {
                    tracing::trace!(
                        %txid,
                        error_count = multi_error.len(),
                        "Transaction not found indicated by one or more Electrum servers"
                    );
                    Ok(None)
                } else {
                    Err(anyhow!(multi_error)
                        .context("Failed to get transaction from the Electrum server"))
                }
            }
        }
    }

    /// Estimate the fee rate (sat/kwu) to be confirmed within `target_block` blocks via
    /// `blockchain.estimatefee`.
    pub async fn estimate_fee_rate(&self, target_block: u32) -> Result<FeeRate> {
        let resp = self
            .inner
            .request(
                "estimate_fee",
                EstimateFee {
                    number: target_block as usize,
                },
            )
            .await?;

        resp.fee_rate
            .filter(|rate| rate.to_sat_per_kwu() > 0)
            .ok_or_else(|| anyhow!("Fee rate returned by Electrum server is less than 0"))
    }

    /// Estimate a fee rate from the mempool fee histogram, adapting faster to mempool spikes.
    async fn estimate_fee_rate_from_histogram(&self, target_block: u32) -> Result<FeeRate> {
        const HISTOGRAM_SAFETY_MARGIN: f32 = 0.8;

        let mut histogram = self
            .inner
            .request("get_fee_histogram", GetFeeHistogram)
            .await?;

        if histogram.is_empty() {
            return Err(anyhow!(
                "The mempool seems to be empty therefore we cannot estimate the fee rate from the histogram"
            ));
        }

        histogram.sort_by(|a, b| a.fee_rate.cmp(&b.fee_rate));

        let estimated_block_size = 1_000_000u64;
        #[allow(clippy::cast_precision_loss)]
        let target_distance_from_tip =
            (estimated_block_size * target_block as u64) as f32 * HISTOGRAM_SAFETY_MARGIN;

        let mut cumulative_vsize = 0u64;
        for pair in &histogram {
            cumulative_vsize += pair.weight.to_vbytes_ceil();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            if cumulative_vsize >= target_distance_from_tip as u64 {
                return Ok(pair.fee_rate);
            }
        }

        Ok(histogram
            .first()
            .expect("The histogram should not be empty")
            .fee_rate)
    }

    async fn min_relay_fee(&self) -> Result<FeeRate> {
        let resp = self.inner.request("relay_fee", RelayFee).await?;

        // The relay fee is reported per kvB; convert to sat / kwu (kwu = kB × 4).
        let sat_per_kwu = resp.fee.to_sat() / 4;
        Ok(FeeRate::from_sat_per_kwu(sat_per_kwu))
    }

    /// Full scan a `bdk_wallet` full-scan request against a single server with failover.
    pub(crate) async fn full_scan<F>(
        &self,
        build_request: F,
        stop_gap: usize,
        batch_size: usize,
    ) -> Result<FullScanResponse<KeychainKind>>
    where
        F: Fn() -> FullScanRequest<KeychainKind> + Send + Sync,
    {
        let build_request = &build_request;
        let caches = self.caches.clone();
        let response = self
            .inner
            .run("full_scan_wallet", move |conn| {
                let request = build_request();
                let glue = SyncGlue::new(conn, caches.clone());
                async move { glue.full_scan(request, stop_gap, batch_size, true).await }
            })
            .await?;

        Ok(response)
    }

    /// Sync a `bdk_wallet` sync request against a single server with failover.
    pub(crate) async fn sync<F>(&self, build_request: F, batch_size: usize) -> Result<SyncResponse>
    where
        F: Fn() -> SyncRequest<(KeychainKind, u32)> + Send + Sync,
    {
        let build_request = &build_request;
        let caches = self.caches.clone();
        let response = self
            .inner
            .run("sync_wallet", move |conn| {
                let request = build_request();
                let glue = SyncGlue::new(conn, caches.clone());
                async move { glue.sync(request, batch_size, true).await }
            })
            .await?;

        Ok(response)
    }
}

impl EstimateFeeRate for Client {
    async fn estimate_feerate(&self, target_block: u32) -> Result<FeeRate> {
        let (conservative, histogram) = tokio::join!(
            self.estimate_fee_rate(target_block),
            self.estimate_fee_rate_from_histogram(target_block)
        );

        match (conservative, histogram) {
            (Ok(conservative), Ok(histogram)) => {
                tracing::debug!(
                    electrum_conservative_fee_rate_sat_vb = conservative.to_sat_per_vb_ceil(),
                    electrum_histogram_fee_rate_sat_vb = histogram.to_sat_per_vb_ceil(),
                    "Successfully fetched fee rates from both sources. We will use the higher one"
                );
                Ok(conservative.max(histogram))
            }
            (Err(conservative_error), Ok(histogram)) => {
                tracing::warn!(
                    ?conservative_error,
                    electrum_histogram_fee_rate_sat_vb = histogram.to_sat_per_vb_ceil(),
                    "Failed to fetch conservative fee rate, using histogram fee rate"
                );
                Ok(histogram)
            }
            (Ok(conservative), Err(histogram_error)) => {
                tracing::warn!(
                    ?histogram_error,
                    electrum_conservative_fee_rate_sat_vb = conservative.to_sat_per_vb_ceil(),
                    "Failed to fetch histogram fee rate, using conservative fee rate"
                );
                Ok(conservative)
            }
            (Err(conservative_error), Err(histogram_error)) => Err(conservative_error
                .context(histogram_error)
                .context(
                    "Failed to fetch both the conservative and histogram fee rates from Electrum",
                )),
        }
    }

    async fn min_relay_fee(&self) -> Result<FeeRate> {
        Client::min_relay_fee(self).await
    }
}

/// Merge history entries by txid, keeping the highest-height entry for each.
fn merge_history(entries: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    let mut best: BTreeMap<Txid, HistoryEntry> = BTreeMap::new();
    for entry in entries {
        best.entry(entry.txid)
            .and_modify(|current| {
                if entry.height > current.height {
                    *current = entry;
                }
            })
            .or_insert(entry);
    }
    best.into_values().collect()
}

/// Whether a server error indicates the transaction does not exist.
fn is_tx_not_found(error: &Error) -> bool {
    let Some(json) = error.response_json() else {
        return false;
    };

    if json.contains("No such mempool or blockchain transaction")
        || json.contains("missing transaction")
    {
        return true;
    }

    if let Some(code) = extract_rpc_error_code(json) {
        return code == i64::from(RpcErrorCode::RpcInvalidAddressOrKey);
    }

    false
}

/// Per-connection chain-sync engine: a faithful async re-port of `bdk_electrum`'s
/// `BdkElectrumClient` against the streaming client and our shared caches.
pub(crate) struct SyncGlue {
    conn: Arc<Connection>,
    caches: Arc<Caches>,
}

struct SpkScanState {
    unused_spk_count: usize,
    last_active_index: Option<u32>,
    stop_gap: usize,
}

enum BatchOutcome {
    Continue,
    Stop,
}

impl SyncGlue {
    pub(crate) fn new(conn: Arc<Connection>, caches: Arc<Caches>) -> Self {
        Self { conn, caches }
    }

    async fn full_scan(
        &self,
        mut request: FullScanRequest<KeychainKind>,
        stop_gap: usize,
        batch_size: usize,
        fetch_prev_txouts: bool,
    ) -> Result<FullScanResponse<KeychainKind>, Error> {
        let start_time = request.start_time();

        let tip_and_latest_blocks = match request.chain_tip() {
            Some(chain_tip) => Some(self.fetch_tip_and_latest_blocks(chain_tip).await?),
            None => None,
        };

        let mut tx_update = TxUpdate::<ConfirmationBlockTime>::default();
        let mut last_active_indices = BTreeMap::<KeychainKind, u32>::new();
        let mut pending_anchors = Vec::new();

        for keychain in request.keychains() {
            let mut state = SpkScanState {
                unused_spk_count: 0,
                last_active_index: None,
                stop_gap,
            };

            loop {
                let batch: Vec<(u32, SpkWithExpectedTxids)> = {
                    let mut spks = request.iter_spks(keychain.clone());
                    (0..batch_size)
                        .map_while(|_| spks.next())
                        .map(|(i, spk)| (i, SpkWithExpectedTxids::from(spk)))
                        .collect()
                };

                if batch.is_empty() {
                    break;
                }

                if let BatchOutcome::Stop = self
                    .process_spk_batch(
                        start_time,
                        &mut tx_update,
                        batch,
                        &mut pending_anchors,
                        &mut state,
                    )
                    .await?
                {
                    break;
                }
            }

            if let Some(last_active_index) = state.last_active_index {
                last_active_indices.insert(keychain, last_active_index);
            }
        }

        if fetch_prev_txouts {
            self.fetch_prev_txout(&mut tx_update).await?;
        }

        self.apply_anchors(&mut tx_update, &pending_anchors).await?;

        let chain_update = match tip_and_latest_blocks {
            Some((chain_tip, latest_blocks)) => Some(chain_update(
                chain_tip,
                &latest_blocks,
                tx_update.anchors.iter().cloned(),
            )),
            None => None,
        };

        Ok(FullScanResponse {
            tx_update,
            chain_update,
            last_active_indices,
        })
    }

    async fn sync(
        &self,
        mut request: SyncRequest<(KeychainKind, u32)>,
        batch_size: usize,
        fetch_prev_txouts: bool,
    ) -> Result<SyncResponse, Error> {
        let start_time = request.start_time();

        let tip_and_latest_blocks = match request.chain_tip() {
            Some(chain_tip) => Some(self.fetch_tip_and_latest_blocks(chain_tip).await?),
            None => None,
        };

        let mut tx_update = TxUpdate::<ConfirmationBlockTime>::default();
        let mut pending_anchors = Vec::new();

        let mut state = SpkScanState {
            unused_spk_count: 0,
            last_active_index: None,
            stop_gap: usize::MAX,
        };
        let mut spk_index = 0u32;
        loop {
            let batch: Vec<(u32, SpkWithExpectedTxids)> = {
                let mut spks = request.iter_spks_with_expected_txids();
                (0..batch_size)
                    .map_while(|_| spks.next())
                    .map(|spk| {
                        let indexed = (spk_index, spk);
                        spk_index += 1;
                        indexed
                    })
                    .collect()
            };

            if batch.is_empty() {
                break;
            }

            self.process_spk_batch(
                start_time,
                &mut tx_update,
                batch,
                &mut pending_anchors,
                &mut state,
            )
            .await?;
        }

        let txids: Vec<Txid> = request.iter_txids().collect();
        self.populate_with_txids(start_time, &mut tx_update, txids, &mut pending_anchors)
            .await?;

        let outpoints: Vec<OutPoint> = request.iter_outpoints().collect();
        self.populate_with_outpoints(start_time, &mut tx_update, outpoints, &mut pending_anchors)
            .await?;

        if fetch_prev_txouts {
            self.fetch_prev_txout(&mut tx_update).await?;
        }

        self.apply_anchors(&mut tx_update, &pending_anchors).await?;

        let chain_update = match tip_and_latest_blocks {
            Some((chain_tip, latest_blocks)) => Some(chain_update(
                chain_tip,
                &latest_blocks,
                tx_update.anchors.iter().cloned(),
            )),
            None => None,
        };

        Ok(SyncResponse {
            tx_update,
            chain_update,
        })
    }

    async fn apply_anchors(
        &self,
        tx_update: &mut TxUpdate<ConfirmationBlockTime>,
        pending_anchors: &[(Txid, usize)],
    ) -> Result<(), Error> {
        if pending_anchors.is_empty() {
            return Ok(());
        }
        let anchors = self.batch_fetch_anchors(pending_anchors).await?;
        for (txid, anchor) in anchors {
            tx_update.anchors.insert((anchor, txid));
        }
        Ok(())
    }

    async fn fetch_tx(&self, txid: Txid) -> Result<Arc<Transaction>, Error> {
        if let Some(tx) = self.caches.txs.lock().expect("tx cache poisoned").get(&txid) {
            return Ok(tx.clone());
        }

        let full = self.conn.request(GetTx { txid }).await?;
        let tx = Arc::new(full.tx);
        self.caches
            .txs
            .lock()
            .expect("tx cache poisoned")
            .insert(txid, tx.clone());
        Ok(tx)
    }

    async fn process_spk_batch(
        &self,
        start_time: u64,
        tx_update: &mut TxUpdate<ConfirmationBlockTime>,
        batch: Vec<(u32, SpkWithExpectedTxids)>,
        pending_anchors: &mut Vec<(Txid, usize)>,
        state: &mut SpkScanState,
    ) -> Result<BatchOutcome, Error> {
        let histories = futures::future::join_all(
            batch
                .iter()
                .map(|(_, spk)| self.conn.request(GetHistory::from_script(spk.spk.clone()))),
        )
        .await;

        for ((spk_index, spk), history_res) in batch.into_iter().zip(histories) {
            let history = history_res?;

            if history.is_empty() {
                match state.unused_spk_count.checked_add(1) {
                    Some(i) if i < state.stop_gap => state.unused_spk_count = i,
                    _ => return Ok(BatchOutcome::Stop),
                }
            } else {
                state.last_active_index = Some(spk_index);
                state.unused_spk_count = 0;
            }

            let history_set: HashSet<Txid> = history.iter().map(|tx| tx.txid()).collect();
            for &txid in spk.expected_txids.difference(&history_set) {
                tx_update.evicted_ats.insert((txid, start_time));
            }

            for tx in history {
                let txid = tx.txid();
                tx_update.txs.push(self.fetch_tx(txid).await?);
                let height = tx.electrum_height();
                if height > 0 {
                    pending_anchors.push((txid, height as usize));
                } else {
                    tx_update.seen_ats.insert((txid, start_time));
                }
            }
        }

        Ok(BatchOutcome::Continue)
    }

    async fn populate_with_txids(
        &self,
        start_time: u64,
        tx_update: &mut TxUpdate<ConfirmationBlockTime>,
        txids: Vec<Txid>,
        pending_anchors: &mut Vec<(Txid, usize)>,
    ) -> Result<(), Error> {
        let mut txs = Vec::<(Txid, Arc<Transaction>)>::new();
        let mut scripts = Vec::new();
        for txid in txids {
            match self.fetch_tx(txid).await {
                Ok(tx) => {
                    let spk = tx
                        .output
                        .first()
                        .expect("tx must have an output")
                        .script_pubkey
                        .clone();
                    txs.push((txid, tx));
                    scripts.push(spk);
                }
                // A "not found" (server response) error means the txid is unknown: skip it.
                Err(Error::Response(_)) => continue,
                Err(e) => return Err(e),
            }
        }

        let histories = futures::future::join_all(
            scripts
                .iter()
                .map(|spk| self.conn.request(GetHistory::from_script(spk.clone()))),
        )
        .await;

        for ((txid, tx), history_res) in txs.into_iter().zip(histories) {
            let history = history_res?;
            if let Some(entry) = history.into_iter().find(|entry| entry.txid() == txid) {
                let height = entry.electrum_height();
                if height > 0 {
                    pending_anchors.push((txid, height as usize));
                } else {
                    tx_update.seen_ats.insert((txid, start_time));
                }
            }
            tx_update.txs.push(tx);
        }

        Ok(())
    }

    async fn populate_with_outpoints(
        &self,
        start_time: u64,
        tx_update: &mut TxUpdate<ConfirmationBlockTime>,
        outpoints: Vec<OutPoint>,
        pending_anchors: &mut Vec<(Txid, usize)>,
    ) -> Result<(), Error> {
        let mut ops_spks_txs = Vec::new();
        for op in outpoints {
            if let Ok(tx) = self.fetch_tx(op.txid).await {
                if let Some(txout) = tx.output.get(op.vout as usize) {
                    ops_spks_txs.push((op, txout.script_pubkey.clone(), tx));
                }
            }
        }

        let unique_spks: Vec<ScriptBuf> = ops_spks_txs
            .iter()
            .map(|(_, spk, _)| spk.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let histories = futures::future::join_all(
            unique_spks
                .iter()
                .map(|spk| self.conn.request(GetHistory::from_script(spk.clone()))),
        )
        .await;

        let mut spk_map: HashMap<ScriptBuf, Vec<response::Tx>> = HashMap::new();
        for (spk, history_res) in unique_spks.into_iter().zip(histories) {
            spk_map.insert(spk, history_res?);
        }

        for (outpoint, spk, tx) in ops_spks_txs {
            let Some(spk_history) = spk_map.get(&spk) else {
                continue;
            };

            let mut has_residing = false;
            let mut has_spending = false;

            for res in spk_history {
                if has_residing && has_spending {
                    break;
                }
                let res_txid = res.txid();

                if !has_residing && res_txid == outpoint.txid {
                    has_residing = true;
                    tx_update.txs.push(tx.clone());
                    let height = res.electrum_height();
                    if height > 0 {
                        pending_anchors.push((res_txid, height as usize));
                    } else {
                        tx_update.seen_ats.insert((res_txid, start_time));
                    }
                }

                if !has_spending && res_txid != outpoint.txid {
                    let res_tx = self.fetch_tx(res_txid).await?;
                    has_spending = res_tx
                        .input
                        .iter()
                        .any(|txin| txin.previous_output == outpoint);
                    if !has_spending {
                        continue;
                    }
                    tx_update.txs.push(res_tx);
                    let height = res.electrum_height();
                    if height > 0 {
                        pending_anchors.push((res_txid, height as usize));
                    } else {
                        tx_update.seen_ats.insert((res_txid, start_time));
                    }
                }
            }
        }

        Ok(())
    }

    async fn batch_fetch_anchors(
        &self,
        txs_with_heights: &[(Txid, usize)],
    ) -> Result<Vec<(Txid, ConfirmationBlockTime)>, Error> {
        let mut results = Vec::with_capacity(txs_with_heights.len());
        let mut to_fetch = Vec::new();

        let mut needed_heights: Vec<u32> =
            txs_with_heights.iter().map(|&(_, h)| h as u32).collect();
        needed_heights.sort_unstable();
        needed_heights.dedup();

        let mut height_to_hash = HashMap::with_capacity(needed_heights.len());

        let mut missing_heights = Vec::new();
        {
            let cache = self.caches.headers.lock().expect("header cache poisoned");
            for &height in &needed_heights {
                if let Some(header) = cache.get(&height) {
                    height_to_hash.insert(height, header.block_hash());
                } else {
                    missing_heights.push(height);
                }
            }
        }

        if !missing_heights.is_empty() {
            let headers = futures::future::join_all(
                missing_heights
                    .iter()
                    .map(|&height| self.conn.request(HeaderReq { height })),
            )
            .await;

            let mut cache = self.caches.headers.lock().expect("header cache poisoned");
            for (height, header_res) in missing_heights.into_iter().zip(headers) {
                let header = header_res?.header;
                height_to_hash.insert(height, header.block_hash());
                cache.insert(height, header);
            }
        }

        {
            let anchor_cache = self.caches.anchors.lock().expect("anchor cache poisoned");
            for &(txid, height) in txs_with_heights {
                let hash = height_to_hash[&(height as u32)];
                if let Some(anchor) = anchor_cache.get(&(txid, hash)) {
                    results.push((txid, *anchor));
                } else {
                    to_fetch.push((txid, height));
                }
            }
        }

        let proofs = futures::future::join_all(to_fetch.iter().map(|&(txid, height)| {
            self.conn.request(GetTxMerkle {
                txid,
                height: height as u32,
            })
        }))
        .await;

        for ((txid, height), proof_res) in to_fetch.into_iter().zip(proofs) {
            let proof = proof_res?;

            let mut header = {
                let cache = self.caches.headers.lock().expect("header cache poisoned");
                cache
                    .get(&(height as u32))
                    .copied()
                    .expect("header already fetched above")
            };

            let mut valid = proof.expected_merkle_root(txid) == header.merkle_root;
            if !valid {
                header = self
                    .conn
                    .request(HeaderReq {
                        height: height as u32,
                    })
                    .await?
                    .header;
                self.caches
                    .headers
                    .lock()
                    .expect("header cache poisoned")
                    .insert(height as u32, header);
                valid = proof.expected_merkle_root(txid) == header.merkle_root;
            }

            if valid {
                let hash = header.block_hash();
                let anchor = ConfirmationBlockTime {
                    confirmation_time: header.time as u64,
                    block_id: BlockId {
                        height: height as u32,
                        hash,
                    },
                };
                self.caches
                    .anchors
                    .lock()
                    .expect("anchor cache poisoned")
                    .insert((txid, hash), anchor);
                results.push((txid, anchor));
            }
        }

        Ok(results)
    }

    async fn fetch_prev_txout(
        &self,
        tx_update: &mut TxUpdate<ConfirmationBlockTime>,
    ) -> Result<(), Error> {
        let mut no_dup = HashSet::<Txid>::new();
        let txs: Vec<Arc<Transaction>> = tx_update.txs.clone();
        for tx in &txs {
            if !tx.is_coinbase() && no_dup.insert(tx.compute_txid()) {
                for vin in &tx.input {
                    let outpoint = vin.previous_output;
                    let prev_tx = self.fetch_tx(outpoint.txid).await?;
                    let txout = prev_tx
                        .output
                        .get(outpoint.vout as usize)
                        .ok_or_else(|| {
                            Error::connection(format!("prevout {outpoint} does not exist"))
                        })?
                        .clone();
                    tx_update.txouts.insert(outpoint, txout);
                }
            }
        }
        Ok(())
    }

    async fn fetch_tip_and_latest_blocks(
        &self,
        prev_tip: CheckPoint,
    ) -> Result<(CheckPoint, BTreeMap<u32, BlockHash>), Error> {
        let new_tip_height = self.conn.request(HeadersSubscribe).await?.height;

        // If the server's tip is lower than ours, checkpoints need no updating.
        if new_tip_height < prev_tip.height() {
            return Ok((prev_tip, BTreeMap::new()));
        }

        let mut new_blocks = {
            let start_height = new_tip_height.saturating_sub(CHAIN_SUFFIX_LENGTH - 1);
            let headers = self
                .conn
                .request(Headers {
                    start_height,
                    count: CHAIN_SUFFIX_LENGTH as usize,
                })
                .await?
                .headers;
            (start_height..)
                .zip(headers.into_iter().map(|h| h.block_hash()))
                .collect::<BTreeMap<u32, BlockHash>>()
        };

        let agreement_cp = {
            let mut agreement_cp = Option::<CheckPoint>::None;
            for cp in prev_tip.iter() {
                let cp_block = cp.block_id();
                let hash = match new_blocks.get(&cp_block.height) {
                    Some(&hash) => hash,
                    None => {
                        let hash = self
                            .conn
                            .request(HeaderReq {
                                height: cp_block.height,
                            })
                            .await?
                            .header
                            .block_hash();
                        new_blocks.insert(cp_block.height, hash);
                        hash
                    }
                };
                if hash == cp_block.hash {
                    agreement_cp = Some(cp);
                    break;
                }
            }
            agreement_cp
                .ok_or_else(|| Error::connection("cannot find agreement block with server"))?
        };

        let agreement_height = agreement_cp.height();
        let extension = new_blocks
            .iter()
            .filter(move |(height, _)| **height > agreement_height)
            .map(|(&height, &hash)| BlockId { height, hash });
        let new_tip = agreement_cp
            .extend(extension)
            .expect("extension heights already checked to be greater than agreement height");

        Ok((new_tip, new_blocks))
    }
}

/// Add a corresponding checkpoint per anchor height if it does not yet exist (bounded by
/// `latest_blocks` to keep hashes consistent across re-orgs).
fn chain_update(
    mut tip: CheckPoint,
    latest_blocks: &BTreeMap<u32, BlockHash>,
    anchors: impl Iterator<Item = (ConfirmationBlockTime, Txid)>,
) -> CheckPoint {
    for (anchor, _txid) in anchors {
        let height = anchor.block_id.height;
        if tip.get(height).is_none() && height <= tip.height() {
            let hash = latest_blocks
                .get(&height)
                .copied()
                .unwrap_or(anchor.block_id.hash);
            tip = tip.insert(BlockId { hash, height });
        }
    }
    tip
}
