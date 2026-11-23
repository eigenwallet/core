pub mod harness;

use harness::SlowCancelConfig;
use harness::bob_run_until::is_swap_setup_completed;
use swap::asb::FixedRate;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

/// Bob negotiates a swap but stops before he commits to it (e.g. the GUI is
/// closed while the approval dialog is open). Alice must not keep the swap, and
/// Bob must not lock his Bitcoin when he resumes it. A new swap with the same
/// wallet must go through.
#[tokio::test]
async fn given_bob_abandons_swap_setup_then_alice_releases_it_and_next_swap_succeeds() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let abandoned_swap_id = bob_swap.id;
        let bob_state = bob::run_until(bob_swap, is_swap_setup_completed).await?;
        assert!(matches!(bob_state, BobState::SwapSetupCompleted(..)));

        // The swap setup does not survive the restart, so Bob aborts the swap
        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, abandoned_swap_id)
            .await;
        let bob_state = bob::run(bob_swap).await?;
        assert!(matches!(bob_state, BobState::SafelyAborted));
        bob_join_handle.abort();

        let (bob_swap, _bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        // Alice never registered the abandoned swap
        let alice_swap = ctx.alice_next_swap().await;
        assert_eq!(alice_swap.swap_id, bob_swap_id);
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let bob_state = bob_swap.await??;
        ctx.assert_bob_redeemed(bob_state).await;

        let alice_state = alice_swap.await??;
        ctx.assert_alice_redeemed(alice_state).await;

        Ok(())
    })
    .await;
}
