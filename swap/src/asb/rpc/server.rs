use crate::asb::event_loop::EventLoopService;
use crate::{bitcoin, monero};
use anyhow::{Context, Result};
use jsonrpsee::server::{ServerBuilder, ServerHandle};
use jsonrpsee::types::ErrorObjectOwned;
use std::sync::Arc;
use swap_controller_api::{
    ActiveConnectionsResponse, AsbApiServer, BitcoinBalanceResponse, MoneroAddressResponse,
    MoneroBalanceResponse, MultiaddressesResponse,
};

pub struct RpcImpl {
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallets>,
    event_loop_service: EventLoopService,
}

#[async_trait::async_trait]
impl AsbApiServer for RpcImpl {
    async fn check_connection(&self) -> Result<(), ErrorObjectOwned> {
        Ok(())
    }

    async fn bitcoin_balance(&self) -> Result<BitcoinBalanceResponse, ErrorObjectOwned> {
        self.bitcoin_wallet
            .balance()
            .await
            .map(|balance| BitcoinBalanceResponse { balance })
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32603,
                    format!("Failed to get Bitcoin balance: {}", e),
                    None::<()>,
                )
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

    async fn multiaddresses(&self) -> Result<MultiaddressesResponse, ErrorObjectOwned> {
        match self.event_loop_service.get_multiaddresses().await {
            Ok((peer_id, addresses)) => {
                let multiaddresses = addresses
                    .iter()
                    .map(|addr| {
                        let mut addr_with_peer_id = addr.clone();
                        addr_with_peer_id.push(libp2p::multiaddr::Protocol::P2p(peer_id));
                        addr_with_peer_id.to_string()
                    })
                    .collect();
                Ok(MultiaddressesResponse { multiaddresses })
            }
            Err(e) => Err(ErrorObjectOwned::owned(
                -32603,
                format!("Failed to get multiaddresses: {}", e),
                None::<()>,
            )),
        }
    }

    async fn active_connections(&self) -> Result<ActiveConnectionsResponse, ErrorObjectOwned> {
        match self.event_loop_service.get_active_connections().await {
            Ok(connections) => Ok(ActiveConnectionsResponse { connections }),
            Err(e) => Err(ErrorObjectOwned::owned(
                -32603,
                format!("Failed to get active connections: {}", e),
                None::<()>,
            )),
        }
    }
}

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
        };
        let handle = server.start(rpc_impl.into_rpc());

        tracing::info!("JSON-RPC server listening on {}", addr);

        Ok(Self { handle })
    }

    pub fn spawn(self) {
        tokio::spawn(async move {
            self.handle.stopped().await;
        });
    }
}
