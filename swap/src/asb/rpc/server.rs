use crate::asb::event_loop::EventLoopService;
use crate::protocol::Database;
use crate::{bitcoin, monero};
use swap_feed::KrakenRate;
use anyhow::{Context, Result};
use jsonrpsee::server::{ServerBuilder, ServerHandle};
use jsonrpsee::types::error::ErrorCode;
use jsonrpsee::types::ErrorObjectOwned;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use swap_env::config::Config;
use swap_controller_api::{
    ActiveConnectionsResponse, AsbApiServer, BitcoinBalanceResponse, BitcoinSeedResponse,
    MoneroAddressResponse, MoneroBalanceResponse, MoneroSeedResponse, 
    MultiaddressesResponse, SetSpreadRequest, SetSpreadResponse, SpreadResponse, Swap,
};
use tokio_util::task::AbortOnDropHandle;

pub struct RpcServer {
    handle: ServerHandle,
}

impl RpcServer {
    pub async fn start(
        host: String,
        port: u16,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallets>,
        event_loop_service: EventLoopService,
        db: Arc<dyn Database + Send + Sync>,
        config_path: PathBuf,
        config: Arc<RwLock<Config>>,
        kraken_rate: Arc<KrakenRate>,
    ) -> Result<Self> {
        let server = ServerBuilder::default()
            .build((host, port))
            .await
            .context("Failed to build RPC server")?;

        let addr = server.local_addr()?;

        let rpc_impl = RpcImpl {
            bitcoin_wallet,
            monero_wallet,
            event_loop_service,
            db,
            config_path,
            config,
            kraken_rate,
        };
        let handle = server.start(rpc_impl.into_rpc());

        tracing::info!("JSON-RPC server listening on {}", addr);

        Ok(Self { handle })
    }

    /// Spawn the server in a new tokio task
    pub fn spawn(self) -> AbortOnDropHandle<()> {
        AbortOnDropHandle::new(tokio::spawn(async move {
            self.handle.stopped().await;
        }))
    }
}

pub struct RpcImpl {
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallets>,
    event_loop_service: EventLoopService,
    db: Arc<dyn Database + Send + Sync>,
    /// Path to the config file for persisting spread changes
    config_path: PathBuf,
    /// Thread-safe access to the current configuration
    config: Arc<RwLock<Config>>,
    /// Kraken rate service for managing spread updates
    kraken_rate: Arc<KrakenRate>,
}

#[async_trait::async_trait]
impl AsbApiServer for RpcImpl {
    async fn check_connection(&self) -> Result<(), ErrorObjectOwned> {
        Ok(())
    }

    async fn bitcoin_balance(&self) -> Result<BitcoinBalanceResponse, ErrorObjectOwned> {
        let balance = self.bitcoin_wallet.balance().await.into_json_rpc_result()?;

        Ok(BitcoinBalanceResponse { balance })
    }

    async fn bitcoin_seed(&self) -> Result<BitcoinSeedResponse, ErrorObjectOwned> {
        static EXPORT_ROLE: &str = "asb";

        let wallet_export = self
            .bitcoin_wallet
            .wallet_export(EXPORT_ROLE)
            .await
            .into_json_rpc_result()?;

        Ok(BitcoinSeedResponse {
            descriptor: format!("{}", wallet_export.descriptor()),
        })
    }

    async fn monero_balance(&self) -> Result<MoneroBalanceResponse, ErrorObjectOwned> {
        let wallet = self.monero_wallet.main_wallet().await;
        let balance = wallet.total_balance().await;

        Ok(MoneroBalanceResponse {
            balance: balance.as_pico(),
        })
    }

    async fn monero_address(&self) -> Result<MoneroAddressResponse, ErrorObjectOwned> {
        let wallet = self.monero_wallet.main_wallet().await;
        let address = wallet.main_address().await;

        Ok(MoneroAddressResponse {
            address: address.to_string(),
        })
    }

    async fn monero_seed(&self) -> Result<MoneroSeedResponse, ErrorObjectOwned> {
        let wallet = self.monero_wallet.main_wallet().await;
        let seed = wallet.seed().await.into_json_rpc_result()?;
        let restore_height = wallet.get_restore_height().await.into_json_rpc_result()?;

        Ok(MoneroSeedResponse {
            seed,
            restore_height,
        })
    }

    async fn multiaddresses(&self) -> Result<MultiaddressesResponse, ErrorObjectOwned> {
        let (_, addresses) = self
            .event_loop_service
            .get_multiaddresses()
            .await
            .into_json_rpc_result()?;

        // TODO: Concenate peer id to the multiaddresses
        let multiaddresses = addresses.iter().map(|addr| addr.to_string()).collect();

        Ok(MultiaddressesResponse { multiaddresses })
    }

    async fn active_connections(&self) -> Result<ActiveConnectionsResponse, ErrorObjectOwned> {
        let connections = self
            .event_loop_service
            .get_active_connections()
            .await
            .into_json_rpc_result()?;

        Ok(ActiveConnectionsResponse { connections })
    }

    async fn get_swaps(&self) -> Result<Vec<Swap>, ErrorObjectOwned> {
        let swaps = self.db.all().await.into_json_rpc_result()?;

        let swaps = swaps
            .into_iter()
            .map(|(swap_id, state)| {
                let state_str = match state {
                    crate::protocol::State::Alice(state) => format!("{state}"),
                    crate::protocol::State::Bob(state) => format!("{state}"),
                };

                Swap {
                    id: swap_id.to_string(),
                    state: state_str,
                }
            })
            .collect();

        Ok(swaps)
    }

    async fn get_spread(&self) -> Result<SpreadResponse, ErrorObjectOwned> {
        let current_spread = self.kraken_rate.get_spread().await
            .map_err(|e| ErrorObjectOwned::owned(
                ErrorCode::InternalError.code(),
                format!("Failed to get current spread: {}", e),
                None::<()>,
            ))?;
        
        Ok(SpreadResponse { 
            current_spread,
        })
    }

    async fn set_spread(&self, request: SetSpreadRequest) -> Result<SetSpreadResponse, ErrorObjectOwned> {
        // Validate spread is between 0 and 1 (inclusive)
        if request.spread < rust_decimal::Decimal::ZERO || request.spread > rust_decimal::Decimal::ONE {
            return Err(ErrorObjectOwned::owned(
                ErrorCode::InvalidParams.code(),
                "Spread must be between 0 and 1 (inclusive)",
                None::<()>,
            ));
        }

        // Validate spread is not negative zero (edge case)
        if request.spread.is_zero() && request.spread.is_sign_negative() {
            return Err(ErrorObjectOwned::owned(
                ErrorCode::InvalidParams.code(),
                "Spread cannot be negative zero",
                None::<()>,
            ));
        }

        // Get current spread for potential rollback
        let old_spread = self.kraken_rate.get_spread().await
            .map_err(|e| ErrorObjectOwned::owned(
                ErrorCode::InternalError.code(),
                format!("Failed to get current spread: {}", e),
                None::<()>,
            ))?;

        // Log the spread change for audit purposes
        tracing::info!(
            old_spread = %old_spread,
            new_spread = %request.spread,
            old_spread_percent = %(old_spread * rust_decimal::Decimal::from(100)),
            new_spread_percent = %(request.spread * rust_decimal::Decimal::from(100)),
            "Updating spread"
        );

        // Step 1: Update spread in memory
        self.kraken_rate.update_spread(request.spread).await
            .map_err(|e| ErrorObjectOwned::owned(
                ErrorCode::InternalError.code(),
                format!("Failed to update spread in memory: {}", e),
                None::<()>,
            ))?;

        // Step 2: Update config struct and prepare for file write
        let updated_config = {
            let mut config = self.config.write().map_err(|_| ErrorObjectOwned::owned(
                ErrorCode::InternalError.code(),
                "Failed to acquire write lock on config",
                None::<()>,
            ))?;
            config.maker.ask_spread = request.spread;
            config.clone()
        };
        
        // Step 3: Persist changes to config file
        if let Err(e) = swap_env::config::update_config(
            self.config_path.clone(), 
            &updated_config
        ) {
            // Rollback in-memory changes if file write fails
            let _ = self.kraken_rate.update_spread(old_spread).await;
            return Err(ErrorObjectOwned::owned(
                ErrorCode::InternalError.code(),
                format!("Failed to persist spread to config file: {}", e),
                None::<()>,
            ));
        }

        // Step 4: Clear quote cache to ensure new quotes use updated spread
        if let Err(e) = self.event_loop_service.clear_quote_cache().await {
            tracing::warn!(
                error = %e,
                "Failed to clear quote cache after spread update (config already persisted)"
            );
            // Note: Config file is already written successfully, so we don't rollback
        }

        tracing::info!(
            old_spread = %old_spread,
            new_spread = %request.spread,
            "Spread successfully updated and persisted"
        );

        Ok(SetSpreadResponse {
            message: "Spread successfully updated and persisted to config file".to_string(),
            old_spread,
            new_spread: request.spread,
        })
    }

}

trait IntoJsonRpcResult<T> {
    fn into_json_rpc_result(self) -> Result<T, ErrorObjectOwned>;
}

impl<T> IntoJsonRpcResult<T> for anyhow::Result<T> {
    fn into_json_rpc_result(self) -> Result<T, ErrorObjectOwned> {
        self.map_err(|e| e.into_json_rpc_error())
    }
}

trait IntoJsonRpcError {
    fn into_json_rpc_error(self) -> ErrorObjectOwned;
}

impl IntoJsonRpcError for anyhow::Error {
    fn into_json_rpc_error(self) -> ErrorObjectOwned {
        ErrorObjectOwned::owned(
            ErrorCode::InternalError.code(),
            format!("{self:?}"),
            None::<()>,
        )
    }
}
