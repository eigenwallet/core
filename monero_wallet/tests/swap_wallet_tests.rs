mod harness;

use anyhow::{Context, Result};
use harness::{TestContext, CONFIRM_BLOCKS, WALLET_NAME};
use monero_address::{AddressType, MoneroAddress, Network};
use monero_oxide_ext::{PrivateKey, PublicKey};
use rand::rngs::OsRng;
use swap_core::monero::primitives::{PrivateViewKey, TxHash};
use uuid::Uuid;
use std::sync::Arc;
use monero_wallet::Wallets;

integration_test!(test_swap_wallet_detects_incoming_balance);

async fn test_swap_wallet_detects_incoming_balance_case(ctx: Arc<TestContext>) -> Result<()> {
    let mut rng = OsRng;

    let spend_key_prim = PrivateViewKey::new_random(&mut rng);
    let view_key_prim = PrivateViewKey::new_random(&mut rng);

    let spend_key: PrivateKey = spend_key_prim.into();
    let view_key_for_addr: PrivateKey = view_key_prim.clone().into();

    let address = MoneroAddress::new(
        Network::Mainnet,
        AddressType::Legacy,
        PublicKey::from_private_key(&spend_key).decompress(),
        PublicKey::from_private_key(&view_key_for_addr).decompress(),
    );

    let amount = 1_000_000_000_000u64;
    let receipt = ctx
        .monero
        .wallet("miner")?
        .transfer(&address, amount)
        .await
        .context("funding swap address")?;

    let tx_hash = TxHash(receipt.txid);

    ctx.generate_blocks(CONFIRM_BLOCKS).await?;

    let wallets = Wallets::new(
            ctx.wallet_dir.path().to_path_buf(),
            WALLET_NAME.to_string(),
            ctx.daemon.clone(),
            Network::Mainnet,
            true,
            None,
            None,
        )
        .await
        .context("creating Wallets")?;

    let main_wallet = wallets.main_wallet().await;
    main_wallet.refresh_blocking().await?;
    let restore_height = main_wallet.blockchain_height().await?.saturating_sub(15);

    let swap_wallet = wallets
        .swap_wallet_spendable(Uuid::new_v4(), spend_key, view_key_prim, tx_hash)
        .await
        .context("creating spendable swap wallet")?;

    swap_wallet.set_restore_height(restore_height).await?;
    swap_wallet.refresh_blocking().await?;

    ctx.wait_for_unlocked_balance(&swap_wallet, amount, 120).await?;

    let balance = swap_wallet.total_balance().await?;
    assert_eq!(
        balance.as_pico(),
        amount,
        "swap wallet balance mismatch"
    );

    Ok(())
}
