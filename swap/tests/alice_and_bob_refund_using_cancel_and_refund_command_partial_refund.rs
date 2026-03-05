pub mod harness;

use harness::alice_run_until::is_xmr_lock_transaction_sent;
use harness::bob_run_until::is_btc_locked;
use harness::FastAmnestyConfig;
use rust_decimal::Decimal;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use swap::{asb, cli};
use swap_env::config::RefundPolicy;

/// Bob locks BTC, Alice locks XMR but stops cooperating.
/// Bob manually cancels and does a partial refund via CLI, then calls refund
/// again to reclaim the anti-spam deposit. Alice does NOT withhold, so Bob ends
/// up in BtcReclaimConfirmed.
#[tokio::test]
async fn given_partial_refund_bob_manually_reclaims_deposit_via_cli() {
    let refund_policy = Some(RefundPolicy {
        anti_spam_deposit_ratio: Decimal::new(5, 2),
        always_withhold_deposit: false,
    });

    harness::setup_test(FastAmnestyConfig, None, refund_policy, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_xmr_lock_transaction_sent,
            FixedRate::default(),
        ));

        let bob_state = bob_swap.await??;
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let alice_state = alice_swap.await??;
        assert!(matches!(alice_state, AliceState::XmrLockTransactionSent { .. }));

        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;

        // Wait for cancel timelock to expire
        if let BobState::BtcLocked { state3, .. } = bob_swap.state.clone() {
            bob_swap
                .bitcoin_wallet
                .subscribe_to(Box::new(state3.tx_lock))
                .await
                .wait_until_confirmed_with(state3.cancel_timelock)
                .await?;
        } else {
            panic!("Bob in unexpected state {}", bob_swap.state);
        }

        // Bob manually cancels + partial refunds
        bob_join_handle.abort();
        let bob_state =
            cli::cancel_and_refund(bob_swap.id, bob_swap.bitcoin_wallet.clone(), bob_swap.db.clone()).await?;
        assert!(matches!(bob_state, BobState::BtcPartiallyRefunded { .. }));

        // Bob calls refund again to reclaim the anti-spam deposit
        let bob_state =
            cli::refund(bob_swap.id, bob_swap.bitcoin_wallet, bob_swap.db).await?;

        ctx.assert_bob_amnesty_received(bob_state).await;

        // Manually refund Alice's XMR
        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_state = asb::refund(
            alice_swap.swap_id,
            alice_swap.bitcoin_wallet,
            alice_swap.monero_wallet,
            alice_swap.db,
        )
        .await?;

        ctx.assert_alice_refunded(alice_state).await;

        Ok(())
    })
    .await
}
