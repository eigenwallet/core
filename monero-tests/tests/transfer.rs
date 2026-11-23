//! End-to-end integration test for `monero_wallet_ng::transfer::construct_transfer`.

use std::collections::HashMap;
use std::time::Duration;

use monero_daemon_rpc::MoneroDaemon;
use monero_harness::Cli;
use monero_interface::{
    ProvidesBlockchainMeta, ProvidesScannableBlocks, PublishTransaction,
};
use monero_oxide_wallet::Scanner;
use monero_oxide_wallet::address::{AddressType, MoneroAddress, Network};
use monero_oxide_wallet::ed25519::Scalar;
use monero_wallet_ng::rpc::{ProvidesTransactionStatus, TransactionStatus};
use monero_wallet_ng::state::{OutputId, ReceivedOutput, WalletState};
use monero_wallet_ng::transfer;
use monero_wallet_ng::util::public_key;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

/// Random view pair for a legacy mainnet address
fn random_view_pair() -> (Zeroizing<Scalar>, Zeroizing<Scalar>, MoneroAddress) {
    let spend = Zeroizing::new(Scalar::random(&mut OsRng));
    let view = Zeroizing::new(Scalar::random(&mut OsRng));
    let address = MoneroAddress::new(
        Network::Mainnet,
        AddressType::Legacy,
        public_key(&spend),
        public_key(&view),
    );
    (spend, view, address)
}

/// Scan the funding transactions of a raw-key wallet into a
/// [`WalletState`] plus the outputs map the transfer engine expects.
async fn fund_state(
    daemon: &MoneroDaemon<monero_simple_request_rpc::SimpleRequestTransport>,
    scanner: &mut Scanner,
    receipts: &[monero_sys::TxReceipt],
) -> anyhow::Result<(WalletState, HashMap<OutputId, monero_oxide_wallet::WalletOutput>)> {
    let mut state = WalletState::default();
    let mut outputs = HashMap::new();

    for receipt in receipts {
        let txid: [u8; 32] = hex::decode(&receipt.txid)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("txid must be 32 bytes"))?;

        let TransactionStatus::InBlock { block_height } = daemon.transaction_status(txid).await?
        else {
            anyhow::bail!("funding tx is not in a block");
        };

        let blocks = daemon
            .contiguous_scannable_blocks(block_height as usize..=block_height as usize)
            .await?;
        let block = blocks
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("funding block at {block_height} missing"))?;

        for output in scanner.scan(block)?.ignore_additional_timelock() {
            if output.transaction() != txid {
                continue;
            }

            let id = transfer::output_id(&output);
            state.receive(ReceivedOutput {
                id,
                amount_pico: output.commitment().amount,
                received_at_height: block_height,
                is_coinbase: false,
                additional_timelock: output.additional_timelock(),
            });
            outputs.insert(id, output);
        }
    }

    Ok((state, outputs))
}

#[tokio::test]
async fn transfer_pays_exact_amounts_and_returns_change() -> anyhow::Result<()> {
    let cli = Cli::default();
    let (monero, _monerod_container, _wallet_containers) =
        monero_harness::Monero::new_with_sync_specified(&cli, vec!["alice", "bob"], false).await?;

    monero.init_and_start_miner().await?;

    let miner = monero.wallet("miner")?;
    let miner_address = miner.address().await?.to_string();
    let alice = monero.wallet("alice")?;
    let alice_address = alice.address().await?;
    let bob = monero.wallet("bob")?;
    let bob_address = bob.address().await?;

    // Source wallet exists only as keys; we hand them directly to the
    // transfer engine.
    let (source_spend, source_view, source_address) = random_view_pair();

    // Four funding transfers of 0.15 XMR each with a 0.45 XMR target:
    // the bare-target prefix is exactly three outputs (0.45), so ANY
    // positive fee forces the grow loop to take the fourth. the grow
    // branch is deterministic, not fee-dependent.
    let funding_each: u64 = 150_000_000_000; // 0.15 XMR
    let receipts = [
        miner.transfer(&source_address, funding_each).await?,
        miner.transfer(&source_address, funding_each).await?,
        miner.transfer(&source_address, funding_each).await?,
        miner.transfer(&source_address, funding_each).await?,
    ];

    // Mine past the 10-block spendable window.
    monero.monerod().generate_blocks(15, &miner_address).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let daemon = monero.monerod().client().clone();

    let mut scanner = Scanner::new(monero_oxide_wallet::ViewPair::new(
        public_key(&source_spend),
        source_view.clone(),
    )?);

    let (mut state, outputs) = fund_state(&daemon, &mut scanner, &receipts).await?;

    let funded_total = funding_each * 4;
    assert_eq!(state.balance()?, funded_total);
    let inclusion_height = daemon.latest_block_number().await? as u64 + 1;
    assert_eq!(state.unlocked_balance(inclusion_height, 0)?, funded_total);

    let amount_a: u64 = 300_000_000_000; // 0.3 XMR
    let amount_b: u64 = 150_000_000_000; // 0.15 XMR

    let transfer = transfer::construct_transfer(
        &daemon,
        &source_spend,
        &outputs,
        &state,
        vec![(alice_address, amount_a), (bob_address, amount_b)],
        source_address,
        // The inclusion height: tip number + 1, see the state module
        // docs.
        inclusion_height,
        // No timelocked outputs in play, the adjusted time is unused.
        0,
        None,
    )
    .await?;

    // The bookkeeping the caller owns: pending before publishing.
    state.mark_pending(&transfer.input_ids)?;
    assert_eq!(state.pending_balance()?, funded_total);

    // The grow branch fired: three outputs cover the bare target, the
    // fee forced the fourth.
    assert_eq!(transfer.input_ids.len(), 4);

    // Inputs = destinations + fee + change, nothing vanishes.
    assert_eq!(
        transfer.fee_pico + transfer.change_pico,
        funded_total - amount_a - amount_b
    );
    assert!(transfer.change_pico > 0);

    daemon.publish_transaction(&transfer.transaction).await?;

    monero.monerod().generate_blocks(15, &miner_address).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    // The bookkeeping the caller owns: confirm the spend at the height
    // the chain actually included it.
    let tx_hash = transfer.transaction.hash();
    let TransactionStatus::InBlock { block_height } = daemon.transaction_status(tx_hash).await?
    else {
        anyhow::bail!("transfer tx is not in a block");
    };
    state.confirm_spent(&transfer.input_ids, block_height)?;
    assert_eq!(state.pending_balance()?, 0);
    assert_eq!(state.balance()?, 0);

    alice.refresh().await?;
    bob.refresh().await?;

    // The gate: exact amounts, no fee envelope.
    assert_eq!(alice.balance().await?, amount_a);
    assert_eq!(bob.balance().await?, amount_b);

    Ok(())
}
