mod harness;

use anyhow::{Context, Result};
use bitcoin::{Amount, Transaction};
use bitcoin_harness::BitcoindRpcApi;
use bitcoin_wallet::{PersisterConfig, WalletBuilder};
use std::time::Duration;
use testcontainers::clients::Cli;

#[derive(Clone, Debug)]
struct TestSeed([u8; 64]);

impl TestSeed {
    fn new(byte: u8) -> Self {
        Self([byte; 64])
    }
}

impl bitcoin_wallet::BitcoinWalletSeed for TestSeed {
    fn derive_extended_private_key(
        &self,
        network: bitcoin::Network,
    ) -> anyhow::Result<bitcoin::bip32::ExtendedPrivKey> {
        #[allow(deprecated)]
        {
            Ok(bitcoin::bip32::ExtendedPrivKey::new_master(network, &self.0)?)
        }
    }

    fn derive_extended_private_key_legacy(
        &self,
        network: bdk::bitcoin::Network,
    ) -> anyhow::Result<bdk::bitcoin::util::bip32::ExtendedPrivKey> {
        Ok(bdk::bitcoin::util::bip32::ExtendedPrivKey::new_master(
            network, &self.0,
        )?)
    }
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,bitcoin_wallet=debug,electrum_pool=debug,testcontainers=info")
        .with_test_writer()
        .try_init();
}

fn feerate(fee: Amount, tx: &Transaction) -> f64 {
    fee.to_sat() as f64 / tx.weight().to_vbytes_floor() as f64
}

async fn make_wallet(env: &harness::TestEnv<'_>, seed: TestSeed) -> Result<bitcoin_wallet::Wallet> {
    let wallet = WalletBuilder::<TestSeed>::default()
        .seed(seed)
        .network(bitcoin::Network::Regtest)
        .electrum_rpc_urls(vec![env.electrum_url.clone()])
        .persister(PersisterConfig::InMemorySqlite)
        .finality_confirmations(1u32)
        .target_block(1u32)
        .sync_interval(Duration::from_millis(0))
        .use_mempool_space_fee_estimation(false)
        .build()
        .await?;

    wallet.sync().await?;
    Ok(wallet)
}

async fn sync_until_balance(
    wallet: &bitcoin_wallet::Wallet,
    expected_at_least: bitcoin::Amount,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        wallet.sync().await?;
        if wallet.balance().await? >= expected_at_least {
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "timed out waiting for wallet balance to reach {} sats",
                expected_at_least.to_sat()
            );
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn funded_wallet(env: &harness::TestEnv<'_>, seed: TestSeed) -> Result<bitcoin_wallet::Wallet> {
    let wallet = make_wallet(env, seed).await?;

    let amount = Amount::from_sat(1_000_000);
    let receive_addr = wallet.new_address().await?;
    harness::fund_and_mine(&env.bitcoind, receive_addr, amount).await?;
    sync_until_balance(&wallet, amount).await?;

    Ok(wallet)
}

/// Broadcasts a self-transfer that pays exactly `fee` and returns the transaction
/// once the wallet has it indexed.
async fn broadcast_low_fee_self_spend(
    wallet: &bitcoin_wallet::Wallet,
    fee: Amount,
) -> Result<Transaction> {
    let amount = wallet
        .balance()
        .await?
        .checked_sub(fee)
        .context("balance too small for low-fee tx")?;

    let psbt = wallet
        .send_to_address(wallet.new_address().await?, amount, fee, None)
        .await?;
    let tx = wallet.sign_and_finalize(psbt).await?;
    let (txid, _sub) = wallet.broadcast(tx.clone(), "low-fee-self-spend").await?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        wallet.sync().await?;
        if wallet.get_raw_transaction(txid).await?.is_some() {
            return Ok(tx);
        }

        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for tx {txid} to be indexed");
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Asserts that the child bumps the package above the feerate its ancestors
/// paid individually and above the minimum relay feerate.
fn assert_child_covers_ancestors(
    ancestors: &[(Amount, &Transaction)],
    child_fee: Amount,
    child_tx: &Transaction,
) {
    let ancestor_fee = ancestors
        .iter()
        .map(|(fee, _)| *fee)
        .fold(Amount::ZERO, |a, b| a + b);
    let ancestor_weight = ancestors
        .iter()
        .map(|(_, tx)| tx.weight())
        .fold(bitcoin::Weight::ZERO, |a, b| a + b);

    let child_feerate = feerate(child_fee, child_tx);
    let package_feerate = (ancestor_fee + child_fee).to_sat() as f64
        / (ancestor_weight + child_tx.weight()).to_vbytes_floor() as f64;

    for (ancestor_fee, ancestor) in ancestors {
        let ancestor_feerate = feerate(*ancestor_fee, ancestor);
        assert!(
            child_feerate > ancestor_feerate,
            "child feerate ({child_feerate}) must exceed ancestor feerate ({ancestor_feerate})"
        );
        assert!(
            package_feerate > ancestor_feerate,
            "package feerate ({package_feerate}) must exceed ancestor feerate ({ancestor_feerate})"
        );
    }

    assert!(
        package_feerate >= 1.0,
        "package feerate ({package_feerate}) must satisfy minimum relay feerate"
    );
}

#[tokio::test]
async fn dynamic_fee_covers_unconfirmed_parent() -> Result<()> {
    init_tracing();

    let cli = Cli::default();
    let env = harness::setup(&cli).await?;

    let wallet = funded_wallet(&env, TestSeed::new(1)).await?;

    // A self-transfer paying only a minimum-relay feerate.
    let parent_fee = Amount::from_sat(200);
    let parent = broadcast_low_fee_self_spend(&wallet, parent_fee).await?;

    let child_psbt = wallet
        .send_to_address_dynamic_fee(wallet.new_address().await?, Amount::from_sat(100_000), None)
        .await?;

    assert!(
        child_psbt
            .unsigned_tx
            .input
            .iter()
            .any(|input| input.previous_output.txid == parent.compute_txid()),
        "child tx did not spend parent output",
    );

    let child_fee = child_psbt.fee().context("PSBT fee must be computable")?;
    let child_tx = wallet.sign_and_finalize(child_psbt).await?;
    let (child_txid, _sub) = wallet.broadcast(child_tx.clone(), "cpfp-child").await?;

    assert_child_covers_ancestors(&[(parent_fee, &parent)], child_fee, &child_tx);

    // The whole package must be mineable in the next block.
    let miner_addr = env
        .bitcoind
        .with_wallet(harness::BITCOIN_TEST_WALLET_NAME)?
        .getnewaddress(None, None)
        .await?
        .require_network(env.bitcoind.network().await?)?;
    let blocks = env.bitcoind.generatetoaddress(1, miner_addr).await?;
    let block = env.bitcoind.getblock(&blocks[0]).await?;

    assert!(
        block.tx.contains(&child_txid),
        "child tx was not mined in the generated block"
    );
    assert!(
        block.tx.contains(&parent.compute_txid()),
        "unconfirmed parent was not mined in the generated block"
    );

    Ok(())
}

#[tokio::test]
async fn dynamic_fee_covers_parent_and_grandparent() -> Result<()> {
    init_tracing();

    let cli = Cli::default();
    let env = harness::setup(&cli).await?;

    let wallet = funded_wallet(&env, TestSeed::new(2)).await?;

    let fee = Amount::from_sat(200);
    let grandparent = broadcast_low_fee_self_spend(&wallet, fee).await?;
    let parent = broadcast_low_fee_self_spend(&wallet, fee).await?;

    assert!(
        parent
            .input
            .iter()
            .any(|input| input.previous_output.txid == grandparent.compute_txid()),
        "parent tx did not spend grandparent output",
    );

    let child_psbt = wallet
        .send_to_address_dynamic_fee(wallet.new_address().await?, Amount::from_sat(100_000), None)
        .await?;

    assert!(
        child_psbt
            .unsigned_tx
            .input
            .iter()
            .any(|input| input.previous_output.txid == parent.compute_txid()),
        "child tx did not spend parent output",
    );

    let child_fee = child_psbt.fee().context("PSBT fee must be computable")?;
    let child_tx = wallet.sign_and_finalize(child_psbt).await?;
    wallet.broadcast(child_tx.clone(), "cpfp-child").await?;

    assert_child_covers_ancestors(&[(fee, &grandparent), (fee, &parent)], child_fee, &child_tx);

    Ok(())
}
