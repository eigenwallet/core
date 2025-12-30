mod harness;

use anyhow::Result;
use harness::{setup_test, TestContext, WALLET_NAME};
use monero::Network;
use monero_sys::WalletHandle;
use monero_wallet::Wallets;
use serial_test::serial;
use std::time::Duration;

#[tokio::test]
#[serial]
async fn test_receive_funds() -> Result<()> {
    setup_test(|context| async move {
        let wallets = context.create_wallets().await?;
        let main_wallet = wallets.main_wallet().await;
        let address = main_wallet.main_address().await?;

        // Receive funds
        let miner_wallet = context.monero.wallet("miner")?;
        let amount = 1_000_000_000_000; // 1 XMR
        miner_wallet.transfer(&address, amount).await?;

        context.monero.generate_block().await?;
        context.monero.generate_block().await?;

        for _ in 0..20 {
            main_wallet
                .wait_until_synced(monero_sys::no_listener())
                .await?;
            let b = main_wallet.unlocked_balance().await?;
            if b.as_pico() > 0 {
                break;
            }
            // Pause execution for a while before checking again
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let wallet_balance = main_wallet.unlocked_balance().await?;
        assert_eq!(wallet_balance.as_pico(), amount);

        Ok(())
    })
    .await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transfer_funds() -> Result<()> {
    setup_test(|context| async move {
        // Create Alice wallet
        let wallets_alice = context.create_wallets().await?;
        let alice_wallet = wallets_alice.main_wallet().await;
        let alice_address = alice_wallet.main_address().await?;

        // Fund Alice from Miner
        let miner_wallet = context.monero.wallet("miner")?;
        let amount = 1_000_000_000_000; // 1 XMR

        // Sending 1 XMR to Alice
        miner_wallet.transfer(&alice_address, amount).await?;

        // Generate blocks to confirm the transaction
        // We need enough blocks for the transaction to be unlocked
        // and ideally enough outputs on chain for ring signatures
        for _ in 0..20 {
            context.monero.generate_block().await?;
        }

        // Wait for sync loop
        let mut initial_balance = monero::Amount::from_pico(0);
        for _ in 0..20 {
            alice_wallet
                .wait_until_synced(monero_sys::no_listener())
                .await?;
            let b = alice_wallet.unlocked_balance().await?;
            if b.as_pico() > 0 {
                initial_balance = b;
                break;
            }
            // Pause execution for a while before checking again
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let total_balance = alice_wallet.total_balance().await?;

        // Total balance should be equal to amount
        assert_eq!(total_balance.as_pico(), amount);
        // Unlocked balance should be equal to amount
        assert_eq!(initial_balance.as_pico(), amount);

        // Create bob's wallet
        let bob_dir = tempfile::TempDir::new()?;
        let bob_name = "bob_wallet";

        let bob_wallet = WalletHandle::open_or_create(
            bob_dir.path().join(bob_name).display().to_string(),
            context.daemon.clone(),
            Network::Mainnet,
            true,
        )
        .await?;

        let bob_address = bob_wallet.main_address().await?;

        // Alice sends to Bob
        let send_amount = 100_000_000_000; // 0.1 XMR

        alice_wallet
            .transfer_single_destination(&bob_address, monero::Amount::from_pico(send_amount))
            .await?;

        // Generate blocks to confirm the transaction
        context.monero.generate_block().await?;
        context.monero.generate_block().await?;

        // Verify Bob received
        let mut bob_received = false;
        for _ in 0..20 {
            bob_wallet
                .wait_until_synced(monero_sys::no_listener())
                .await?;
            let b = bob_wallet.unlocked_balance().await?;
            if b.as_pico() == send_amount {
                bob_received = true;
                break;
            }
            // Pause execution for a while before checking again
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let bob_total = bob_wallet.total_balance().await?;

        assert!(bob_received);
        assert_eq!(bob_total.as_pico(), send_amount);

        Ok(())
    })
    .await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_history() -> Result<()> {
    setup_test(|context| async move {
        let wallets = context.create_wallets().await?;
        let main_wallet = wallets.main_wallet().await;
        let address = main_wallet.main_address().await?;

        // Receive funds
        let miner_wallet = context.monero.wallet("miner")?;
        let amount = 1_000_000_000_000; // 1 XMR
        miner_wallet.transfer(&address, amount).await?;

        context.monero.generate_block().await?;
        context.monero.generate_block().await?;

        // Polling loop for history
        let mut history_found = false;
        let mut transactions = Vec::new();
        for _ in 0..20 {
            main_wallet
                .wait_until_synced(monero_sys::no_listener())
                .await?;
            let h = main_wallet.history().await?;
            if !h.is_empty() {
                history_found = true;
                transactions = h;
                break;
            }
            // Pause execution for a while before checking again
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Assert that the history is not empty
        assert!(history_found);

        let tx = &transactions[0];

        // Assert that the transaction is an incoming transaction
        assert_eq!(tx.direction, monero_sys::TransactionDirection::In);

        // Assert that the transaction amount is correct
        assert_eq!(tx.amount.as_pico(), amount);

        Ok(())
    })
    .await;
    Ok(())
}
