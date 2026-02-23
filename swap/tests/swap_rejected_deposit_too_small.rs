pub mod harness;

use harness::FastAmnestyConfig;
use rust_decimal::Decimal;
use swap::protocol::bob;
use swap_env::config::RefundPolicy;

/// Bob tries to swap a very small BTC amount (10,000 sats).
/// Alice has a 5% anti-spam deposit ratio.
///
/// Alice computes: amnesty = max(10000 * 0.05, 3001) = 3001 sats.
/// But 3001 / 10000 = 30.01% which exceeds the 20% ceiling.
///
/// Alice rejects the swap during setup and sends an error to Bob.
/// Bob must receive a clear rejection error (not a timeout or generic
/// "connection closed" error).
#[tokio::test]
async fn swap_rejected_due_to_small_deposit_reported_to_bob() {
    let refund_policy = Some(RefundPolicy {
        anti_spam_deposit_ratio: Decimal::new(5, 2), // 0.05 = 5%
        always_withhold_deposit: false,
    });

    harness::setup_test(FastAmnestyConfig, None, refund_policy, |ctx| async move {
        // 10,000 sats is small enough that the minimum fee floor (3001 sats)
        // pushes the amnesty ratio above the 20% ceiling.
        let small_btc = bitcoin::Amount::from_sat(10_000);
        let (bob_swap, event_loop) = ctx.bob_params.new_swap(small_btc).await?;
        let bob_event_loop = tokio::spawn(event_loop.run());

        let result = bob::run(bob_swap).await;

        let err = result.expect_err("Expected swap to be rejected due to small deposit");
        let err_msg = format!("{:#}", err);

        assert!(
            err_msg.contains("Anti-spam deposit ratio"),
            "Expected rejection error about anti-spam deposit ratio, got: {err_msg}"
        );

        bob_event_loop.abort();

        Ok(())
    })
    .await;
}
