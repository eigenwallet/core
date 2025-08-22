use anyhow::Result;
use bitcoin::{FeeRate, ScriptBuf, Txid};

/// An object that can estimate fee rates and minimum relay fees.
pub trait EstimateFeeRate {
    /// Estimate the fee rate for a given target block.
    fn estimate_feerate(
        &self,
        target_block: u32,
    ) -> impl std::future::Future<Output = Result<FeeRate>> + Send;
    /// Get the minimum relay fee.
    fn min_relay_fee(&self) -> impl std::future::Future<Output = Result<FeeRate>> + Send;
}

/// A subscription to the status of a given transaction
/// that can be used to wait for the transaction to be confirmed.
#[derive(Debug, Clone)]
pub struct Subscription {
    /// A receiver used to await updates to the status of the transaction.
    pub receiver: tokio::sync::watch::Receiver<ScriptStatus>,
    /// The number of confirmations we require for a transaction to be considered final.
    pub finality_confirmations: u32,
    /// The transaction ID we are subscribing to.
    pub txid: Txid,
}

impl Subscription {
    pub fn new(
        receiver: tokio::sync::watch::Receiver<ScriptStatus>,
        finality_confirmations: u32,
        txid: Txid,
    ) -> Self {
        Self {
            receiver,
            finality_confirmations,
            txid,
        }
    }

    pub async fn wait_until_final(&self) -> anyhow::Result<()> {
        let conf_target = self.finality_confirmations;
        let txid = self.txid;

        tracing::info!(%txid, required_confirmation=%conf_target, "Waiting for Bitcoin transaction finality");

        let mut seen_confirmations = 0;

        self.wait_until(|status| match status {
            ScriptStatus::Confirmed(inner) => {
                let confirmations = inner.confirmations();

                if confirmations > seen_confirmations {
                    tracing::info!(%txid,
                        seen_confirmations = %confirmations,
                        needed_confirmations = %conf_target,
                        "Waiting for Bitcoin transaction finality");
                    seen_confirmations = confirmations;
                }

                inner.meets_target(conf_target)
            }
            _ => false,
        })
        .await
    }

    pub async fn wait_until_seen(&self) -> anyhow::Result<()> {
        self.wait_until(ScriptStatus::has_been_seen).await
    }

    pub async fn wait_until_confirmed_with<T>(&self, target: T) -> anyhow::Result<()>
    where
        T: Into<u32>,
        T: Copy,
    {
        self.wait_until(|status| status.is_confirmed_with(target))
            .await
    }

    pub async fn wait_until(
        &self,
        mut predicate: impl FnMut(&ScriptStatus) -> bool,
    ) -> anyhow::Result<()> {
        let mut receiver = self.receiver.clone();

        while !predicate(&receiver.borrow()) {
            receiver
                .changed()
                .await
                .map_err(|_| anyhow::anyhow!("Failed while waiting for next status update"))?;
        }

        Ok(())
    }
}

/// The possible statuses of a script.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ScriptStatus {
    Unseen,
    InMempool,
    Confirmed(Confirmed),
    Retrying,
}

/// The status of a confirmed transaction.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Confirmed {
    /// The depth of this transaction within the blockchain.
    ///
    /// Zero if the transaction is included in the latest block.
    depth: u32,
}

impl Confirmed {
    pub fn new(depth: u32) -> Self {
        Self { depth }
    }

    /// Compute the depth of a transaction based on its inclusion height and the
    /// latest known block.
    ///
    /// Our information about the latest block might be outdated. To avoid an
    /// overflow, we make sure the depth is 0 in case the inclusion height
    /// exceeds our latest known block,
    pub fn from_inclusion_and_latest_block(inclusion_height: u32, latest_block: u32) -> Self {
        let depth = latest_block.saturating_sub(inclusion_height);

        Self { depth }
    }

    pub fn confirmations(&self) -> u32 {
        self.depth + 1
    }

    pub fn meets_target<T>(&self, target: T) -> bool
    where
        T: Into<u32>,
    {
        self.confirmations() >= target.into()
    }

    pub fn blocks_left_until<T>(&self, target: T) -> u32
    where
        T: Into<u32> + Copy,
    {
        if self.meets_target(target) {
            0
        } else {
            target.into() - self.confirmations()
        }
    }
}

impl std::fmt::Display for ScriptStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptStatus::Unseen => write!(f, "unseen"),
            ScriptStatus::InMempool => write!(f, "in mempool"),
            ScriptStatus::Retrying => write!(f, "retrying"),
            ScriptStatus::Confirmed(inner) => {
                write!(f, "confirmed with {} blocks", inner.confirmations())
            }
        }
    }
}

/// Defines a watchable transaction.
///
/// For a transaction to be watchable, we need to know two things: Its
/// transaction ID and the specific output script that is going to change.
/// A transaction can obviously have multiple outputs but our protocol purposes,
/// we are usually interested in a specific one.
pub trait Watchable {
    /// The transaction ID.
    fn id(&self) -> Txid;
    /// The script of the output we are interested in.
    fn script(&self) -> ScriptBuf;
    /// Convenience method to get both the script and the txid.
    fn script_and_txid(&self) -> (ScriptBuf, Txid) {
        (self.script(), self.id())
    }
}

impl Watchable for (Txid, ScriptBuf) {
    fn id(&self) -> Txid {
        self.0
    }

    fn script(&self) -> ScriptBuf {
        self.1.clone()
    }
}

impl ScriptStatus {
    pub fn from_confirmations(confirmations: u32) -> Self {
        match confirmations {
            0 => Self::InMempool,
            confirmations => Self::Confirmed(Confirmed::new(confirmations - 1)),
        }
    }
}

impl ScriptStatus {
    /// Check if the script has any confirmations.
    pub fn is_confirmed(&self) -> bool {
        matches!(self, ScriptStatus::Confirmed(_))
    }

    /// Check if the script has met the given confirmation target.
    pub fn is_confirmed_with<T>(&self, target: T) -> bool
    where
        T: Into<u32>,
    {
        match self {
            ScriptStatus::Confirmed(inner) => inner.meets_target(target),
            _ => false,
        }
    }

    // Calculate the number of blocks left until the target is met.
    pub fn blocks_left_until<T>(&self, target: T) -> u32
    where
        T: Into<u32> + Copy,
    {
        match self {
            ScriptStatus::Confirmed(inner) => inner.blocks_left_until(target),
            _ => target.into(),
        }
    }

    pub fn has_been_seen(&self) -> bool {
        matches!(self, ScriptStatus::InMempool | ScriptStatus::Confirmed(_))
    }
}
