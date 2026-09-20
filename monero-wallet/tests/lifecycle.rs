//! Integration tests covering the wallet lifecycle functionality the
//! `monero-wallet` crate adds on top of `monero-sys`:
//!
//! - creating and opening wallets through [`Wallets`]
//! - re-opening an existing wallet
//! - restoring a wallet from its seed phrase
//! - tracking recently opened wallets via [`monero_sys::Database`]

mod harness;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use harness::{NETWORK, TestEnv};
use monero_harness::Cli;
use monero_sys::WalletHandle;

/// `Wallets::new` creates the main wallet on disk and keeps it synced.
#[tokio::test]
async fn creates_main_wallet() -> Result<()> {
    harness::init_tracing();

    let cli = Cli::default();
    let env = TestEnv::new(&cli).await?;

    let wallets = env.wallets("main-wallet").await?;
    let main_wallet = wallets.main_wallet().await;

    let address = main_wallet.main_address().await?;
    assert_eq!(address.network(), NETWORK);
    assert!(
        address.to_string().starts_with('4'),
        "unexpected mainnet address: {address}"
    );

    // The wallet was persisted to the wallet directory.
    assert!(env.wallet_dir().join("main-wallet").exists());
    assert!(env.wallet_dir().join("main-wallet.keys").exists());

    // The wallet is connected to the daemon.
    assert!(main_wallet.connected().await?);

    Ok(())
}

/// Opening a `Wallets` instance over a directory containing an existing wallet
/// must re-open that wallet rather than creating a new one.
#[tokio::test]
async fn reopens_existing_wallet() -> Result<()> {
    harness::init_tracing();

    let cli = Cli::default();
    let env = TestEnv::new(&cli).await?;

    let original_address = {
        let wallets = env.wallets("main-wallet").await?;
        wallets.main_wallet().await.main_address().await?
    };

    // Give the wallet thread a moment to release the wallet file.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let wallets = env.wallets("main-wallet").await?;
    let reopened_address = wallets.main_wallet().await.main_address().await?;

    assert_eq!(
        reopened_address, original_address,
        "re-opened wallet has a different address"
    );

    Ok(())
}

/// A wallet whose seed phrase is exported can be recovered into a fresh wallet
/// which sees the same address and, after syncing, the same funds.
#[tokio::test]
async fn restores_wallet_from_seed_phrase() -> Result<()> {
    harness::init_tracing();

    let cli = Cli::default();
    let env = TestEnv::new(&cli).await?;

    let amount = 1_000_000_000u64;

    let (seed, original_address, restore_height) = {
        let wallet = env.open_wallet("funded").await?;
        env.fund_wallet(&wallet, amount).await?;
        env.wait_for_unlocked_balance(&wallet, amount).await?;

        let seed = wallet.seed().await?;
        let restore_height = wallet.creation_height().await?;
        (seed, wallet.main_address().await?, restore_height)
    };

    assert_eq!(
        seed.split_whitespace().count(),
        25,
        "expected a 25-word seed phrase"
    );

    let restore_dir = tempfile::TempDir::new().context("Failed to create restore dir")?;
    let restore_path = restore_dir.path().join("restored").display().to_string();

    let restored = WalletHandle::open_or_create_from_seed(
        restore_path,
        seed,
        NETWORK,
        restore_height,
        true,
        env.daemon.clone(),
    )
    .await
    .context("Failed to restore wallet from seed phrase")?;
    restored.unsafe_prepare_for_regtest().await;

    assert_eq!(
        restored.main_address().await?,
        original_address,
        "restored wallet has a different address"
    );

    // A wallet restored from a seed phrase can be used as the main wallet of a
    // `Wallets` instance and still sees the funds of the original wallet.
    let wallets = monero_wallet::Wallets::new_with_existing_wallet(
        env.wallet_dir(),
        env.daemon.clone(),
        NETWORK,
        true,
        None,
        restored,
        None,
    )
    .await
    .context("Failed to create Wallets from restored wallet")?;

    let main_wallet = wallets.main_wallet().await;
    main_wallet
        .wait_until_synced(monero_sys::no_listener())
        .await?;
    env.wait_for_total_balance(&main_wallet, amount).await?;

    Ok(())
}

/// `Wallets` records main wallet access in the wallet database and reports the
/// most recently opened wallets.
#[tokio::test]
async fn tracks_recently_opened_wallets() -> Result<()> {
    harness::init_tracing();

    let cli = Cli::default();
    let env = TestEnv::new(&cli).await?;

    let db_dir = tempfile::TempDir::new().context("Failed to create database dir")?;
    let database = Arc::new(
        monero_sys::Database::new(db_dir.path().to_path_buf())
            .await
            .context("Failed to open wallet database")?,
    );

    // Without a database no recent wallets are reported.
    let wallets = env.wallets("untracked-wallet").await?;
    assert!(wallets.get_recent_wallets().await?.is_empty());
    drop(wallets);
    tokio::time::sleep(Duration::from_secs(2)).await;

    // With a database the main wallet is recorded on open.
    let wallets = env
        .wallets_with("tracked-wallet", None, Some(database.clone()))
        .await?;

    let recent = wallets.get_recent_wallets().await?;
    assert_eq!(recent.len(), 1);
    assert!(
        recent[0].ends_with("tracked-wallet"),
        "unexpected wallet path recorded: {}",
        recent[0]
    );

    // Accesses of other wallets can be recorded and are ordered most recent
    // first.
    wallets.record_wallet_access("/some/other/wallet").await?;

    let recent = wallets.get_recent_wallets().await?;
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0], "/some/other/wallet");
    assert!(recent[1].ends_with("tracked-wallet"));

    Ok(())
}
