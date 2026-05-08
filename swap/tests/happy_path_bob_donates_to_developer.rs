pub mod harness;

use harness::SlowCancelConfig;
use rust_decimal::Decimal;
use swap::asb::FixedRate;
use swap::monero::{LabeledMoneroAddress, MoneroAddressPool};
use swap::protocol::{alice, bob};
use tokio::join;

const DONATION_RATIO: f32 = 0.1;
const SPLIT_TOLERANCE_PICO: u64 = 100_000_000_000;

#[tokio::test]
async fn happy_path_bob_donates_to_developer() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        let bob_main_address = ctx
            .bob_monero_wallet
            .main_wallet()
            .await
            .main_address()
            .await?;
        let bob_donation_address = ctx
            .bob_donation_monero_wallet
            .main_wallet()
            .await
            .main_address()
            .await?;

        let donation_ratio = Decimal::from_f32_retain(DONATION_RATIO).unwrap();
        let user_ratio = Decimal::ONE - donation_ratio;

        let monero_receive_pool = MoneroAddressPool::new(vec![
            LabeledMoneroAddress::with_address(
                bob_main_address,
                user_ratio,
                "Bob main wallet".to_string(),
            )?,
            LabeledMoneroAddress::with_address(
                bob_donation_address,
                donation_ratio,
                "Tip to the developers".to_string(),
            )?,
        ]);
        monero_receive_pool.assert_sum_to_one()?;

        let (bob_swap, _) = ctx.bob_swap_with_pool(monero_receive_pool).await;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let (bob_state, alice_state) = join!(bob_swap, alice_swap);

        ctx.assert_alice_redeemed(alice_state??).await;
        ctx.assert_bob_redeemed(bob_state??).await;

        let total_xmr_pico = ctx.xmr_amount().as_pico();
        let expected_main_pico = (total_xmr_pico as f64 * (1.0 - DONATION_RATIO as f64)) as u64;
        let expected_donation_pico = (total_xmr_pico as f64 * DONATION_RATIO as f64) as u64;

        ctx.assert_eventual_balance_within(
            &ctx.bob_monero_wallet,
            expected_main_pico,
            SPLIT_TOLERANCE_PICO,
        )
        .await;
        ctx.assert_eventual_balance_within(
            &ctx.bob_donation_monero_wallet,
            expected_donation_pico,
            SPLIT_TOLERANCE_PICO,
        )
        .await;

        Ok(())
    })
    .await;
}
