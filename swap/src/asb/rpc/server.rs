use crate::{bitcoin, monero};
use anyhow::{Context, Result};
use jsonrpsee::server::{ServerBuilder, ServerHandle};
use jsonrpsee::types::ErrorObjectOwned;
use std::sync::Arc;
use swap_controller_api::{AsbApiServer, BitcoinBalanceResponse, MoneroBalanceResponse};

/// RPC implementation
pub struct RpcImpl {
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallets>,
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
}

/// RPC Server wrapper
pub struct RpcServer {
    handle: ServerHandle,
}

impl RpcServer {
    pub async fn start(
        rpc_bind: &str,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallets>,
    ) -> Result<Self> {
        let server = ServerBuilder::default()
            .build(rpc_bind)
            .await
            .context("Failed to build RPC server")?;

        let addr = server.local_addr()?;

        let rpc_impl = RpcImpl {
            bitcoin_wallet,
            monero_wallet,
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
