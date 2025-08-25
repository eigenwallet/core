//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
pub use crate::protocol::alice::swap::*;
use crate::protocol::Database;
use crate::{asb, monero};
use std::sync::Arc;
use swap_env::env::Config;
pub use swap_machine::alice::*;
use uuid::Uuid;

pub mod swap;

pub struct Swap {
    pub state: AliceState,
    pub event_loop_handle: asb::EventLoopHandle,
    pub bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallets>,
    pub env_config: Config,
    pub swap_id: Uuid,
    pub db: Arc<dyn Database + Send + Sync>,
}
