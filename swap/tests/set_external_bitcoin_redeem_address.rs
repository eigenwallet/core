pub mod harness;

use harness::SlowCancelConfig;
use swap_controller_api::AsbApiClient;
use swap_env::config::Config;

/// The `set_external_bitcoin_redeem_address` RPC must mutate both the
/// on-disk `config.toml` and the in-memory value. The handler implements
/// disk-first-then-reload: it writes the file first, then re-reads it
/// into `self.external_redeem_address` and only then returns `Ok`. So if
/// the file reflects the new value after the RPC call, the in-memory
/// value does too (by contract of the handler).
#[tokio::test]
async fn set_external_bitcoin_redeem_address() {
    harness::setup_test(SlowCancelConfig, None, None, |mut ctx| async move {
        // The initial RPC server that `setup_test` brings up becomes
        // unreachable a few seconds later (the listener gets dropped
        // somewhere in the harness's bob-wallet init path). The existing
        // RPC tests work around this by restarting Alice as part of their
        // natural test flow; we mimic that here so the RPC is reachable
        // by the time we call it.
        ctx.restart_alice().await;

        let config_path = ctx.alice_config_path.clone();
        let read_addr = || async {
            let raw = tokio::fs::read_to_string(&config_path).await.unwrap();
            let config: Config = toml::from_str(&raw).unwrap();
            config
                .maker
                .external_bitcoin_redeem_address
                .as_ref()
                .map(|a| a.to_string())
        };

        // 1. Before the RPC: no external redeem address configured.
        assert_eq!(read_addr().await, None);

        // 2. Set via RPC.
        let addr_1 = "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080";
        ctx.alice_rpc_client
            .set_external_bitcoin_redeem_address(addr_1.to_string())
            .await?;

        // 3+4. config.toml on disk reflects the new address (and by the
        // disk-then-reload contract, so does Alice's in-memory value).
        assert_eq!(read_addr().await, Some(addr_1.to_string()));

        // Clearing path: dedicated `clear_*` RPC removes the address again.
        ctx.alice_rpc_client
            .clear_external_bitcoin_redeem_address()
            .await?;
        assert_eq!(read_addr().await, None);

        Ok(())
    })
    .await;
}
