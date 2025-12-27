use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObjectOwned;
use serde::{Deserialize, Serialize};

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
pub struct PeerIdResponse {
    pub peer_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ActiveConnectionsResponse {
    pub connections: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum RendezvousConnectionStatus {
    Connected,
    Disconnected,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum RendezvousRegistrationStatus {
    Registered,
    WillRegisterAfterDelay,
    RegisterOnceConnected,
    RequestInflight,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegistrationStatusItem {
    pub address: Option<String>,
    pub connection: RendezvousConnectionStatus,
    pub registration: RendezvousRegistrationStatus,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegistrationStatusResponse {
    pub registrations: Vec<RegistrationStatusItem>,
}

// TODO: we should not need both this and asb::SwapDetails
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Swap {
    pub swap_id: String,
    pub start_date: String,
    pub state: String,
    pub btc_lock_txid: String,
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub btc_amount: bitcoin::Amount,
    /// Monero amount in piconero
    pub xmr_amount: u64,
    /// Exchange rate: BTC per XMR (amount of BTC needed to buy 1 XMR)
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub exchange_rate: bitcoin::Amount,
    pub peer_id: String,
    pub completed: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MoneroSeedResponse {
    pub seed: String,
    pub restore_height: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetBurnOnRefundRequest {
    pub swap_id: String,
    pub burn: bool,
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
    #[method(name = "peer_id")]
    async fn peer_id(&self) -> Result<PeerIdResponse, ErrorObjectOwned>;
    #[method(name = "active_connections")]
    async fn active_connections(&self) -> Result<ActiveConnectionsResponse, ErrorObjectOwned>;
    #[method(name = "get_swaps")]
    async fn get_swaps(&self) -> Result<Vec<Swap>, ErrorObjectOwned>;
    #[method(name = "registration_status")]
    async fn registration_status(&self) -> Result<RegistrationStatusResponse, ErrorObjectOwned>;
    #[method(name = "set_burn_on_refund")]
    async fn set_burn_on_refund(
        &self,
        swap_id: String,
        burn: bool,
    ) -> Result<(), ErrorObjectOwned>;
}
