#![allow(non_snake_case)]

use crate::bitcoin::{Address, Amount, PublicKey, Transaction, build_shared_output_descriptor};
use ::bitcoin::psbt::Psbt as PartiallySignedTransaction;
use ::bitcoin::{OutPoint, TxIn, TxOut, Txid};
use anyhow::{Context, Result, bail};
use bdk_wallet::miniscript::Descriptor;
use bitcoin::{ScriptBuf, Sequence, locktime::absolute::LockTime as PackedLockTime};
use bitcoin_wallet::primitives::Watchable;
use serde::{Deserialize, Serialize};

const SCRIPT_SIZE: usize = 34;
const TX_LOCK_WEIGHT: usize = 485;
const MAX_TX_LOCK_OUTPUTS: usize = 5;
/// Maximum serialized size of a canonical lock PSBT received from the other party.
///
/// The largest standard lock PSBT that the protocol allows today (25 P2WPKH inputs,
/// 5 P2TR outputs) is about 2,140 bytes. The limit leaves room for future protocol
/// changes and still bounds what is queued for verification and stored with a swap.
const MAX_CANONICAL_TX_LOCK_PSBT_SIZE: usize = 10_000;

/// Return an outpoint spent by both PSBTs, if one exists.
pub fn find_conflicting_psbt_input(
    first: &PartiallySignedTransaction,
    second: &PartiallySignedTransaction,
) -> Option<OutPoint> {
    first
        .unsigned_tx
        .input
        .iter()
        .map(|input| input.previous_output)
        .find(|outpoint| {
            second
                .unsigned_tx
                .input
                .iter()
                .any(|input| input.previous_output == *outpoint)
        })
}

/// Rebuild a lock PSBT received from the other party from its unsigned
/// transaction and each input's `witness_utxo`, and drop every other field.
///
/// The ownership proofs and the chain check only cover the `witness_utxo`s, so
/// nothing else in a received PSBT is needed. Dropping it bounds what is queued
/// for verification and what is stored with the swap.
pub fn canonical_received_psbt(
    psbt: PartiallySignedTransaction,
) -> Result<PartiallySignedTransaction> {
    let inputs = psbt.unsigned_tx.input.len();
    let outputs = psbt.unsigned_tx.output.len();
    anyhow::ensure!(
        psbt.inputs.len() == inputs,
        "PSBT has {} input maps for {inputs} inputs",
        psbt.inputs.len()
    );
    anyhow::ensure!(
        psbt.outputs.len() == outputs,
        "PSBT has {} output maps for {outputs} outputs",
        psbt.outputs.len()
    );
    anyhow::ensure!(
        outputs <= MAX_TX_LOCK_OUTPUTS,
        "PSBT has {outputs} outputs, expected at most {MAX_TX_LOCK_OUTPUTS}"
    );

    // Rejects unsigned transactions with a non-empty script_sig or witness.
    let mut canonical = PartiallySignedTransaction::from_unsigned_tx(psbt.unsigned_tx)
        .context("PSBT contains a signed transaction")?;
    for (canonical_input, input) in canonical.inputs.iter_mut().zip(psbt.inputs) {
        canonical_input.witness_utxo = input.witness_utxo;
    }

    // Output scripts and witness_utxo scripts have no size limit of their own.
    let size = canonical.serialize().len();
    anyhow::ensure!(
        size <= MAX_CANONICAL_TX_LOCK_PSBT_SIZE,
        "PSBT has {size} bytes, expected at most {MAX_CANONICAL_TX_LOCK_PSBT_SIZE}"
    );

    Ok(canonical)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxLock {
    inner: PartiallySignedTransaction,
    pub(in crate::bitcoin) output_descriptor: Descriptor<::bitcoin::PublicKey>,
}

impl TxLock {
    pub async fn new(
        wallet: &dyn bitcoin_wallet::BitcoinWallet,
        amount: Amount,
        spending_fee: Amount,
        A: PublicKey,
        B: PublicKey,
        change: bitcoin::Address,
    ) -> Result<Self> {
        let lock_output_descriptor = build_shared_output_descriptor(A.0, B.0)?;
        let address = lock_output_descriptor
            .address(wallet.network())
            .expect("can derive address from descriptor");

        let psbt = wallet
            .send_to_address(
                address,
                amount,
                spending_fee,
                Some(change),
                Some(super::MAX_TX_LOCK_INPUTS),
            )
            .await?;

        Ok(Self {
            inner: psbt,
            output_descriptor: lock_output_descriptor,
        })
    }

    /// Creates an instance of `TxLock` from a PSBT, the public keys of the
    /// parties and the specified amount.
    ///
    /// This function validates that the given PSBT does indeed pay that
    /// specified amount to a shared output.
    pub fn from_psbt(
        psbt: PartiallySignedTransaction,
        A: PublicKey,
        B: PublicKey,
        btc: Amount,
    ) -> Result<Self> {
        if psbt.unsigned_tx.output.len() > MAX_TX_LOCK_OUTPUTS {
            bail!(
                "PSBT has {} outputs, expected at most {MAX_TX_LOCK_OUTPUTS}",
                psbt.unsigned_tx.output.len()
            );
        }

        let mut outpoints = std::collections::HashSet::new();
        if let Some(duplicate) = psbt
            .unsigned_tx
            .input
            .iter()
            .find(|input| !outpoints.insert(input.previous_output))
        {
            bail!(
                "PSBT spends input {} more than once",
                duplicate.previous_output
            );
        }

        let descriptor = build_shared_output_descriptor(A.0, B.0)?;
        let legit_shared_output_script = descriptor.script_pubkey();

        let shared_outputs = psbt
            .unsigned_tx
            .output
            .iter()
            .filter(|output| output.script_pubkey == legit_shared_output_script)
            .collect::<Vec<_>>();

        let [shared_output] = shared_outputs.as_slice() else {
            bail!(
                "PSBT must have exactly one output paying to the shared descriptor, found {}",
                shared_outputs.len()
            );
        };

        if shared_output.value != btc {
            bail!(
                "Shared output pays {} but the agreed amount is {}",
                shared_output.value,
                btc
            );
        }

        Ok(TxLock {
            inner: psbt,
            output_descriptor: descriptor,
        })
    }

    pub fn lock_amount(&self) -> Amount {
        self.inner.clone().extract_tx_unchecked_fee_rate().output[self.lock_output_vout()].value
    }

    pub fn fee(&self) -> Result<Amount> {
        self.inner.fee().context("Failed to calculate TxLock fee")
    }

    pub fn txid(&self) -> Txid {
        self.inner
            .clone()
            .extract_tx_unchecked_fee_rate()
            .compute_txid()
    }

    pub fn as_psbt(&self) -> &PartiallySignedTransaction {
        &self.inner
    }

    pub fn as_outpoint(&self) -> OutPoint {
        // This is fine because a transaction that has that many outputs is not
        // realistic
        #[allow(clippy::cast_possible_truncation)]
        OutPoint::new(self.txid(), self.lock_output_vout() as u32)
    }

    /// Calculate the size of the script used by this transaction.
    pub fn script_size() -> usize {
        SCRIPT_SIZE
    }

    pub fn script_pubkey(&self) -> ScriptBuf {
        self.output_descriptor.script_pubkey()
    }

    /// Retrieve the index of the locked output in the transaction outputs
    /// vector
    fn lock_output_vout(&self) -> usize {
        self.inner
            .clone()
            .extract_tx_unchecked_fee_rate()
            .output
            .iter()
            .position(|output| output.script_pubkey == self.output_descriptor.script_pubkey())
            .expect("transaction contains lock output")
    }

    pub fn build_spend_transaction(
        &self,
        spend_address: &Address,
        sequence: Option<u32>,
        spending_fee: Amount,
    ) -> Transaction {
        let previous_output = self.as_outpoint();

        let sequence = Sequence(sequence.unwrap_or(0xFFFF_FFFF));
        let tx_in = TxIn {
            previous_output,
            script_sig: Default::default(),
            sequence,
            witness: Default::default(),
        };

        let tx_out = TxOut {
            value: self.inner.clone().extract_tx_unchecked_fee_rate().output
                [self.lock_output_vout()]
            .value
                - spending_fee,
            script_pubkey: spend_address.script_pubkey(),
        };

        tracing::debug!(fee=%spending_fee.to_sat(), "Constructed Bitcoin spending transaction");

        Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: PackedLockTime::from_height(0).expect("0 to be below lock time threshold"),
            input: vec![tx_in],
            output: vec![tx_out],
        }
    }

    pub fn weight() -> ::bitcoin::Weight {
        ::bitcoin::Weight::from_wu(TX_LOCK_WEIGHT as u64)
    }
}

impl From<TxLock> for PartiallySignedTransaction {
    fn from(from: TxLock) -> Self {
        from.inner
    }
}

impl Watchable for TxLock {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> ScriptBuf {
        self.output_descriptor.script_pubkey()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitcoin::Amount;
    use ::bitcoin::hashes::Hash;
    use bitcoin::Psbt;
    use bitcoin_wallet::*;

    // Basic setup function for tests
    async fn setup() -> (
        PublicKey,
        PublicKey,
        Wallet<bdk_wallet::rusqlite::Connection, StaticFeeRate>,
    ) {
        let (A, B) = alice_and_bob();
        let wallet = TestWalletBuilder::new(100_000).build().await;
        (A, B, wallet)
    }

    #[tokio::test]
    async fn given_bob_sends_good_psbt_when_reconstructing_then_succeeeds() {
        let (A, B, wallet) = setup().await;
        let agreed_amount = Amount::from_sat(10000);
        let spending_fee = Amount::from_sat(1000);

        let psbt = bob_make_psbt(A, B, &wallet, agreed_amount, spending_fee).await;
        let result = TxLock::from_psbt(psbt, A, B, agreed_amount);

        result.expect("PSBT to be valid");
    }

    #[tokio::test]
    async fn bob_can_fund_without_a_change_output() {
        let (A, B, _) = setup().await;
        let amount = 10_000;
        let agreed_amount = Amount::from_sat(amount);
        let spending_fee = Amount::from_sat(300);
        let wallet = TestWalletBuilder::new(amount + 300).build().await;

        let psbt = bob_make_psbt(A, B, &wallet, agreed_amount, spending_fee).await;
        assert_eq!(
            psbt.unsigned_tx.output.len(),
            1,
            "Expected no change output"
        );
    }

    #[tokio::test]
    async fn given_bob_is_sending_less_than_agreed_when_reconstructing_txlock_then_fails() {
        let (A, B, wallet) = setup().await;
        let agreed_amount = Amount::from_sat(10000);
        let spending_fee = Amount::from_sat(1000);

        let bad_amount = Amount::from_sat(5000);
        let psbt = bob_make_psbt(A, B, &wallet, bad_amount, spending_fee).await;
        let result = TxLock::from_psbt(psbt, A, B, agreed_amount);

        result.expect_err("PSBT to be invalid");
    }

    #[tokio::test]
    async fn given_two_outputs_pay_to_shared_descriptor_then_reconstructing_txlock_fails() {
        let (A, B, wallet) = setup().await;
        let agreed_amount = Amount::from_sat(10000);
        let spending_fee = Amount::from_sat(1000);

        let mut psbt = bob_make_psbt(A, B, &wallet, agreed_amount, spending_fee).await;

        let descriptor = build_shared_output_descriptor(A.0, B.0).unwrap();
        let dust_shared_output = TxOut {
            value: Amount::from_sat(1),
            script_pubkey: descriptor.script_pubkey(),
        };
        psbt.unsigned_tx.output.insert(0, dust_shared_output);

        let result = TxLock::from_psbt(psbt, A, B, agreed_amount);

        result.expect_err("PSBT with two shared outputs must be rejected");
    }

    #[test]
    fn accepts_up_to_five_outputs_with_the_swap_output_at_any_position() {
        let (A, B) = alice_and_bob();
        let agreed_amount = Amount::from_sat(10000);
        let descriptor = build_shared_output_descriptor(A.0, B.0).unwrap();
        for count in 1..=6 {
            for swap_index in 0..count {
                let mut outputs = vec![
                    TxOut {
                        value: Amount::from_sat(1000),
                        script_pubkey: ScriptBuf::new(),
                    };
                    count
                ];
                outputs[swap_index] = TxOut {
                    value: agreed_amount,
                    script_pubkey: descriptor.script_pubkey(),
                };
                let psbt = Psbt::from_unsigned_tx(Transaction {
                    version: bitcoin::transaction::Version(2),
                    lock_time: PackedLockTime::ZERO,
                    input: vec![],
                    output: outputs,
                })
                .unwrap();
                let result = TxLock::from_psbt(psbt, A, B, agreed_amount);
                if count == 6 {
                    assert!(result.unwrap_err().to_string().contains("at most 5"));
                    continue;
                }
                let lock = result.unwrap();
                assert_eq!(lock.as_outpoint().vout as usize, swap_index);
                assert_eq!(lock.lock_amount(), agreed_amount);
            }
        }
    }

    #[test]
    fn given_psbt_spends_the_same_input_twice_when_reconstructing_txlock_then_fails() {
        let (A, B) = alice_and_bob();
        let agreed_amount = Amount::from_sat(10000);
        let descriptor = build_shared_output_descriptor(A.0, B.0).unwrap();
        let input = TxIn {
            previous_output: OutPoint::new(Txid::all_zeros(), 1),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: ::bitcoin::Witness::new(),
        };
        let psbt = Psbt::from_unsigned_tx(Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: PackedLockTime::ZERO,
            input: vec![input.clone(), input],
            output: vec![TxOut {
                value: agreed_amount,
                script_pubkey: descriptor.script_pubkey(),
            }],
        })
        .unwrap();

        let result = TxLock::from_psbt(psbt, A, B, agreed_amount);

        result.expect_err("PSBT spending the same input twice must be rejected");
    }

    #[tokio::test]
    async fn given_bob_is_sending_to_a_bad_output_reconstructing_txlock_then_fails() {
        let (A, B, wallet) = setup().await;
        let agreed_amount = Amount::from_sat(10000);
        let spending_fee = Amount::from_sat(1000);

        let E = eve();
        let psbt = bob_make_psbt(E, B, &wallet, agreed_amount, spending_fee).await;
        let result = TxLock::from_psbt(psbt, A, B, agreed_amount);

        result.expect_err("PSBT to be invalid");
    }

    #[test]
    fn given_outputs_exceed_inputs_when_calculating_fee_then_fails() {
        let (A, B) = alice_and_bob();
        let agreed_amount = Amount::from_sat(10_000);
        let transaction = ::bitcoin::Transaction {
            version: ::bitcoin::transaction::Version::TWO,
            lock_time: ::bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: ::bitcoin::Witness::new(),
            }],
            output: vec![TxOut {
                value: agreed_amount,
                script_pubkey: build_shared_output_descriptor(A.0, B.0)
                    .expect("shared descriptor is valid")
                    .script_pubkey(),
            }],
        };
        let mut psbt =
            Psbt::from_unsigned_tx(transaction).expect("transaction can be represented as a PSBT");
        psbt.inputs[0].witness_utxo = Some(TxOut {
            value: Amount::from_sat(9_000),
            script_pubkey: ScriptBuf::new(),
        });

        let tx_lock = TxLock::from_psbt(psbt, A, B, agreed_amount).expect("shared output is valid");
        let error = tx_lock
            .fee()
            .expect_err("outputs exceeding inputs must be rejected");

        assert!(matches!(
            error.downcast_ref::<::bitcoin::psbt::Error>(),
            Some(::bitcoin::psbt::Error::NegativeFee)
        ));
    }

    #[test]
    fn given_psbts_spend_the_same_outpoint_then_finds_conflict() {
        let shared_outpoint = OutPoint::new(Txid::all_zeros(), 1);
        let first = psbt_spending(shared_outpoint);
        let second = psbt_spending(shared_outpoint);

        assert_eq!(
            find_conflicting_psbt_input(&first, &second),
            Some(shared_outpoint)
        );
    }

    #[test]
    fn given_psbts_spend_different_outpoints_then_finds_no_conflict() {
        let first = psbt_spending(OutPoint::new(Txid::all_zeros(), 1));
        let second = psbt_spending(OutPoint::new(Txid::all_zeros(), 2));

        assert_eq!(find_conflicting_psbt_input(&first, &second), None);
    }

    #[test]
    fn largest_standard_lock_psbt_fits_the_size_limit_with_headroom() {
        let psbt = standard_lock_psbt(25, MAX_TX_LOCK_OUTPUTS);

        let canonical = canonical_received_psbt(psbt).unwrap();

        assert!(canonical.serialize().len() * 4 <= MAX_CANONICAL_TX_LOCK_PSBT_SIZE);
    }

    #[test]
    fn rejects_oversized_scripts() {
        let mut psbt = standard_lock_psbt(1, 1);
        psbt.unsigned_tx.output[0].script_pubkey = op_return(MAX_CANONICAL_TX_LOCK_PSBT_SIZE);
        assert!(canonical_received_psbt(psbt).is_err());

        let mut psbt = standard_lock_psbt(1, 1);
        psbt.inputs[0].witness_utxo.as_mut().unwrap().script_pubkey =
            op_return(MAX_CANONICAL_TX_LOCK_PSBT_SIZE);
        assert!(canonical_received_psbt(psbt).is_err());
    }

    #[test]
    fn rejects_maps_that_do_not_agree_with_the_transaction() {
        let mut psbt = standard_lock_psbt(2, 2);
        psbt.inputs.pop();
        assert!(canonical_received_psbt(psbt).is_err());

        let mut psbt = standard_lock_psbt(2, 2);
        psbt.outputs.push(Default::default());
        assert!(canonical_received_psbt(psbt).is_err());
    }

    #[test]
    fn rejects_more_than_five_outputs() {
        assert!(canonical_received_psbt(standard_lock_psbt(1, MAX_TX_LOCK_OUTPUTS)).is_ok());
        assert!(canonical_received_psbt(standard_lock_psbt(1, MAX_TX_LOCK_OUTPUTS + 1)).is_err());
    }

    #[test]
    fn rejects_a_signed_transaction() {
        let mut psbt = standard_lock_psbt(1, 1);
        psbt.unsigned_tx.input[0].script_sig = ScriptBuf::from_bytes(vec![0x51]);
        assert!(canonical_received_psbt(psbt).is_err());

        let mut psbt = standard_lock_psbt(1, 1);
        psbt.unsigned_tx.input[0].witness = ::bitcoin::Witness::from_slice(&[[1u8; 72]]);
        assert!(canonical_received_psbt(psbt).is_err());
    }

    #[test]
    fn keeps_only_the_transaction_and_witness_utxos() {
        let mut psbt = standard_lock_psbt(1, 1);
        let unsigned_tx = psbt.unsigned_tx.clone();
        let witness_utxo = psbt.inputs[0].witness_utxo.clone();
        let key = ::bitcoin::psbt::raw::Key {
            type_value: 0xF0,
            key: vec![1],
        };
        let public_key = ::bitcoin::secp256k1::PublicKey::from_slice(&[
            2, 121, 190, 102, 126, 249, 220, 187, 172, 85, 160, 98, 149, 206, 135, 11, 7, 2, 155,
            252, 219, 45, 206, 40, 217, 89, 242, 129, 91, 22, 248, 23, 152,
        ])
        .unwrap();
        psbt.unknown.insert(key.clone(), vec![0; 100]);
        psbt.inputs[0].unknown.insert(key.clone(), vec![0; 100]);
        psbt.inputs[0].non_witness_utxo = Some(unsigned_tx.clone());
        psbt.inputs[0].bip32_derivation.insert(
            public_key,
            (::bitcoin::bip32::Fingerprint::default(), Default::default()),
        );
        psbt.inputs[0].partial_sigs.insert(
            ::bitcoin::PublicKey::new(public_key),
            ::bitcoin::ecdsa::Signature::sighash_all(
                ::bitcoin::secp256k1::ecdsa::Signature::from_compact(&[1; 64]).unwrap(),
            ),
        );
        psbt.outputs[0].unknown.insert(key, vec![0; 100]);

        let canonical = canonical_received_psbt(psbt).unwrap();

        assert_eq!(canonical.unsigned_tx, unsigned_tx);
        assert_eq!(canonical.inputs[0].witness_utxo, witness_utxo);
        assert!(canonical.unknown.is_empty());
        assert!(canonical.inputs[0].unknown.is_empty());
        assert!(canonical.inputs[0].non_witness_utxo.is_none());
        assert!(canonical.inputs[0].bip32_derivation.is_empty());
        assert!(canonical.inputs[0].partial_sigs.is_empty());
        assert!(canonical.outputs[0].unknown.is_empty());
    }

    /// A lock PSBT with P2WPKH inputs and P2TR outputs, the largest standard scripts.
    fn standard_lock_psbt(inputs: u32, outputs: usize) -> Psbt {
        let mut psbt = Psbt::from_unsigned_tx(::bitcoin::Transaction {
            version: ::bitcoin::transaction::Version::TWO,
            lock_time: ::bitcoin::absolute::LockTime::ZERO,
            input: (0..inputs)
                .map(|vout| TxIn {
                    previous_output: OutPoint::new(Txid::all_zeros(), vout),
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: ::bitcoin::Witness::new(),
                })
                .collect(),
            output: (0..outputs)
                .map(|_| TxOut {
                    value: Amount::from_sat(100_000),
                    script_pubkey: ScriptBuf::from_bytes([&[0x51, 0x20][..], &[9; 32]].concat()),
                })
                .collect(),
        })
        .expect("transaction can be represented as a PSBT");
        for input in &mut psbt.inputs {
            input.witness_utxo = Some(TxOut {
                value: Amount::from_sat(100_000),
                script_pubkey: ScriptBuf::from_bytes([&[0x00, 0x14][..], &[7; 20]].concat()),
            });
        }
        psbt
    }

    fn op_return(size: usize) -> ScriptBuf {
        ScriptBuf::from_bytes([vec![0x6a], vec![0; size]].concat())
    }

    fn psbt_spending(previous_output: OutPoint) -> Psbt {
        Psbt::from_unsigned_tx(::bitcoin::Transaction {
            version: ::bitcoin::transaction::Version::TWO,
            lock_time: ::bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: ::bitcoin::Witness::new(),
            }],
            output: vec![],
        })
        .expect("transaction can be represented as a PSBT")
    }

    proptest::proptest! {
        #[test]
        fn estimated_tx_lock_script_size_never_changes(a in swap_proptest::ecdsa_fun::point(), b in swap_proptest::ecdsa_fun::point()) {
            proptest::prop_assume!(a != b);

            let computed_size = build_shared_output_descriptor(a, b).unwrap().script_pubkey().len();

            assert_eq!(computed_size, TxLock::script_size());
        }
    }

    // Helper function for testing PSBT creation by Bob
    async fn bob_make_psbt(
        A: PublicKey,
        B: PublicKey,
        wallet: &dyn BitcoinWallet,
        amount: Amount,
        spending_fee: Amount,
    ) -> Psbt {
        let change = wallet.new_address().await.unwrap();
        TxLock::new(wallet, amount, spending_fee, A, B, change)
            .await
            .unwrap()
            .into()
    }

    fn alice_and_bob() -> (PublicKey, PublicKey) {
        (PublicKey::random(), PublicKey::random())
    }

    fn eve() -> PublicKey {
        PublicKey::random()
    }
}
