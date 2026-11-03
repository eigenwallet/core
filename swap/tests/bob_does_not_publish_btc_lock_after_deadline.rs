pub mod harness;

use harness::SlowCancelConfig;
use harness::bob_run_until::is_btc_lock_ready_to_publish;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

/// Bob stops after Alice confirmed the swap but before he broadcast the lock
/// transaction, and resumes only after Alice stopped waiting for it. He must not
/// broadcast it anymore, because Alice would not lock the Monero or refund early.
#[tokio::test]
async fn given_bob_resumes_after_publish_deadline_then_he_aborts_without_locking_btc() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_state = bob::run_until(bob_swap, is_btc_lock_ready_to_publish).await?;
        assert!(matches!(
            bob_state,
            BobState::BtcLockReadyToPublish {
                broadcast_attempted: false,
                ..
            }
        ));

        // Alice stops waiting for the lock transaction
        let alice_swap = ctx.alice_next_swap().await;
        let alice_state = alice::run(alice_swap, FixedRate::default()).await?;
        assert!(matches!(alice_state, AliceState::SafelyAborted));

        // Bob's deadline is half of Alice's timeout, so it has passed as well
        let (bob_swap, _bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        let bob_state = bob::run(bob_swap).await?;
        assert!(matches!(bob_state, BobState::SafelyAborted));

        ctx.assert_bob_did_not_lock_btc().await;

        Ok(())
    })
    .await;
}
