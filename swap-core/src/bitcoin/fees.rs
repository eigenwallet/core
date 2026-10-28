//! Swap setup v5 fees calculated from locally constructed transaction templates.
use super::*;
use ::bitcoin::{OutPoint, Psbt, Script, TxIn, TxOut, Witness, absolute, transaction};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Multiplier of the shared base fee, encoded in hundredths on the wire.
///
/// Keep at least a twofold fee margin over cancel, so that redeem can replace an
/// unconfirmed cancel under the replace-by-fee rules. Refund's confirmation delay
/// prevents it from contributing CPFP fees to unconfirmed cancel under standard
/// relay policy. Both peers reject multipliers above 4.00, so that redeem cannot
/// use most of the spending-path budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct RedeemFeeMultiplier(u32);

impl Default for RedeemFeeMultiplier {
    fn default() -> Self {
        Self(250)
    }
}

impl TryFrom<u32> for RedeemFeeMultiplier {
    type Error = anyhow::Error;

    fn try_from(hundredths: u32) -> Result<Self> {
        ensure!(
            (200..=400).contains(&hundredths),
            "Redeem fee multiplier must be between 2.00 and 4.00"
        );
        Ok(Self(hundredths))
    }
}

impl From<RedeemFeeMultiplier> for u32 {
    fn from(value: RedeemFeeMultiplier) -> Self {
        value.0
    }
}

impl TryFrom<rust_decimal::Decimal> for RedeemFeeMultiplier {
    type Error = anyhow::Error;

    fn try_from(value: rust_decimal::Decimal) -> Result<Self> {
        let hundredths = value
            .checked_mul(rust_decimal::Decimal::from(100))
            .context("Redeem fee multiplier overflow")?;
        ensure!(
            hundredths.fract().is_zero(),
            "Redeem fee multiplier must be exactly representable with two decimal places"
        );
        u32::try_from(hundredths)
            .context("Redeem fee multiplier is outside the supported range")?
            .try_into()
    }
}

impl RedeemFeeMultiplier {
    pub fn fee(self, base: Amount) -> Result<Amount> {
        let product = base
            .to_sat()
            .checked_mul(u64::from(self.0))
            .context("Redeem fee overflow")?;
        let sats = (product / 100)
            .checked_add(u64::from(product % 100 != 0))
            .context("Redeem fee rounding overflow")?;
        Ok(Amount::from_sat(sats))
    }
}

/// Integer satoshis per 1,000 virtual bytes. The wire representation is exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapFeeRate(pub u64);

impl SwapFeeRate {
    /// Lowest rate either peer proposes or accepts (1 sat/vB).
    ///
    /// Presigned transactions may be published long after setup, and refund
    /// cannot raise an unconfirmed cancel's fee through CPFP. The relay minimum
    /// reported at setup time is therefore no safe lower bound.
    pub const MIN: SwapFeeRate = SwapFeeRate(1_000);

    /// Convert a local fee estimate, raised to at least [`Self::MIN`].
    pub fn from_estimate(estimate: ::bitcoin::FeeRate) -> Result<Self> {
        let rate = estimate
            .to_sat_per_kwu()
            .checked_mul(4)
            .context("Swap fee rate conversion overflow")?;
        Ok(Self(rate.max(Self::MIN.0)))
    }

    pub fn fee(self, vsize: u64) -> Result<Amount> {
        ensure!(self.0 > 0, "Swap fee rate must be positive");
        let product = self.0.checked_mul(vsize).context("Swap fee overflow")?;
        let sats = (product / 1000)
            .checked_add(u64::from(product % 1000 != 0))
            .context("Swap fee rounding overflow")?;
        Ok(Amount::from_sat(sats))
    }
}

/// Local policy, never sent by the peer. Budgets cover an entire spending path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapFeeLimits {
    pub min_rate: SwapFeeRate,
    pub max_rate: SwapFeeRate,
    pub max_path_fee: Amount,
}

/// Expected validation failures that can be reported to the swap peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SwapFeeError {
    #[error(
        "Proposed swap fee rate {rate} sat/kvB is outside local limits {min}..={max} sat/kvB",
        rate = .rate.0, min = .min.0, max = .max.0
    )]
    RateOutsideLimits {
        rate: SwapFeeRate,
        min: SwapFeeRate,
        max: SwapFeeRate,
    },
    #[error("Anti-spam deposit {deposit} is too small; minimum required is {minimum}")]
    AntiSpamDepositTooSmall { deposit: Amount, minimum: Amount },
    #[error("Swap spending path fees ({fees}) exceed local budget ({budget})")]
    SwapFeesExceedBudget { fees: Amount, budget: Amount },
}

impl SwapFeeLimits {
    pub fn validate_rate(self, rate: SwapFeeRate) -> Result<()> {
        ensure!(
            rate.0 > 0 && rate.0 >= self.min_rate.0 && rate.0 <= self.max_rate.0,
            SwapFeeError::RateOutsideLimits {
                rate,
                min: self.min_rate,
                max: self.max_rate,
            }
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapFees {
    pub max_vsize: u64,
    pub base: Amount,
    pub redeem: Amount,
}

/// All output scripts and keys are agreed before either side signs.
pub struct SwapFeeInputs<'a> {
    pub a: PublicKey,
    pub b: PublicKey,
    pub redeem_address: &'a Address,
    pub refund_address: &'a Address,
    pub punish_address: &'a Address,
    pub lock_amount: Amount,
    pub deposit: Amount,
    pub redeem_multiplier: RedeemFeeMultiplier,
}

impl SwapFeeInputs<'_> {
    pub fn calculate(&self, rate: SwapFeeRate, limits: SwapFeeLimits) -> Result<SwapFees> {
        limits.validate_rate(rate)?;
        ensure!(
            self.lock_amount <= Amount::MAX_MONEY,
            "Lock amount exceeds Bitcoin supply"
        );
        ensure!(
            self.deposit <= self.lock_amount,
            "Deposit exceeds lock amount"
        );
        let descriptor = build_shared_output_descriptor(self.a.0, self.b.0)?;
        let shared_script = descriptor.script_pubkey();
        let witness_script = descriptor.explicit_script()?;
        let max_vsize = self.max_signed_vsize(&shared_script, &witness_script)?;
        let base = rate.fee(max_vsize)?;
        let redeem = self.redeem_multiplier.fee(base)?;

        // Compare redeem with cancel -> full refund (two base fees), or with
        // cancel -> partial refund -> withhold -> mercy (four when using a deposit).
        let longest_path = base
            .checked_mul(if self.deposit == Amount::ZERO { 2 } else { 4 })
            .context("Spending path fee overflow")?
            .max(redeem);
        ensure!(
            longest_path <= limits.max_path_fee,
            SwapFeeError::SwapFeesExceedBudget {
                fees: longest_path,
                budget: limits.max_path_fee,
            }
        );
        self.validate_funding(base, redeem, &shared_script)?;

        Ok(SwapFees {
            max_vsize,
            base,
            redeem,
        })
    }

    fn max_signed_vsize(&self, shared_script: &Script, witness_script: &Script) -> Result<u64> {
        // Values and outpoints do not affect serialized size. Use funded templates
        // with zero fees to measure the actual constructors before validating values.
        let sizing_amount = Amount::ONE_BTC;
        let lock = TxLock::from_psbt(
            Psbt::from_unsigned_tx(Transaction {
                version: transaction::Version::TWO,
                lock_time: absolute::LockTime::ZERO,
                input: vec![TxIn {
                    previous_output: OutPoint::null(),
                    ..Default::default()
                }],
                output: vec![TxOut {
                    value: sizing_amount,
                    script_pubkey: shared_script.to_owned(),
                }],
            })?,
            self.a,
            self.b,
            sizing_amount,
        )?;
        let cancel = TxCancel::new(
            &lock,
            CancelTimelock::new(1),
            self.a,
            self.b,
            Amount::ZERO,
            CancelRefundVersion::Truc,
        )?;
        let redeem = TxRedeem::new(&lock, self.redeem_address, Amount::ZERO);
        let refund = TxFullRefund::new(&cancel, self.refund_address, Amount::ZERO);
        let early = TxEarlyRefund::new(&lock, self.refund_address, Amount::ZERO);
        let punish = TxPunish::new(
            &cancel,
            self.punish_address,
            PunishTimelock::new(1),
            Amount::ZERO,
        );
        // Include every output of redeem, including an anchor if its template has one.
        // Lock and wallet-created fee-bump transactions are deliberately excluded.
        let mut templates = vec![
            cancel.inner.clone(),
            redeem.inner,
            refund.inner,
            early.inner,
            punish.inner,
        ];
        // These spending paths exist only when the swap includes an anti-spam deposit.
        if self.deposit != Amount::ZERO {
            let partial = TxPartialRefund::new(
                &cancel,
                self.refund_address,
                self.a,
                self.b,
                Amount::from_sat(1_000_000),
                Amount::ZERO,
            )?;
            let reclaim = TxReclaim::new(
                &partial,
                self.refund_address,
                Amount::ZERO,
                RemainingRefundTimelock::new(1),
            )?;
            let withhold = TxWithhold::new(&partial, self.a, self.b, Amount::ZERO)?;
            let mercy = TxMercy::new(&withhold, self.refund_address, Amount::ZERO);
            templates.extend([partial.inner, reclaim.inner, withhold.inner, mercy.inner]);
        }
        templates
            .into_iter()
            .map(|mut tx| {
                // Two maximum DER signatures, each including its sighash byte, and
                // the actual 2-of-2 witness script. Transaction::vsize rounds up.
                tx.input[0].witness =
                    Witness::from_slice(&[vec![0; 73], vec![0; 73], witness_script.to_bytes()]);
                tx.vsize() as u64
            })
            .max()
            .context("No presigned transaction templates")
    }

    fn validate_funding(&self, base: Amount, redeem: Amount, shared_script: &Script) -> Result<()> {
        fn checked_output_amount(input: Amount, fee: Amount, script: &Script) -> Result<Amount> {
            let value = input
                .checked_sub(fee)
                .context("Insufficient transaction input value")?;
            ensure!(
                value > Amount::ZERO && value >= script.minimal_non_dust(),
                "Required transaction output is dust"
            );
            Ok(value)
        }
        checked_output_amount(self.lock_amount, Amount::ZERO, shared_script)?;
        checked_output_amount(
            self.lock_amount,
            redeem,
            &self.redeem_address.script_pubkey(),
        )?;
        checked_output_amount(self.lock_amount, base, &self.refund_address.script_pubkey())?; // early refund
        let cancelled = checked_output_amount(self.lock_amount, base, shared_script)?;
        checked_output_amount(cancelled, base, &self.punish_address.script_pubkey())?;
        checked_output_amount(cancelled, base, &self.refund_address.script_pubkey())?;
        if self.deposit != Amount::ZERO {
            // Withhold leaves a shared output after one fee; mercy leaves a
            // refund output after two. These also cover the deposit and reclaim.
            let minimum = base
                .checked_add(shared_script.minimal_non_dust())
                .context("Withhold minimum deposit overflow")?
                .max(
                    base.checked_mul(2)
                        .and_then(|fees| {
                            fees.checked_add(self.refund_address.script_pubkey().minimal_non_dust())
                        })
                        .context("Mercy minimum deposit overflow")?,
                );
            ensure!(
                self.deposit >= minimum,
                SwapFeeError::AntiSpamDepositTooSmall {
                    deposit: self.deposit,
                    minimum,
                }
            );
            let partial_cost = self
                .deposit
                .checked_add(base)
                .context("Partial refund amount overflow")?;
            checked_output_amount(
                cancelled,
                partial_cost,
                &self.refund_address.script_pubkey(),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::bitcoin::{Network, ScriptBuf};

    fn address(bytes: usize) -> Address {
        let script = if bytes == 22 {
            ScriptBuf::from_bytes([vec![0, 20], vec![1; 20]].concat())
        } else {
            ScriptBuf::from_bytes([vec![0, 32], vec![1; 32]].concat())
        };
        Address::from_script(&script, Network::Regtest).unwrap()
    }

    fn limits() -> SwapFeeLimits {
        SwapFeeLimits {
            min_rate: SwapFeeRate(1),
            max_rate: SwapFeeRate(u64::MAX),
            max_path_fee: Amount::MAX_MONEY,
        }
    }

    fn inputs<'a>(address: &'a Address) -> SwapFeeInputs<'a> {
        let mut rng = rand::thread_rng();
        SwapFeeInputs {
            a: SecretKey::new_random(&mut rng).public(),
            b: SecretKey::new_random(&mut rng).public(),
            redeem_address: address,
            refund_address: address,
            punish_address: address,
            lock_amount: Amount::from_sat(1_000_000),
            deposit: Amount::from_sat(100_000),
            redeem_multiplier: RedeemFeeMultiplier::try_from(200).unwrap(),
        }
    }

    #[test]
    fn deterministic_for_agreed_scripts_and_exact_two_to_one() {
        let address = address(22);
        let inputs = inputs(&address);
        let alice = inputs.calculate(SwapFeeRate(1250), limits()).unwrap();
        let bob = inputs.calculate(SwapFeeRate(1250), limits()).unwrap();
        assert_eq!(alice, bob);
        // 1 input, 2 outputs, two 73-byte signatures and a 70-byte witness script.
        assert_eq!(alice.max_vsize, 181);
        assert_eq!(alice.base.to_sat(), 227);
        assert_eq!(alice.redeem, alice.base * 2);
        let rate = SwapFeeRate(1250);
        let recovered: SwapFeeRate =
            serde_json::from_str(&serde_json::to_string(&rate).unwrap()).unwrap();
        assert_eq!(rate, recovered);
        assert_eq!(inputs.calculate(recovered, limits()).unwrap(), alice);
    }

    #[test]
    fn redeem_multiplier_has_exact_decimal_and_wire_encodings() {
        for (decimal, encoded) in [("2.00", 200), ("2.30", 230), ("3.000", 300), ("4", 400)] {
            let multiplier =
                RedeemFeeMultiplier::try_from(decimal.parse::<rust_decimal::Decimal>().unwrap())
                    .unwrap();
            assert_eq!(u32::from(multiplier), encoded);
            assert_eq!(
                serde_json::to_string(&multiplier).unwrap(),
                encoded.to_string()
            );
            assert_eq!(
                serde_json::from_str::<RedeemFeeMultiplier>(&encoded.to_string()).unwrap(),
                multiplier
            );
        }
        for decimal in ["1.99", "4.01", "2.301", "-2.30"] {
            assert!(
                RedeemFeeMultiplier::try_from(decimal.parse::<rust_decimal::Decimal>().unwrap())
                    .is_err()
            );
        }
        for invalid in ["0", "199", "401", "4294967296", "230.5"] {
            assert!(serde_json::from_str::<RedeemFeeMultiplier>(invalid).is_err());
        }
        assert_eq!(u32::from(RedeemFeeMultiplier::default()), 250);
        let multiplier = RedeemFeeMultiplier::try_from(230).unwrap();
        assert_eq!(multiplier.fee(Amount::from_sat(101)).unwrap().to_sat(), 233);
        assert!(multiplier.fee(Amount::from_sat(u64::MAX)).is_err());
    }

    #[test]
    fn budgets_cover_the_larger_of_redeem_and_the_refund_path() {
        let address = address(22);
        let mut inputs = inputs(&address);
        for deposit in [Amount::ZERO, Amount::from_sat(100_000)] {
            inputs.deposit = deposit;
            for hundredths in [200, 230, 250, 300, 400] {
                inputs.redeem_multiplier = RedeemFeeMultiplier::try_from(hundredths).unwrap();
                let fees = inputs.calculate(SwapFeeRate(1000), limits()).unwrap();
                let refund_path = fees.base * if deposit == Amount::ZERO { 2 } else { 4 };
                let mut policy = limits();
                policy.max_path_fee = fees.redeem.max(refund_path);
                assert!(inputs.calculate(SwapFeeRate(1000), policy).is_ok());
                policy.max_path_fee -= Amount::ONE_SAT;
                assert_eq!(
                    inputs
                        .calculate(SwapFeeRate(1000), policy)
                        .unwrap_err()
                        .downcast::<SwapFeeError>()
                        .unwrap(),
                    SwapFeeError::SwapFeesExceedBudget {
                        fees: fees.redeem.max(refund_path),
                        budget: policy.max_path_fee,
                    }
                );
            }
        }
    }

    #[test]
    fn output_scripts_determine_maximum_size() {
        let small = address(22);
        let large = address(34);
        let mut inputs = inputs(&small);
        let original = inputs.calculate(SwapFeeRate(1000), limits()).unwrap();
        inputs.refund_address = &large;
        let larger = inputs.calculate(SwapFeeRate(1000), limits()).unwrap();
        assert_eq!(larger.max_vsize, original.max_vsize + 12);
        assert_eq!(larger.base, original.base + Amount::from_sat(12));
        inputs.deposit = Amount::ZERO;
        let full = inputs.calculate(SwapFeeRate(1000), limits()).unwrap();
        assert!(full.max_vsize < larger.max_vsize);
    }

    #[test]
    fn local_estimate_is_raised_to_one_sat_per_vbyte() {
        for (sat_per_kwu, expected) in
            [(0, 1000), (25, 1000), (249, 1000), (250, 1000), (251, 1004)]
        {
            assert_eq!(
                SwapFeeRate::from_estimate(::bitcoin::FeeRate::from_sat_per_kwu(sat_per_kwu))
                    .unwrap(),
                SwapFeeRate(expected)
            );
        }
        assert!(
            SwapFeeRate::from_estimate(::bitcoin::FeeRate::from_sat_per_kwu(u64::MAX)).is_err()
        );
    }

    #[test]
    fn rounds_up_once_before_multiplying_redeem() {
        assert_eq!(SwapFeeRate(1001).fee(180).unwrap().to_sat(), 181);
        assert_eq!(SwapFeeRate(1000).fee(180).unwrap().to_sat(), 180);
        assert_eq!(SwapFeeRate(1).fee(180).unwrap().to_sat(), 1);
        let address = address(22);
        let fees = inputs(&address)
            .calculate(SwapFeeRate(1001), limits())
            .unwrap();
        assert_eq!(fees.base.to_sat(), 182);
        assert_eq!(fees.redeem.to_sat(), 364);
    }

    #[test]
    fn rejects_invalid_rates_overflow_and_local_limits() {
        assert!(SwapFeeRate(0).fee(180).is_err());
        assert!(SwapFeeRate(u64::MAX).fee(180).is_err());
        let address = address(22);
        let inputs = inputs(&address);
        let mut policy = limits();
        policy.min_rate = SwapFeeRate(1000);
        policy.max_rate = SwapFeeRate(2000);
        assert!(inputs.calculate(SwapFeeRate(999), policy).is_err());
        assert!(inputs.calculate(SwapFeeRate(2001), policy).is_err());
        policy.max_path_fee = Amount::from_sat(723);
        assert!(inputs.calculate(SwapFeeRate(1000), policy).is_err());
        policy.max_path_fee = Amount::from_sat(724);
        assert!(inputs.calculate(SwapFeeRate(1000), policy).is_ok());
    }

    #[test]
    fn rejects_insufficient_inputs_and_dust_in_every_deposit_path() {
        let address = address(22);
        let mut inputs = inputs(&address);
        for invalid in [0, 1, 500, u64::MAX] {
            inputs.lock_amount = Amount::from_sat(invalid);
            assert!(inputs.calculate(SwapFeeRate(1000), limits()).is_err());
        }
        inputs.lock_amount = Amount::from_sat(1_000_000);
        for invalid in [1, 329, 510, 655, 999_999, u64::MAX] {
            inputs.deposit = Amount::from_sat(invalid);
            assert!(
                inputs.calculate(SwapFeeRate(1000), limits()).is_err(),
                "deposit {invalid}"
            );
        }
        // 2 fees of 181 plus a 294-sat P2WPKH output: mercy is the limiting path.
        inputs.deposit = Amount::from_sat(656);
        assert!(inputs.calculate(SwapFeeRate(1000), limits()).is_ok());
        inputs.deposit = Amount::ZERO;
        inputs.lock_amount = Amount::from_sat(2 * 150 + 294 - 1);
        assert!(inputs.calculate(SwapFeeRate(1000), limits()).is_err());
    }

    #[test]
    fn reports_minimum_deposit_for_withhold_and_mercy() {
        for script_bytes in [22, 34] {
            let address = address(script_bytes);
            let mut inputs = inputs(&address);
            // At low rates the shared withhold output is the limiting path;
            // at higher rates it is the two fees needed to reach mercy.
            for rate in [SwapFeeRate(1), SwapFeeRate(1000), SwapFeeRate(5000)] {
                let fees = inputs.calculate(rate, limits()).unwrap();
                let minimum = (fees.base + Amount::from_sat(330))
                    .max(fees.base * 2 + address.script_pubkey().minimal_non_dust());
                for deposit in [Amount::ONE_SAT, minimum - Amount::ONE_SAT] {
                    inputs.deposit = deposit;
                    assert_eq!(
                        inputs
                            .calculate(rate, limits())
                            .unwrap_err()
                            .downcast::<SwapFeeError>()
                            .unwrap(),
                        SwapFeeError::AntiSpamDepositTooSmall { deposit, minimum }
                    );
                }
                inputs.deposit = minimum;
                assert!(inputs.calculate(rate, limits()).is_ok());
                inputs.deposit = Amount::from_sat(100_000);
            }
        }
    }
}
