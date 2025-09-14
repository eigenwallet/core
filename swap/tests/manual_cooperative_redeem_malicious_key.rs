pub mod harness;

use anyhow::Context;
use harness::FastPunishConfig;
use swap::asb::FixedRate;
use swap::monero::{TransferProof, TxHash};
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob, State};
use swap_controller_api::AsbApiClient;

use crate::harness::bob_run_until::is_btc_punished;

/// Bob locks Btc and Alice locks Xmr. Bob does not act; he fails to send Alice
/// the encsig and fail to refund or redeem. Alice punishes. Bob then cooperates with Alice and redeems XMR with her key.
/// But this time, we use the manual export of the cooperative redeem key via the asb-controller.
/// And also, alice sends a malicious key! So we expect the cooperative redeem check to fail before changing states.
#[tokio::test]
async fn alice_and_bob_manual_cooperative_redeem_after_punish() {
    harness::setup_test(FastPunishConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_punished));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let bob_state = bob_swap.await??;
        assert!(matches!(bob_state, BobState::BtcPunished { .. }));

        let alice_state = alice_swap.await??;
        ctx.assert_alice_punished(alice_state).await;

        // Manually do the cooperative redeem via rpc server
        let mut manual_cooperative_redeem_info = ctx
            .alice_rpc_client
            .get_coop_redeem_info(bob_swap_id)
            .await?
            .context("swap not found")?;
        let BobState::BtcPunished { state, .. } = bob_state else {
            panic!("bob unexpected state")
        };
        // Malicous: alice doesn't give the correct secret key
        manual_cooperative_redeem_info.inner.scalar =
            manual_cooperative_redeem_info.inner.scalar.invert();
        let state5 = state.attempt_cooperative_redeem(
            manual_cooperative_redeem_info.inner.scalar,
            TransferProof::new(
                TxHash(manual_cooperative_redeem_info.lock_tx_id),
                manual_cooperative_redeem_info.lock_tx_key,
            ),
        );
        assert!(
            state5.is_err(),
            "cooperative redeem key doesn't match actual secret key - the check should fail"
        );
        Ok(())
    })
    .await;
}
