pub mod harness;

use harness::SlowCancelConfig;
use swap::protocol::bob;
use swap::protocol::bob::BobState;

/// Bob commits to the swap, but Alice fails to persist it. Without her
/// confirmation Bob must not lock his Bitcoin, because Alice would not know
/// the swap and could not refund it early.
#[tokio::test]
async fn given_alice_fails_to_persist_swap_then_bob_does_not_lock_btc() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap, _bob_join_handle) = ctx.bob_swap().await;
        ctx.make_alice_fail_to_persist_swap(bob_swap.id).await;

        let bob_state = bob::run(bob_swap).await?;
        assert!(matches!(bob_state, BobState::SafelyAborted));

        ctx.assert_bob_did_not_lock_btc().await;

        Ok(())
    })
    .await;
}
