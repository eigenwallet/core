//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::protocol::Database;
use crate::{asb, bitcoin, monero};
use rust_decimal::Decimal;
use std::sync::Arc;
use swap_env::env::Config;
use uuid::Uuid;

pub use self::state::*;
pub use self::swap::{run, run_until};

pub mod state;
pub mod swap;

pub struct Swap {
    pub state: AliceState,
    pub event_loop_handle: asb::EventLoopHandle,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallets>,
    pub env_config: Config,
    pub developer_tip: TipConfig,
    pub swap_id: Uuid,
    pub db: Arc<dyn Database + Send + Sync>,
}

/// Configures how much the and where the user wants to send tips to
///
/// The ratio is a number between 0 and 1
///
/// ratio = 0 means that no tip will be sent
/// ratio = 0.5 means that for a swap of 1 XMR, 0.5 XMR will be tipped
#[derive(Clone)]
pub struct TipConfig {
    pub ratio: Decimal,
    pub address: ::monero::Address,
}
