pub mod primitives;

pub use crate::primitives::{ScriptStatus, Subscription, Watchable};
use anyhow::Result;
use bdk_wallet::{export::FullyNodedExport, Balance};
use bitcoin::{Address, Amount, Network, Psbt, Txid, Weight};

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

    async fn broadcast(
        &self,
        transaction: bitcoin::Transaction,
        kind: &str,
    ) -> Result<(Txid, Subscription)>;

    async fn sync(&self) -> Result<()>;

    async fn subscribe_to(&self, tx: impl Watchable + Send + Sync + 'static) -> Subscription;

    async fn status_of_script<T>(&self, tx: &T) -> Result<ScriptStatus>
    where
        T: Watchable + Send + Sync;

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
