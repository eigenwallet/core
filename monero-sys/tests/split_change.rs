use std::{collections::HashMap, ops::Not};

use monero::Amount;
use monero_sys::{ChangeManagement, Daemon, Enote, SyncProgress, WalletHandle};

const STAGENET_REMOTE_NODE: &str = "http://node.sethforprivacy.com:38089";
const STAGENET_WALLET_SEED: &str = "echo ourselves ruined oven masterful wives enough addicted future cottage illness adopt lucky movement tiger taboo imbalance antics iceberg hobby oval aloof tuesday uttered oval";
const STAGENET_WALLET_RESTORE_HEIGHT: u64 = 1728128;

#[tokio::test]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            "info,test=debug,monero_harness=debug,monero_rpc=debug,monero_cpp=error,split_change=trace,monero_sys=trace",
        )
        .with_test_writer()
        .init();

    let temp_dir = tempfile::tempdir().unwrap();
    let daemon = Daemon {
        address: STAGENET_REMOTE_NODE.into(),
        ssl: true,
    };

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
        ChangeManagement::Split {
            extra_outputs: 5,
            threshold: Amount::ZERO,
        },
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

    tracing::info!("Wallet is synchronized!");

    tracing::info!("Blockheight: {}", wallet.blockchain_height().await.unwrap());
    let original_enotes = wallet.enotes().await;
    tracing::info!(
        "Original enotes: {:?}",
        &original_enotes[original_enotes.len() - 10..]
    );

    let original_balance = Amount::from_pico(
        original_enotes
            .iter()
            .filter(|e| e.is_spent().not())
            .map(|e| e.amount().as_pico())
            .sum::<u64>(),
    );
    tracing::info!("Original balance: {}", original_balance);
    tracing::info!(
        "Spent balance: {}",
        Amount::from_pico(
            original_enotes
                .iter()
                .filter(|e| e.is_spent())
                .map(|e| e.amount().as_pico())
                .sum::<u64>()
        )
    );

    let balance = wallet.total_balance().await;
    tracing::info!("Balance: {}", balance);

    let unlocked_balance = wallet.unlocked_balance().await;
    tracing::info!("Unlocked balance: {}", unlocked_balance);

    assert!(balance > Amount::ZERO);
    assert!(unlocked_balance > Amount::ONE_XMR);

    let transfer_amount = Amount::ONE_XMR;
    tracing::info!("Transferring 1 XMR to ourselves");

    wallet
        .transfer(&wallet.main_address().await, transfer_amount)
        .await
        .unwrap();

    let new_balance = wallet.total_balance().await;
    tracing::info!("Balance: {}", new_balance);

    let new_unlocked_balance = wallet.unlocked_balance().await;
    tracing::info!("Unlocked balance: {}", new_unlocked_balance);

    let new_enotes = wallet.enotes().await;
    tracing::info!("New enotes: {:?}", &new_enotes[new_enotes.len() - 10..]);

    let calculated_total_balance = Amount::from_pico(
        new_enotes
            .iter()
            .filter(|e| e.is_spent().not())
            .map(|e| e.amount().as_pico())
            .sum::<u64>(),
    );

    tracing::info!("Blockheight: {}", wallet.blockchain_height().await.unwrap());
    tracing::info!("Calculated total balance: {}", calculated_total_balance);
    tracing::info!(
        "Spent balance: {}",
        Amount::from_pico(
            new_enotes
                .iter()
                .filter(|e| e.is_spent())
                .map(|e| e.amount().as_pico())
                .sum::<u64>()
        )
    );

    let fee = balance - new_balance;

    tracing::info!("Fee: {}", fee);

    assert!(fee > Amount::ZERO);
    assert!(new_balance > Amount::ZERO);
    assert!(new_balance <= balance);
    assert!(new_unlocked_balance <= balance - transfer_amount);
}

// assumes old is subset of new
fn find_newly_spent_enotes<'a>(
    original_enotes: &'a [Enote],
    new_enotes: &'a [Enote],
) -> Vec<&'a Enote> {
    let mut old = HashMap::new();
    let mut new = HashMap::new();

    for enote in original_enotes {
        old.insert(enote.global_enote_index(), enote);
    }

    for enote in new_enotes {
        new.insert(enote.global_enote_index(), enote);
    }

    let mut newly_spent = Vec::new();

    for (index, enote) in new.iter() {
        if let Some(old_enote) = old.get(index) {
            if old_enote.is_spent() != enote.is_spent() {
                newly_spent.push(*enote);
            }
        }
    }

    newly_spent
}
