pub mod harness;

use harness::FastCancelConfig;
use std::time::Duration;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::{alice, bob};
use swap_env::env::{Config, GetConfig};

struct LongConstructionTurn;

impl GetConfig for LongConstructionTurn {
    fn get_config() -> Config {
        Config {
            monero_lock_construction_cooldown: Duration::from_secs(120),
            ..FastCancelConfig::get_config()
        }
    }
}

#[tokio::test]
async fn alice_cancels_while_waiting_for_xmr_construction_turn() {
    harness::setup_test(LongConstructionTurn, None, None, |mut ctx| async move {
        let wallets = ctx.alice_monero_wallet.clone();
        let starting_balance = wallets.main_wallet().await.unlocked_balance().await?;

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
        wallets.wait_for_construction_turn().await;
        let state = tokio::time::timeout(
            Duration::from_secs(120),
            alice::run(alice_swap, FixedRate::default()),
        )
        .await??;

        assert!(matches!(state, AliceState::SafelyAborted));
        assert_eq!(
            wallets.main_wallet().await.unlocked_balance().await?,
            starting_balance
        );

        ctx.assert_bob_refunded(bob_handle.await??).await;
        Ok(())
    })
    .await;
}
