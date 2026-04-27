//! End-to-end integration test for `monero_wallet_ng::sweep::construct_sweep_tx_to`.

use std::time::Duration;

use monero_harness::Cli;
use monero_interface::PublishTransaction;
use monero_oxide_wallet::address::{AddressType, MoneroAddress, Network};
use monero_oxide_wallet::ed25519::Scalar;
use monero_wallet_ng::sweep;
use monero_wallet_ng::util::public_key;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

/// Random view pair for a legacy mainnet address
fn random_view_pair() -> (Zeroizing<Scalar>, Zeroizing<Scalar>, MoneroAddress) {
    let spend = Zeroizing::new(Scalar::random(&mut OsRng));
    let view = Zeroizing::new(Scalar::random(&mut OsRng));
    let address = MoneroAddress::new(
        Network::Mainnet,
        AddressType::Legacy,
        public_key(&spend),
        public_key(&view),
    );
    (spend, view, address)
}

#[tokio::test]
async fn sweep_moves_largest_output_to_destination() -> anyhow::Result<()> {
    let cli = Cli::default();
    let (monero, _monerod_container, _wallet_containers) =
        monero_harness::Monero::new_with_sync_specified(&cli, vec!["destination"], false).await?;

    monero.init_and_start_miner().await?;

    let miner = monero.wallet("miner")?;
    let miner_address = miner.address().await?.to_string();
    let destination = monero.wallet("destination")?;
    let dest_address = destination.address().await?;

    // Source wallet exists only as keys; we hand them directly to the sweep API.
    let (source_spend, source_view, source_address) = random_view_pair();

    // Fund the source address and mine past the 10-confirmation RingCT unlock
    let funding_amount: u64 = 1_000_000_000_000; // 1 XMR
    let receipt = miner.transfer(&source_address, funding_amount).await?;
    monero.monerod().generate_blocks(15, &miner_address).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let funding_txid: [u8; 32] = hex::decode(&receipt.txid)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("txid must be 32 bytes"))?;

    let daemon = monero.monerod().client().clone();
    let signed = sweep::construct_sweep_tx_to(
        daemon.clone(),
        source_spend,
        source_view,
        funding_txid,
        vec![(dest_address, 1.0)],
    )
    .await?;
    daemon.publish_transaction(&signed).await?;

    monero.monerod().generate_blocks(15, &miner_address).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    destination.refresh().await?;
    let dest_balance = destination.balance().await?;

    // Assert we receive the full amount within a fee envelope.
    let max_fee: u64 = 10_000_000_000; // 0.01 XMR
    assert!(
        dest_balance > funding_amount - max_fee && dest_balance <= funding_amount,
        "destination balance {} outside expected range (funded {})",
        dest_balance,
        funding_amount
    );

    Ok(())
}

#[tokio::test]
async fn sweep_splits_output_across_multiple_destinations() -> anyhow::Result<()> {
    let cli = Cli::default();
    let (monero, _monerod_container, _wallet_containers) =
        monero_harness::Monero::new_with_sync_specified(
            &cli,
            vec!["destination_a", "destination_b"],
            false,
        )
        .await?;

    monero.init_and_start_miner().await?;

    let miner = monero.wallet("miner")?;
    let miner_address = miner.address().await?.to_string();
    let dest_a = monero.wallet("destination_a")?;
    let dest_b = monero.wallet("destination_b")?;
    let dest_a_address = dest_a.address().await?;
    let dest_b_address = dest_b.address().await?;

    let (source_spend, source_view, source_address) = random_view_pair();

    let funding_amount: u64 = 1_000_000_000_000; // 1 XMR
    let receipt = miner.transfer(&source_address, funding_amount).await?;
    monero.monerod().generate_blocks(15, &miner_address).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let funding_txid: [u8; 32] = hex::decode(&receipt.txid)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("txid must be 32 bytes"))?;

    // 70/30 split between two destinations.
    let ratio_a = 0.7;
    let ratio_b = 0.3;

    let daemon = monero.monerod().client().clone();
    let signed = sweep::construct_sweep_tx_to(
        daemon.clone(),
        source_spend,
        source_view,
        funding_txid,
        vec![(dest_a_address, ratio_a), (dest_b_address, ratio_b)],
    )
    .await?;
    daemon.publish_transaction(&signed).await?;

    monero.monerod().generate_blocks(15, &miner_address).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    dest_a.refresh().await?;
    dest_b.refresh().await?;
    let balance_a = dest_a.balance().await?;
    let balance_b = dest_b.balance().await?;

    // Each destination should receive approximately its ratio of `funding_amount`.
    // We allow tolerance to absorb regtest fees and distribute() rounding.
    let tolerance: u64 = 10_000_000_000; // 0.01 XMR
    let expected_a = (funding_amount as f64 * ratio_a) as u64;
    let expected_b = (funding_amount as f64 * ratio_b) as u64;

    assert!(
        balance_a.abs_diff(expected_a) < tolerance,
        "destination_a balance {} not close to expected {} (ratio {})",
        balance_a,
        expected_a,
        ratio_a,
    );
    assert!(
        balance_b.abs_diff(expected_b) < tolerance,
        "destination_b balance {} not close to expected {} (ratio {})",
        balance_b,
        expected_b,
        ratio_b,
    );

    Ok(())
}
