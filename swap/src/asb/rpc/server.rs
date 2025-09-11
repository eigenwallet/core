use crate::asb::event_loop::EventLoopService;
use crate::protocol::alice::AliceState;
use crate::protocol::{Database, State};
use crate::{bitcoin, monero};
use ::monero::PrivateKey;
use anyhow::{anyhow, Context, Result};
use jsonrpsee::server::{ServerBuilder, ServerHandle};
use jsonrpsee::types::error::ErrorCode;
use jsonrpsee::types::ErrorObjectOwned;
use std::sync::Arc;
use swap_controller_api::{
    ActiveConnectionsResponse, AsbApiServer, BitcoinBalanceResponse, BitcoinSeedResponse,
    CooperativeRedeemResponse, MoneroAddressResponse, MoneroBalanceResponse, MoneroSeedResponse,
    MultiaddressesResponse, Swap,
};
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

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

    async fn get_coop_redeem_info(
        &self,
        swap_id: Uuid,
    ) -> Result<Option<CooperativeRedeemResponse>, ErrorObjectOwned> {
        let states = self.db.get_states(swap_id).await.into_json_rpc_result()?;

        if states.is_empty() {
            return Ok(None);
        }

        states
            .into_iter()
            .find_map(|state| match state {
                // Todo: maybe also allow XmrLockTransactionSent
                State::Alice(AliceState::XmrLocked {
                    transfer_proof,
                    state3,
                    ..
                }) => Some(Some(CooperativeRedeemResponse {
                    inner: PrivateKey::from_scalar(state3.s_a),
                    lock_tx_id: transfer_proof.tx_hash().to_string(),
                    lock_tx_key: transfer_proof.tx_key(),
                })),
                _ => None,
            })
            .context("swap not cooperatively redeemable because we didn't lock the Monero")
            .into_json_rpc_result()
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
