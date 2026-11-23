//! End-to-end integration test for `monero_harness::funder_wallet::FunderWallet`.

use monero_harness::Cli;
use monero_harness::funder_wallet::FunderWallet;

#[tokio::test]
async fn harness_funder_wallet_pays_exact_amounts() -> anyhow::Result<()> {
    let cli = Cli::default();
    let (monero, _monerod_container, _wallet_containers) =
        monero_harness::Monero::new_with_sync_specified(&cli, vec!["alice", "bob"], false).await?;

    monero.init_and_start_miner().await?;

    let alice = monero.wallet("alice")?;
    let alice_address = alice.address().await?;
    let bob = monero.wallet("bob")?;
    let bob_address = bob.address().await?;

    // the engine-backed wallet, created fresh with random keys
    let carol = FunderWallet::new("carol".to_owned(), monero.monerod().client().clone()).await?;

    // coinbase straight to carol, past the 60 block window
    monero
        .monerod()
        .generate_blocks(70, &carol.address().await?.to_string())
        .await?;

    // the harness refresh: the scan loop must catch up to the tip
    carol.refresh().await?;

    let balance_before = carol.balance().await?;
    assert!(balance_before > 0, "carol never saw her coinbase");

    let unlocked = carol.unlocked_balance().await?;
    assert!(unlocked > 0, "carol's coinbase never matured");

    // single destination through the harness surface
    let amount_a: u64 = 300_000_000_000; // 0.3 XMR
    let receipt = carol.transfer(&alice_address, amount_a).await?;
    assert_eq!(receipt.txid.len(), 64, "receipt txid should be a tx hash");
    assert!(receipt.height > 0);
    assert_eq!(
        receipt.tx_keys.len(),
        1,
        "one destination should get one tx key"
    );

    // wallet2 itself checks the engine's re-derived tx key: given
    // the key from the receipt, it must compute that alice received
    // exactly the amount the engine sent. this is the cross-
    // implementation proof of the whole tx-key derivation chain
    let tx_key = *receipt
        .tx_keys
        .get(&alice_address.to_string())
        .expect("alice's tx key should be in the receipt");
    let status = alice.check_tx_key(receipt.txid.clone(), tx_key).await?;
    assert_eq!(status.received.as_pico(), amount_a);
    assert!(!status.in_pool);
    assert!(status.confirmations >= 1);

    // multi destination through the harness surface
    let amount_b: u64 = 100_000_000_000; // 0.1 XMR
    let amount_c: u64 = 200_000_000_000; // 0.2 XMR
    let receipt = carol
        .transfer_multi(&[(alice_address, amount_b), (bob_address, amount_c)])
        .await?;
    assert_eq!(receipt.txid.len(), 64);
    assert_eq!(
        receipt.tx_keys.len(),
        2,
        "two destinations should get two tx keys"
    );

    alice.refresh().await?;
    bob.refresh().await?;

    // the gate: exact amounts out of coinbase, through the harness api
    assert_eq!(alice.balance().await?, amount_a + amount_b);
    assert_eq!(bob.balance().await?, amount_c);

    // carol's books: everything received minus what left. the fees
    // are positive so the bound is strict
    carol.refresh().await?;
    let balance_after = carol.balance().await?;
    assert!(
        balance_after < balance_before - (amount_a + amount_b + amount_c),
        "carol's balance should have dropped by destinations plus fees"
    );

    Ok(())
}
