use monero_harness::Cli;

/// Create a transaction with transaction proofs, and verify them.
/// Fails if the publishing fails due to the transfer keys not being extracted successfully
/// or due to them not being able to be verified by the recipients.
#[tokio::test]
async fn monero_transfers() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            "info,test=debug,monero_harness=debug,monero_rpc=debug,monero_sys=trace,transfers=trace,monero_cpp=info",
        ).init();

    let cli = Cli::default();
    let wallets = vec!["alice", "bob", "candice"];
    // Disbale background sync for these wallet -- this way we _have_ to use the transfer proof to discover the transactions.
    let (monero, _container, _wallet_conainers) =
        monero_harness::Monero::new_with_sync_specified(&cli, wallets, false).await?;

    tracing::info!("Starting miner");

    monero.init_and_start_miner().await?;

    let miner = monero.wallet("miner")?;
    let alice = monero.wallet("alice")?;
    let bob = monero.wallet("bob")?;
    let candice = monero.wallet("candice")?;

    tracing::info!("Checking miner balance");

    assert!(miner.balance().await? > 0);

    tracing::info!("Sending money");

    let proof = miner
        .sweep_multi(
            &[
                alice.address().await?,
                bob.address().await?,
                candice.address().await?,
            ],
            &[0.33333333, 0.333333333, 0.3333333333],
        )
        .await?;

    assert_eq!(
        proof.tx_keys.len(),
        3,
        "Expect one transaction key per non-change output"
    );

    alice
        .check_tx_key(
            proof.txid.clone(),
            *proof.tx_keys.get(&alice.address().await?).unwrap(),
        )
        .await?;

    assert_eq!(alice.sweep(&bob.address().await?).await?.tx_keys.len(), 1);

    Ok(())
}
