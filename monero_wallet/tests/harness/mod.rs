#![allow(dead_code)]

use anyhow::{Context, Result};
use monero_address::Network;
use monero_harness::{image, Monero};
use monero_sys::{Daemon, WalletHandle};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Once, OnceLock};
use tempfile::TempDir;
use testcontainers::clients::Cli;
use testcontainers::Container;
use tokio::time::{sleep, Duration};

pub const WALLET_NAME: &str = "test_wallet";
pub const SETTLE_DURATION: Duration = Duration::from_secs(5);
pub const CONFIRM_BLOCKS: usize = 3;

static INIT_RUSTLS: Once = Once::new();
static INIT_TRACING: Once = Once::new();
static CLI: OnceLock<Cli> = OnceLock::new();

pub fn docker_client() -> &'static Cli {
    CLI.get_or_init(Cli::default)
}

fn init_rustls() {
    INIT_RUSTLS.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("failed to install rustls ring crypto provider");
    });
}

fn init_tracing() {
    INIT_TRACING.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("info,monero_wallet=debug,monero_sys=debug")
            .with_test_writer()
            .try_init();
    });
}

pub struct TestContext {
    pub monero: Monero,
    pub daemon: Daemon,
    _monerod: Container<'static, image::Monerod>,
    pub wallet_dir: TempDir,
}

impl TestContext {
    async fn new() -> Result<Self> {
        let (monero, monerod, _) = Monero::new(docker_client(), vec![WALLET_NAME])
            .await
            .context("spawning monero containers")?;

        let monerod_port = monerod
            .ports()
            .map_to_host_port_ipv4(image::RPC_PORT)
            .context("monerod RPC port should be mapped")?;

        let daemon = Daemon {
            hostname: "127.0.0.1".to_string(),
            port: monerod_port,
            ssl: false,
        };

        let wallet_dir = TempDir::new().context("creating wallet temp dir")?;

        Ok(Self {
            monero,
            daemon,
            _monerod: monerod,
            wallet_dir,
        })
    }

    pub fn wallet_path(&self) -> PathBuf {
        self.wallet_dir.path().join(WALLET_NAME)
    }
    
    pub async fn open_regtest_wallet(&self, wallet_path: PathBuf) -> Result<WalletHandle> {
        let wallet = WalletHandle::open_or_create(
            wallet_path.display().to_string(),
            self.daemon.clone(),
            Network::Mainnet,
            false,
        )
        .await
        .context("opening wallet handle")?;

        wallet.unsafe_prepare_for_regtest().await;
        Ok(wallet)
    }
    
    pub async fn generate_blocks(&self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.monero
                .generate_blocks()
                .await
                .context("generate block")?;
        }

        sleep(Duration::from_millis(150)).await;
        Ok(())
    }

    pub async fn sync_wallet(&self, wallet: &WalletHandle) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(90), async {
            wallet.wait_until_synced(monero_sys::no_listener()).await
        })
        .await
        .context("wallet sync timed out after 90 s")?
        .context("wait_until_synced returned an error")
    }

    pub async fn wait_for_unlocked_balance(
        &self,
        wallet: &WalletHandle,
        expected_pico: u64,
        timeout_secs: u64,
    ) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);

        loop {
            let unlocked = wallet
                .unlocked_balance()
                .await
                .context("reading unlocked balance")?;

            if unlocked.as_pico() >= expected_pico {
                return Ok(());
            }

            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "timed out waiting for unlocked balance ≥ {} pico (current: {})",
                    expected_pico,
                    unlocked.as_pico()
                );
            }

            sleep(Duration::from_millis(100)).await;
        }
    }
}

pub async fn setup_test<F, Fut>(test: F) -> Result<()>
where
    F: FnOnce(Arc<TestContext>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    init_rustls();
    init_tracing();

    let ctx = Arc::new(TestContext::new().await.context("spawning test context")?);
    ctx.monero.init_miner().await.context("init miner")?;
    ctx.monero.start_miner().await.context("start miner")?;

    let result = test(ctx.clone()).await;

    sleep(SETTLE_DURATION).await;

    result
}

#[macro_export]
macro_rules! integration_test {
    ($name:ident) => {
        paste::paste! {
            #[tokio::test]
            #[serial_test::serial]
            async fn $name() -> anyhow::Result<()> {
                crate::harness::setup_test([<$name _case>]).await
            }
        }
    };
}
