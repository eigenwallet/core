pub mod harness;

use harness::SlowCancelConfig;
use swap::asb::FixedRate;
use swap::protocol::{alice, bob};
use tokio::join;

/// The happy path still completes when the on-chain encrypted signature
/// channel (Hermes) is disabled. Alice attaches no Hermes funding output to the
/// Monero lock transaction, so the encrypted signature can only flow over p2p.
#[tokio::test]
async fn happy_path_hermes_disabled() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        ctx.disable_alice_hermes().await;

        let (bob_swap, _) = ctx.bob_swap().await;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let (bob_state, alice_state) = join!(bob_swap, alice_swap);

        ctx.assert_alice_redeemed(alice_state??).await;
        ctx.assert_bob_redeemed(bob_state??).await;

        Ok(())
    })
    .await;
}
