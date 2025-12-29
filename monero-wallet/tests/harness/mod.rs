use anyhow::{Context, Result};
use monero_harness::{image, Monero};
use monero_sys::Daemon;
use monero_wallet::Wallets;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use testcontainers::clients::Cli;
use testcontainers::{Container, RunnableImage};

pub const WALLET_NAME: &str = "test_wallet";

pub struct TestContext {
    pub monero: Monero,
    pub wallet_dir: TempDir,
    pub daemon: Daemon,
}

impl TestContext {
    pub async fn new<'a>(cli: &'a Cli) -> Result<(Self, Container<'a, image::Monerod>)> {
        // start monero daemon
        let (monero, monerod_container, _) = Monero::new(cli, vec![WALLET_NAME]).await?;

        let monerod_port = monerod_container
            .ports()
            .map_to_host_port_ipv4(image::RPC_PORT)
            .context("rpc port should be mapped to some external port")?;

        let daemon = Daemon {
            hostname: "127.0.0.1".to_string(),
            port: monerod_port,
            ssl: false,
        };

        // create wallet dir
        let wallet_dir = TempDir::new()?;

        Ok((
            Self {
                monero,
                wallet_dir,
                daemon,
            },
            monerod_container,
        ))
    }

    pub async fn create_wallets(&self) -> Result<Wallets> {
        // create wallets
        Wallets::new(
            self.wallet_dir.path().to_path_buf(),
            WALLET_NAME.to_string(),
            self.daemon.clone(),
            monero::Network::Mainnet,
            true,
            None,
            None,
        )
        .await
    }
}

/// setup test environment for monero wallet
pub async fn setup_test<F, Fut>(test: F)
where
    F: FnOnce(TestContext) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,monero_wallet=debug,monero_sys=debug")
        .with_test_writer()
        .try_init();

    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::default();
    let (context, _container) = TestContext::new(&cli).await.unwrap();

    context.monero.init_miner().await.unwrap();
    context.monero.start_miner().await.unwrap();

    test(context).await.unwrap();
}
