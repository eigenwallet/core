pub mod harness;

use harness::FastAmnestyConfig;
use swap::protocol::bob::{self, BobState};

#[tokio::test]
async fn alice_rejects_bob_lock_fee_below_her_local_floor() {
    harness::setup_test(FastAmnestyConfig, None, None, |ctx| async move {
        let (mut bob_swap, event_loop) = ctx
            .bob_params
            .new_swap(bitcoin::Amount::from_sat(1_000_000))
            .await?;
        let BobState::Started { tx_lock_fee, .. } = &mut bob_swap.state else {
            panic!("a new swap must start in Started");
        };
        *tx_lock_fee = bitcoin::Amount::ZERO;
        let bob_event_loop = tokio::spawn(event_loop.run());

        let error = bob::run(bob_swap)
            .await
            .expect_err("Alice must reject the insufficient Bitcoin lock fee during setup");
        let error = format!("{error:#}");
        assert!(
            error.contains("Transaction fee is too low compared to our estimate"),
            "unexpected swap rejection: {error}"
        );
        bob_event_loop.abort();

        Ok(())
    })
    .await;
}
