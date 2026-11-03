use anyhow::{Result, ensure};
use bdk_wallet::coin_selection::{
    CoinSelectionAlgorithm, CoinSelectionResult, DefaultCoinSelectionAlgorithm, InsufficientFunds,
    LargestFirstCoinSelection,
};
use bdk_wallet::{TxBuilder, Wallet, WeightedUtxo};
use bitcoin::secp256k1::rand::RngCore;
use bitcoin::transaction::Version;
use bitcoin::{Amount, FeeRate, OutPoint, Script, Txid};
use std::collections::HashSet;

/// Start a transaction capped at `max_inputs` that never spends an output of an
/// unconfirmed v3 transaction. We build v2 transactions, and TRUC policy
/// rejects a non-v3 child of an unconfirmed v3 parent.
pub(crate) fn build_tx(
    wallet: &mut Wallet,
    max_inputs: Option<usize>,
    drain: bool,
) -> Result<TxBuilder<'_, InputLimitCoinSelection>> {
    let unconfirmed_v3_txids: HashSet<Txid> = wallet
        .transactions()
        .filter(|tx| !tx.chain_position.is_confirmed() && tx.tx_node.tx.version == Version(3))
        .map(|tx| tx.tx_node.txid)
        .collect();
    let unconfirmed_v3_outputs: Vec<OutPoint> = wallet
        .list_unspent()
        .map(|utxo| utxo.outpoint)
        .filter(|outpoint| unconfirmed_v3_txids.contains(&outpoint.txid))
        .collect();

    let mut tx_builder = wallet
        .build_tx()
        .coin_selection(InputLimitCoinSelection::new(max_inputs, drain)?);
    tx_builder.unspendable(unconfirmed_v3_outputs);
    Ok(tx_builder)
}

/// Apply the input cap after BDK has filtered out unspendable wallet outputs.
/// `drain` is only used with our own drain builders, which mark all eligible
/// outputs as required. Ordinary sends keep BDK's required inputs intact.
#[derive(Debug)]
pub(crate) struct InputLimitCoinSelection {
    maximum: Option<usize>,
    drain: bool,
}

impl InputLimitCoinSelection {
    pub fn new(maximum: Option<usize>, drain: bool) -> Result<Self> {
        ensure!(maximum != Some(0), "Input limit must be positive");
        Ok(Self { maximum, drain })
    }
}

impl CoinSelectionAlgorithm for InputLimitCoinSelection {
    fn coin_select<R: RngCore>(
        &self,
        mut required: Vec<WeightedUtxo>,
        mut optional: Vec<WeightedUtxo>,
        fee_rate: FeeRate,
        target_amount: Amount,
        drain_script: &Script,
        rng: &mut R,
    ) -> Result<CoinSelectionResult, InsufficientFunds> {
        let Some(maximum) = self.maximum else {
            return DefaultCoinSelectionAlgorithm::default().coin_select(
                required,
                optional,
                fee_rate,
                target_amount,
                drain_script,
                rng,
            );
        };
        let largest_first = |utxo: &WeightedUtxo| {
            std::cmp::Reverse((utxo.utxo.txout().value, utxo.utxo.outpoint()))
        };
        if self.drain {
            required.extend(optional);
            required.sort_by_key(largest_first);
            required.truncate(maximum);
            return LargestFirstCoinSelection.coin_select(
                required,
                vec![],
                fee_rate,
                target_amount,
                drain_script,
                rng,
            );
        }
        if required.len() > maximum {
            return Err(InsufficientFunds {
                needed: target_amount,
                available: Amount::ZERO,
            });
        }
        optional.sort_by_key(largest_first);
        optional.truncate(maximum - required.len());
        LargestFirstCoinSelection.coin_select(
            required,
            optional,
            fee_rate,
            target_amount,
            drain_script,
            rng,
        )
    }
}
