//! bitcoind RPC backend for the wallet (bounty #747).
//!
//! Wraps `bdk_bitcoind_rpc`'s Emitter so the wallet can sync from a
//! user-operated full node instead of third-party electrum servers.
use bdk_bitcoind_rpc::Emitter;
use bdk_chain::BlockId;
use bitcoin::{Block, Network};

/// Connection parameters for a user's own bitcoind node.
#[derive(Debug, Clone, PartialEq)]
pub struct BitcoindRpcConfig {
    /// RPC URL, e.g. "http://127.0.0.1:8332"
    pub url: String,
    /// RPC auth: "user:password" or cookie path understood by bitcoincore-rpc
    pub auth: String,
    /// Bitcoin network the node serves (must match wallet network)
    pub network: Network,
    /// Optional explicit start height for initial emission
    pub start_height: Option<u32>,
}

/// Emitter-backed sync source over a user's own bitcoind node.
#[derive(Clone)]
pub struct BitcoindRpcClient {
    config: BitcoindRpcConfig,
}

impl BitcoindRpcClient {
    pub fn new(config: BitcoindRpcConfig) -> anyhow::Result<Self> {
        match config.network {
            Network::Bitcoin | Network::Regtest | Network::Testnet | Network::Signet => {}
            n => anyhow::bail!("unsupported network {n:?}"),
        }
        Ok(Self { config })
    }

    pub fn rpc_client(&self) -> anyhow::Result<bitcoincore_rpc::Client> {
        Ok(bitcoincore_rpc::Client::new(
            &self.config.url,
            bitcoincore_rpc::Auth::UserPass(
                self.config.auth.split(':').next().unwrap_or("").into(),
                self.config.auth.split(':').nth(1).unwrap_or("").into(),
            ),
        )?)
    }

    /// Build an emitter starting from the node's current tip checkpoint.
    /// The caller persists `emitter.block_height()` + last seen hash between
    /// syncs and passes them back via `last_cp`/`start_height` to resume.
    pub fn emitter<'a>(
        &self,
        client: &'a bitcoincore_rpc::Client,
        last_cp: bdk_chain::CheckPoint,
        start_height: u32,
    ) -> anyhow::Result<Emitter<&'a bitcoincore_rpc::Client>> {
        Ok(Emitter::new(
            client,
            last_cp,
            start_height,
            bdk_bitcoind_rpc::NO_EXPECTED_MEMPOOL_TXS,
        ))
    }

    /// Convenience: emit the next confirmed block (with its height), if any.
    pub fn next_block<'a>(
        &self,
        em: &mut Emitter<&'a bitcoincore_rpc::Client>,
    ) -> anyhow::Result<Option<(Block, u32)>> {
        Ok(em.next_block()?.map(|ev| {
            let h = ev.block_height();
            (ev.block, h)
        }))
    }
}

/// Persistent state for RPC-based wallet sync (bounty #747).
/// Held optionally by the wallet; `None` = legacy electrum path.
#[derive(Clone)]
pub struct BitcoindRpcSyncState {
    pub client: BitcoindRpcClient,
    /// Node rpc client reused across syncs.
    rpc: std::sync::Arc<bitcoincore_rpc::Client>,
    /// Last applied checkpoint (height, hash) — update after each successful sync.
    pub last_cp: bdk_chain::CheckPoint,
    pub start_height: u32,
}

impl BitcoindRpcSyncState {
    pub fn new(config: BitcoindRpcConfig, start_height: u32) -> anyhow::Result<Self> {
        let client = BitcoindRpcClient::new(config.clone())?;
        let rpc = std::sync::Arc::new(client.rpc_client()?);
        use bitcoincore_rpc::RpcApi;
        let genesis_hash = rpc.get_block_hash(0)?;
        let anchor_height = if start_height == 0 { 0 } else { start_height };
        let anchor_hash = if start_height == 0 {
            genesis_hash
        } else {
            rpc.get_block_hash(anchor_height as u64)?
        };
        let last_cp = bdk_chain::CheckPoint::new(bdk_chain::BlockId {
            height: anchor_height,
            hash: anchor_hash,
        });
        Ok(Self {
            client,
            rpc,
            last_cp,
            start_height,
        })
    }

    /// Full RPC sync pass: drain confirmed blocks into the wallet, then apply mempool deltas.
    /// Mirrors the electrum path's contract: caller persists the wallet afterwards.
    pub async fn sync_pass<P: bdk_wallet::WalletPersister>(
        &mut self,
        wallet: &mut bdk_wallet::PersistedWallet<P>,
    ) -> anyhow::Result<()>
    where
        P::Error: std::fmt::Debug,
    {
        let mut em = self
            .client
            .emitter(&self.rpc, self.last_cp.clone(), self.start_height)?;

        // Confirmed blocks: apply in order until tip.
        let mut last_block_id: Option<bdk_chain::BlockId> = None;
        while let Some((block, height)) = self.client.next_block(&mut em)? {
            last_block_id = Some(bdk_chain::BlockId {
                height,
                hash: block.block_hash(),
            });
            wallet
                .apply_block(&block, height)
                .map_err(|e| anyhow::anyhow!("RPC sync apply_block failed at {height}: {e:?}"))?;
        }

        // Mempool deltas: new seen + evicted.
        let mem = em.mempool()?;
        wallet.apply_unconfirmed_txs(mem.update.clone().into_iter().map(|(tx, ts)| (tx, ts)));
        // Evicted txs are dropped implicitly once no longer in mempool update.

        // Advance checkpoint state for the next pass.
        if let Some(bid) = last_block_id {
            self.last_cp = bdk_chain::CheckPoint::new(bid);
        }
        Ok(())
    }
}
