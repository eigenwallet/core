pub mod harness;

use std::time::Duration;

use harness::alice_run_until::is_xmr_lock_transaction_sent;
use harness::bob_run_until::is_btc_partially_refunded;
use harness::SlowAmnestyConfig;
use rust_decimal::Decimal;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use swap_env::config::RefundPolicy;

/// Bob locks Btc and Alice locks Xmr. Alice does not act so Bob does a partial
/// refund. Alice then burns the refund, denying Bob access to the amnesty.
#[tokio::test]
async fn given_partial_refund_alice_burns_the_amnesty() {
    // Use 95% refund ratio - Bob gets 95% immediately, 5% locked in amnesty
    // Alice burns the amnesty
    let refund_policy = Some(RefundPolicy {
        taker_refund_ratio: Decimal::new(95, 2), // 0.95 = 95%
        burn_on_refund: true,
    });

    harness::setup_test(
        SlowAmnestyConfig,
        None,
        refund_policy,
        |mut ctx| async move {
            // Start Bob's swap
            let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
            let bob_swap_id = bob_swap.id;
            // Bob runs until he has done the partial refund
            let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_partially_refunded));

            // Alice sends XMR lock then stops
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

            // Bob runs until partial refund is done
            let bob_state = bob_swap.await??;
            assert!(matches!(bob_state, BobState::BtcPartiallyRefunded { .. }));

            // Restart Alice so she can refund her XMR and burn Bob's amnesty
            // Alice needs to publish burn BEFORE Bob's remaining refund timelock expires
            ctx.restart_alice().await;
            let alice_swap = ctx.alice_next_swap().await;
            let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

            // Bob continues - he's watching for TxRefundBurn while waiting for timelock
            // Alice's burn should get published before Bob's timelock expires
            let (bob_swap, _) = ctx
                .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
                .await;
            let bob_swap = tokio::spawn(bob::run(bob_swap));

            // Generate some Monero blocks such that Alice's
            // monero refund transaction gets confirmed in time.
            tokio::time::sleep(Duration::from_secs(15)).await;
            ctx.monero.generate_blocks().await?;

            // Bob should end up in BtcRefundBurnt because Alice's burn beat his amnesty
            let bob_state = bob_swap.await??;
            ctx.assert_bob_refund_burnt(bob_state).await;

            // Alice should be in refund burn confirmed state
            let alice_state = alice_swap.await??;
            ctx.assert_alice_refund_burn_confirmed(alice_state).await;

            Ok(())
        },
    )
    .await;
}
