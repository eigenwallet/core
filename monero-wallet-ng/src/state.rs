//! pure wallet state tracking, no I/O, no surprises.
//!
//! i keep the accounting for one wallet: what came in, whats pending,
//! whats spent, what is actually unlocked at a given height, and which
//! outputs to grab when you need to hit an exact amount. the rpc side
//! (scanner, daemon clients) feeds me plain facts via
//! [`ReceivedOutput`] and asks questions back. everything here is
//! deterministic, same inputs give you the same answer every time.
//!
//! # height convention, read this or lose an afternoon
//!
//! every `inclusion_height` is the height a spending tx would land in.
//! that is the block count, ie tip number + 1. same thing wallet2 does
//! with `m_blockchain.size()`. if you are holding a 0-based tip from
//! `latest_block_number` you add one before calling me, otherwise
//! everything looks locked for one extra block and you will chase
//! ghosts.
//!
//! # time locks
//!
//! evaluated against the daemon's `adjusted_time` (from `/get_info`)
//! plus [`TIME_LOCK_LEEWAY`], ie wallet2's rule:
//! `adjusted_time + 120 >= unlock_time`.
//!
//! # unlock rules
//!
//! - every output waits [`TX_SPENDABLE_AGE`] blocks before it can be
//!   spent, no exceptions
//! - coinbase waits [`MINED_MONEY_UNLOCK_WINDOW`], miner money is
//!   patient money
//! - per-output timelocks ([`Timelock::Block`], [`Timelock::Time`])
//!   get checked against inclusion height and adjusted time
//!
//! # stuff i deliberately dont do
//!
//! - pending is a flat set, not keyed by tx. dont interleave two
//!   in-flight transactions over one wallet, the engine above me has
//!   to enforce that
//! - no mempool/change model. balance is what the scanner saw
//!   on-chain, period
//! - no serialization, persistence is the db layer's job

use std::collections::HashMap;

use monero_oxide::transaction::Timelock;

/// consensus rule: an output has to sit for this many blocks before
/// any tx can spend it.
pub const TX_SPENDABLE_AGE: u64 = 10;

/// consensus rule: coinbase money waits this many blocks before
/// anyone gets to spend it.
pub const MINED_MONEY_UNLOCK_WINDOW: u64 = 60;

/// leeway wallet2 grants on time locks: `DIFFICULTY_TARGET_V2 * 1`
/// per block, and it allows one block of drift, so 120 seconds.
/// wallet2 gives outputs created before the 2018 V2 fork half of
/// this; that only matters for predicting ancient spends, the daemon
/// enforces the real rule at publish either way.
pub const TIME_LOCK_LEEWAY: u64 = 120;

/// one output on-chain: tx hash plus index. if you have ever debugged
/// an out-of-index panic you know why both are here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutputId {
    pub transaction: [u8; 32],
    pub index: u64,
}

/// what i need to know about a received output to do accounting.
/// keys, commitments, ring members, all of that lives with the
/// scanner. this is the receipt, not the whole shopping bag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceivedOutput {
    pub id: OutputId,
    /// piconero, always piconero.
    pub amount_pico: u64,
    /// height of the block that included the creating tx.
    pub received_at_height: u64,
    /// miner reward tx? then its on the 60 window.
    pub is_coinbase: bool,
    /// whatever extra timelock the output carries.
    pub additional_timelock: Timelock,
}

/// the ways i can say no.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    #[error("unlocked balance ({available} pico) is insufficient for {required} pico")]
    InsufficientUnlocked { available: u64, required: u64 },
    #[error("a balance sum overflowed u64; scanner data is inconsistent")]
    BalanceOverflow,
    #[error("target amount must be non-zero")]
    ZeroTarget,
    #[error("unknown outputs: {0:?}")]
    UnknownOutputs(Vec<OutputId>),
    #[error(
        "output {id:?} was already confirmed spent at height {existing} \
         and cannot be re-confirmed at height {new}"
    )]
    InconsistentSpend {
        id: OutputId,
        existing: u64,
        new: u64,
    },
}

/// where an output is in its life, from this wallet's point of view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SpendState {
    /// got it, havent spent it.
    #[default]
    Available,
    /// picked as input for a tx i built and handed to the daemon,
    /// chain hasnt blessed it yet.
    Pending,
    /// spent on-chain, at this height.
    Spent { at_height: u64 },
}

#[derive(Debug)]
struct TrackedOutput {
    received: ReceivedOutput,
    spend: SpendState,
}

/// one wallet's outputs, tracked.
///
/// queries are deterministic: selection takes biggest first and
/// breaks ties by [`OutputId`], so the same state always picks the
/// same inputs. i like being able to reproduce a run.
#[derive(Debug, Default)]
pub struct WalletState {
    outputs: HashMap<OutputId, TrackedOutput>,
}

impl WalletState {
    /// record an output i received. idempotent: if i already know it
    /// i keep the first facts i saw, so scanner replays and rescans
    /// dont double count anything.
    pub fn receive(&mut self, received: ReceivedOutput) {
        self.outputs
            .entry(received.id)
            .or_insert_with(|| TrackedOutput {
                received,
                spend: SpendState::Available,
            });
    }

    /// outputs just went into a tx i built but havent seen on-chain
    /// yet. pending outputs sit outside every balance until they
    /// confirm or get released.
    ///
    /// i refuse ids i never received, and i mutate nothing when i
    /// refuse. otherwise a mark that lands before the receive (scan
    /// race) would vanish and the same output could get picked again
    /// while a constructed tx still points at it. that bug is no fun
    /// to find at 2am.
    pub fn mark_pending(&mut self, ids: &[OutputId]) -> Result<(), StateError> {
        self.for_each_known(ids, |tracked| {
            if tracked.spend == SpendState::Available {
                tracked.spend = SpendState::Pending;
            }
        })
    }

    /// is this output spent on chain? None means i never saw the
    /// output at all.
    pub fn is_spent(&self, id: &OutputId) -> Option<bool> {
        self.outputs
            .get(id)
            .map(|tracked| matches!(tracked.spend, SpendState::Spent { .. }))
    }

    /// pending outputs come back to available, for when a tx will
    /// never confirm (daemon said no, i replaced it, i gave up).
    ///
    /// same unknown-id refusal as [`WalletState::mark_pending`].
    pub fn release_pending(&mut self, ids: &[OutputId]) -> Result<(), StateError> {
        self.for_each_known(ids, |tracked| {
            if tracked.spend == SpendState::Pending {
                tracked.spend = SpendState::Available;
            }
        })
    }

    /// the tx spending these outputs landed on-chain at `at_height`.
    ///
    /// calling it twice at the same height is fine. unknown ids are an
    /// error, and re-confirming at a different height is an error too,
    /// because that means the chain moved under me and papering over
    /// it here would hide a reorg. use [`WalletState::rollback_to`]
    /// for that.
    pub fn confirm_spent(&mut self, ids: &[OutputId], at_height: u64) -> Result<(), StateError> {
        let unknown = unknown_ids(self, ids);
        if !unknown.is_empty() {
            return Err(StateError::UnknownOutputs(unknown));
        }

        for id in ids {
            let tracked = &self.outputs[id];
            if let SpendState::Spent {
                at_height: existing,
            } = tracked.spend
                && existing != at_height
            {
                return Err(StateError::InconsistentSpend {
                    id: *id,
                    existing,
                    new: at_height,
                });
            }
        }

        let requested = requested_set(ids);
        for tracked in self.outputs.values_mut() {
            if requested.contains(&tracked.received.id) {
                tracked.spend = SpendState::Spent { at_height };
            }
        }

        Ok(())
    }

    /// a reorg ate everything from `from_block_number` up, so forget
    /// i ever saw it: outputs received in that block or above are
    /// gone, spends that happened up there never happened.
    ///
    /// note the convention: this takes a 0-based block number, the
    /// number the reorged chain and the scanner both use, not the
    /// inclusion height (tip + 1) the unlock rules take.
    ///
    /// pending outputs i leave alone, they belong to my in-flight
    /// constructions, not to the chain.
    pub fn rollback_to(&mut self, from_block_number: u64) {
        self.outputs
            .retain(|_, tracked| tracked.received.received_at_height < from_block_number);

        for tracked in self.outputs.values_mut() {
            if let SpendState::Spent { at_height } = tracked.spend
                && at_height >= from_block_number
            {
                tracked.spend = SpendState::Available;
            }
        }
    }

    /// everything i hold that isnt pending or spent, locked or not.
    pub fn balance(&self) -> Result<u64, StateError> {
        self.outputs
            .values()
            .filter(|tracked| tracked.spend == SpendState::Available)
            .map(|tracked| tracked.received.amount_pico)
            .try_fold(0u64, u64::checked_add)
            .ok_or(StateError::BalanceOverflow)
    }

    /// like [`WalletState::balance`] but only what you could actually
    /// spend in a tx landing at `inclusion_height`, given the daemon's
    /// `adjusted_time`. check the module docs for the height
    /// convention, seriously.
    pub fn unlocked_balance(
        &self,
        inclusion_height: u64,
        adjusted_time: u64,
    ) -> Result<u64, StateError> {
        self.outputs
            .values()
            .filter(|tracked| tracked.spend == SpendState::Available)
            .map(|tracked| tracked.received)
            .filter(|received| is_unlocked(received, inclusion_height, adjusted_time))
            .map(|received| received.amount_pico)
            .try_fold(0u64, u64::checked_add)
            .ok_or(StateError::BalanceOverflow)
    }

    /// whats currently sitting in txs i built but havent seen
    /// confirm yet.
    pub fn pending_balance(&self) -> Result<u64, StateError> {
        self.outputs
            .values()
            .filter(|tracked| tracked.spend == SpendState::Pending)
            .map(|tracked| tracked.received.amount_pico)
            .try_fold(0u64, u64::checked_add)
            .ok_or(StateError::BalanceOverflow)
    }

    /// ids of everything pending, for retry/replacement flows.
    pub fn pending_ids(&self) -> Vec<OutputId> {
        self.outputs
            .iter()
            .filter(|(_, tracked)| tracked.spend == SpendState::Pending)
            .map(|(id, _)| *id)
            .collect()
    }

    /// the facts i recorded for one output, if i have it.
    pub fn output(&self, id: &OutputId) -> Option<&ReceivedOutput> {
        self.outputs.get(id).map(|tracked| &tracked.received)
    }

    /// pick unlocked outputs covering `target_pico` for a tx landing
    /// at `inclusion_height`. biggest first, ties by [`OutputId`], so
    /// its reproducible.
    ///
    /// this one ignores fees. if you care about fees (you do), use
    /// [`WalletState::select_inputs_with_fee`].
    pub fn select_inputs(
        &self,
        target_pico: u64,
        inclusion_height: u64,
        adjusted_time: u64,
    ) -> Result<Vec<OutputId>, StateError> {
        if target_pico == 0 {
            return Err(StateError::ZeroTarget);
        }

        let mut candidates = self
            .sorted_unlocked_candidates(inclusion_height, adjusted_time)
            .collect::<Vec<_>>();

        let available = candidates
            .iter()
            .map(|o| o.amount_pico)
            .try_fold(0u64, u64::checked_add)
            .ok_or(StateError::BalanceOverflow)?;
        if available < target_pico {
            return Err(StateError::InsufficientUnlocked {
                available,
                required: target_pico,
            });
        }

        let mut selected = Vec::new();
        let mut accumulated = 0u64;
        for output in candidates.drain(..) {
            selected.push(output.id);
            accumulated += output.amount_pico;
            if accumulated >= target_pico {
                break;
            }
        }

        Ok(selected)
    }

    /// selection that knows fees exist.
    ///
    /// `fee_for(n)` tells me what a tx paying `n` inputs costs in
    /// total. i keep adding inputs (biggest first) until the pile
    /// covers target AND fee of the current pile.
    ///
    /// works for any `fee_for` you hand me, i always re-check the
    /// returned set against the fee of its own size before returning
    /// it. the set is minimal when `fee_for` doesnt shrink as you add
    /// inputs, which every real fee schedule satisfies.
    ///
    /// you get the input ids back together with how much they sum to.
    pub fn select_inputs_with_fee(
        &self,
        target_pico: u64,
        inclusion_height: u64,
        adjusted_time: u64,
        fee_for: impl Fn(usize) -> u64,
    ) -> Result<(Vec<OutputId>, u64), StateError> {
        if target_pico == 0 {
            return Err(StateError::ZeroTarget);
        }

        let mut candidates = self
            .sorted_unlocked_candidates(inclusion_height, adjusted_time)
            .collect::<Vec<_>>();

        let mut selected = Vec::new();
        let mut accumulated = 0u64;
        let mut last_error = StateError::InsufficientUnlocked {
            available: 0,
            required: target_pico,
        };

        for output in candidates.drain(..) {
            selected.push(output.id);
            let Some(incremented) = accumulated.checked_add(output.amount_pico) else {
                return Err(StateError::BalanceOverflow);
            };
            accumulated = incremented;
            let required = target_pico
                .checked_add(fee_for(selected.len()))
                .ok_or(StateError::BalanceOverflow)?;
            if accumulated >= required {
                return Ok((selected, accumulated));
            }
            last_error = StateError::InsufficientUnlocked {
                available: accumulated,
                required,
            };
        }

        Err(last_error)
    }

    /// unlocked and available, biggest first, ties by [`OutputId`].
    fn sorted_unlocked_candidates(
        &self,
        inclusion_height: u64,
        adjusted_time: u64,
    ) -> impl Iterator<Item = ReceivedOutput> + '_ {
        let mut candidates = self
            .outputs
            .values()
            .filter(|tracked| tracked.spend == SpendState::Available)
            .map(|tracked| tracked.received)
            .filter(|received| is_unlocked(received, inclusion_height, adjusted_time))
            .collect::<Vec<_>>();

        candidates.sort_by(|a, b| {
            b.amount_pico
                .cmp(&a.amount_pico)
                .then_with(|| a.id.cmp(&b.id))
        });

        candidates.into_iter()
    }

    /// apply `mutate` to every tracked output in `ids`. unknown ids
    /// are an error and nothing gets touched when there is one, all or
    /// nothing.
    fn for_each_known(
        &mut self,
        ids: &[OutputId],
        mut mutate: impl FnMut(&mut TrackedOutput),
    ) -> Result<(), StateError> {
        let unknown = unknown_ids(self, ids);
        if !unknown.is_empty() {
            return Err(StateError::UnknownOutputs(unknown));
        }

        let requested = requested_set(ids);
        for tracked in self.outputs.values_mut() {
            if requested.contains(&tracked.received.id) {
                mutate(tracked);
            }
        }

        Ok(())
    }
}

/// the ids as a set, duh.
fn requested_set(ids: &[OutputId]) -> std::collections::HashSet<OutputId> {
    ids.iter().copied().collect()
}

/// which of `ids` i never received.
fn unknown_ids(state: &WalletState, ids: &[OutputId]) -> Vec<OutputId> {
    ids.iter()
        .filter(|id| !state.outputs.contains_key(id))
        .copied()
        .collect()
}

/// can a tx landing at `inclusion_height` (adjusted time
/// `adjusted_time`) actually spend this output? coinbase waits 60,
/// everything waits 10, per-output timelocks on top.
fn is_unlocked(received: &ReceivedOutput, inclusion_height: u64, adjusted_time: u64) -> bool {
    let window = if received.is_coinbase {
        MINED_MONEY_UNLOCK_WINDOW
    } else {
        TX_SPENDABLE_AGE
    };

    if inclusion_height < received.received_at_height.saturating_add(window) {
        return false;
    }

    match received.additional_timelock {
        Timelock::None => true,
        Timelock::Block(block) => inclusion_height >= block as u64,
        // wallet2: adjusted_time + TIME_LOCK_LEEWAY >= unlock_time,
        // rearranged so neither side can overflow.
        Timelock::Time(time) => time.saturating_sub(TIME_LOCK_LEEWAY) <= adjusted_time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// an inclusion height comfortably above every output's window.
    const INCLUSION_HEIGHT: u64 = 600;
    const ADJUSTED_TIME: u64 = 100_000;

    fn output(id: u8, amount_pico: u64, at_height: u64, is_coinbase: bool) -> ReceivedOutput {
        ReceivedOutput {
            id: OutputId {
                transaction: [id; 32],
                index: 0,
            },
            amount_pico,
            received_at_height: at_height,
            is_coinbase,
            additional_timelock: Timelock::None,
        }
    }

    fn id(id: u8) -> OutputId {
        OutputId {
            transaction: [id; 32],
            index: 0,
        }
    }

    /// outputs received low enough that the default inclusion height
    /// already clears every unlock rule.
    fn mature_output(id: u8, amount_pico: u64) -> ReceivedOutput {
        output(id, amount_pico, INCLUSION_HEIGHT - 100, false)
    }

    #[test]
    fn empty_wallet_reports_zero_balances_and_rejects_selection() {
        let state = WalletState::default();

        assert_eq!(state.balance(), Ok(0));
        assert_eq!(
            state.unlocked_balance(INCLUSION_HEIGHT, ADJUSTED_TIME),
            Ok(0)
        );
        assert_eq!(state.pending_balance(), Ok(0));
        assert_eq!(
            state.select_inputs(1, INCLUSION_HEIGHT, ADJUSTED_TIME),
            Err(StateError::InsufficientUnlocked {
                available: 0,
                required: 1
            })
        );
    }

    #[test]
    fn balance_sums_available_outputs() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 100));
        state.receive(mature_output(2, 50));

        assert_eq!(state.balance(), Ok(150));
    }

    #[test]
    fn receive_is_idempotent_and_keeps_first_facts() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 100));
        state.receive(output(1, 999, 1, false));

        assert_eq!(state.balance(), Ok(100));
        assert_eq!(
            state
                .output(&id(1))
                .map(|received| received.received_at_height),
            Some(INCLUSION_HEIGHT - 100)
        );
    }

    /// the height convention: an output received at 200 (0-based)
    /// opens up for a tx landing at 210, ie tip 209 plus one.
    #[test]
    fn regular_output_unlocks_at_inclusion_height_ten_blocks_on() {
        let mut state = WalletState::default();
        state.receive(output(1, 100, 200, false));

        assert_eq!(state.unlocked_balance(209, 0), Ok(0));
        assert_eq!(state.unlocked_balance(210, 0), Ok(100));
    }

    #[test]
    fn coinbase_unlocks_after_sixty_blocks() {
        let mut state = WalletState::default();
        state.receive(output(1, 100, 200, true));

        assert_eq!(state.unlocked_balance(259, 0), Ok(0));
        assert_eq!(state.unlocked_balance(260, 0), Ok(100));
    }

    #[test]
    fn additional_block_timelock_is_respected() {
        let mut state = WalletState::default();
        state.receive(ReceivedOutput {
            additional_timelock: Timelock::Block(300),
            ..output(1, 100, 200, false)
        });

        assert_eq!(state.unlocked_balance(299, 0), Ok(0));
        assert_eq!(state.unlocked_balance(300, 0), Ok(100));
    }

    #[test]
    fn additional_time_timelock_grants_wallet2_leeway() {
        let mut state = WalletState::default();
        state.receive(ReceivedOutput {
            additional_timelock: Timelock::Time(1_000),
            ..output(1, 100, 200, false)
        });

        // wallet2: adjusted_time + 120 >= unlock_time.
        assert_eq!(state.unlocked_balance(210, 879), Ok(0));
        assert_eq!(state.unlocked_balance(210, 880), Ok(100));
    }

    #[test]
    fn spent_outputs_leave_every_balance() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 100));
        state.receive(mature_output(2, 50));

        state.confirm_spent(&[id(1)], 220).unwrap();

        assert_eq!(state.balance(), Ok(50));
        assert_eq!(
            state.unlocked_balance(INCLUSION_HEIGHT, ADJUSTED_TIME),
            Ok(50)
        );
    }

    #[test]
    fn pending_outputs_leave_balances_and_selection_until_resolved() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 100));
        state.receive(mature_output(2, 50));

        let ids = [id(1)];
        state.mark_pending(&ids).unwrap();

        assert_eq!(state.balance(), Ok(50));
        assert_eq!(state.pending_balance(), Ok(100));
        assert_eq!(state.pending_ids(), vec![id(1)]);

        // pending outputs dont get selected
        assert_eq!(
            state.select_inputs(100, INCLUSION_HEIGHT, ADJUSTED_TIME),
            Err(StateError::InsufficientUnlocked {
                available: 50,
                required: 100
            })
        );

        state.release_pending(&ids).unwrap();
        assert_eq!(state.balance(), Ok(150));
        assert_eq!(
            state
                .select_inputs(100, INCLUSION_HEIGHT, ADJUSTED_TIME)
                .unwrap(),
            vec![id(1)]
        );

        state.mark_pending(&ids).unwrap();
        state.confirm_spent(&ids, 220).unwrap();
        assert_eq!(state.balance(), Ok(50));
        assert_eq!(state.pending_balance(), Ok(0));
    }

    #[test]
    fn mutations_on_unknown_outputs_are_errors_and_mutate_nothing() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 100));

        let unknown = [id(9)];
        assert_eq!(
            state.mark_pending(&unknown),
            Err(StateError::UnknownOutputs(vec![id(9)]))
        );
        assert_eq!(
            state.release_pending(&unknown),
            Err(StateError::UnknownOutputs(vec![id(9)]))
        );
        assert_eq!(
            state.confirm_spent(&unknown, 220),
            Err(StateError::UnknownOutputs(vec![id(9)]))
        );

        // the failed calls touched nothing
        assert_eq!(state.balance(), Ok(100));
    }

    #[test]
    fn reconfirming_a_spend_at_a_different_height_is_an_error() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 100));

        state.confirm_spent(&[id(1)], 220).unwrap();
        // Same height: idempotent.
        state.confirm_spent(&[id(1)], 220).unwrap();
        // different height: a reorg has to go through rollback_to
        assert_eq!(
            state.confirm_spent(&[id(1)], 230),
            Err(StateError::InconsistentSpend {
                id: id(1),
                existing: 220,
                new: 230
            })
        );
    }

    #[test]
    fn releasing_after_confirmation_cannot_resurrect_an_output() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 100));

        state.confirm_spent(&[id(1)], 220).unwrap();
        // release_pending only touches Pending outputs.
        state.release_pending(&[id(1)]).unwrap();

        assert_eq!(state.balance(), Ok(0));
    }

    #[test]
    fn rollback_drops_outputs_and_reverts_spends_above_the_height() {
        let mut state = WalletState::default();
        state.receive(output(1, 100, 200, false));
        state.receive(output(2, 50, 300, false));
        state.receive(output(3, 70, 400, false));

        state.confirm_spent(&[id(1)], 250).unwrap();
        state.confirm_spent(&[id(3)], 450).unwrap();

        // A reorg orphans everything from height 400 on:
        // output 3 vanishes, its spend with it.
        state.rollback_to(400);

        assert_eq!(state.balance(), Ok(50));
        assert!(state.output(&id(3)).is_none());
    }

    #[test]
    fn selection_prefers_larger_outputs_and_is_deterministic() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 30));
        state.receive(mature_output(2, 70));
        state.receive(mature_output(3, 40));

        let selected = state
            .select_inputs(100, INCLUSION_HEIGHT, ADJUSTED_TIME)
            .unwrap();
        assert_eq!(selected.len(), 2);
        assert!(selected.contains(&id(2)));
        assert!(selected.contains(&id(3)));

        let again = state
            .select_inputs(100, INCLUSION_HEIGHT, ADJUSTED_TIME)
            .unwrap();
        assert_eq!(selected, again);
    }

    #[test]
    fn equal_amounts_are_tie_broken_by_output_id() {
        let mut state = WalletState::default();
        state.receive(mature_output(2, 40));
        state.receive(mature_output(1, 40));
        state.receive(mature_output(3, 40));

        assert_eq!(
            state
                .select_inputs(40, INCLUSION_HEIGHT, ADJUSTED_TIME)
                .unwrap(),
            vec![id(1)]
        );
        assert_eq!(
            state
                .select_inputs(80, INCLUSION_HEIGHT, ADJUSTED_TIME)
                .unwrap(),
            vec![id(1), id(2)]
        );
    }

    #[test]
    fn zero_amount_outputs_are_counted_but_never_selected() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 0));
        state.receive(mature_output(2, 10));

        assert_eq!(state.balance(), Ok(10));
        // greedy stops at the target, so a zero
        // output never gets picked up
        assert_eq!(
            state
                .select_inputs(10, INCLUSION_HEIGHT, ADJUSTED_TIME)
                .unwrap(),
            vec![id(2)]
        );
    }

    #[test]
    fn selection_never_takes_locked_outputs() {
        let mut state = WalletState::default();
        state.receive(output(1, 100, 200, true));

        let err = state.select_inputs(50, 259, 0).unwrap_err();
        assert_eq!(
            err,
            StateError::InsufficientUnlocked {
                available: 0,
                required: 50
            }
        );

        // same output is selectable once its mature
        assert_eq!(state.select_inputs(50, 260, 0).unwrap(), vec![id(1)]);
    }

    #[test]
    fn zero_target_is_rejected() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 100));

        assert_eq!(
            state.select_inputs(0, INCLUSION_HEIGHT, ADJUSTED_TIME),
            Err(StateError::ZeroTarget)
        );
        assert_eq!(
            state.select_inputs_with_fee(0, INCLUSION_HEIGHT, ADJUSTED_TIME, |_| 0),
            Err(StateError::ZeroTarget)
        );
    }

    #[test]
    fn selection_fails_when_insufficient() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 10));

        assert!(matches!(
            state.select_inputs(11, INCLUSION_HEIGHT, ADJUSTED_TIME),
            Err(StateError::InsufficientUnlocked { .. })
        ));
    }

    #[test]
    fn fee_aware_selection_grows_until_target_and_fee_are_covered() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 40));
        state.receive(mature_output(2, 35));
        state.receive(mature_output(3, 25));

        // A flat fee of 30 regardless of input count.
        let (selected, total) = state
            .select_inputs_with_fee(70, INCLUSION_HEIGHT, ADJUSTED_TIME, |_| 30)
            .unwrap();
        assert_eq!(selected.len(), 3);
        assert_eq!(total, 100);
    }

    #[test]
    fn fee_aware_selection_fails_when_never_covered() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 40));

        assert!(matches!(
            state.select_inputs_with_fee(70, INCLUSION_HEIGHT, ADJUSTED_TIME, |_| 5),
            Err(StateError::InsufficientUnlocked { .. })
        ));
    }

    #[test]
    fn a_zero_fee_schedule_matches_plain_selection() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 30));
        state.receive(mature_output(2, 70));

        let plain = state
            .select_inputs(70, INCLUSION_HEIGHT, ADJUSTED_TIME)
            .unwrap();
        let (fee_aware, _) = state
            .select_inputs_with_fee(70, INCLUSION_HEIGHT, ADJUSTED_TIME, |_| 0)
            .unwrap();

        assert_eq!(plain, fee_aware);
    }

    #[test]
    fn an_overflowing_fee_requirement_is_an_error() {
        let mut state = WalletState::default();
        state.receive(mature_output(1, 40));

        assert_eq!(
            state.select_inputs_with_fee(70, INCLUSION_HEIGHT, ADJUSTED_TIME, |_| u64::MAX),
            Err(StateError::BalanceOverflow)
        );
    }
}
