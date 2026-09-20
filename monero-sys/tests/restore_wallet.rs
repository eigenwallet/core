use monero_address::Network;
use monero_sys::{Daemon, WalletHandle};
use tempfile::tempdir;

const STAGENET_WALLET_SEED: &str = "echo ourselves ruined oven masterful wives enough addicted future cottage illness adopt lucky movement tiger taboo imbalance antics iceberg hobby oval aloof tuesday uttered oval";

#[tokio::test]
async fn recover_wallet_creates_missing_parent_directory() {
    let tempdir = tempdir().unwrap();
    let wallet_path = tempdir.path().join("missing-wallets-dir").join("wallet");
    let keys_path = wallet_path.with_file_name("wallet.keys");

    assert!(!wallet_path.parent().unwrap().exists());

    let daemon = Daemon::try_from("http://127.0.0.1:38089").unwrap();
    let wallet = WalletHandle::open_or_create_from_seed(
        wallet_path.display().to_string(),
        STAGENET_WALLET_SEED.to_string(),
        Network::Stagenet,
        0,
        false,
        daemon,
    )
    .await
    .expect("wallet recovery should create the parent directory");

    assert!(wallet_path.parent().unwrap().exists());
    assert!(keys_path.exists());

    drop(wallet);
}
