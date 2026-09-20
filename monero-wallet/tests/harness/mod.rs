//! Shared test harness for the `monero-wallet` integration tests.
//!
//! Spins up a regtest monerod in Docker via [`monero_harness`] and provides
//! helpers for creating [`Wallets`] instances, funding wallets and waiting for
//! synchronization.

// Each test binary compiles this module separately and only uses a subset of
// the helpers.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use monero_address::Network;
use monero_harness::{Cli, Monero, image};
use monero_sys::{Daemon, TxReceipt, WalletHandle};
use tempfile::TempDir;
use testcontainers::Container;

/// The network wallets are opened on. The regtest daemon accepts mainnet
/// addresses, so we use `Mainnet` (there is no `Network::Regtest`).
pub const NETWORK: Network = Network::Mainnet;

/// Number of confirmations a regular output needs to become spendable.
pub const UNLOCK_BLOCKS: u64 = 10;

/// Dockerized regtest environment: one monerod container and a funded miner
/// wallet (running on the host via `monero-sys`) used to fund test wallets.
pub struct TestEnv<'a> {
    pub cli: &'a Cli,
    pub monero: Monero,
    pub daemon: Daemon,
    wallet_dir: TempDir,
    _monerod: Container<'a, image::Monerod>,
}

impl<'a> TestEnv<'a> {
    /// Start a monerod container, mine 120 blocks to the miner wallet and start
    /// the background miner (one block per second).
    pub async fn new(cli: &'a Cli) -> Result<Self> {
        ensure_docker_available()?;

        let (monero, monerod, _) = Monero::new(cli, vec![])
            .await
            .context("Failed to start monero containers")?;

        let daemon = Daemon {
            hostname: "127.0.0.1".to_string(),
            port: monerod.get_host_port_ipv4(image::RPC_PORT),
            ssl: false,
        };

        monero.init_and_start_miner().await?;

        Ok(Self {
            cli,
            monero,
            daemon,
            wallet_dir: TempDir::new().context("Failed to create wallet directory")?,
            _monerod: monerod,
        })
    }

    /// Directory in which test wallets are stored.
    pub fn wallet_dir(&self) -> PathBuf {
        self.wallet_dir.path().to_path_buf()
    }

    /// Open (or create) a standalone regtest wallet with background syncing.
    pub async fn open_wallet(&self, name: &str) -> Result<WalletHandle> {
        let wallet = WalletHandle::open_or_create(
            self.wallet_dir().join(name).display().to_string(),
            self.daemon.clone(),
            NETWORK,
            true,
        )
        .await
        .with_context(|| format!("Failed to open or create wallet `{name}`"))?;

        wallet.unsafe_prepare_for_regtest().await;

        Ok(wallet)
    }

    /// Create a [`Wallets`] instance with `main_wallet_name` as the main wallet.
    pub async fn wallets(&self, main_wallet_name: &str) -> Result<monero_wallet::Wallets> {
        self.wallets_with(main_wallet_name, None, None).await
    }

    /// Create a [`Wallets`] instance with optional Tauri handle and wallet
    /// database.
    pub async fn wallets_with(
        &self,
        main_wallet_name: &str,
        tauri_handle: Option<monero_wallet::TauriHandle>,
        wallet_database: Option<std::sync::Arc<monero_sys::Database>>,
    ) -> Result<monero_wallet::Wallets> {
        monero_wallet::Wallets::new(
            self.wallet_dir(),
            main_wallet_name.to_string(),
            self.daemon.clone(),
            NETWORK,
            true,
            tauri_handle,
            wallet_database,
        )
        .await
        .context("Failed to create Wallets")
    }

    /// Send `amount_pico` from the miner wallet to `wallet`'s main address,
    /// mine enough blocks for the transfer to unlock and wait until `wallet`
    /// is synchronized. Returns the receipt of the funding transaction.
    pub async fn fund_wallet(&self, wallet: &WalletHandle, amount_pico: u64) -> Result<TxReceipt> {
        let address = wallet.main_address().await?;

        let receipt = self
            .monero
            .wallet("miner")?
            .transfer(&address, amount_pico)
            .await
            .context("Failed to fund wallet from miner")?;

        self.mine_blocks(UNLOCK_BLOCKS).await?;

        wallet
            .wait_until_synced(monero_sys::no_listener())
            .await
            .context("Wallet failed to sync after funding")?;

        Ok(receipt)
    }

    /// Mine `n` blocks to the miner wallet.
    pub async fn mine_blocks(&self, n: u64) -> Result<()> {
        let miner_address = self.monero.wallet("miner")?.address().await?.to_string();
        self.monero
            .monerod()
            .generate_blocks(n, &miner_address)
            .await?;
        Ok(())
    }

    /// Wait until `wallet` reports an unlocked balance of at least
    /// `expected_pico`.
    pub async fn wait_for_unlocked_balance(
        &self,
        wallet: &WalletHandle,
        expected_pico: u64,
    ) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(120);

        loop {
            let unlocked = wallet.unlocked_balance().await?.as_pico();
            if unlocked >= expected_pico {
                return Ok(());
            }

            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "Timed out waiting for unlocked balance of at least {expected_pico} piconero (currently {unlocked})"
                );
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Wait until `wallet` reports a total balance of at least
    /// `expected_pico`.
    pub async fn wait_for_total_balance(
        &self,
        wallet: &WalletHandle,
        expected_pico: u64,
    ) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(120);

        loop {
            let total = wallet.total_balance().await?.as_pico();
            if total >= expected_pico {
                return Ok(());
            }

            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "Timed out waiting for total balance of at least {expected_pico} piconero (currently {total})"
                );
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Spawn an additional, independent monerod container and return a [`Daemon`]
    /// handle for it.
    pub async fn spawn_daemon(&self) -> Result<(Daemon, Container<'a, image::Monerod>, Monero)> {
        let (monero, monerod, _) = Monero::new(self.cli, vec![])
            .await
            .context("Failed to start additional monerod")?;

        let daemon = Daemon {
            hostname: "127.0.0.1".to_string(),
            port: monerod.get_host_port_ipv4(image::RPC_PORT),
            ssl: false,
        };

        Ok((daemon, monerod, monero))
    }
}

/// Install a tracing subscriber for tests. Safe to call from every test; only
/// the first call has an effect.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            "info,monero_wallet=debug,monero_harness=debug,monero_sys=debug,testcontainers=info",
        )
        .with_test_writer()
        .try_init();
}

fn ensure_docker_available() -> Result<()> {
    let output = std::process::Command::new("docker")
        .arg("info")
        .output()
        .context("failed to execute `docker info` (is Docker installed?)")?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Docker daemon is not reachable. Start Docker and re-run the tests.\n\n`docker info` error:\n{stderr}"
    )
}
