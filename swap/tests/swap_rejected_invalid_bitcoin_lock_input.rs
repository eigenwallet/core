pub mod harness;

use harness::FastAmnestyConfig;
use swap::protocol::bob;

/// Bob's wallet contains a locally-known UTXO that does not exist on the
/// Bitcoin chain. Bob can construct and prove ownership of the resulting lock
/// PSBT, but Alice must reject it when she verifies the prevout through
/// Electrum.
#[tokio::test]
async fn alice_rejects_bob_lock_psbt_with_nonexistent_input() {
    harness::setup_test_with_fake_bob_bitcoin_utxo(
        FastAmnestyConfig,
        None,
        None,
        |ctx| async move {
            let btc = bitcoin::Amount::from_sat(1_000_000);
            let (bob_swap, event_loop) = ctx.bob_params.new_swap(btc).await?;
            let bob_event_loop = tokio::spawn(event_loop.run());

            let error = bob::run(bob_swap)
                .await
                .expect_err("Alice must reject Bob's nonexistent lock input");
            let error = format!("{error:#}");

            assert!(
                error.contains("Input verification failed. Yeah."),
                "unexpected swap rejection: {error}"
            );

            assert!(!error.contains("not found unspent on chain or in mempool"));
            bob_event_loop.abort();

            Ok(())
        },
    )
    .await;
}
