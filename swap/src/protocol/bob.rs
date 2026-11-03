use std::sync::Arc;

use anyhow::Result;
use bitcoin_wallet::BitcoinWallet;
use std::convert::TryInto;
use uuid::Uuid;

use crate::cli::api::tauri_bindings::TauriHandle;
use crate::monero::MoneroAddressPool;
use crate::protocol::Database;
use crate::{cli, monero};

use swap_core::bitcoin;
use swap_env::env;

pub use crate::protocol::bob::swap::*;
pub use swap_machine::bob::*;

pub mod apollo;
pub(crate) mod common;
pub mod swap;

use apollo::{ApolloEncryptionKey, ApolloManagerHandle};

pub struct Swap {
    pub state: BobState,
    pub event_loop_handle: cli::SwapEventLoopHandle,
    pub db: Arc<dyn Database + Send + Sync>,
    pub bitcoin_wallet: Arc<dyn BitcoinWallet>,
    pub monero_wallet: Arc<monero::Wallets>,
    pub env_config: env::Config,
    pub id: Uuid,
    pub monero_receive_pool: MoneroAddressPool,
    pub secret_keys: SwapSecretKeys,
    pub apollo_encryption_key: ApolloEncryptionKey,
    pub event_emitter: Option<TauriHandle>,
    /// Wakes the Apollo manager. `None` means the swap runs without a manager.
    pub apollo: Option<ApolloManagerHandle>,
}

impl Swap {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Arc<dyn Database + Send + Sync>,
        id: Uuid,
        bitcoin_wallet: Arc<dyn BitcoinWallet>,
        monero_wallet: Arc<monero::Wallets>,
        env_config: env::Config,
        event_loop_handle: cli::SwapEventLoopHandle,
        monero_receive_pool: MoneroAddressPool,
        apollo_encryption_key: ApolloEncryptionKey,
        secret_keys: SwapSecretKeys,
        bitcoin_change_address: bitcoin::Address,
        btc_amount: bitcoin::Amount,
        tx_lock_fee: bitcoin::Amount,
        // `None` means the swap has no Apollo.
        tx_apollo_fee: Option<bitcoin::Amount>,
    ) -> Self {
        Self {
            state: BobState::Started {
                btc_amount,
                tx_lock_fee,
                tx_apollo_fee,
                change_address: bitcoin_change_address,
            },
            event_loop_handle,
            db,
            bitcoin_wallet,
            monero_wallet,
            env_config,
            id,
            monero_receive_pool,
            apollo_encryption_key,
            secret_keys,
            event_emitter: None,
            apollo: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn from_db(
        db: Arc<dyn Database + Send + Sync>,
        id: Uuid,
        bitcoin_wallet: Arc<dyn BitcoinWallet>,
        monero_wallet: Arc<monero::Wallets>,
        env_config: env::Config,
        event_loop_handle: cli::SwapEventLoopHandle,
        monero_receive_pool: MoneroAddressPool,
        apollo_encryption_key: ApolloEncryptionKey,
        secret_keys: SwapSecretKeys,
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
            apollo_encryption_key,
            secret_keys,
            event_emitter: None,
            apollo: None,
        })
    }

    pub fn with_event_emitter(mut self, event_emitter: Option<TauriHandle>) -> Self {
        self.event_emitter = event_emitter;
        self
    }

    pub fn with_apollo(mut self, apollo: Option<ApolloManagerHandle>) -> Self {
        self.apollo = apollo;
        self
    }
}
