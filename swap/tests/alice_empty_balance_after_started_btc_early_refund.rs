pub mod harness;

use harness::bob_run_until::is_btc_locked;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

use crate::harness::SlowCancelConfig;

#[tokio::test]
async fn alice_zero_xmr_refunds_bitcoin() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap, bob_handle) = ctx.bob_swap().await;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        // Run until the Bitcoin lock transaction is seen
        let alice_swap = ctx.alice_next_swap().await;
        let swap_id = alice_swap.swap_id;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            |state| matches!(state, AliceState::BtcLockTransactionSeen { .. }),
            FixedRate::default(),
        ));

        // Wait for both Alice and Bob to reach the Bitcoin locked state
        let alice_state = alice_swap.await??;
        let bob_state = bob_swap.await??;

        assert!(matches!(
            alice_state,
            AliceState::BtcLockTransactionSeen { .. }
        ));
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        // Empty Alice Monero Wallet
        // This will prevent Alice from locking her Monero
        // in turn forcing an early refund
        ctx.empty_alice_monero_wallet().await;
        ctx.assert_alice_monero_wallet_empty().await;

        ctx.monero.stop_miner().await;

        ctx.restart_alice().await;
        let (swap, _) = ctx.stop_and_resume_bob_from_db(bob_handle, swap_id).await;

        let bob_swap = tokio::spawn(bob::run(swap));

        let alice_swap = ctx.alice_next_swap().await;
        let interval = alice_swap.env_config.monero_lock_construction_cooldown;
        let wallets = ctx.alice_monero_wallet.clone();
        let turn_duration = wallets.wait_for_construction_turn().await;
        let mut previous_turn = tokio::time::Instant::now() + turn_duration;
        let mut alice_turns = 0;
        let mut alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let alice_state = loop {
            tokio::select! {
                result = &mut alice_swap => break result??,
                turn_duration = wallets.wait_for_construction_turn() => {
                    let next_turn = tokio::time::Instant::now() + turn_duration;
                    if next_turn.duration_since(previous_turn) >= interval * 2 {
                        alice_turns += 1;
                    }
                    previous_turn = next_turn;
                }
            }
        };

        let bob_state = bob_swap.await??;

        assert!(
            alice_turns >= 2,
            "Alice must requeue after her first turn expires"
        );
        assert!(matches!(alice_state, AliceState::BtcEarlyRefunded(_)));
        assert!(matches!(bob_state, BobState::BtcEarlyRefunded(_)));

        Ok(())
    })
    .await;
}
