pub mod harness;

use harness::SlowCancelConfig;
use swap::asb::FixedRate;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

/// Bob commits to the swap while Alice cannot write to her database. Alice must
/// persist nothing and release the reserved inputs, and Bob must not lock his
/// Bitcoin. Once the database is free again, a new swap with the same wallet
/// must go through right away.
#[tokio::test]
async fn given_alice_database_is_locked_then_bob_does_not_lock_and_next_swap_succeeds() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let database_lock = ctx.lock_alice_database().await;

        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_state = bob::run(bob_swap).await?;
        assert!(matches!(bob_state, BobState::SafelyAborted));
        bob_join_handle.abort();
        ctx.assert_bob_did_not_lock_btc().await;

        drop(database_lock);

        let (bob_swap, _bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        // Alice never started the swap she failed to persist
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
