pub mod harness;

use harness::FastPunishConfig;
use harness::bob_run_until::is_btc_locked;
use swap::asb::FixedRate;
use swap::network::cooperative_xmr_redeem_after_punish::CooperativeXmrRedeemRejectReason;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

/// Bob locks BTC, Alice locks XMR, Bob goes dark, Alice punishes. We then tamper
/// with Alice's persisted state so the swap looks malicious (abnormally high
/// TxCancel fee -> large BTC loss). When the punished Bob asks Alice to
/// cooperatively redeem the XMR, Alice must refuse with `MaliciousRequest`.
#[tokio::test]
async fn alice_refuses_cooperative_redeem_for_malicious_swap() {
    harness::setup_test(FastPunishConfig, None, None, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let bob_state = bob_swap.await??;
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let alice_state = alice_swap.await??;
        ctx.assert_alice_punished(alice_state).await;

        // Make the swap look malicious from Alice's side: an abnormally high
        // TxCancel fee (half the swap). Because TxPunish spends TxCancel's output,
        // this trips Alice's holistic loss check (lost_btc > btc / 4).
        ctx.corrupt_alice_state(bob_swap_id, |state| {
            let AliceState::BtcPunished { state3, .. } = state else {
                panic!("expected Alice in BtcPunished, was {state:?}");
            };
            state3.tx_cancel_fee = bitcoin::Amount::from_sat(state3.btc.to_sat() / 2);
        })
        .await;

        // Resume Bob: he detects the punishment, moves to BtcPunished, and asks
        // Alice to cooperatively redeem the XMR.
        let (bob_swap, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        // Alice refuses -> bob::run errors out carrying the rejection reason.
        let error = bob::run(bob_swap)
            .await
            .expect_err("Bob's cooperative redeem must be refused for a malicious swap");

        let reason = error
            .downcast_ref::<CooperativeXmrRedeemRejectReason>()
            .expect("error should carry the cooperative-redeem rejection reason");
        assert!(
            matches!(reason, CooperativeXmrRedeemRejectReason::MaliciousRequest),
            "expected MaliciousRequest, got: {reason:?}"
        );

        Ok(())
    })
    .await;
}
