pub mod harness;

use harness::SlowCancelConfig;
use std::time::Duration;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::{alice, bob};
use tokio::time::{Instant, timeout};

#[tokio::test]
async fn alice_waits_for_xmr_construction_cooldown() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let wallets = ctx.alice_monero_wallet.clone();

        let (bob_swap, _) = ctx.bob_swap().await;
        let bob_handle = tokio::spawn(bob::run(bob_swap));

        let alice_swap = ctx.alice_next_swap().await;
        let state = alice::run_until(
            alice_swap,
            |state| matches!(state, AliceState::XmrReadyToLock { .. }),
            FixedRate::default(),
        )
        .await?;
        assert!(matches!(state, AliceState::XmrReadyToLock { .. }));

        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let next_turn = wallets.wait_for_construction_turn().await;
        let mut alice_handle = tokio::spawn(alice::run_until(
            alice_swap,
            |state| matches!(state, AliceState::XmrLockTransactionConstructed { .. }),
            FixedRate::default(),
        ));

        assert!(
            timeout(Duration::from_secs(1), &mut alice_handle)
                .await
                .is_err()
        );

        let state = alice_handle.await??;
        assert!(matches!(
            state,
            AliceState::XmrLockTransactionConstructed { .. }
        ));
        assert!(Instant::now() >= next_turn);

        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_state = alice::run(alice_swap, FixedRate::default()).await?;
        let bob_state = bob_handle.await??;
        ctx.assert_alice_redeemed(alice_state).await;
        ctx.assert_bob_redeemed(bob_state).await;
        Ok(())
    })
    .await;
}
