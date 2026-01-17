pub mod harness;

use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use swap_core::monero::Amount;

use crate::harness::SlowCancelConfig;

#[tokio::test]
async fn given_alice_locks_wrong_xmr_amount_bob_rejects() {
    harness::setup_test(SlowCancelConfig, None, |mut ctx| async move {
        // Run Bob until he finds a swap candidate
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, |s| {
            matches!(s, BobState::XmrLockTransactionCandidate { .. })
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
        alice_swap.state = alice_state.lower_by_one_piconero();
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        // Assert that Bob recognises Alice as a swap candidate
        let bob_state = bob_swap.await??;
        assert!(matches!(
            bob_state,
            BobState::XmrLockTransactionCandidate { .. }
        ));

        // Resume Bob and wait run until Bob detects the problem
        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        let bob_state =
            bob::run_until(bob_swap, 
                |s| matches!(s, BobState::WaitingForCancelTimelockExpiration { .. })
            ).await?;
        
        // Assert that Bob detects the problem
        assert!(matches!(
            bob_state,
            BobState::WaitingForCancelTimelockExpiration { .. }
        ));

        // Resume Bob and wait till completion
        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        let bob_state =
            bob::run_until(bob_swap, |s| matches!(s, BobState::BtcRefunded(..))).await?;

        // Assert that both Bob and Alice refunded
        ctx.assert_bob_refunded(bob_state).await;
        let alice_state = alice_swap.await??;
        ctx.assert_alice_refunded(alice_state).await;


        Ok(())
    })
    .await;
}

#[tokio::test]
async fn given_significantly_wrong_xmr_amount_bob_immediately_aborts() {
    harness::setup_test(SlowCancelConfig, None, |mut ctx| async move {
        // Run Bob until he finds a swap candidate
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, |s| {
            matches!(s, BobState::XmrLockTransactionCandidate { .. })
        }));

        // Run until Alice detects the Bitcoin as having been locked
        let alice_swap = ctx.alice_next_swap().await;

        let alice_state = alice::run_until(
            alice_swap,
            |s| matches!(s, AliceState::BtcLocked { .. }),
            FixedRate::default(),
        )
        .await?;


        // Resume Alice such that she locks half the amount
        ctx.restart_alice().await;
        let mut alice_swap = ctx.alice_next_swap().await;

        // Modify Alice such that she locks half the amount
        alice_swap.state = alice_state.lower_by_one_piconero();
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        // Assert that Bob recognises Alice as a swap candidate
        let bob_state = bob_swap.await??;
        assert!(matches!(
            bob_state,
            BobState::XmrLockTransactionCandidate { .. }
        ));

        // Resume Bob and wait run until Bob detects the problem
        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        let bob_state =
            bob::run_until(bob_swap, 
                |s| matches!(s, BobState::WaitingForCancelTimelockExpiration { .. })
            ).await?;
        
        // Assert that Bob detects the problem
        assert!(matches!(
            bob_state,
            BobState::WaitingForCancelTimelockExpiration { .. }
        ));

        // Resume Bob and wait till completion
        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        let bob_state =
            bob::run_until(bob_swap, |s| matches!(s, BobState::BtcRefunded(..))).await?;

        // Assert that both Bob and Alice refunded
        ctx.assert_bob_refunded(bob_state).await;
        let alice_state = alice_swap.await??;
        ctx.assert_alice_refunded(alice_state).await;


        Ok(())
    })
    .await;
}

trait FakeModifyMoneroAmount {
    /// Reduces the Monero amount by one piconero
    fn lower_by_one_piconero(self) -> Self;
    // Halves the Monero amount
    fn halve_xmr_amount(self) -> Self;
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

    fn halve_xmr_amount(self) -> Self {
        let AliceState::BtcLocked { mut state3 } = self else {
            panic!("Expected BtcLocked state to be able to modify the Monero amount");
        };
        state3.xmr = Amount::from_piconero(state3.xmr.as_piconero() / 2);
        Self::BtcLocked { state3 }
    }
}
