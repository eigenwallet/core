pub mod harness;

use harness::SlowCancelConfig;
use harness::bob_run_until::is_btc_locked;
use std::time::Duration;
use swap::asb::FixedRate;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

async fn wait_for_confirmed_apollo(
    db: &(dyn swap::protocol::Database + Send + Sync),
    swap_id: uuid::Uuid,
) -> bitcoin::Transaction {
    tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            if let Some(bob::ApolloProgress::Confirmed(tx)) =
                db.get_apollo_progress(swap_id).await.unwrap()
            {
                return tx;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("Apollo was not confirmed in time")
}

#[tokio::test]
async fn given_bob_loses_database_after_btc_locked_then_bob_recovers_swap_from_apollo() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_db = bob_swap.db.clone();
        let alice_peer_id = bob_swap.db.get_peer_id(bob_swap_id).await?;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let bob_state = bob_swap.await??;
        let BobState::BtcLocked { state3, .. } = bob_state else {
            panic!("Bob is not in BtcLocked state: {bob_state:?}");
        };

        // The publisher runs on its own. Wait until it confirms the Apollo transaction,
        // before the test simulates the loss of the database.
        let apollo_tx = wait_for_confirmed_apollo(bob_db.as_ref(), bob_swap_id).await;

        let (bob_swap, _) = ctx
            .stop_bob_delete_database_and_recover_from_apollo(bob_join_handle, bob_swap_id)
            .await;
        let BobState::BtcLocked {
            state3: recovered_state3,
            ..
        } = &bob_swap.state
        else {
            panic!(
                "Recovered swap is not in BtcLocked state: {:?}",
                bob_swap.state
            );
        };
        assert_eq!(recovered_state3.tx_lock_id(), state3.tx_lock_id());
        // The publisher of the recovered swap can already have confirmed the Apollo transaction again
        let recovered_apollo = bob_swap.db.get_apollo_progress(bob_swap_id).await?;
        assert!(
            matches!(
                &recovered_apollo,
                Some(bob::ApolloProgress::Published(tx) | bob::ApolloProgress::Confirmed(tx))
                    if *tx == apollo_tx
            ),
            "unexpected recovered Apollo progress: {recovered_apollo:?}"
        );
        assert_eq!(recovered_state3.xmr_view_keys(), state3.xmr_view_keys());
        assert_eq!(bob_swap.db.get_peer_id(bob_swap_id).await?, alice_peer_id);
        assert!(bob_swap.db.get_addresses(alice_peer_id).await?.is_empty());
        assert!(
            bob_swap
                .db
                .get_monero_address_pool(bob_swap_id)
                .await?
                .iter()
                .all(|entry| entry.label().is_empty())
        );
        assert!(
            state3
                .apollo(&bob::SwapSecretKeys::derive([0; 32], uuid::Uuid::nil()))
                .is_err()
        );

        let bob_state = bob::run(bob_swap).await?;
        ctx.assert_bob_redeemed(bob_state).await;

        let alice_state = alice_swap.await??;
        ctx.assert_alice_redeemed(alice_state).await;

        Ok(())
    })
    .await;
}
