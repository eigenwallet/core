pub mod rpc;
mod core;
mod wallet;

pub use core::*;
pub use wallet::*;

pub mod primitives;

pub use crate::primitives::{ScriptStatus, Subscription, Watchable};
use anyhow::Result;
use bdk_wallet::{export::FullyNodedExport, Balance};
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

    async fn broadcast(
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

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rpc_inventory_registration() {
        // This brings the inventory and struct into scope
        use crate::rpc::RpcHandler;
        
        println!("\n\n------------- RPC REGISTRY PROOF -------------");
        println!("Scanning inventory for tagged functions...");
        
        let mut count = 0;
        for handler in inventory::iter::<RpcHandler> {
            println!("FOUND HANDLER:");
            println!("  Function Name: {}", handler.name);
            println!("  Arguments:     {}", handler.args);
            println!("  Return Type:   {}", handler.return_type);
            println!("----------------------------------------------");
            count += 1;
        }
        
        println!("Total handlers found: {}", count);
        println!("----------------------------------------------\n\n");
        
        // Assert that we actually found the ones we added
        assert!(count > 0, "No RPC handlers were registered! The macro failed silently.");
    }
}
