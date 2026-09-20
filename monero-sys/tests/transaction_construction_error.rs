//! Regression test for transaction construction error reporting.
//!
//! wallet2 returns a non-null `PendingTransaction` even when construction fails,
//! recording the real cause (e.g. "not enough money") in the object's own status
//! rather than returning a null pointer. If we don't inspect that status right
//! after creation, the failure is later masked by the empty-txid check, which
//! reports the misleading "Expected 1 txid, got 0" instead.
//!
//! This drives a freshly created (and therefore empty) stagenet wallet, which
//! makes `create_transactions_2` throw `not_enough_money` deterministically.

use futures::FutureExt;
use monero_oxide_ext::Amount;
use monero_sys::{ApprovalCallback, Daemon, SyncProgress, WalletHandle};
use std::sync::Arc;

const STAGENET_REMOTE_NODE: &str = "http://node.sethforprivacy.com:38089";

#[tokio::test]
async fn construction_failure_surfaces_real_error() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,monero_sys=debug")
        .with_test_writer()
        .init();

    let temp_dir = tempfile::tempdir().unwrap();
    let daemon = Daemon::try_from(STAGENET_REMOTE_NODE).unwrap();

    let wallet_path = temp_dir.path().join("empty_wallet").display().to_string();

    // A freshly generated wallet starts empty with its restore height at the
    // current chain tip, so there is nothing to scan and it syncs near-instantly.
    let wallet =
        WalletHandle::open_or_create(wallet_path, daemon, monero_address::Network::Stagenet, false)
            .await
            .expect("Failed to create wallet");

    wallet
        .wait_until_synced(None::<fn(SyncProgress)>)
        .await
        .expect("Failed to sync wallet");

    assert_eq!(
        wallet.unlocked_balance().await?,
        Amount::ZERO,
        "A freshly created wallet should have no funds",
    );

    // Construction fails before approval is ever requested, so this must not run.
    let approval_callback: ApprovalCallback = Arc::new(|_txid, _amount, _fee| {
        async {
            panic!("approval callback must not run when construction fails");
            #[allow(unreachable_code)]
            false
        }
        .boxed()
    });

    let address = wallet.main_address().await?;

    let error = match wallet
        .transfer_with_approval(&address, Some(Amount::ONE_XMR), approval_callback)
        .await
    {
        Ok(_) => panic!("Transferring from an empty wallet must fail"),
        Err(error) => error,
    };

    let message = format!("{error:#}");
    tracing::info!(%message, "Got the transfer error");

    assert!(
        message.contains("not enough money"),
        "Expected the real wallet2 construction error, got: {message}",
    );
    assert!(
        !message.contains("Expected 1 txid"),
        "The txid-count symptom masked the real error: {message}",
    );

    Ok(())
}
