pub mod harness;

use harness::SlowCancelConfig;
use rust_decimal::Decimal;
use swap::asb::FixedRate;
use swap::protocol::{alice, bob};
use tokio::join;

#[tokio::test]
async fn happy_path_alice_developer_tip() {
    harness::setup_test(
        SlowCancelConfig,
        Some(Decimal::from_f32_retain(0.1).unwrap()),
        |mut ctx| async move {
            let (bob_swap, _) = ctx.bob_swap().await;
            let bob_swap = tokio::spawn(bob::run(bob_swap));

            let alice_swap = ctx.alice_next_swap().await;
            let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

            let (bob_state, alice_state) = join!(bob_swap, alice_swap);

            ctx.assert_alice_redeemed(alice_state??).await;
            ctx.assert_bob_redeemed(bob_state??).await;
            ctx.assert_alice_developer_tip_received().await;

            Ok(())
        },
    )
    .await;
}
