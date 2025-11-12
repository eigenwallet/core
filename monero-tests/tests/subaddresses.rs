use anyhow::Context;
use monero_harness::Cli;

/// Create subaddresses, send funds to them, and verify per-subaddress balances.
#[tokio::test]
async fn subaddress_methods_and_balances() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            "info,test=debug,monero_harness=debug,monero_rpc=debug,monero_sys=trace,subaddresses=trace,monero_cpp=info",
        )
        .init();

    let cli = Cli::default();
    let wallets = vec!["alice"];
    // Disable background sync so we rely on tx proofs / explicit scanning
    let (monero, _container, _wallet_conainers) =
        monero_harness::Monero::new_with_sync_specified(&cli, wallets, false).await?;

    tracing::info!("Starting miner");
    monero.init_and_start_miner().await?;

    let miner = monero.wallet("miner")?;
    let alice = monero.wallet("alice")?;

    tracing::info!("Checking miner balance");
    assert!(miner.balance().await? > 0);

    // Create two subaddresses under account 0 for Alice
    tracing::info!("Creating subaddresses for Alice");
    alice.create_subaddress(0, "sa1").await?;
    alice.create_subaddress(0, "sa2").await?;

    // Fetch their addresses (indices 1 and 2; index 0 is main address)
    let alice_sa1 = alice.address_at(0, 1).await?;
    let alice_sa2 = alice.address_at(0, 2).await?;

    tracing::info!(addr1=%alice_sa1, addr2=%alice_sa2, "Created subaddresses");

    // Send funds to both subaddresses in a single transaction
    tracing::info!("Sending funds to Alice's subaddresses via sweep_multi");
    let tx_receipt = miner
        .sweep_multi(&[alice_sa1.clone(), alice_sa2.clone()], &[0.5, 0.5])
        .await?;

    assert_eq!(
        tx_receipt.tx_keys.len(),
        2,
        "Expect one tx key per non-change output"
    );

    // Mine a block to confirm the transaction and make funds visible
    monero.generate_block().await?;

    // Import tx keys so Alice scans this transaction explicitly
    let sa1_txkey = tx_receipt
        .tx_keys
        .get(&alice_sa1)
        .context("tx key not found for alice subaddress 1")?;
    let sa2_txkey = tx_receipt
        .tx_keys
        .get(&alice_sa2)
        .context("tx key not found for alice subaddress 2")?;

    tracing::info!("Importing tx keys for Alice subaddresses");
    let _ = alice
        .check_tx_key(tx_receipt.txid.clone(), *sa1_txkey)
        .await?;
    let _ = alice
        .check_tx_key(tx_receipt.txid.clone(), *sa2_txkey)
        .await?;

    // Optional: refresh to ensure any pending state is applied
    alice.refresh().await?;

    // Verify per-subaddress balances reflect received funds
    let per_sub = alice.balance_per_subaddress().await?;

    let bal_sa1 = per_sub.get(&1).copied().unwrap_or(0);
    let bal_sa2 = per_sub.get(&2).copied().unwrap_or(0);

    tracing::info!(bal_sa1=%monero::Amount::from_pico(bal_sa1), bal_sa2=%monero::Amount::from_pico(bal_sa2), "Per-subaddress balances");

    assert!(bal_sa1 > 0, "Subaddress 1 expected to have received funds");
    assert!(bal_sa2 > 0, "Subaddress 2 expected to have received funds");

    Ok(())
}
