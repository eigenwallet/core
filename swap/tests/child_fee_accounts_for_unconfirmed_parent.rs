pub mod harness;

use anyhow::{Context, Result};
use bitcoin::Amount;
use harness::FastCancelConfig;

fn psbt_fee_sats(psbt: &bitcoin::psbt::Psbt) -> u64 {
    let input_value: u64 = psbt
        .inputs
        .iter()
        .map(|input| {
            input
                .witness_utxo
                .as_ref()
                .expect("missing witness_utxo")
                .value
                .to_sat()
        })
        .sum();

    let output_value: u64 = psbt
        .unsigned_tx
        .output
        .iter()
        .map(|output| output.value.to_sat())
        .sum();

    input_value.saturating_sub(output_value)
}

#[tokio::test]
async fn child_fee_accounts_for_unconfirmed_parent() -> Result<()> {
    harness::setup_test(FastCancelConfig, None, None, |mut ctx| async move {
        let (bob_swap, bob_handle) = ctx.bob_swap().await;
        let wallet = bob_swap.bitcoin_wallet.clone();

        bob_handle.abort();

        let balance = wallet.balance().await?;
        let parent_fee = Amount::from_sat(200);
        let parent_amount = balance
            .checked_sub(parent_fee)
            .context("balance too small for parent transaction")?;

        let parent_dest = wallet.new_address().await?;
        let parent_psbt = wallet
            .send_to_address(parent_dest, parent_amount, parent_fee, None)
            .await?;

        let parent_tx = wallet.sign_and_finalize(parent_psbt).await?;
        let parent_txid = parent_tx.compute_txid();

        wallet
            .ensure_broadcasted(parent_tx.clone(), "low-fee-parent")
            .await?;

        wallet.sync().await?;

        let child_dest = wallet.new_address().await?;
        let child_amount = Amount::from_sat(100_000);
        let child_psbt = wallet
            .send_to_address_dynamic_fee(child_dest, child_amount, None)
            .await?;

        assert!(
            child_psbt
                .unsigned_tx
                .input
                .iter()
                .any(|input| input.previous_output.txid == parent_txid),
            "child tx did not spend the unconfirmed parent output"
        );

        let child_fee_sat = psbt_fee_sats(&child_psbt);
        let child_vbytes = child_psbt.unsigned_tx.weight().to_vbytes_floor() as u64;
        let parent_vbytes = parent_tx.weight().to_vbytes_floor() as u64;

        // Minimum child fee needed for the combined package
        // (parent + child) to reach ~1 sat/vB relay feerate
        let min_package_relay_child_fee = parent_vbytes
            .saturating_add(child_vbytes)
            .saturating_sub(parent_fee.to_sat());

        assert!(
            child_fee_sat >= min_package_relay_child_fee,
            "CPFP fee too low: child_fee={} sat, expected at least {} sat (parent_vbytes={}, child_vbytes={}, parent_fee={} sat)",
            child_fee_sat,
            min_package_relay_child_fee,
            parent_vbytes,
            child_vbytes,
            parent_fee.to_sat(),
        );

        let package_fee_sat = parent_fee.to_sat().saturating_add(child_fee_sat);
        let package_vbytes = parent_vbytes.saturating_add(child_vbytes);

        let parent_feerate = parent_fee.to_sat() as f64 / parent_vbytes as f64;
        let package_feerate = package_fee_sat as f64 / package_vbytes as f64;

        assert!(
            package_feerate > parent_feerate,
            "CPFP did not improve package feerate (parent={} sat/vB, package={} sat/vB)",
            parent_feerate,
            package_feerate,
        );

        Ok(())
    })
    .await;

    Ok(())
}
