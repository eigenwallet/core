//! exact-amount transfers with change, the construction half of the
//! funding engine.
//!
//! sweep spreads one output across ratio destinations and lets the
//! fee eat whatever is left. i do the opposite job: pay these exact
//! piconero amounts to these exact addresses, send the change home,
//! and verify the fee at the end so nothing drifted.
//!
//! fees are resolved with a build-and-grow loop, no fee math of my
//! own anywhere: selection starts from the smallest prefix of
//! unlocked outputs covering the bare target, then i make a real
//! build attempt with the real amounts. oxide's own funding check
//! (inputs >= payments + necessary_fee) is exactly the coverage
//! question, and its `NotEnoughFunds` rejection carries the fee for
//! the shape, so the prefix grows until the build succeeds. every
//! fee number in here comes from oxide, never from my own
//! arithmetic. if the math is wrong the grow loop or the final fee
//! check catches it, loudly.
//!
//! the caller owns the pending/spend bookkeeping: mark the returned
//! input ids pending BEFORE handing the transaction to the daemon,
//! then confirm or release depending on what the chain does.

use std::collections::HashMap;

use backoff::ExponentialBackoff;
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use monero_interface::{
    FeePriority, FeeRate, ProvidesBlockchainMeta, ProvidesDecoys, ProvidesFeeRates,
};
use monero_oxide::ed25519::{Point, Scalar};
use monero_oxide::ringct::RctType;
use monero_oxide_wallet::address::MoneroAddress;
use monero_oxide_wallet::send::{Change, SendError, SignableTransaction, TransactionKeys};
use monero_oxide_wallet::transaction::{NotPruned, Transaction};
use monero_oxide_wallet::{OutputWithDecoys, WalletOutput};

use crate::retry::with_retry;
use crate::state::{OutputId, StateError, WalletState};
use crate::{MAX_FEE_PER_WEIGHT, RING_LEN};

/// a successfully constructed transfer.
///
/// `input_ids` are what the transaction spends: mark them pending on
/// your [`WalletState`] before publishing, then confirm them once the
/// tx is on-chain (or release them if it never lands).
#[derive(Debug)]
pub struct Transfer {
    pub transaction: Transaction<NotPruned>,
    pub input_ids: Vec<OutputId>,
    /// the fee oxide's build reported, cross-checked against the
    /// signed tx.
    pub fee_pico: u64,
    /// inputs minus destinations minus fee. zero is allowed, see
    /// [`construct_transfer`].
    pub change_pico: u64,
    /// the transaction key: the scalar behind the tx pubkey in
    /// extra. every base-address output's shared secret derives from
    /// it, so it is what a payment proof or a check-tx-key needs.
    /// re-derived from the same seed the signer used, so it is the
    /// key the signed tx actually carries.
    pub tx_key: Zeroizing<Scalar>,
}

/// the ways i can refuse to build you a transfer.
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("no destinations given")]
    Empty,
    #[error("destination {index} has a zero amount, paying nobody nothing makes no sense")]
    ZeroAmountDestination { index: usize },
    #[error("destination total or fee arithmetic overflowed u64")]
    Overflow,
    #[error("the signed transaction was not the RingCT v2 shape i built")]
    UnexpectedTransactionShape,
    #[error("input selection: {0}")]
    Selection(#[from] StateError),
    #[error("output {id:?} is missing from the outputs map or the state; the two are out of sync")]
    MissingOutputData { id: OutputId },
    #[error(
        "output {id:?} has different amounts in the state and the scan data; \
         the change output would silently disagree with the reported change"
    )]
    InconsistentOutputData { id: OutputId },
    #[error("fee error: {0}")]
    Fee(#[from] monero_interface::FeeError),
    #[error("decoy selection error: {0}")]
    Decoys(#[from] monero_interface::TransactionsError),
    #[error("interface error: {0}")]
    Interface(#[from] monero_interface::InterfaceError),
    #[error("failed to build the transaction: {0}")]
    Send(#[from] SendError),
    #[error("final fee {actual} differs from the built fee {built}")]
    FeeMismatch { built: u64, actual: u64 },
}

/// the key under which the engine maps a scanned output.
///
/// state and the outputs map must agree on this or
/// [`construct_transfer`] starts throwing
/// [`TransferError::MissingOutputData`].
pub fn output_id(output: &WalletOutput) -> OutputId {
    OutputId {
        transaction: output.transaction(),
        index: output.index_in_transaction(),
    }
}

/// sanity-check the destinations and return their total.
fn validate_destinations(destinations: &[(MoneroAddress, u64)]) -> Result<u64, TransferError> {
    if destinations.is_empty() {
        return Err(TransferError::Empty);
    }

    for (index, (_, amount)) in destinations.iter().enumerate() {
        if *amount == 0 {
            return Err(TransferError::ZeroAmountDestination { index });
        }
    }

    destinations
        .iter()
        .map(|(_, amount)| *amount)
        .try_fold(0u64, u64::checked_add)
        .ok_or(TransferError::Overflow)
}

/// build a signed transfer paying exact amounts, change going to
/// `change_address`.
///
/// `outputs` is the engine's scan data keyed by [`output_id`];
/// `state` decides which outputs are unlocked and selectable. the two
/// must describe the same wallet or you get
/// [`TransferError::MissingOutputData`], and the amounts get
/// cross-checked between them too, a mismatch being
/// [`TransferError::InconsistentOutputData`].
///
/// `inclusion_height` and `adjusted_time` follow the state module's
/// conventions (tip + 1, daemon adjusted time; module docs there).
///
/// the fee rate comes from the daemon capped at
/// `MAX_FEE_PER_WEIGHT`, same as sweep. a change output is always
/// present, even at amount zero, so the destination count alone
/// decides the shape.
#[allow(clippy::too_many_arguments)]
pub async fn construct_transfer<P>(
    provider: &P,
    private_spend_key: &Zeroizing<monero_oxide::ed25519::Scalar>,
    outputs: &HashMap<OutputId, WalletOutput>,
    state: &WalletState,
    destinations: Vec<(MoneroAddress, u64)>,
    change_address: MoneroAddress,
    inclusion_height: u64,
    adjusted_time: u64,
    inner_retry: Option<ExponentialBackoff>,
) -> Result<Transfer, TransferError>
where
    P: ProvidesBlockchainMeta + ProvidesDecoys + ProvidesFeeRates + Send + Sync,
{
    let target = validate_destinations(&destinations)?;

    let fee_rate = with_retry(inner_retry.clone(), "Transfer fee-rate lookup", || async {
        provider
            .fee_rate(FeePriority::Normal, MAX_FEE_PER_WEIGHT)
            .await
    })
    .await?;

    let block_number = with_retry(
        inner_retry.clone(),
        "Transfer latest-block-number lookup",
        || async { provider.latest_block_number().await },
    )
    .await?;

    let mut outgoing_view_key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(outgoing_view_key.as_mut());

    // selection starts bare (fee_for = 0), the grow loop below adds
    // whatever the real fee turns out to need. selections are
    // prefixes of the same deterministic order, so growing only ever
    // extends the set.
    let (mut selected, mut total) =
        state.select_inputs_with_fee(target, inclusion_height, adjusted_time, |_| 0)?;

    // decoys are fetched once per output and cached across rounds
    let mut decoy_cache: HashMap<OutputId, OutputWithDecoys> = HashMap::new();

    let (tx, fee, input_keys_and_commitments) = loop {
        let mut inputs = Vec::with_capacity(selected.len());
        let mut fetched_ids = Vec::new();
        let mut fetches = Vec::new();
        for id in &selected {
            if let Some(input) = decoy_cache.get(id) {
                inputs.push(input.clone());
                continue;
            }

            let output = outputs
                .get(id)
                .ok_or(TransferError::MissingOutputData { id: *id })?;
            let received = state
                .output(id)
                .ok_or(TransferError::MissingOutputData { id: *id })?;
            // the state and the scan data must agree on amounts, or
            // the change output will quietly disagree with what i
            // report.
            if received.amount_pico != output.commitment().amount {
                return Err(TransferError::InconsistentOutputData { id: *id });
            }

            // every missing decoy set is up to three daemon round
            // trips on its own, so fetch them all at once instead of
            // one input at a time
            let retry = inner_retry.clone();
            fetched_ids.push(id);
            fetches.push(async move {
                with_retry(retry, "Transfer decoy selection", || async {
                    OutputWithDecoys::new(
                        &mut OsRng,
                        provider,
                        RING_LEN,
                        block_number,
                        output.clone(),
                    )
                    .await
                })
                .await
            });
        }

        let results = futures::future::join_all(fetches).await;
        for (id, result) in fetched_ids.into_iter().zip(results) {
            let input = result?;
            decoy_cache.insert(*id, input.clone());
            inputs.push(input);
        }

        // what the tx key re-derivation needs: the spent outputs'
        // keys and commitments, in the shape oxide seeds from
        let input_keys_and_commitments: Vec<(Point, Point)> = inputs
            .iter()
            .map(|input| (input.key(), input.commitment().commit()))
            .collect();

        // a real build attempt with the real amounts IS the probe:
        // oxide's own funding check (inputs >= payments +
        // necessary_fee) is exactly the coverage question, and its
        // NotEnoughFunds error carries the fee for this shape. no
        // dummy amounts anywhere, the successful build is the tx i
        // sign.
        match build_transaction(
            inputs,
            destinations.clone(),
            change_address,
            fee_rate,
            &outgoing_view_key,
        ) {
            Ok(tx) => {
                let fee = tx.necessary_fee();
                break (tx, fee, input_keys_and_commitments);
            }
            Err(SendError::NotEnoughFunds {
                necessary_fee: Some(fee),
                ..
            }) => {
                // short: re-select with the real fee as the bar. the
                // bar only went up so the prefix only gets longer,
                // and if even everything i have cant cover it,
                // selection errors out.
                let (next_ids, next_total) = state.select_inputs_with_fee(
                    target,
                    inclusion_height,
                    adjusted_time,
                    |_| fee,
                )?;
                selected = next_ids;
                total = next_total;
            }
            Err(e) => return Err(e.into()),
        }
    };

    let signed = tx.sign(&mut OsRng, private_spend_key)?;

    // the signed tx must charge exactly what the build told me. if it
    // doesnt, something about the shape changed between construction
    // and signing and every guarantee here is off.
    let actual_fee = {
        let Transaction::V2 {
            proofs: Some(proofs),
            ..
        } = &signed
        else {
            // the signer built this from my V2 build with Bulletproofs+
            // proofs. if that ever stops holding, every fee guarantee
            // here is off, so say so instead of guessing.
            return Err(TransferError::UnexpectedTransactionShape);
        };
        proofs.base.fee
    };

    if actual_fee != fee {
        return Err(TransferError::FeeMismatch {
            built: fee,
            actual: actual_fee,
        });
    }

    let change = total
        .checked_sub(target)
        .and_then(|remainder| remainder.checked_sub(fee))
        .ok_or(TransferError::Overflow)?;

    // the tx key is drawn from a stream seeded by the outgoing view
    // key and the spent outputs, deterministically, so re-deriving
    // it here yields the key the signer used
    let tx_key = TransactionKeys::new(&outgoing_view_key, input_keys_and_commitments)
        .next()
        .expect("TransactionKeys (never-ending) was exhausted");

    Ok(Transfer {
        transaction: signed,
        input_ids: selected,
        fee_pico: fee,
        change_pico: change,
        tx_key,
    })
}

/// build the real transaction for these inputs and payments.
///
/// called inside the grow loop with the real destination amounts: a
/// successful build means the inputs cover payments plus fee (oxide
/// validates exactly that), and a `NotEnoughFunds` rejection carries
/// the necessary fee for the shape, which is what the loop grows on.
fn build_transaction(
    inputs: Vec<OutputWithDecoys>,
    payments: Vec<(MoneroAddress, u64)>,
    change_address: MoneroAddress,
    fee_rate: FeeRate,
    outgoing_view_key: &Zeroizing<[u8; 32]>,
) -> Result<SignableTransaction, SendError> {
    SignableTransaction::new(
        RctType::ClsagBulletproofPlus,
        outgoing_view_key.clone(),
        inputs,
        payments,
        Change::fingerprintable(Some(change_address)),
        vec![],
        fee_rate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use monero_oxide::ed25519::Point;

    fn addr(seed: u8) -> MoneroAddress {
        use curve25519_dalek::Scalar as DalekScalar;
        let spend_scalar = DalekScalar::from(u64::from(seed).wrapping_add(1));
        let view_scalar = DalekScalar::from(u64::from(seed).wrapping_add(101));
        let spend =
            Point::from(curve25519_dalek::constants::ED25519_BASEPOINT_POINT * spend_scalar);
        let view = Point::from(curve25519_dalek::constants::ED25519_BASEPOINT_POINT * view_scalar);
        MoneroAddress::new(
            monero_oxide_wallet::address::Network::Mainnet,
            monero_oxide_wallet::address::AddressType::Legacy,
            spend,
            view,
        )
    }

    #[test]
    fn destinations_must_not_be_empty() {
        assert!(matches!(
            validate_destinations(&[]),
            Err(TransferError::Empty)
        ));
    }

    #[test]
    fn zero_amount_destinations_are_rejected_with_their_index() {
        let destinations = vec![(addr(0), 5), (addr(1), 0), (addr(2), 0)];
        assert!(matches!(
            validate_destinations(&destinations),
            Err(TransferError::ZeroAmountDestination { index: 1 })
        ));
    }

    #[test]
    fn destination_totals_are_summed() {
        let destinations = vec![(addr(0), 5), (addr(1), 7)];
        assert_eq!(validate_destinations(&destinations).unwrap(), 12);
    }

    #[test]
    fn destination_totals_overflow_is_an_error_not_a_wrap() {
        let destinations = vec![(addr(0), u64::MAX), (addr(1), 1)];
        assert!(matches!(
            validate_destinations(&destinations),
            Err(TransferError::Overflow)
        ));
    }
}
