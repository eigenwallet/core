use monero_harness::Cli;
use monero_sys::WalletHandle;
use testcontainers::Container;

type TestSetup<'a> = (
    WalletHandle,
    u64,
    monero::Address,
    WalletHandle,
    Container<'a, monero_harness::image::Monerod>,
    Vec<Container<'a, monero_harness::image::MoneroWalletRpc>>,
);

async fn setup(cli: &Cli) -> anyhow::Result<TestSetup<'_>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            "info,test=debug,monero_harness=debug,monero_sys=trace,reserve_proof=trace",
        )
        .try_init()
        .ok();

    let wallets = vec!["alice", "bob"];
    let (monero, container, wallet_containers) =
        monero_harness::Monero::new_with_sync_specified(cli, wallets, false).await?;

    monero.init_and_start_miner().await?;

    let miner = monero.wallet("miner")?;
    let alice = monero.wallet("alice")?;
    let bob = monero.wallet("bob")?;

    // Fund alice's wallet
    miner.sweep(&alice.address().await?).await?;
    monero.generate_block().await?;
    alice.refresh().await?;

    let alice_balance = alice.balance().await?;
    assert!(alice_balance > 0, "Alice should have received funds");

    let alice_address = alice.address().await?;
    let alice_wallet = alice.wallet().clone();
    let bob_wallet = bob.wallet().clone();

    Ok((
        alice_wallet,
        alice_balance,
        alice_address,
        bob_wallet,
        container,
        wallet_containers,
    ))
}

#[tokio::test]
async fn reserve_proof_full_balance() -> anyhow::Result<()> {
    let cli = Cli::default();
    let (alice_wallet, _, alice_address, bob_wallet, _c, _wc) = setup(&cli).await?;

    let message = "test reserve proof";

    // Alice generates a reserve proof for the full balance
    let proof = alice_wallet.get_reserve_proof(0, None, message).await?;

    assert!(!proof.is_empty(), "Proof should not be empty");

    // Bob verifies Alice's proof
    let status = bob_wallet
        .check_reserve_proof(&alice_address, message, &proof)
        .await?;

    assert!(status.good, "Reserve proof should be valid");
    assert!(status.total.as_pico() > 0, "Total should be non-zero");

    Ok(())
}

#[tokio::test]
async fn reserve_proof_specific_amount() -> anyhow::Result<()> {
    let cli = Cli::default();
    let (alice_wallet, alice_balance, alice_address, bob_wallet, _c, _wc) = setup(&cli).await?;

    let message = "proving specific amount";

    // Alice generates a reserve proof for a specific amount (half of balance)
    let prove_amount = monero::Amount::from_pico(alice_balance / 2);
    let proof = alice_wallet
        .get_reserve_proof(0, Some(prove_amount), message)
        .await?;

    assert!(!proof.is_empty(), "Proof should not be empty");

    // Bob verifies Alice's proof
    let status = bob_wallet
        .check_reserve_proof(&alice_address, message, &proof)
        .await?;

    assert!(status.good, "Reserve proof should be valid");
    assert!(
        status.total >= prove_amount,
        "Total should be at least the proven amount"
    );

    Ok(())
}

#[tokio::test]
async fn reserve_proof_wrong_message() -> anyhow::Result<()> {
    let cli = Cli::default();
    let (alice_wallet, _, alice_address, bob_wallet, _c, _wc) = setup(&cli).await?;

    // Alice generates proof with one message
    let proof = alice_wallet
        .get_reserve_proof(0, None, "original message")
        .await?;

    // Bob verifies with a different message - should fail
    let status = bob_wallet
        .check_reserve_proof(&alice_address, "wrong message", &proof)
        .await?;

    assert!(
        !status.good,
        "Reserve proof should be invalid with wrong message"
    );

    Ok(())
}

#[tokio::test]
async fn reserve_proof_invalid_signature() -> anyhow::Result<()> {
    let cli = Cli::default();
    let (_, _, alice_address, bob_wallet, _c, _wc) = setup(&cli).await?;

    // Bob tries to verify a garbage proof string
    let result = bob_wallet
        .check_reserve_proof(&alice_address, "some message", "not_a_valid_proof_string")
        .await;

    assert!(
        result.is_err() || !result.unwrap().good,
        "Invalid proof should fail verification"
    );

    Ok(())
}
