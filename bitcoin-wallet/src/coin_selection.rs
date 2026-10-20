use anyhow::{Result, ensure};
use bdk_wallet::WeightedUtxo;
use bdk_wallet::coin_selection::{
    CoinSelectionAlgorithm, CoinSelectionResult, DefaultCoinSelectionAlgorithm, InsufficientFunds,
    LargestFirstCoinSelection,
};
use bitcoin::secp256k1::rand::RngCore;
use bitcoin::{Amount, FeeRate, Script};

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
