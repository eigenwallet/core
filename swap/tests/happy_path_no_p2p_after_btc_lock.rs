pub mod harness;

use harness::SlowCancelConfig;
use harness::alice_run_until::is_btc_lock_transaction_seen;
use harness::bob_run_until::is_btc_locked;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

/// Bob and Alice lose their p2p connection for good once Bob has locked his
/// Bitcoin. The swap must still succeed: Alice detects the Bitcoin lock and
/// Bob detects the Monero lock by scanning the respective chains, the
/// encrypted signature reaches Alice on-chain via Hermes, and Bob learns of
/// Alice's redeem from the Bitcoin chain.
#[tokio::test]
async fn swap_succeeds_without_p2p_after_btc_locked() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap, bob_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;

        let bob_state = bob::run_until(bob_swap, is_btc_locked).await?;
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        // Run Alice until her state is persisted, so the restarted ASB
        // resumes the swap.
        let alice_swap = ctx.alice_next_swap().await;
        let alice_state = alice::run_until(
            alice_swap,
            is_btc_lock_transaction_seen,
            FixedRate::default(),
        )
        .await?;
        assert!(matches!(
            alice_state,
            AliceState::BtcLockTransactionSeen { .. }
        ));

        // Sever the p2p connection: Alice moves to a fresh listen address
        // while Bob keeps dialing the old one.
        ctx.restart_alice_unreachable().await;

        let (bob_swap, _bob_handle) = ctx
            .stop_and_resume_bob_from_db(bob_handle, bob_swap_id)
            .await;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let (bob_state, alice_state) = tokio::join!(bob_swap, alice_swap);

        ctx.assert_alice_redeemed(alice_state??).await;
        ctx.assert_bob_redeemed(bob_state??).await;

        Ok(())
    })
    .await;
}
