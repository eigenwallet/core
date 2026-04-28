pub mod harness;

use harness::SlowCancelConfig;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use swap_core::monero::Amount;

fn lower_by_one_pico(state: AliceState) -> AliceState {
    let AliceState::BtcLocked { mut state3 } = state else {
        panic!("expected Alice to be in BtcLocked state");
    };

    state3.xmr = state3
        .xmr
        .checked_sub(Amount::from_pico(1))
        .expect("expected XMR amount to be larger than 1 pico");

    AliceState::BtcLocked { state3 }
}

#[tokio::test]
async fn bob_refunds_when_xmr_amount_is_not_exact() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;

        let bob_swap = tokio::spawn(bob::run_until(bob_swap, |s| {
            matches!(s, BobState::XmrLockTransactionCandidate { .. })
        }));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_state = alice::run_until(
            alice_swap,
            |s| matches!(s, AliceState::BtcLocked { .. }),
            FixedRate::default(),
        )
        .await?;

        ctx.restart_alice().await;
        let mut alice_swap = ctx.alice_next_swap().await;
        alice_swap.state = lower_by_one_pico(alice_state);
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        assert!(matches!(
            bob_swap.await??,
            BobState::XmrLockTransactionCandidate { .. }
        ));

        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;

        bob::run_until(bob_swap, |s| {
            matches!(s, BobState::WaitingForCancelTimelockExpiration { .. })
        })
        .await?;

        let (bob_swap, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;

        let bob_state = bob::run_until(bob_swap, |s| matches!(s, BobState::BtcRefunded(..)))
            .await?;

        ctx.assert_bob_refunded(bob_state).await;
        ctx.assert_alice_refunded(alice_swap.await??).await;

        Ok::<(), anyhow::Error>(())
    })
    .await;
}
