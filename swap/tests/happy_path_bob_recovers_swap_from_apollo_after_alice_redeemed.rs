pub mod harness;

use crate::harness::alice_run_until::is_encsig_learned;
use crate::harness::bob_run_until::is_encsig_sent;
use harness::SlowCancelConfig;
use std::time::Duration;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use tokio::join;

/// Alice redeems the Bitcoin while Bob is gone, then Bob loses his database. After recovery,
/// cancel/refund must refuse (not hang), and resuming the swap must claim the Monero.
#[tokio::test]
async fn given_bob_loses_database_and_alice_redeemed_then_bob_recovers_and_redeems_xmr() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap, bob_handle) = ctx.bob_swap().await;
        let swap_id = bob_swap.id;
        let bob_db = bob_swap.db.clone();

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
        let BobState::EncSigSent { .. } = bob_state?? else {
            panic!("Bob is not in EncSigSent state");
        };

        // The publisher runs on its own. Wait until it confirms the Apollo transaction,
        // before the test deletes the database.
        tokio::time::timeout(Duration::from_secs(300), async {
            while !matches!(
                bob_db.get_apollo_progress(swap_id).await.unwrap(),
                Some(bob::ApolloProgress::Confirmed(_))
            ) {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        })
        .await
        .expect("Apollo was not confirmed in time");

        // Bob is gone. Alice redeems the Bitcoin.
        bob_handle.abort();
        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_state = alice::run(alice_swap, FixedRate::default()).await?;
        ctx.assert_alice_redeemed(alice_state).await;

        // Bob loses his database only now, and recovery restores the swap from the seed
        let (bob_swap, _) = ctx
            .stop_bob_delete_database_and_recover_from_apollo(bob_handle, swap_id)
            .await;
        let BobState::BtcLocked { state3, .. } = &bob_swap.state else {
            panic!(
                "Recovered swap is not in BtcLocked state: {}",
                bob_swap.state
            );
        };

        bob_swap
            .bitcoin_wallet
            .subscribe_to(Box::new(state3.tx_lock.clone()))
            .await
            .wait_until_confirmed_with(state3.cancel_timelock)
            .await?;

        // Cancel/refund must fail fast instead of waiting for transactions that can never confirm
        let result = tokio::time::timeout(
            Duration::from_secs(60),
            swap::cli::cancel_and_refund(
                swap_id,
                bob_swap.bitcoin_wallet.clone(),
                bob_swap.db.clone(),
            ),
        )
        .await
        .expect("cancel_and_refund hung after Alice redeemed");
        let err = result.expect_err("cancel_and_refund must fail after Alice redeemed");
        assert!(
            err.to_string().contains("Alice has already redeemed"),
            "unexpected error: {err:#}"
        );

        let bob_state = bob::run(bob_swap).await?;
        ctx.assert_bob_redeemed(bob_state).await;

        Ok(())
    })
    .await;
}
