use std::sync::Arc;

use anyhow::Result;
use uuid::Uuid;

use crate::cli::api::tauri_bindings::TauriHandle;
use crate::monero::MoneroAddressPool;
use crate::protocol::Database;
use crate::{bitcoin, cli, monero};
use swap_env::env;

pub use self::state::*;
pub use self::swap::{run, run_until};
use std::convert::TryInto;

pub mod state;
pub mod swap;

pub struct Swap {
    pub state: BobState,
    pub event_loop_handle: cli::EventLoopHandle,
    pub db: Arc<dyn Database + Send + Sync>,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallets>,
    pub env_config: env::Config,
    pub id: Uuid,
    pub monero_receive_pool: MoneroAddressPool,
    pub event_emitter: Option<TauriHandle>,
}

impl Swap {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Arc<dyn Database + Send + Sync>,
        id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallets>,
        env_config: env::Config,
        event_loop_handle: cli::EventLoopHandle,
        monero_receive_pool: MoneroAddressPool,
        bitcoin_change_address: bitcoin::Address,
        btc_amount: bitcoin::Amount,
        tx_lock_fee: bitcoin::Amount,
    ) -> Self {
        Self {
            state: BobState::Started {
                btc_amount,
                tx_lock_fee,
                change_address: bitcoin_change_address,
            },
            event_loop_handle,
            db,
            bitcoin_wallet,
            monero_wallet,
            env_config,
            id,
            monero_receive_pool,
            event_emitter: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn from_db(
        db: Arc<dyn Database + Send + Sync>,
        id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallets>,
        env_config: env::Config,
        event_loop_handle: cli::EventLoopHandle,
        monero_receive_pool: MoneroAddressPool,
    ) -> Result<Self> {
        let state = db.get_state(id).await?.try_into()?;

        Ok(Self {
            state,
            event_loop_handle,
            db,
            bitcoin_wallet,
            monero_wallet,
            env_config,
            id,
            monero_receive_pool,
            event_emitter: None,
        })
    }

    pub fn with_event_emitter(mut self, event_emitter: Option<TauriHandle>) -> Self {
        self.event_emitter = event_emitter;
        self
    }
}
