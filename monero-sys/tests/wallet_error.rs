use monero_address::Network;
use monero_sys::{Daemon, WalletHandle};

#[tokio::test]
async fn scan_failure_preserves_cpp_error_message() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let wallet = WalletHandle::open_or_create(
        directory.path().join("wallet").display().to_string(),
        Daemon::try_from("http://127.0.0.1:1")?,
        Network::Stagenet,
        false,
    )
    .await?;

    // The C++ wallet rejects an invalid transaction ID before contacting a daemon.
    let error = wallet
        .scan_transaction("not-a-transaction-id".to_owned())
        .await
        .expect_err("an invalid transaction ID must fail");

    let message = format!("{error:#}");
    assert!(
        message.contains("Invalid txid specified: not-a-transaction-id"),
        "Expected the C++ wallet error, got: {message}",
    );

    Ok(())
}
