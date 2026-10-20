//! Fee schedule for setup v3. Never derive these fees from peer-supplied sizes.
use super::*;
use ::bitcoin::{OutPoint, Psbt, TxIn, TxOut, Witness, absolute, transaction};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Integer satoshis per 1,000 virtual bytes. The wire representation is exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapFeeRate(pub u64);

impl SwapFeeRate {
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

impl SwapFeeLimits {
    pub fn validate_rate(self, rate: SwapFeeRate) -> Result<()> {
        ensure!(
            rate.0 > 0 && rate.0 >= self.min_rate.0 && rate.0 <= self.max_rate.0,
            "Proposed swap fee rate {} sat/kvB is outside local limits {}..={} sat/kvB",
            rate.0,
            self.min_rate.0,
            self.max_rate.0
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
                    script_pubkey: shared_script.clone(),
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
        let max_vsize = templates
            .into_iter()
            .map(|mut tx| {
                // Two maximum DER signatures, each including its sighash byte, and
                // the actual 2-of-2 witness script. Transaction::vsize rounds up.
                tx.input[0].witness =
                    Witness::from_slice(&[vec![0; 73], vec![0; 73], witness_script.to_bytes()]);
                tx.vsize() as u64
            })
            .max()
            .context("No presigned transaction templates")?;
        let base = rate.fee(max_vsize)?;
        let redeem = base.checked_mul(3).context("Redeem fee overflow")?;
        let longest_path = base
            .checked_mul(if self.deposit == Amount::ZERO { 3 } else { 4 })
            .context("Spending path fee overflow")?;
        ensure!(
            longest_path <= limits.max_path_fee,
            "Swap spending path fees ({longest_path}) exceed local budget ({})",
            limits.max_path_fee
        );

        fn output(input: Amount, fee: Amount, script: &::bitcoin::Script) -> Result<Amount> {
            let value = input
                .checked_sub(fee)
                .context("Insufficient transaction input value")?;
            ensure!(
                value > Amount::ZERO && value >= script.minimal_non_dust(),
                "Required transaction output is dust"
            );
            Ok(value)
        }
        output(self.lock_amount, Amount::ZERO, &shared_script)?;
        output(
            self.lock_amount,
            redeem,
            &self.redeem_address.script_pubkey(),
        )?;
        output(self.lock_amount, base, &self.refund_address.script_pubkey())?; // early refund
        let cancelled = output(self.lock_amount, base, &shared_script)?;
        output(cancelled, base, &self.punish_address.script_pubkey())?;
        output(cancelled, base, &self.refund_address.script_pubkey())?;
        if self.deposit != Amount::ZERO {
            let partial_cost = self
                .deposit
                .checked_add(base)
                .context("Partial refund amount overflow")?;
            output(
                cancelled,
                partial_cost,
                &self.refund_address.script_pubkey(),
            )?;
            output(self.deposit, Amount::ZERO, &shared_script)
                .context("Anti-spam deposit output is dust")?;
            output(self.deposit, base, &self.refund_address.script_pubkey())
                .context("Anti-spam deposit cannot fund reclaim")?;
            let withheld = output(self.deposit, base, &shared_script)
                .context("Anti-spam deposit cannot fund withhold")?;
            output(withheld, base, &self.refund_address.script_pubkey())
                .context("Anti-spam deposit cannot fund mercy")?;
        }
        Ok(SwapFees {
            max_vsize,
            base,
            redeem,
        })
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
        }
    }

    #[test]
    fn deterministic_for_agreed_scripts_and_exact_three_to_one() {
        let address = address(22);
        let inputs = inputs(&address);
        let alice = inputs.calculate(SwapFeeRate(1250), limits()).unwrap();
        let bob = inputs.calculate(SwapFeeRate(1250), limits()).unwrap();
        assert_eq!(alice, bob);
        // 1 input, 2 outputs, two 73-byte signatures and a 70-byte witness script.
        assert_eq!(alice.max_vsize, 181);
        assert_eq!(alice.base.to_sat(), 227);
        assert_eq!(alice.redeem, alice.base * 3);
        let rate = SwapFeeRate(1250);
        let recovered: SwapFeeRate =
            serde_json::from_str(&serde_json::to_string(&rate).unwrap()).unwrap();
        assert_eq!(rate, recovered);
        assert_eq!(inputs.calculate(recovered, limits()).unwrap(), alice);
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
    fn rounds_up_once_before_multiplying_redeem() {
        assert_eq!(SwapFeeRate(1001).fee(180).unwrap().to_sat(), 181);
        assert_eq!(SwapFeeRate(1000).fee(180).unwrap().to_sat(), 180);
        assert_eq!(SwapFeeRate(1).fee(180).unwrap().to_sat(), 1);
        let address = address(22);
        let fees = inputs(&address)
            .calculate(SwapFeeRate(1001), limits())
            .unwrap();
        assert_eq!(fees.base.to_sat(), 182);
        assert_eq!(fees.redeem.to_sat(), 546);
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
        inputs.lock_amount = Amount::from_sat(3 * 150 + 294 - 1);
        assert!(inputs.calculate(SwapFeeRate(1000), limits()).is_err());
    }
}
