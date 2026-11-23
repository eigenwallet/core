pub mod harness;

use crate::harness::alice_run_until::is_encsig_learned;
use crate::harness::bob_run_until::is_encsig_sent;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use tokio::join;

#[tokio::test]
async fn given_bob_restarts_while_alice_redeems_btc() {
    harness::setup_test(
        harness::SlowCancelConfig,
        None,
        None,
        |mut ctx| async move {
            let (bob_swap, bob_handle) = ctx.bob_swap().await;
            let swap_id = bob_swap.id;

            let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_encsig_sent));

            // Alice stops before redeeming, so Bob is guaranteed to stop in EncSigSent
            let alice_swap = ctx.alice_next_swap().await;
            let alice_swap = tokio::spawn(alice::run_until(
                alice_swap,
                is_encsig_learned,
                FixedRate::default(),
            ));

            let (bob_state, alice_state) = join!(bob_swap, alice_swap);
            assert!(matches!(alice_state??, AliceState::EncSigLearned { .. }));
            assert!(matches!(bob_state??, BobState::EncSigSent { .. }));

            let (bob_swap, _) = ctx.stop_and_resume_bob_from_db(bob_handle, swap_id).await;

            // Alice redeems while Bob's swap is not running (only his event loop is)
            ctx.restart_alice().await;
            let alice_swap = ctx.alice_next_swap().await;
            let alice_state = alice::run(alice_swap, FixedRate::default()).await?;
            ctx.assert_alice_redeemed(alice_state).await;

            if let BobState::EncSigSent { state: state4, .. } = bob_swap.state.clone() {
                bob_swap
                    .bitcoin_wallet
                    .subscribe_to(Box::new(state4.tx_lock))
                    .await
                    .wait_until_confirmed_with(state4.cancel_timelock)
                    .await?;
            } else {
                panic!("Bob in unexpected state {}", bob_swap.state);
            }

            // Restart Bob
            let bob_state = bob::run(bob_swap).await?;
            ctx.assert_bob_redeemed(bob_state).await;

            Ok(())
        },
    )
    .await;
}
