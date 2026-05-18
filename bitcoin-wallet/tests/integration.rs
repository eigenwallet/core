mod harness;

use anyhow::Result;
use bitcoin_harness::BitcoindRpcApi;
use bitcoin_wallet::{PersisterConfig, WalletBuilder};
use std::time::Duration;
use testcontainers::clients::Cli;

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

#[derive(Clone, Debug)]
struct TestSeed([u8; 64]);

impl Default for TestSeed {
    fn default() -> Self {
        // Deterministic seed for reproducible integration tests.
        Self([42u8; 64])
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

#[tokio::test]
async fn wallet_syncs_and_receives_funds() -> Result<()> {
    init_tracing();

    let cli = Cli::default();
    let env = harness::setup(&cli).await?;

    let wallet = WalletBuilder::<TestSeed>::default()
        .seed(TestSeed::default())
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

    let receive_addr = wallet.new_address().await?;

    // Fund wallet via bitcoind+electrs
    let amount = bitcoin::Amount::from_sat(50_000);
    harness::fund_and_mine(&env.bitcoind, receive_addr, amount).await?;

    sync_until_balance(&wallet, amount).await?;

    Ok(())
}

#[tokio::test]
async fn wallet_sends_broadcasts_and_confirms() -> Result<()> {
    init_tracing();

    let cli = Cli::default();
    let env = harness::setup(&cli).await?;

    let wallet = WalletBuilder::<TestSeed>::default()
        .seed(TestSeed::default())
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

    let receive_addr = wallet.new_address().await?;
    let funding = bitcoin::Amount::from_sat(100_000);
    harness::fund_and_mine(&env.bitcoind, receive_addr, funding).await?;

    sync_until_balance(&wallet, funding).await?;

    // Build spend
    let recipient = env
        .bitcoind
        .with_wallet(harness::BITCOIN_TEST_WALLET_NAME)?
        .getnewaddress(None, None)
        .await?
        .require_network(env.bitcoind.network().await?)?;

    let send_amount = bitcoin::Amount::from_sat(25_000);
    let fee = bitcoin::Amount::from_sat(2_000);

    let psbt = wallet
        .send_to_address(recipient, send_amount, fee, None)
        .await?;

    let tx = wallet.sign_and_finalize(psbt).await?;

    let (txid, sub) = wallet.broadcast(tx, "it-send").await?;

    // Confirm it
    let miner_addr = env
        .bitcoind
        .with_wallet(harness::BITCOIN_TEST_WALLET_NAME)?
        .getnewaddress(None, None)
        .await?
        .require_network(env.bitcoind.network().await?)?;
    env.bitcoind.generatetoaddress(1, miner_addr).await?;

    // Sync until electrum indexes the tx and it becomes final.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        wallet.sync().await?;
        if wallet.get_raw_transaction(txid).await?.is_some() {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for raw transaction {txid}");
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    sub.wait_until_final().await?;

    Ok(())
}
