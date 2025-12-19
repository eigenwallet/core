use monero_harness::Cli;

/// Verify that checking a transaction with a wrong/random transfer key fails.
#[tokio::test]
async fn monero_transfers_wrong_key() {
    tracing_subscriber::fmt()
        .with_env_filter(
            "info,test=debug,monero_harness=debug,monero_sys=trace,transfers=trace,monero_cpp=info",
        )
        .init();

    let cli = Cli::default();
    let wallets = vec!["alice"];
    // Disable background sync for this wallet -- this way we _have_ to use the transfer proof to discover the transactions.
    let (monero, _container, _wallet_conainers) =
        monero_harness::Monero::new_with_sync_specified(&cli, wallets, false)
            .await
            .unwrap();

    tracing::info!("Starting miner");

    monero.init_and_start_miner().await.unwrap();

    let miner_wallet = monero.wallet("miner").unwrap();
    let alice = monero.wallet("alice").unwrap();

    tracing::info!("Checking miner balance");

    assert!(miner_wallet.balance().await.unwrap() > 0);

    tracing::info!("Sending money");

    let tx_receipt = miner_wallet
        .sweep(&alice.address().await.unwrap())
        .await
        .unwrap();

    assert_eq!(
        tx_receipt.tx_keys.len(),
        1,
        "Expect one tx key for the output"
    );

    monero.generate_block().await.unwrap();

    // Use a wrong private key (just a simple constant key, not the real transfer key)
    let wrong_key = monero_oxide_ext::PrivateKey::from_slice(&[
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ])
    .unwrap();

    tracing::info!("Importing tx key with wrong key - should fail");

    let status = alice
        .check_tx_key(tx_receipt.txid.clone(), wrong_key)
        .await
        .unwrap();

    // Wrong tx key -> amount is zero.
    if status.received != monero_oxide_ext::Amount::ZERO {
        panic!("could decrypt payment - this is not supposed to happen since we got a bogus key");
    }
}
