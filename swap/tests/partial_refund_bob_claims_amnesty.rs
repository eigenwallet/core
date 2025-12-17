pub mod harness;

use harness::alice_run_until::is_xmr_lock_transaction_sent;
use harness::FastAmnestyConfig;
use rust_decimal::Decimal;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::{alice, bob};
use swap_env::config::RefundPolicy;

/// Bob locks Btc and Alice locks Xmr. Alice does not act so Bob does a partial
/// refund, waits for the remaining refund timelock, and then claims the amnesty.
#[tokio::test]
async fn given_partial_refund_bob_claims_amnesty_after_timelock() {
    // Use 95% refund ratio - Bob gets 95% immediately, 5% locked in amnesty
    let refund_policy = Some(RefundPolicy {
        taker_refund_ratio: Decimal::new(95, 2), // 0.95 = 95%
    });

    harness::setup_test(FastAmnestyConfig, None, refund_policy, |mut ctx| async move {
        let (bob_swap, _) = ctx.bob_swap().await;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_xmr_lock_transaction_sent,
            FixedRate::default(),
        ));

        // Alice finishes first (just sends XMR lock and stops)
        let alice_state = alice_swap.await??;
        assert!(matches!(
            alice_state,
            AliceState::XmrLockTransactionSent { .. }
        ));

        // Bob takes longer: cancel timelock -> partial refund -> remaining refund timelock -> amnesty
        let bob_state = bob_swap.await??;
        ctx.assert_bob_amnesty_received(bob_state).await;

        // Restart Alice so she can refund her XMR
        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let alice_state = alice_swap.await??;
        ctx.assert_alice_refunded(alice_state).await;

        Ok(())
    })
    .await;
}
