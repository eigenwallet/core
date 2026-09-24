pub mod harness;

use harness::SlowCancelConfig;
use swap::asb::FixedRate;
use swap::protocol::{alice, bob};
use tokio::join;

#[tokio::test]
async fn given_swap_completed_bob_obtains_swap_attestation_from_alice() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let (bob_state, alice_state) = join!(bob_swap, alice_swap);
        ctx.assert_alice_redeemed(alice_state??).await;
        ctx.assert_bob_redeemed(bob_state??).await;

        // A fresh event loop looks for swaps awaiting an attestation right away.
        let (_, _bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;

        ctx.assert_bob_obtains_swap_attestation(bob_swap_id).await;

        Ok(())
    })
    .await;
}
