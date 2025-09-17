pub mod harness;

use anyhow::Context;
use harness::FastPunishConfig;
use swap::asb::FixedRate;
use swap::monero::{TransferProof, TxHash};
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob, State};
use swap_controller_api::AsbApiClient;

use crate::harness::bob_run_until::{is_btc_locked, is_btc_punished};

/// Bob locks Btc and Alice locks Xmr. Bob does not act; he fails to send Alice
/// the encsig and fail to refund or redeem. Alice punishes. Bob then cooperates with Alice and redeems XMR with her key.
/// But this time, we use the manual export of the cooperative redeem key via the asb-controller.
#[tokio::test]
async fn alice_and_bob_manual_cooperative_redeem_after_punish() {
    harness::setup_test(FastPunishConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let bob_state = bob_swap.await??;
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let alice_state = alice_swap.await??;
        ctx.assert_alice_punished(alice_state).await;

        // Let bob realize he was punished
        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        let bob_state = tokio::spawn(bob::run_until(bob_swap, is_btc_punished)).await??;
        assert!(matches!(bob_state, BobState::BtcPunished { .. }));

        // Manually do the cooperative redeem via rpc server
        let manual_cooperative_redeem_info = ctx
            .alice_rpc_client
            .cooperative_redeem_info(bob_swap_id)
            .await?
            .context("swap not found")?;
        let BobState::BtcPunished { state, .. } = bob_state else {
            panic!("bob unexpected state")
        };
        let state5 = state.attempt_cooperative_redeem(
            manual_cooperative_redeem_info.s_a.scalar,
            TransferProof::new(
                TxHash(manual_cooperative_redeem_info.lock_tx_id),
                manual_cooperative_redeem_info.lock_tx_key,
            ),
        )?;
        let new_state = State::Bob(BobState::BtcRedeemed(state5));
        // Insert new state (BtcRedeemed  but we got the key manually)
        ctx.bob_swap()
            .await
            .0
            .db
            .insert_latest_state(bob_swap_id, new_state)
            .await?;

        // Now try to have bob finish the swap normally
        let (bob_swap, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;

        let bob_state = bob::run(bob_swap).await?;
        ctx.assert_bob_redeemed(bob_state).await;
        Ok(())
    })
    .await;
}
