use anyhow::Context;
use monero_harness::Cli;

/// Create a transaction with transaction proofs, and verify them.
/// Fails if the publishing fails due to the transfer keys not being extracted successfully
/// or due to them not being able to be verified by the recipients.
#[tokio::test]
async fn monero_transfers() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            "info,test=debug,monero_harness=debug,monero_sys=trace,transfers=trace,monero_cpp=info",
        )
        .init();

    let cli = Cli::default();
    let wallets = vec!["alice", "bob"];
    // Disable background sync for these wallet -- this way we _have_ to use the transfer proof to discover the transactions.
    let (monero, _container, _wallet_conainers) =
        monero_harness::Monero::new_with_sync_specified(&cli, wallets, false).await?;

    tracing::info!("Starting miner");

    monero.init_and_start_miner().await?;

    let miner_wallet = monero.wallet("miner")?;
    let alice = monero.wallet("alice")?;
    let bob = monero.wallet("bob")?;

    tracing::info!("Checking miner balance");

    assert!(miner_wallet.balance().await? > 0);

    tracing::info!("Sending money");

    let tx_receipt = miner_wallet
        .sweep_multi(&[alice.address().await?, bob.address().await?], &[0.5, 0.5])
        .await?;

    assert_eq!(
        tx_receipt.tx_keys.len(),
        2,
        "Expect one tx key for each non-change output"
    );

    monero.generate_block().await?;

    let alice_txkey = tx_receipt
        .tx_keys
        .get(&alice.address().await?)
        .context("tx key not found for alice")?;

    let bob_txkey = tx_receipt
        .tx_keys
        .get(&bob.address().await?)
        .context("tx key not found for bob")?;

    tracing::info!("Importing tx keys");

    let alice_status = alice
        .check_tx_key(tx_receipt.txid.clone(), *alice_txkey)
        .await?;
    let bob_status = bob
        .check_tx_key(tx_receipt.txid.clone(), *bob_txkey)
        .await?;

    tracing::info!(
        ?alice_status,
        ?bob_status,
        "Successfully checked transactions keys!"
    );

    // sanity check: we should have actually received the money...

    assert!(
        alice.balance().await? > 0,
        "Alice expected to have received funds"
    );
    assert!(
        bob.balance().await? > 0,
        "Bob expected to have received funds"
    );

    Ok(())
}
