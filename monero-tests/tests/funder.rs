//! End-to-end integration test for `monero_wallet_ng::funder::Funder`.

use std::time::Duration;

use monero_harness::Cli;
use monero_oxide_wallet::address::{AddressType, MoneroAddress, Network};
use monero_oxide_wallet::ed25519::Scalar;
use monero_wallet_ng::funder::Funder;
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
async fn funder_pays_exact_amounts_from_coinbase() -> anyhow::Result<()> {
    let cli = Cli::default();
    let (monero, _monerod_container, _wallet_containers) =
        monero_harness::Monero::new_with_sync_specified(&cli, vec!["alice", "bob"], false).await?;

    monero.init_and_start_miner().await?;

    let miner = monero.wallet("miner")?;
    let miner_address = miner.address().await?.to_string();
    let alice = monero.wallet("alice")?;
    let alice_address = alice.address().await?;
    let bob = monero.wallet("bob")?;
    let bob_address = bob.address().await?;

    // the funder exists only as keys plus a scan loop
    let (spend, view, funder_address) = random_view_pair();
    let daemon = monero.monerod().client().clone();

    let funder = Funder::new(daemon.clone(), spend, view, 0)?;

    // mine coinbase straight to the funder, past the 60 block
    // maturity window. coinbase outputs arrive at the scanner with
    // their consensus unlock time, so the whole coinbase path runs
    // through the engine: scan, timelock, unlock, selection.
    monero
        .monerod()
        .generate_blocks(70, &funder_address.to_string())
        .await?;

    // wait for the scan loop to catch up and the early coinbase to
    // mature
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let unlocked = funder.unlocked_balance().await?;
        if unlocked > 0 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "funder never saw unlocked coinbase"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let amount_a: u64 = 300_000_000_000; // 0.3 XMR
    let amount_b: u64 = 150_000_000_000; // 0.15 XMR

    // let the scan finish catching up, then measure: the balance has
    // to hold still across a poll before it means anything
    let mut balance_before = funder.balance().await?;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let next = funder.balance().await?;
        if next == balance_before {
            break;
        }
        balance_before = next;
    }

    let transfer = funder
        .fund(
            vec![(alice_address, amount_a), (bob_address, amount_b)],
            funder_address,
        )
        .await?;

    assert!(!transfer.input_ids.is_empty());
    assert!(transfer.change_pico > 0);

    alice.refresh().await?;
    bob.refresh().await?;

    // the gate: exact amounts, straight out of coinbase
    assert_eq!(alice.balance().await?, amount_a);
    assert_eq!(bob.balance().await?, amount_b);

    // and the funder's own books close exactly: the scan loop marks
    // the inputs spent off the chain and the change comes back
    // through the same loop, so the balance lands on everything
    // received minus what actually left (destinations plus fee)
    let spent_total = amount_a + amount_b + transfer.fee_pico;
    let expected_after = balance_before
        .checked_sub(spent_total)
        .expect("the funder had more than it paid");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let balance = funder.balance().await?;
        if balance == expected_after {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the funder's books never closed: {balance} != {expected_after}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Ok(())
}
