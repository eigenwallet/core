use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObjectOwned;
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
    /// Fee paid by Alice for the Bitcoin redeem transaction, in satoshis.
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub btc_redeem_fee: bitcoin::Amount,
    pub peer_id: String,
    pub completed: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WithdrawBtcResponse {
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub amount: bitcoin::Amount,
    pub txid: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MoneroSeedResponse {
    pub seed: String,
    pub restore_height: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WormholeServiceItem {
    pub peer_id: String,
    pub address: String,
    pub state: Option<String>,
    pub reachable: bool,
    pub problem: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WormholeServicesResponse {
    pub services: Vec<WormholeServiceItem>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OnionServiceStatusResponse {
    /// The high-level state (e.g. "Running", "Bootstrapping", "Broken").
    /// `None` if no onion service is registered.
    pub state: Option<String>,
    /// Whether the service is believed to be fully reachable.
    pub reachable: bool,
    /// Description of the current problem, if any.
    pub problem: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetBurnOnRefundRequest {
    pub swap_id: String,
    pub burn: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QuoteResponse {
    /// Price offered per 1 XMR, in satoshis.
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub price: bitcoin::Amount,
    /// Minimum BTC amount the maker is willing to swap, in satoshis.
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub min_quantity: bitcoin::Amount,
    /// Maximum BTC amount the maker is willing to swap, in satoshis.
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub max_quantity: bitcoin::Amount,
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
    async fn set_withhold_deposit(&self, swap_id: Uuid, burn: bool)
    -> Result<(), ErrorObjectOwned>;
    #[method(name = "grant_mercy")]
    async fn grant_mercy(&self, swap_id: Uuid) -> Result<(), ErrorObjectOwned>;
    #[method(name = "wormhole_services")]
    async fn wormhole_services(&self) -> Result<WormholeServicesResponse, ErrorObjectOwned>;
    #[method(name = "onion_service_status")]
    async fn onion_service_status(&self) -> Result<OnionServiceStatusResponse, ErrorObjectOwned>;
    #[method(name = "withdraw_btc")]
    async fn withdraw_btc(
        &self,
        address: String,
        amount: Option<u64>,
    ) -> Result<WithdrawBtcResponse, ErrorObjectOwned>;
    #[method(name = "refresh_bitcoin_wallet")]
    async fn refresh_bitcoin_wallet(&self) -> Result<(), ErrorObjectOwned>;
    #[method(name = "get_current_quote")]
    async fn get_current_quote(&self) -> Result<QuoteResponse, ErrorObjectOwned>;
}
