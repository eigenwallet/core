pub mod harness;

use std::time::Duration;

use harness::bob_run_until::is_remaining_refund_timelock_expired;
use harness::FastAmnestyConfig;
use rust_decimal::Decimal;
use swap::asb::FixedRate;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use swap_env::config::RefundPolicy;

/// Bob does a partial refund, reaches ReclaimTimelockExpired, then crashes.
/// While Bob is offline, Alice publishes TxWithhold.
/// When Bob restarts, he detects the TxWithhold and transitions to BtcWithheld
/// instead of trying to publish TxReclaim against a spent UTXO.
///
/// This tests the defensive check in ReclaimTimelockExpired (bob/swap.rs)
/// that verifies TxWithhold hasn't been published before attempting TxReclaim.
#[tokio::test]
async fn bob_restart_at_reclaim_timelock_expired_detects_withhold() {
    let refund_policy = Some(RefundPolicy {
        anti_spam_deposit_ratio: Decimal::new(5, 2), // 0.05 = 5%
        always_withhold_deposit: true,
    });

    harness::setup_test(
        FastAmnestyConfig,
        None,
        refund_policy,
        |mut ctx| async move {
            // Start Bob and save his swap ID for later restart
            let (bob_swap, bob_app_handle) = ctx.bob_swap().await;
            let bob_swap_id = bob_swap.id;

            // Bob runs until ReclaimTimelockExpired.
            // With FastAmnestyConfig (3-block remaining refund timelock), this
            // happens quickly — well before Alice finishes XMR recovery.
            let bob_task = tokio::spawn(bob::run_until(
                bob_swap,
                is_remaining_refund_timelock_expired,
            ));

            // Alice runs all the way to BtcWithholdConfirmed.
            // She will: lock XMR → see cancel → see partial refund → recover
            // XMR → publish TxWithhold → wait for confirmation.
            let alice_swap = ctx.alice_next_swap().await;
            let alice_task =
                tokio::spawn(alice::run(alice_swap, FixedRate::default()));

            // Bob finishes first (3-block timelock is much faster than XMR recovery)
            let bob_state = bob_task.await??;
            assert!(
                matches!(bob_state, BobState::ReclaimTimelockExpired(..)),
                "Expected ReclaimTimelockExpired, got: {bob_state}"
            );

            // Stop Bob's event loop and prepare for restart
            let (bob_swap, _) = ctx
                .stop_and_resume_bob_from_db(bob_app_handle, bob_swap_id)
                .await;
            // Verify Bob loaded the correct state from DB
            assert!(
                matches!(bob_swap.state, BobState::ReclaimTimelockExpired(..)),
                "Expected ReclaimTimelockExpired after DB load, got: {}",
                bob_swap.state
            );

            // Help Alice's Monero recovery along by generating blocks
            tokio::time::sleep(Duration::from_secs(15)).await;
            ctx.monero.generate_blocks().await?;

            // Wait for Alice to finish: XMR recovered, TxWithhold confirmed
            let alice_state = alice_task.await??;
            ctx.assert_alice_withhold_confirmed(alice_state).await;

            // Now restart Bob. He's in ReclaimTimelockExpired but TxWithhold
            // has been published while he was offline. The defensive check
            // should detect it and route to BtcWithholdPublished → BtcWithheld.
            let bob_task = tokio::spawn(bob::run(bob_swap));
            let bob_state = bob_task.await??;
            ctx.assert_bob_withheld(bob_state).await;

            Ok(())
        },
    )
    .await;
}
