pub mod harness;

use std::time::Duration;

use harness::FastAmnestyConfig;
use rust_decimal::Decimal;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::{alice, bob};
use swap_controller_api::AsbApiClient;
use swap_env::config::RefundPolicy;
use swap_machine::bob::BobState;

use crate::harness::alice_run_until::is_xmr_refunded;
use crate::harness::bob_run_until;

/// Bob locks Btc and Alice locks Xmr. Alice does not act so Bob does a partial
/// refund. Alice burns the refund, then later grants final amnesty to Bob.
/// NOTE: This test cannot pass yet because we haven't implemented the manual
/// trigger for final amnesty. BtcRefundBurnConfirmed is currently terminal.
#[tokio::test]
async fn given_partial_refund_alice_grants_final_amnesty() {
    // Use 95% refund ratio - Bob gets 95% immediately, 5% locked in amnesty
    // Alice burns the amnesty, then grants final amnesty
    let refund_policy = Some(RefundPolicy {
        anti_spam_deposit_ratio: Decimal::new(95, 2), // 0.95 = 95%
        always_withhold_deposit: true,
    });

    harness::setup_test(
        FastAmnestyConfig,
        None,
        refund_policy,
        |mut ctx| async move {
            let (bob_swap, bob_app_handle) = ctx.bob_swap().await;
            let bob_state = tokio::spawn(bob::run_until(
                bob_swap,
                bob_run_until::is_btc_partially_refunded,
            ));

            let alice_swap = ctx.alice_next_swap().await;
            let alice_swap = tokio::spawn(alice::run_until(
                alice_swap,
                is_xmr_refunded,
                FixedRate::default(),
            ));

            // Wait for bob to partially refund - stop here such that he doesn't publish amnesty
            // TODO: fix regtest blocktimes instead
            let _bob_state = bob_state.await??;

            let alice_state = alice_swap.await??;
            assert!(matches!(alice_state, AliceState::XmrRefunded { .. }));

            ctx.monero.generate_blocks().await?;

            // Restart alice and wait for bob to be burnt.
            ctx.restart_alice().await;
            let alice_swap = ctx.alice_next_swap().await;
            let swap_id = alice_swap.swap_id;
            let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

            // Give alice time to publish TxRefundBurn before restarting bob
            tokio::time::sleep(Duration::from_secs(20)).await;

            let (bob_swap, bob_app_handle) = ctx
                .stop_and_resume_bob_from_db(bob_app_handle, swap_id)
                .await;
            let bob_state = tokio::spawn(bob::run(bob_swap)); // Bob should stop automatically after BtcRefundBurnt

            let alice_state = alice_swap.await??;
            assert!(matches!(
                alice_state,
                AliceState::BtcWithholdConfirmed { .. }
            ));

            let bob_state = bob_state.await??;
            assert!(matches!(bob_state, BobState::BtcWithheld(..)));

            // Simulate alice's controller sending the final amnesty command via `controller` cli
            ctx.restart_alice().await;
            ctx.alice_rpc_client.grant_mercy(swap_id).await?;

            let alice_swap = ctx.alice_next_swap().await;
            let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

            let (bob_swap, _) = ctx
                .stop_and_resume_bob_from_db(bob_app_handle, swap_id)
                .await;
            assert!(matches!(bob_swap.state, BobState::BtcWithheld(..)));

            let alice_state = alice_swap.await??;
            // Only start bob again after alice published the tx. otherwise bob immediately
            // terminates when not finding the tx.
            // TODO: maybe make bob check for a few minutes before giving up?
            let bob_state = tokio::spawn(bob::run(bob_swap));
            let bob_state = bob_state.await??;

            assert!(
                matches!(
                    alice_state,
                    AliceState::BtcMercyConfirmed { .. }
                ),
                "Actual state: {alice_state}"
            );
            assert!(
                matches!(bob_state, bob::BobState::BtcMercyConfirmed(..)),
                "Actual state: {bob_state}"
            );

            Ok(())
        },
    )
    .await;
}
