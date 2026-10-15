pub mod harness;

use harness::SlowCancelConfig;
use std::time::Duration;
use swap::monero;
use tokio::sync::oneshot;
use tokio::time::timeout;

#[tokio::test]
async fn xmr_construction_discards_cancelled_native_result() {
    harness::setup_test(SlowCancelConfig, None, None, |ctx| async move {
        let wallet = ctx.alice_monero_wallet.main_wallet().await;
        let starting_balance = wallet.unlocked_balance().await?;
        let destination = monero_address::MoneroAddress::from_str_with_unchecked_network(
            "49LEH26DJGuCyr8xzRAzWPUryzp7bpccC7Hie1DiwyfJEyUKvMFAethRLybDYrFdU1eHaMkKQpUPebY4WT3cSjEvThmpjPa",
        )?;
        let amount = monero_oxide_ext::Amount::from_pico(1_000_000_000_000);
        let (entered, entry) = oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let (observed, receipt) = oneshot::channel();
        let construction_wallet = wallet.clone();
        let construction = tokio::spawn(async move {
            construction_wallet
                .call(move |wallet| -> anyhow::Result<_> {
                    let _ = entered.send(());
                    blocked.recv()?;
                    let result = wallet.construct_multi_destination_tx(&[(destination, amount)])?;
                    let _ = observed.send(result.0.txid.clone());
                    Ok(result)
                })
                .await
        });

        timeout(Duration::from_secs(30), entry).await??;
        construction.abort();
        assert!(matches!(construction.await, Err(error) if error.is_cancelled()));
        release.send(())?;

        let txid = timeout(Duration::from_secs(60), receipt).await??;
        timeout(Duration::from_secs(30), wallet.call(|_| ())).await??;
        assert!(
            !ctx.alice_monero_wallet
                .is_transaction_present(&monero::TxHash(txid))
                .await?
        );
        assert_eq!(wallet.unlocked_balance().await?, starting_balance);
        Ok(())
    })
    .await;
}
