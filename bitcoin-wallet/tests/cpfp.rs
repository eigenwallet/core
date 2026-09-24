mod harness;

use anyhow::{Context, Result};
use bitcoin::Amount;
use bitcoin_wallet::{PersisterConfig, WalletBuilder};
use std::time::Duration;
use testcontainers::clients::Cli;

fn feerate(fee_sat: u64, vbytes: u64) -> f64 {
    fee_sat as f64 / vbytes as f64
}

fn tx_vbytes(tx: &bitcoin::Transaction) -> u64 {
    tx.weight().to_vbytes_floor() as u64
}

fn psbt_vbytes(psbt: &bitcoin::psbt::Psbt) -> u64 {
    psbt.unsigned_tx.weight().to_vbytes_floor() as u64
}

#[derive(Clone)]
struct BroadcastTx {
    tx: bitcoin::Transaction,
    fee_sat: u64,
    vbytes: u64,
}

impl BroadcastTx {
    fn txid(&self) -> bitcoin::Txid {
        self.tx.compute_txid()
    }
}

#[derive(Clone, Debug)]
struct TestSeed([u8; 64]);

impl TestSeed {
    fn new(byte: u8) -> Self {
        Self([byte; 64])
    }
}

impl Default for TestSeed {
    fn default() -> Self {
        Self::new(42)
    }
}

impl bitcoin_wallet::BitcoinWalletSeed for TestSeed {
    fn derive_extended_private_key(
        &self,
        network: bitcoin::Network,
    ) -> anyhow::Result<bitcoin::bip32::Xpriv> {
        #[allow(deprecated)]
        {
            Ok(bitcoin::bip32::Xpriv::new_master(network, &self.0)?)
        }
    }

    fn derive_extended_private_key_legacy(
        &self,
        network: bdk::bitcoin::Network,
    ) -> anyhow::Result<bdk::bitcoin::util::bip32::ExtendedPrivKey> {
        Ok(bdk::bitcoin::util::bip32::ExtendedPrivKey::new_master(
            network,
            &self.0,
        )?)
    }
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,bitcoin_wallet=debug,electrum_pool=debug,testcontainers=info")
        .with_test_writer()
        .try_init();
}

async fn make_wallet(
    env: &harness::TestEnv<'_>,
    seed: TestSeed,
) -> Result<bitcoin_wallet::Wallet> {
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
    expected_at_least: Amount,
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

async fn wait_until_tx_seen(
    wallet: &bitcoin_wallet::Wallet,
    txid: bitcoin::Txid,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

    loop {
        wallet.sync().await?;

        if wallet.get_raw_transaction(txid).await?.is_some() {
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for tx {txid} to be indexed");
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn funded_wallet(
    env: &harness::TestEnv<'_>,
    seed: TestSeed,
    amount: Amount,
) -> Result<bitcoin_wallet::Wallet> {
    let wallet = make_wallet(env, seed).await?;

    let receive_addr = wallet.new_address().await?;

    harness::fund_and_mine(&env.bitcoind, receive_addr, amount).await?;

    sync_until_balance(&wallet, amount).await?;

    Ok(wallet)
}

async fn build_and_broadcast_tx(
    wallet: &bitcoin_wallet::Wallet,
    amount: Amount,
    fee: Amount,
    label: &str,
) -> Result<BroadcastTx> {
    let dest = wallet.new_address().await?;

    let psbt = wallet.send_to_address(dest, amount, fee, None).await?;

    let actual_fee = psbt
        .fee()
        .expect("PSBT fee must be computable");

    assert_eq!(
        actual_fee,
        fee,
        "constructed tx fee differs from expected fee",
    );

    let tx = wallet.sign_and_finalize(psbt).await?;

    let (txid, _sub) = wallet.broadcast(tx.clone(), label).await?;

    wait_until_tx_seen(wallet, txid).await?;

    Ok(BroadcastTx {
        vbytes: tx_vbytes(&tx),
        fee_sat: fee.to_sat(),
        tx,
    })
}

async fn build_low_fee_chain(
    wallet: &bitcoin_wallet::Wallet,
    chain_length: usize,
    fee: Amount,
) -> Result<Vec<BroadcastTx>> {
    assert!(chain_length > 0);

    let mut txs = Vec::with_capacity(chain_length);

    for i in 0..chain_length {
        let balance = wallet.balance().await?;

        let amount = balance
            .checked_sub(fee)
            .context("balance too small for low-fee tx")?;

        let tx = build_and_broadcast_tx(
            wallet,
            amount,
            fee,
            &format!("low-fee-chain-{i}"),
        )
        .await?;

        txs.push(tx);
    }

    Ok(txs)
}

fn assert_cpfp_requirements(
    ancestor_fee_sat: u64,
    ancestor_vbytes: u64,
    child_fee_sat: u64,
    child_vbytes: u64,
) {
    let ancestor_feerate = feerate(ancestor_fee_sat, ancestor_vbytes);
    let child_feerate = feerate(child_fee_sat, child_vbytes);

    assert!(
        child_feerate > ancestor_feerate,
        "child feerate ({}) must exceed ancestor feerate ({})",
        child_feerate,
        ancestor_feerate,
    );

    let package_fee_sat = ancestor_fee_sat.saturating_add(child_fee_sat);
    let package_vbytes = ancestor_vbytes.saturating_add(child_vbytes);
    let package_feerate = feerate(package_fee_sat, package_vbytes);

    assert!(
        package_feerate > ancestor_feerate,
        "package feerate ({}) must exceed ancestor feerate ({})",
        package_feerate,
        ancestor_feerate,
    );

    assert!(
        package_feerate >= 1.0,
        "package feerate ({}) must satisfy minimum relay feerate",
        package_feerate,
    );
}

#[tokio::test]
async fn cpfp_accounts_for_direct_parent() -> Result<()> {
    init_tracing();

    let cli = Cli::default();
    let env = harness::setup(&cli).await?;

    let wallet = funded_wallet(&env, TestSeed::new(1), Amount::from_sat(1_000_000)).await?;

    let parent_fee = Amount::from_sat(200);
    let chain = build_low_fee_chain(&wallet, 1, parent_fee).await?;
    let parent = &chain[0];

    let child_dest = wallet.new_address().await?;
    let child_psbt = wallet
        .send_to_address_dynamic_fee(child_dest, Amount::from_sat(100_000), None)
        .await?;

    assert!(
        child_psbt
            .unsigned_tx
            .input
            .iter()
            .any(|input| input.previous_output.txid == parent.txid()),
        "child tx did not spend parent output",
    );

    let child_fee_sat = child_psbt.fee()?.to_sat();
    let child_vbytes = psbt_vbytes(&child_psbt);

    assert_cpfp_requirements(
        parent.fee_sat,
        parent.vbytes,
        child_fee_sat,
        child_vbytes,
    );

    Ok(())
}

#[tokio::test]
async fn cpfp_accounts_for_parent_and_grandparent() -> Result<()> {
    init_tracing();

    let cli = Cli::default();
    let env = harness::setup(&cli).await?;

    let wallet = funded_wallet(&env, TestSeed::new(2), Amount::from_sat(1_000_000)).await?;

    let fee = Amount::from_sat(200);
    let chain = build_low_fee_chain(&wallet, 2, fee).await?;

    let grandparent = &chain[0];
    let parent = &chain[1];

    assert!(
        parent
            .tx
            .input
            .iter()
            .any(|input| input.previous_output.txid == grandparent.txid()),
        "parent tx did not spend grandparent output",
    );

    let child_dest = wallet.new_address().await?;
    let child_psbt = wallet
        .send_to_address_dynamic_fee(child_dest, Amount::from_sat(100_000), None)
        .await?;

    assert!(
        child_psbt
            .unsigned_tx
            .input
            .iter()
            .any(|input| input.previous_output.txid == parent.txid()),
        "child tx did not spend parent output",
    );

    let child_fee_sat = child_psbt.fee()?.to_sat();
    let child_vbytes = psbt_vbytes(&child_psbt);

    let included_fee_sat = grandparent.fee_sat + parent.fee_sat;
    let included_vbytes = grandparent.vbytes + parent.vbytes;

    assert_cpfp_requirements(
        included_fee_sat,
        included_vbytes,
        child_fee_sat,
        child_vbytes,
    );

    Ok(())
}

#[tokio::test]
async fn cpfp_ignores_great_grandparent() -> Result<()> {
    init_tracing();

    let cli = Cli::default();
    let env = harness::setup(&cli).await?;
    let low_fee = Amount::from_sat(200);

    let wallet_a = funded_wallet(&env, TestSeed::new(10), Amount::from_sat(1_000_000)).await?;
    let chain_a = build_low_fee_chain(&wallet_a, 2, low_fee).await?;
    let grandparent_a = &chain_a[0];
    let parent_a = &chain_a[1];

    let child_a_psbt = wallet_a
        .send_to_address_dynamic_fee(
            wallet_a.new_address().await?,
            Amount::from_sat(100_000),
            None,
        )
        .await?;
    let child_a_fee_sat = child_a_psbt.fee().expect("PSBT fee must be computable").to_sat();
    let child_a_vbytes = psbt_vbytes(&child_a_psbt);

    let wallet_b = funded_wallet(&env, TestSeed::new(11), Amount::from_sat(1_000_000)).await?;
    let chain_b = build_low_fee_chain(&wallet_b, 3, low_fee).await?;

    let grandparent_b = &chain_b[1];
    let parent_b = &chain_b[2];

    let child_b_psbt = wallet_b
        .send_to_address_dynamic_fee(
            wallet_b.new_address().await?,
            Amount::from_sat(100_000),
            None,
        )
        .await?;
    let child_b_fee_sat = child_b_psbt.fee().expect("PSBT fee must be computable").to_sat();
    let child_b_vbytes = psbt_vbytes(&child_b_psbt);

    assert!(
        child_b_psbt
            .unsigned_tx
            .input
            .iter()
            .any(|input| input.previous_output.txid == parent_b.txid()),
        "child tx did not spend parent output"
    );

    assert_eq!(
        child_a_fee_sat,
        child_b_fee_sat,
        "great-grandparent must be ignored: child fee with depth 3 must equal child fee with depth 2",
    );

    assert_cpfp_requirements(
        grandparent_a.fee_sat + parent_a.fee_sat,
        grandparent_a.vbytes + parent_a.vbytes,
        child_a_fee_sat,
        child_a_vbytes,
    );
    assert_cpfp_requirements(
        grandparent_b.fee_sat + parent_b.fee_sat,
        grandparent_b.vbytes + parent_b.vbytes,
        child_b_fee_sat,
        child_b_vbytes,
    );

    Ok(())
}
