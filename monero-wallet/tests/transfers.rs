//! Integration tests for receiving and transferring funds between wallets
//! managed by [`Wallets`].

mod harness;

use anyhow::{Context, Result};
use harness::TestEnv;
use monero_harness::Cli;
use monero_oxide_ext::Amount;
use monero_sys::TransactionDirection;
use swap_core::monero::primitives::TxHash;

/// A wallet opened through [`Wallets`] detects incoming funds and reports them
/// in its transaction history.
#[tokio::test]
async fn wallet_receives_funds() -> Result<()> {
    harness::init_tracing();

    let cli = Cli::default();
    let env = TestEnv::new(&cli).await?;

    let wallets = env.wallets("alice").await?;
    let alice = wallets.main_wallet().await;

    let amount = 1_000_000_000u64;
    let receipt = env.fund_wallet(&alice, amount).await?;

    // The transaction is known to the daemon the wallet is connected to.
    let tx_hash = TxHash(receipt.txid.clone());
    assert!(wallets.is_transaction_present(&tx_hash).await?);

    // The wallet sees the incoming funds (total) and, after enough
    // confirmations, can spend them (unlocked).
    env.wait_for_total_balance(&alice, amount).await?;
    env.wait_for_unlocked_balance(&alice, amount).await?;

    // The transaction shows up in the wallet's history.
    let history = alice.history().await?;
    let tx = history
        .iter()
        .find(|tx| tx.tx_hash == receipt.txid)
        .context("funding transaction not found in wallet history")?;
    assert_eq!(tx.direction, TransactionDirection::In);
    assert_eq!(tx.amount.as_pico(), amount);

    Ok(())
}

/// A funded main wallet can send funds to another wallet, which detects the
/// incoming transfer once the transaction confirms.
#[tokio::test]
async fn wallet_transfers_funds() -> Result<()> {
    harness::init_tracing();

    let cli = Cli::default();
    let env = TestEnv::new(&cli).await?;

    let wallets = env.wallets("alice").await?;
    let alice = wallets.main_wallet().await;

    // Regtest daemons report inflated fee estimates, so the funding needs a
    // generous margin over the transferred amount to cover fees.
    let funding = 100_000_000_000u64;
    env.fund_wallet(&alice, funding).await?;
    env.wait_for_unlocked_balance(&alice, funding).await?;

    let bob = env.open_wallet("bob").await?;
    let bob_address = bob.main_address().await?;

    let amount = 500_000_000u64;
    let receipt = alice
        .transfer_single_destination(&bob_address, Amount::from_pico(amount))
        .await
        .context("Alice failed to transfer to Bob")?;

    assert!(
        wallets
            .is_transaction_present(&TxHash(receipt.txid.clone()))
            .await?
    );

    env.mine_blocks(harness::UNLOCK_BLOCKS).await?;
    bob.wait_until_synced(monero_sys::no_listener()).await?;
    env.wait_for_unlocked_balance(&bob, amount).await?;

    assert_eq!(bob.unlocked_balance().await?.as_pico(), amount);

    // The outgoing transaction is in Alice's history with the correct amount.
    let history = alice.history().await?;
    let tx = history
        .iter()
        .find(|tx| tx.tx_hash == receipt.txid)
        .context("outgoing transaction not found in wallet history")?;
    assert_eq!(tx.direction, TransactionDirection::Out);
    assert_eq!(tx.amount.as_pico(), amount);

    Ok(())
}
