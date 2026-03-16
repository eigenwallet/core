mod core;
mod wallet;

pub use core::*;
pub use wallet::*;

pub mod primitives;

pub use crate::primitives::{ScriptStatus, Subscription, Watchable};
use anyhow::Result;
use bdk_wallet::{Balance, export::FullyNodedExport};
pub use bitcoin::{Address, Amount, Network, Psbt, Txid, Weight};

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
    ) -> Result<Psbt>;

    async fn send_to_address_dynamic_fee(
        &self,
        address: Address,
        amount: Amount,
        change_override: Option<Address>,
    ) -> Result<bitcoin::psbt::Psbt>;

    async fn sweep_balance_to_address_dynamic_fee(
        &self,
        address: Address,
    ) -> Result<bitcoin::psbt::Psbt>;

    async fn sign_and_finalize(&self, psbt: bitcoin::psbt::Psbt) -> Result<bitcoin::Transaction>;

    async fn ensure_broadcasted(
        &self,
        transaction: bitcoin::Transaction,
        kind: &str,
    ) -> Result<(Txid, Subscription)>;

    async fn sync(&self) -> Result<()>;

    async fn subscribe_to(&self, tx: Box<dyn Watchable>) -> Subscription;

    async fn status_of_script(&self, tx: &dyn Watchable) -> Result<ScriptStatus>;

    async fn get_raw_transaction(
        &self,
        txid: Txid,
    ) -> Result<Option<std::sync::Arc<bitcoin::Transaction>>>;

    async fn max_giveable(&self, locking_script_size: usize) -> Result<(Amount, Amount)>;

    async fn estimate_fee(&self, weight: Weight, transfer_amount: Option<Amount>)
    -> Result<Amount>;

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
            .send_to_address_dynamic_fee(address, amount, None)
            .await?;
        (tx, amount)
    } else {
        let (max_giveable, spending_fee) =
            wallet.max_giveable(address.script_pubkey().len()).await?;
        let tx = wallet
            .send_to_address(address, max_giveable, spending_fee, None)
            .await?;
        (tx, max_giveable)
    };

    let signed_tx = wallet.sign_and_finalize(unsigned_tx).await?;
    let (txid, _subscription) = wallet.ensure_broadcasted(signed_tx, "withdraw").await?;

    Ok((txid, amount))
}
