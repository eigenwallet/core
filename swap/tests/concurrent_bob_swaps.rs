pub mod harness;

use harness::SlowCancelConfig;
use std::time::Duration;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

/// Two Bob swaps against the same Alice, driven by a single shared event loop.
/// Swap 2 starts once swap 1 has locked its Bitcoin; both must then progress
/// in parallel to redemption.
#[tokio::test]
async fn concurrent_bob_swaps() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap_1, bob_swap_2, _bob_join_handle) = ctx.bob_two_concurrent_swaps().await;

        let swap_1_id = bob_swap_1.id;
        let swap_1_db = bob_swap_1.db.clone();

        let bob_swap_1 = tokio::spawn(bob::run(bob_swap_1));

        let alice_swap_1 = ctx.alice_next_swap().await;
        let alice_swap_1 = tokio::spawn(alice::run(alice_swap_1, FixedRate::default()));

        // Wait for swap 1 to reach `BtcLocked` or any subsequent state.
        loop {
            let state: BobState = swap_1_db.get_state(swap_1_id).await?.try_into()?;
            if !matches!(
                state,
                BobState::Started { .. } | BobState::SwapSetupCompleted(..)
            ) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        let bob_swap_2 = tokio::spawn(bob::run(bob_swap_2));

        let alice_swap_2 = ctx.alice_next_swap().await;
        let alice_swap_2 = tokio::spawn(alice::run(alice_swap_2, FixedRate::default()));

        let bob_state_1 = bob_swap_1.await??;
        let bob_state_2 = bob_swap_2.await??;
        let alice_state_1 = alice_swap_1.await??;
        let alice_state_2 = alice_swap_2.await??;

        assert!(matches!(bob_state_1, BobState::XmrRedeemed { .. }));
        assert!(matches!(bob_state_2, BobState::XmrRedeemed { .. }));
        assert!(matches!(alice_state_1, AliceState::BtcRedeemed { .. }));
        assert!(matches!(alice_state_2, AliceState::BtcRedeemed { .. }));

        Ok(())
    })
    .await;
}
