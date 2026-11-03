pub mod harness;

use harness::SlowCancelConfig;
use harness::bob_run_until::is_swap_setup_completed;
use swap::asb::FixedRate;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

/// Bob negotiates a swap and then declines the offer (e.g. the user denies the
/// approval dialog). Alice must not keep the swap, and Bob must not lock his
/// Bitcoin. A new swap with the same wallet must go through.
#[tokio::test]
async fn given_bob_declines_swap_setup_then_alice_releases_it_and_next_swap_succeeds() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (mut bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_state = bob::run_swap_until(&mut bob_swap, is_swap_setup_completed).await?;
        assert!(matches!(bob_state, BobState::SwapSetupCompleted(..)));

        // Returns once Alice was told that we decline the swap
        bob_swap.event_loop_handle.abort_swap_setup().await?;

        let bob_state = bob::run(bob_swap).await?;
        assert!(matches!(bob_state, BobState::SafelyAborted));
        bob_join_handle.abort();

        let (bob_swap, _bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        // Alice never registered the declined swap
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
