/// Construct, publish and return the transaction keys of a complex transaction
/// (sending to multiple addresses, some of which are subaddresses)
use monero::Amount;
use monero_sys::{Daemon, SyncProgress, WalletHandle};

const STAGENET_REMOTE_NODE: &str = "http://node.sethforprivacy.com:38089";
const STAGENET_WALLET_SEED: &str = "echo ourselves ruined oven masterful wives enough addicted future cottage illness adopt lucky movement tiger taboo imbalance antics iceberg hobby oval aloof tuesday uttered oval";
const STAGENET_WALLET_RESTORE_HEIGHT: u64 = 1728128;

#[tokio::test]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            "info,test=debug,monero_harness=debug,monero_rpc=debug,transaction_keys=trace,monero_sys=trace",
        )
        .with_test_writer()
        .init();

    let temp_dir = tempfile::tempdir().unwrap();
    let daemon = Daemon::try_from(STAGENET_REMOTE_NODE).unwrap();

    let wallet_name = "recovered_wallet";
    let wallet_path = temp_dir.path().join(wallet_name).display().to_string();

    tracing::info!("Recovering wallet from seed");
    let wallet = WalletHandle::open_or_create_from_seed(
        wallet_path,
        STAGENET_WALLET_SEED.to_string(),
        monero::Network::Stagenet,
        STAGENET_WALLET_RESTORE_HEIGHT,
        true,
        daemon,
    )
    .await
    .expect("Failed to recover wallet");

    tracing::info!("Primary address: {}", wallet.main_address().await);

    // Wait for a while to let the wallet sync, checking sync status
    tracing::info!("Waiting for wallet to sync...");

    wallet
        .wait_until_synced(Some(|sync_progress: SyncProgress| {
            tracing::info!("Sync progress: {}%", sync_progress.percentage());
        }))
        .await
        .expect("Failed to sync wallet");

    wallet.store_in_current_file().await?;

    // Test sending to some (sub)addresses
    let subaddress1 = wallet.address(1, 0).await;
    let subaddress2 = wallet.address(0, 2).await;

    let addresses = [subaddress1.to_string(), subaddress2.to_string()];
    tracing::info!(addresses=?addresses, "Got the destination addresses");

    let amount = Amount::from_xmr(0.02)?;

    let tx_receipt = wallet
        .transfer_multi_destination(&[(subaddress1, amount), (subaddress2, amount)])
        .await?;

    // at this point we managed to publish the transaction and
    // got all transaction keys (for each output).
    // The test passed, the logs are just for debugging.
    tracing::info!(tx_id = &tx_receipt.txid, "Transaction published! (good)");
    for (addr, key) in tx_receipt.tx_keys {
        tracing::info!(address=%addr, %key, "Got transaction key");
    }

    Ok(())
}
