mod coin_selection;
mod core;
mod wallet;

pub use core::*;
pub use wallet::*;

pub mod primitives;

pub use crate::primitives::{ScriptStatus, Subscription, Watchable};
use anyhow::Result;
use bdk_wallet::{Balance, export::FullyNodedExport};
pub use bitcoin::{Address, Amount, Network, Psbt, Txid, Weight};

#[derive(Debug, thiserror::Error)]
pub enum PsbtPrevoutVerificationError {
    #[error("Lock input verification was cancelled")]
    Cancelled,
    #[error("Lock input verification timed out")]
    TimedOut,
    #[error("PSBT has {input_maps} input maps but {transaction_inputs} transaction inputs")]
    InputMapCountMismatch {
        transaction_inputs: usize,
        input_maps: usize,
    },
    #[error("Input {input_index} is missing its witness UTXO")]
    MissingWitnessUtxo { input_index: usize },
    #[error("Failed to query Electrum servers for unspent outputs")]
    Electrum(#[source] anyhow::Error),
    /// Every Electrum server that answered refused to list this script,
    /// e.g. because it has too many UTXOs, or was busy. Not retried within
    /// the check.
    #[error("Electrum servers refused to list the unspent outputs of script {script}: {refusal}")]
    ScriptRefused {
        script: bitcoin::ScriptBuf,
        refusal: String,
    },
    #[error("Prevout {outpoint} not found unspent on chain or in mempool")]
    PrevoutNotFound { outpoint: bitcoin::OutPoint },
    #[error("Prevout {outpoint} has value {actual} but the PSBT claims {claimed}")]
    ValueMismatch {
        outpoint: bitcoin::OutPoint,
        actual: Amount,
        claimed: Amount,
    },
}

impl PsbtPrevoutVerificationError {
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Electrum(_))
    }
}

#[async_trait::async_trait]
pub trait VerifyPsbtPrevouts: Send + Sync {
    /// Verify that every input of the PSBT references an existing unspent
    /// output (confirmed or in mempool) whose script and value match the
    /// claimed `witness_utxo`.
    async fn verify_psbt_prevouts(
        &self,
        psbt: &bitcoin::psbt::Psbt,
        cancel: &tokio::sync::watch::Receiver<bool>,
    ) -> std::result::Result<(), PsbtPrevoutVerificationError>;
}

#[async_trait::async_trait]
pub trait BitcoinWallet: Send + Sync {
    async fn balance(&self) -> Result<Amount>;

    async fn balance_info(&self) -> Result<Balance>;

    async fn new_address(&self) -> Result<Address>;

    async fn send_to_address(
        &self,
        address: Address,
        amount: Amount,
        spending_fee: Amount,
        change_override: Option<Address>,
        max_inputs: Option<usize>,
    ) -> Result<Psbt>;

    async fn send_to_address_dynamic_fee(
        &self,
        address: Address,
        amount: Amount,
        change_override: Option<Address>,
        max_inputs: Option<usize>,
    ) -> Result<bitcoin::psbt::Psbt>;

    async fn sweep_balance_to_address_dynamic_fee(
        &self,
        address: Address,
    ) -> Result<bitcoin::psbt::Psbt>;

    async fn sign_and_finalize(&self, psbt: bitcoin::psbt::Psbt) -> Result<bitcoin::Transaction>;

    /// Prove control of the keys behind every input of an unsigned PSBT owned
    /// by this wallet.
    fn prove_utxo_ownership(
        &self,
        psbt: &bitcoin::psbt::Psbt,
        challenge: utxo_ownership_proof::Challenge,
    ) -> Result<Vec<utxo_ownership_proof::OwnershipProof>>;

    async fn ensure_broadcasted(
        &self,
        transaction: bitcoin::Transaction,
        kind: &str,
    ) -> Result<(Txid, Subscription)>;

    async fn sync(&self) -> Result<()>;

    async fn health_check(&self) -> Result<()>;

    async fn subscribe_to(&self, tx: Box<dyn Watchable>) -> Subscription;

    async fn status_of_script(&self, tx: &dyn Watchable) -> Result<ScriptStatus>;

    async fn get_raw_transaction(
        &self,
        txid: Txid,
    ) -> Result<Option<std::sync::Arc<bitcoin::Transaction>>>;

    async fn max_giveable(
        &self,
        locking_script_size: usize,
        max_inputs: Option<usize>,
    ) -> Result<(Amount, Amount)>;

    async fn estimate_fee(&self, weight: Weight, transfer_amount: Option<Amount>)
    -> Result<Amount>;

    /// Uncapped rate for presigned swap transactions.
    async fn swap_fee_rate(&self) -> Result<bitcoin::FeeRate>;

    fn network(&self) -> Network;

    fn finality_confirmations(&self) -> u32;

    async fn wallet_export(&self, role: &str) -> Result<FullyNodedExport>;
}

/// Withdraw BTC to the given address. If `amount` is `None`, sweeps the entire balance.
pub async fn withdraw(
    wallet: &dyn BitcoinWallet,
    address: Address,
    amount: Option<Amount>,
) -> Result<(Txid, Amount)> {
    let (unsigned_tx, amount) = if let Some(amount) = amount {
        let tx = wallet
            .send_to_address_dynamic_fee(address, amount, None, None)
            .await?;
        (tx, amount)
    } else {
        let (max_giveable, spending_fee) = wallet
            .max_giveable(address.script_pubkey().len(), None)
            .await?;
        let tx = wallet
            .send_to_address(address, max_giveable, spending_fee, None, None)
            .await?;
        (tx, max_giveable)
    };

    let signed_tx = wallet.sign_and_finalize(unsigned_tx).await?;
    let (txid, _subscription) = wallet.ensure_broadcasted(signed_tx, "withdraw").await?;

    Ok((txid, amount))
}
