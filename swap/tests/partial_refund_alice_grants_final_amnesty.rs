pub mod harness;

use harness::alice_run_until::{is_btc_refund_burn_confirmed, is_xmr_lock_transaction_sent};
use harness::FastAmnestyConfig;
use rust_decimal::Decimal;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::{alice, bob};
use swap_env::config::RefundPolicy;

/// Bob locks Btc and Alice locks Xmr. Alice does not act so Bob does a partial
/// refund. Alice burns the refund, then later grants final amnesty to Bob.
/// NOTE: This test cannot pass yet because we haven't implemented the manual
/// trigger for final amnesty. BtcRefundBurnConfirmed is currently terminal.
#[tokio::test]
#[ignore = "final amnesty manual trigger not implemented yet"]
async fn given_partial_refund_alice_grants_final_amnesty() {
    // Use 95% refund ratio - Bob gets 95% immediately, 5% locked in amnesty
    // Alice burns the amnesty, then grants final amnesty
    let refund_policy = Some(RefundPolicy {
        taker_refund_ratio: Decimal::new(95, 2), // 0.95 = 95%
        burn_on_refund: true,
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

        // Bob continues: cancel timelock -> partial refund
        // Bob will end up in BtcRefundBurnt because Alice burns the amnesty
        let bob_state = bob_swap.await??;
        ctx.assert_bob_refund_burnt(bob_state.clone()).await;

        // Restart Alice so she can refund her XMR and burn Bob's amnesty
        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_btc_refund_burn_confirmed,
            FixedRate::default(),
        ));

        let alice_state = alice_swap.await??;
        assert!(matches!(
            alice_state,
            AliceState::BtcRefundBurnConfirmed { .. }
        ));

        // TODO: Trigger final amnesty manually here
        // This requires a manual command to Alice to grant final amnesty
        // For now, this test is ignored.

        // Bob should receive final amnesty
        // ctx.assert_bob_final_amnesty_received(bob_state).await;

        // Alice should be in final amnesty confirmed state
        // ctx.assert_alice_final_amnesty_confirmed(alice_state).await;

        Ok(())
    })
    .await;
}
