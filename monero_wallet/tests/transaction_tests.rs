mod harness;

use anyhow::{Context, Result};
use harness::{TestContext, CONFIRM_BLOCKS};
use monero_address::{MoneroAddress};
use monero_sys::{TransactionDirection, WalletHandle};
use swap_core::monero::Amount;
use tempfile::TempDir;
use std::sync::Arc;

async fn open_wallet_with_address(
    ctx: &TestContext,
) -> Result<(WalletHandle, MoneroAddress)> {
    let wallet = ctx.open_regtest_wallet(ctx.wallet_path()).await?;
    let address = wallet.main_address().await?;
    Ok((wallet, address))
}

async fn fund_wallet(ctx: &TestContext, wallet: &WalletHandle, amount: u64) -> Result<()> {
    let address = wallet.main_address().await?;

    ctx.monero.wallet("miner")?
        .transfer(&address, amount)
        .await
        .context("funding wallet")?;

    ctx.generate_blocks(CONFIRM_BLOCKS).await?;
    ctx.sync_wallet(wallet).await?;

    Ok(())
}


integration_test!(test_receive_funds_into_wallet);

async fn test_receive_funds_into_wallet_case(ctx: Arc<TestContext>) -> Result<()> {
    let (main_wallet, _) = open_wallet_with_address(&ctx).await?;

    let amount = 1_000_000_000_000u64;
    fund_wallet(&ctx, &main_wallet, amount).await?;
    ctx.wait_for_unlocked_balance(&main_wallet, amount, 60).await?;

    let unlocked = main_wallet.unlocked_balance().await?;
    assert_eq!(unlocked.as_pico(), amount);

    Ok(())

}


integration_test!(test_records_incoming_transaction_in_history);

async fn test_records_incoming_transaction_in_history_case(ctx: Arc<TestContext>) -> Result<()> {
    let (main_wallet, _) = open_wallet_with_address(&ctx).await?;

    let amount = 1_000_000_000_000u64;
    fund_wallet(&ctx, &main_wallet, amount).await?;

    let transactions = main_wallet.history().await?;
    assert!(
        !transactions.is_empty(),
        "transaction history is empty after receiving funds"
    );

    let tx = transactions
        .iter()
        .find(|t| t.direction == TransactionDirection::In && t.amount.as_pico() == amount)
        .context("expected incoming transaction not found in history")?;

    assert_eq!(tx.direction, TransactionDirection::In);
    assert_eq!(tx.amount.as_pico(), amount);

    Ok(())
}

integration_test!(test_transfers_funds_between_wallets);

async fn test_transfers_funds_between_wallets_case(ctx: Arc<TestContext>) -> Result<()> {
    let (alice, _alice_address) = open_wallet_with_address(&ctx).await?;

    let fund_amount = 1_000_000_000_000u64;
    fund_wallet(&ctx, &alice, fund_amount).await?;
    ctx.wait_for_unlocked_balance(&alice, fund_amount, 120).await?;

    let bob_dir = TempDir::new().context("creating Bob's wallet dir")?;
    let bob = ctx
        .open_regtest_wallet(bob_dir.path().join("bob_wallet"))
        .await
        .context("creating Bob's wallet")?;

    let bob_address = bob.main_address().await?;
    let send_amount = 100_000_000_000u64;

    alice
        .transfer_single_destination(&bob_address, Amount::from_pico(send_amount))
        .await
        .context("Alice -> Bob transfer")?;

    ctx.generate_blocks(CONFIRM_BLOCKS).await?;
    ctx.sync_wallet(&bob).await?;
    ctx.wait_for_unlocked_balance(&bob, send_amount, 120).await?;

    let bob_unlocked = bob.unlocked_balance().await?;
    assert_eq!(bob_unlocked.as_pico(), send_amount);

    Ok(())
}
