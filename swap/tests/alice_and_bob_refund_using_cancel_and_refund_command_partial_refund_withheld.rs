pub mod harness;

use std::time::Duration;

use harness::alice_run_until::is_xmr_lock_transaction_sent;
use harness::bob_run_until::is_btc_locked;
use harness::SlowAmnestyConfig;
use rust_decimal::Decimal;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use swap::{asb, cli};
use swap_env::config::RefundPolicy;

/// Bob locks BTC, Alice locks XMR but stops cooperating.
/// Bob manually cancels and does a partial refund via CLI, then calls refund
/// again to reclaim the deposit. Alice withholds the deposit before Bob's
/// reclaim timelock expires, so Bob ends up in BtcWithheld.
#[tokio::test]
async fn given_partial_refund_alice_withholds_deposit_while_bob_reclaims_via_cli() {
    let refund_policy = Some(RefundPolicy {
        anti_spam_deposit_ratio: Decimal::new(5, 2),
        always_withhold_deposit: true,
    });

    harness::setup_test(SlowAmnestyConfig, None, refund_policy, |mut ctx| async move {
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

        // Restart Alice so she can refund XMR and publish TxWithhold via manual command
        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;

        // Spawn Alice's manual refund (runs in background: refunds XMR, publishes TxWithhold)
        let alice_refund = tokio::spawn(asb::refund(
            alice_swap.swap_id,
            alice_swap.bitcoin_wallet,
            alice_swap.monero_wallet,
            alice_swap.db,
        ));

        // Generate Monero blocks so Alice's XMR refund confirms in time
        tokio::time::sleep(Duration::from_secs(15)).await;
        ctx.monero.generate_blocks().await?;

        // Bob calls refund again — races reclaim against Alice's TxWithhold
        // Alice's withhold should win because SlowAmnestyConfig has a long timelock
        let bob_state =
            cli::refund(bob_swap.id, bob_swap.bitcoin_wallet, bob_swap.db).await?;

        ctx.assert_bob_withheld(bob_state).await;

        // Alice should be in withhold confirmed state
        let alice_state = alice_refund.await??;
        ctx.assert_alice_withhold_confirmed(alice_state).await;

        Ok(())
    })
    .await
}
