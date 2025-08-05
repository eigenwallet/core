use anyhow::{Context, Result};
use jsonrpsee::server::{ServerBuilder, ServerHandle};
use jsonrpsee::types::ErrorObjectOwned;
use swap_controller_api::AsbApiServer;

/// RPC implementation
pub struct RpcImpl {}

#[async_trait::async_trait]
impl AsbApiServer for RpcImpl {
    async fn check_connection(&self) -> Result<(), ErrorObjectOwned> {
        Ok(())
    }
}

/// RPC Server wrapper
pub struct RpcServer {
    handle: ServerHandle,
}

impl RpcServer {
    pub async fn start(rpc_port: u16) -> Result<Self> {
        let server = ServerBuilder::default()
            .build(format!("127.0.0.1:{}", rpc_port))
            .await
            .context("Failed to build RPC server")?;

        let addr = server.local_addr()?;

        let rpc_impl = RpcImpl {};
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
