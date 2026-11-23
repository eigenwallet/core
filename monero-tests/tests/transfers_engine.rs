//! The `transfers` / `transaction_keys` suite semantics behind the
//! oxide engine: the sender is a `FunderWallet`, the receivers are
//! the regular wallet2 harness wallets, and the transaction proofs
//! must carry — a right key reveals the exact amount, a wrong key
//! reveals nothing.

use anyhow::Context;
use monero_harness::Cli;
use monero_harness::funder_wallet::FunderWallet;

/// Engine-paid transfer with proofs both receivers can verify, and a
/// bogus key that must reveal nothing.
#[tokio::test]
async fn engine_transfers_with_proofs() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,test=debug,monero_harness=debug,monero_wallet_ng=trace")
        .init();

    let cli = Cli::default();
    // background sync off for the receivers: they have to discover
    // the payments through the transfer proofs, like the wallet2
    // version of this test
    let (monero, _container, _wallet_containers) =
        monero_harness::Monero::new_with_sync_specified(&cli, vec!["alice", "bob"], false).await?;

    monero.init_and_start_miner().await?;

    let alice = monero.wallet("alice")?;
    let bob = monero.wallet("bob")?;

    // the sender is the engine, funded with coinbase
    let carol = FunderWallet::new("carol".to_owned(), monero.monerod().client().clone()).await?;
    monero
        .monerod()
        .generate_blocks(70, &carol.address().await?.to_string())
        .await?;
    carol.refresh().await?;
    assert!(carol.balance().await? > 0, "carol never saw her coinbase");

    tracing::info!("Sending money");

    let amount = 1_000_000_000;
    let tx_receipt = carol
        .transfer_multi(&[
            (alice.address().await?, amount),
            (bob.address().await?, amount),
        ])
        .await?;

    assert_eq!(
        tx_receipt.tx_keys.len(),
        2,
        "Expect one tx key for each non-change output"
    );

    monero.generate_blocks().await?;

    let alice_txkey = tx_receipt
        .tx_keys
        .get(&alice.address().await?.to_string())
        .context("tx key not found for alice")?;

    let bob_txkey = tx_receipt
        .tx_keys
        .get(&bob.address().await?.to_string())
        .context("tx key not found for bob")?;

    tracing::info!("Checking a wrong key reveals nothing");

    // a bogus key must not decrypt anything. checked FIRST, on a
    // wallet that has not imported the tx yet, exactly like the
    // wallet2 wrong-key test: a right-key check imports the
    // transaction into the receiving wallet, and the bogus key must
    // stand on its own before that
    let wrong_key = monero_oxide_ext::PrivateKey::from_slice(&[
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ])
    .unwrap();

    let wrong_status = alice
        .check_tx_key(tx_receipt.txid.clone(), wrong_key)
        .await?;
    assert_eq!(
        wrong_status.received.as_pico(),
        0,
        "a bogus key must reveal nothing"
    );

    tracing::info!("Checking tx keys");

    // the right keys reveal the exact amounts, in a block, confirmed
    let alice_status = alice
        .check_tx_key(tx_receipt.txid.clone(), *alice_txkey)
        .await?;
    assert_eq!(
        alice_status.received.as_pico(),
        amount,
        "alice's key must reveal exactly what was sent"
    );
    assert!(!alice_status.in_pool);

    let bob_status = bob
        .check_tx_key(tx_receipt.txid.clone(), *bob_txkey)
        .await?;
    assert_eq!(
        bob_status.received.as_pico(),
        amount,
        "bob's key must reveal exactly what was sent"
    );

    // and the payments actually landed
    alice.refresh().await?;
    bob.refresh().await?;
    assert_eq!(
        alice.balance().await?,
        amount,
        "alice should hold exactly what the engine sent"
    );
    assert_eq!(
        bob.balance().await?,
        amount,
        "bob should hold exactly what the engine sent"
    );

    Ok(())
}
