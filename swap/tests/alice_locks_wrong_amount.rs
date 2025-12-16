pub mod harness;

use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use swap_core::monero::Amount;

use crate::harness::SlowCancelConfig;

// Test: Alice locks LESS XMR than expected - by 1 piconero
// Bob should reject because validation uses strict equality
#[tokio::test]
async fn given_alice_locks_wrong_xmr_amount_bob_rejects() {
    run_test(|state| state.lower_by_one_piconero()).await;
}

// Test: Alice locks MORE XMR than expected - by 1 piconero
// Bob should still reject because validation uses strict equality
#[tokio::test]
async fn given_alice_locks_too_much_xmr_bob_rejects() {
    run_test(|state| state.raise_by_one_piconero()).await;
}

async fn run_test<F>(modify_alice_state: F)
where
    F: Fn(AliceState) -> AliceState + Send + Sync + Clone + 'static,
{
    harness::setup_test(SlowCancelConfig, None, move |mut ctx| {
        let modify_alice_state = modify_alice_state.clone();
        async move {
            // Run Bob until he gives up on the swap
            let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
            let bob_swap_id = bob_swap.id;
            let bob_swap = tokio::spawn(bob::run_until(bob_swap, |s| {
                matches!(s, BobState::WaitingForCancelTimelockExpiration { .. })
            }));

            // Run until Alice detects the Bitcoin as having been locked
            let alice_swap = ctx.alice_next_swap().await;
            let alice_state = alice::run_until(
                alice_swap,
                |s| matches!(s, AliceState::BtcLocked { .. }),
                FixedRate::default(),
            )
            .await?;

            // Resume Alice such that she locks the wrong amount of Monero
            ctx.restart_alice().await;
            let mut alice_swap = ctx.alice_next_swap().await;

            // Modify Alice such that she locks the wrong amount of Monero
            alice_swap.state = modify_alice_state(alice_state);
            let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

            // Run until Bob detects the wrong amount of Monero and gives up
            // He gives up by waiting for the cancel timelock to expire
            let bob_state = bob_swap.await??;
            assert!(matches!(
                bob_state,
                BobState::WaitingForCancelTimelockExpiration { .. }
            ));

            // Resume Bob and wait run until completion
            let (bob_swap, _) = ctx
                .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
                .await;
            let bob_state =
                bob::run_until(bob_swap, |s| matches!(s, BobState::BtcRefunded(..))).await?;

            // Assert that both Bob and Alice refunded
            ctx.assert_bob_refunded(bob_state).await;
            let alice_state = alice_swap.await??;
            ctx.assert_alice_refunded(alice_state).await;

            Ok(())
        }
    })
    .await;
}

trait FakeModifyMoneroAmount {
    /// Reduces the Monero amount by one piconero
    fn lower_by_one_piconero(self) -> Self;
    /// Raises the Monero amount by one piconero
    fn raise_by_one_piconero(self) -> Self;
}

impl FakeModifyMoneroAmount for AliceState {
    fn lower_by_one_piconero(self) -> Self {
        let AliceState::BtcLocked { mut state3 } = self else {
            panic!("Expected BtcLocked state to be able to modify the Monero amount");
        };

        let one_piconero = Amount::from_piconero(1);
        state3.xmr = state3.xmr.checked_sub(one_piconero).unwrap();
        Self::BtcLocked { state3 }
    }

    fn raise_by_one_piconero(self) -> Self {
        let AliceState::BtcLocked { mut state3 } = self else {
            panic!("Expected BtcLocked state to be able to modify the Monero amount");
        };

        let one_piconero = Amount::from_piconero(1);
        state3.xmr = state3.xmr + one_piconero;
        Self::BtcLocked { state3 }
    }
}
