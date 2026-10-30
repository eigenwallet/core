pub mod harness;

use harness::FastAmnestyConfig;
use rust_decimal::Decimal;
use swap::protocol::bob;
use swap_env::config::RefundPolicy;

/// A one-satoshi deposit cannot fund its required outputs and spending paths.
/// Alice must send Bob a clear setup rejection before either party locks funds.
#[tokio::test]
async fn swap_rejected_due_to_small_deposit_reported_to_bob() {
    let refund_policy = Some(RefundPolicy {
        anti_spam_deposit_ratio: Decimal::new(1, 6), // A one-satoshi deposit for this swap.
        always_withhold_deposit: false,
    });

    harness::setup_test(FastAmnestyConfig, None, refund_policy, |ctx| async move {
        let small_btc = bitcoin::Amount::from_sat(1_000_000);
        let (bob_swap, event_loop) = ctx.bob_params.new_swap(small_btc).await?;
        let bob_event_loop = tokio::spawn(event_loop.run());

        let result = bob::run(bob_swap).await;

        let err = result.expect_err("Expected swap to be rejected due to small deposit");
        let err_msg = format!("{:#}", err);

        assert!(
            err_msg.contains("Anti-spam deposit") && err_msg.contains("minimum required"),
            "Expected rejection error about the minimum deposit, got: {err_msg}"
        );

        bob_event_loop.abort();

        Ok(())
    })
    .await;
}
