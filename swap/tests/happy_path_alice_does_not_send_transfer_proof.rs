pub mod harness;

use harness::alice_run_until::is_xmr_lock_transaction_sent;
use harness::bob_run_until::is_xmr_locked;
use harness::SlowCancelConfig;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

/// Test that Bob can detect the Monero lock transaction using the view key
/// even when Alice goes offline and doesn't send the transfer proof.
#[tokio::test]
async fn given_alice_goes_offline_after_xmr_locked_bob_detects_xmr_via_view_key() {
    harness::setup_test(SlowCancelConfig, None, |mut ctx| async move {
        // Bob runs until he detects the Monero as having been locked
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_xmr_locked));

        // Alice runs until the locks the Monero but before she sends the transfer proof
        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_xmr_lock_transaction_sent,
            FixedRate::default(),
        ));

        // Assert that Alice is in correct state
        let alice_state = alice_swap.await??;
        assert!(matches!(
            alice_state,
            AliceState::XmrLockTransactionSent { .. }
        ));

        // Assert that Bob is in correct state
        let bob_state = bob_swap.await??;
        assert!(
            matches!(bob_state, BobState::XmrLocked(..)),
            "Bob should have detected XMR lock via view key, but state is: {:?}",
            bob_state
        );

        // Resume the swap
        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let (bob_swap, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;

        // Run the swap to completion
        let bob_state = bob::run(bob_swap).await?;
        ctx.assert_bob_redeemed(bob_state).await;

        let alice_state = alice_swap.await??;
        ctx.assert_alice_redeemed(alice_state).await;

        Ok(())
    })
    .await;
}
