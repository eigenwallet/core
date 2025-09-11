use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObjectOwned;
use monero::PrivateKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BitcoinBalanceResponse {
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub balance: bitcoin::Amount,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BitcoinSeedResponse {
    pub descriptor: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MoneroBalanceResponse {
    pub balance: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MoneroAddressResponse {
    pub address: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MultiaddressesResponse {
    pub multiaddresses: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ActiveConnectionsResponse {
    pub connections: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Swap {
    pub id: String,
    pub state: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MoneroSeedResponse {
    pub seed: String,
    pub restore_height: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CooperativeRedeemResponse {
    /// Actual secret key
    #[serde(with = "swap_serde::monero::private_key")]
    pub inner: PrivateKey,
    /// Monero lock tx id
    pub lock_tx_id: String,
    /// Monero lock tx key -> combined with tx id is the transfer proof
    #[serde(with = "swap_serde::monero::private_key")]
    pub lock_tx_key: PrivateKey,
}

#[rpc(client, server)]
pub trait AsbApi {
    #[method(name = "check_connection")]
    async fn check_connection(&self) -> Result<(), ErrorObjectOwned>;
    #[method(name = "bitcoin_balance")]
    async fn bitcoin_balance(&self) -> Result<BitcoinBalanceResponse, ErrorObjectOwned>;
    #[method(name = "bitcoin_seed")]
    async fn bitcoin_seed(&self) -> Result<BitcoinSeedResponse, ErrorObjectOwned>;
    #[method(name = "monero_balance")]
    async fn monero_balance(&self) -> Result<MoneroBalanceResponse, ErrorObjectOwned>;
    #[method(name = "monero_address")]
    async fn monero_address(&self) -> Result<MoneroAddressResponse, ErrorObjectOwned>;
    #[method(name = "monero_seed")]
    async fn monero_seed(&self) -> Result<MoneroSeedResponse, ErrorObjectOwned>;
    #[method(name = "multiaddresses")]
    async fn multiaddresses(&self) -> Result<MultiaddressesResponse, ErrorObjectOwned>;
    #[method(name = "active_connections")]
    async fn active_connections(&self) -> Result<ActiveConnectionsResponse, ErrorObjectOwned>;
    #[method(name = "get_swaps")]
    async fn get_swaps(&self) -> Result<Vec<Swap>, ErrorObjectOwned>;
    #[method(name = "get_coop_redeem_key")]
    async fn get_coop_redeem_info(
        &self,
        swap_id: Uuid,
    ) -> Result<Option<CooperativeRedeemResponse>, ErrorObjectOwned>;
}
