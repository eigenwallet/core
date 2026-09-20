//! Integration test for [`Wallets::swap_wallet_spendable`], which opens a
//! temporary wallet from a view/spend key pair, scans a single transaction
//! into it and skips syncing the rest of the blockchain.

mod harness;

use anyhow::{Context, Result};
use harness::{NETWORK, TestEnv};
use monero_address::{AddressType, MoneroAddress};
use monero_harness::Cli;
use monero_oxide_ext::{PrivateKey, PublicKey};
use swap_core::monero::primitives::{PrivateViewKey, TxHash};
use uuid::Uuid;

/// A swap wallet opened from freshly generated keys detects the lock
/// transaction sent to its address without performing a full blockchain sync.
#[tokio::test]
async fn swap_wallet_scans_single_transaction_without_syncing() -> Result<()> {
    harness::init_tracing();

    let cli = Cli::default();
    let env = TestEnv::new(&cli).await?;

    // Simulate the counterparty's view pair: fresh random keys whose address
    // receives the lock transaction.
    let mut rng = rand::thread_rng();
    let spend_key = PrivateViewKey::new_random(&mut rng).0;
    let view_key = PrivateViewKey::new_random(&mut rng);

    // Derive the address exactly like `swap_wallet_spendable` does.
    let address = MoneroAddress::new(
        NETWORK,
        AddressType::Legacy,
        PublicKey::from_private_key(&spend_key).decompress(),
        PublicKey::from_private_key(&view_key.0).decompress(),
    );

    // Lock funds to the swap address and wait for the transaction to be mined.
    let amount = 1_000_000_000u64;
    let receipt = env
        .monero
        .wallet("miner")?
        .transfer(&address, amount)
        .await
        .context("Failed to lock funds to swap address")?;
    env.mine_blocks(1).await?;

    let wallets = env.wallets("main-wallet").await?;

    let swap_id = Uuid::new_v4();
    let swap_wallet = wallets
        .swap_wallet_spendable(swap_id, spend_key, view_key, TxHash(receipt.txid.clone()))
        .await
        .context("Failed to open spendable swap wallet")?;

    // The temporary wallet was created in the wallet directory.
    assert!(
        env.wallet_dir()
            .join(format!("swap_{swap_id}_spendable"))
            .exists()
    );

    // `scan_transaction` imported the locked output even though the wallet
    // skipped syncing the blockchain.
    env.wait_for_total_balance(&swap_wallet, amount).await?;
    assert_eq!(swap_wallet.total_balance().await?.as_pico(), amount);

    // Once the lock transaction has enough confirmations the funds are
    // spendable.
    env.wait_for_unlocked_balance(&swap_wallet, amount).await?;

    Ok(())
}

/// The main wallet must be unaffected by opening a swap wallet.
#[tokio::test]
async fn main_wallet_unaffected_by_swap_wallet() -> Result<()> {
    harness::init_tracing();

    let cli = Cli::default();
    let env = TestEnv::new(&cli).await?;

    let wallets = env.wallets("main-wallet").await?;
    let main_wallet = wallets.main_wallet().await;

    let funding = 1_000_000_000u64;
    env.fund_wallet(&main_wallet, funding).await?;
    env.wait_for_unlocked_balance(&main_wallet, funding).await?;

    // Open a swap wallet receiving funds from the miner.
    let mut rng = rand::thread_rng();
    let spend_key: PrivateKey = PrivateViewKey::new_random(&mut rng).0;
    let view_key = PrivateViewKey::new_random(&mut rng);

    let address = MoneroAddress::new(
        NETWORK,
        AddressType::Legacy,
        PublicKey::from_private_key(&spend_key).decompress(),
        PublicKey::from_private_key(&view_key.0).decompress(),
    );

    let locked = 500_000_000u64;
    let receipt = env
        .monero
        .wallet("miner")?
        .transfer(&address, locked)
        .await?;
    env.mine_blocks(1).await?;

    let swap_wallet = wallets
        .swap_wallet_spendable(Uuid::new_v4(), spend_key, view_key, TxHash(receipt.txid))
        .await?;

    env.wait_for_total_balance(&swap_wallet, locked).await?;

    // The main wallet's balance did not change.
    assert_eq!(
        main_wallet.unlocked_balance().await?.as_pico(),
        funding,
        "main wallet balance changed by opening a swap wallet"
    );

    Ok(())
}
