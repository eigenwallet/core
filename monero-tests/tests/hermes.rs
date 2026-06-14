use std::time::Duration;

use anyhow::Context;
use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
use monero_harness::{Cli, Monero};
use monero_oxide_wallet::{
    OutputWithDecoys, Scanner, ViewPair,
    address::Network,
    ed25519::{Point, Scalar},
    interface::prelude::*,
    ringct::RctType,
    send::{Change, SignableTransaction},
};
use monero_wallet_ng::hermes::HermesMessage;
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

const HERMES_TEST_MESSAGE: &[u8] = b"hermes happy path message from bob to alice";

fn keypair() -> (Zeroizing<Scalar>, Point) {
    let secret = Zeroizing::new(Scalar::random(&mut OsRng));
    let public = Point::from(&(*secret).into() * ED25519_BASEPOINT_TABLE);

    (secret, public)
}

/// Bob sends a Hermes message to the shared wallet, Alice discovers and decrypts it
/// by scanning the chain with the shared wallet's view key.
#[tokio::test]
async fn hermes_happy_path() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,monero_harness=debug,monero_wallet_ng=trace")
        .init();

    let cli = Cli::default();
    let (monero, _monerod_container, _wallet_containers) = Monero::new(&cli, vec![]).await?;
    let daemon = monero.monerod().client().clone();

    let (bob_spend, bob_spend_pub) = keypair();
    let (bob_view, _) = keypair();
    let bob_view_pair = ViewPair::new(bob_spend_pub, bob_view)?;
    let bob_address = bob_view_pair.legacy_address(Network::Mainnet);

    // Fund Bob and provide enough on-chain outputs for decoy selection.
    // The funding coinbase output unlocks after 60 confirmations.
    let funding_height = daemon.latest_block_number().await? + 1;
    daemon.generate_blocks(&bob_address, 110).await?;

    let funding_block = daemon.block_by_number(funding_height).await?;
    let funding_output = Scanner::new(bob_view_pair.clone())
        .scan(daemon.expand_to_scannable_block(funding_block).await?)?
        .ignore_additional_timelock()
        .swap_remove(0);

    // The shared wallet of which both parties know the private view key
    let (_shared_spend, shared_spend_pub) = keypair();
    let (shared_view, _) = keypair();
    let shared_view_pair = ViewPair::new(shared_spend_pub, shared_view.clone())?;
    let shared_address = shared_view_pair.legacy_address(Network::Mainnet);

    let message = HermesMessage::new(HERMES_TEST_MESSAGE.to_vec())?;
    let arbitrary_data = message.to_arbitrary_data(shared_view.clone(), &mut OsRng);

    let hardfork_version = daemon
        .block_by_number(daemon.latest_block_number().await?)
        .await?
        .header
        .hardfork_version;
    anyhow::ensure!(
        matches!(hardfork_version, 15 | 16),
        "Unexpected hardfork version {hardfork_version}"
    );

    let input = OutputWithDecoys::fingerprintable_deterministic_new(
        &mut OsRng,
        &daemon,
        16,
        daemon.latest_block_number().await?,
        funding_output,
    )
    .await?;

    let mut outgoing_view_key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(outgoing_view_key.as_mut());

    let transaction = SignableTransaction::new(
        RctType::ClsagBulletproofPlus,
        outgoing_view_key,
        vec![input],
        vec![(shared_address, 1_000_000_000)],
        Change::new(bob_view_pair, None),
        arbitrary_data,
        daemon.fee_rate(FeePriority::Unimportant, u64::MAX).await?,
    )?
    .sign(&mut OsRng, &bob_spend)?;

    let scan_start_height = daemon.latest_block_number().await?;
    daemon.publish_transaction(&transaction).await?;
    daemon.generate_blocks(&bob_address, 1).await?;

    let mut subscription = monero_wallet_ng::scanner::naive_scanner(
        daemon.clone(),
        shared_spend_pub,
        shared_view.clone(),
        scan_start_height,
        Duration::from_millis(250),
    )?;

    let output = tokio::time::timeout(
        Duration::from_secs(60),
        subscription.wait_until(|output| output.transaction() == transaction.hash()),
    )
    .await
    .context("Timed out waiting for the Hermes output")??;

    let received = HermesMessage::from_wallet_output(&output, shared_view)?;
    assert_eq!(received.as_bytes(), HERMES_TEST_MESSAGE);

    Ok(())
}
