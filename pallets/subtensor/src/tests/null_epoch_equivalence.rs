#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

//! The shared Null epoch must leave exactly what the per-mechanism epoch leaves.
//!
//! `epoch_per_mechanism_with_budgets` is the implementation that Null subnets used
//! before `null_epoch_with_mechanism_budgets`. Each case builds one state, runs both
//! paths on that state, and compares the returned emissions, the events, the decoded
//! epoch storage and the root of the complete state.

use super::mock::*;
use crate::epoch::math::{
    fixed_proportion_to_u16, fixed64_to_fixed32, inplace_normalize_64, vec_fixed64_to_fixed32,
};
use crate::epoch::null_epoch::apportion_equal_units;
use crate::epoch::run_epoch::{EpochTerms, apportion_units};
use crate::subnets::mechanism::{
    MAX_MECHANISM_COUNT_PER_SUBNET, NULL_UID_BUDGET, TOTAL_NULL_WEIGHT_BUDGET,
};
use crate::*;
use alloc::collections::BTreeMap;
use frame_support::assert_ok;
use frame_support::storage::{TransactionOutcome, with_transaction};
use safe_math::FixedExt;
use sp_core::{H256, U256};
use sp_runtime::{DispatchError, PerU16, StateVersion};
use sp_std::collections::vec_deque::VecDeque;
use substrate_fixed::types::{I32F32, I64F64, U64F64};
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid, NetUidStorageIndex};

mod arithmetic;
mod directed;
mod random;

/// Epoch storage in decoded form. It gives a readable difference when two runs disagree.
#[derive(Debug, PartialEq)]
struct EpochStorage {
    incentive: Vec<Vec<PerU16>>,
    consensus_by_mechanism: Vec<Vec<PerU16>>,
    active: Vec<bool>,
    emission: Vec<AlphaBalance>,
    consensus: Vec<PerU16>,
    dividends: Vec<PerU16>,
    validator_trust: Vec<PerU16>,
    validator_permit: Vec<bool>,
    stake_weight: Vec<u16>,
    voting_power: Vec<(U256, u64)>,
    total_voting_power: u64,
}

/// Everything that one epoch leaves behind.
#[derive(Debug, PartialEq)]
struct Outcome {
    emissions: Vec<(U256, AlphaBalance, AlphaBalance)>,
    events: Vec<RuntimeEvent>,
    storage: EpochStorage,
    /// Root of the complete state. It covers every key and every value.
    root: Vec<u8>,
}

fn mechanism_index(netuid: NetUid, mechanism: u8) -> NetUidStorageIndex {
    SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(mechanism))
}

fn epoch_storage(netuid: NetUid) -> EpochStorage {
    let indices = || (0..MAX_MECHANISM_COUNT_PER_SUBNET).map(|m| mechanism_index(netuid, m));
    let mut voting_power: Vec<(U256, u64)> = VotingPower::<Test>::iter_prefix(netuid).collect();
    voting_power.sort();
    EpochStorage {
        incentive: indices().map(Incentive::<Test>::get).collect(),
        consensus_by_mechanism: indices().map(ConsensusByMechanism::<Test>::get).collect(),
        active: Active::<Test>::get(netuid),
        emission: Emission::<Test>::get(netuid),
        consensus: Consensus::<Test>::get(netuid),
        dividends: Dividends::<Test>::get(netuid),
        validator_trust: ValidatorTrust::<Test>::get(netuid),
        validator_permit: ValidatorPermit::<Test>::get(netuid),
        stake_weight: StakeWeight::<Test>::get(netuid),
        voting_power,
        total_voting_power: TotalVotingPower::<Test>::get(netuid),
    }
}

/// Runs `work` on the current state and restores the state.
fn rolled_back<R>(work: impl FnOnce() -> R) -> R {
    let result: Result<R, DispatchError> =
        with_transaction(|| TransactionOutcome::Rollback(Ok(work())));
    result.unwrap()
}

/// Runs `epoch` on the current state, records what it leaves, and restores the state.
fn outcome_of(
    netuid: NetUid,
    epoch: impl FnOnce() -> Vec<(U256, AlphaBalance, AlphaBalance)>,
) -> Outcome {
    rolled_back(|| {
        System::reset_events();
        let emissions = epoch();
        Outcome {
            emissions,
            events: System::events()
                .into_iter()
                .map(|record| record.event)
                .collect(),
            storage: epoch_storage(netuid),
            root: sp_io::storage::root(StateVersion::V1),
        }
    })
}

/// Sum of two balances, capped at the largest balance: the epoch never wraps a sum.
fn capped_sum(a: AlphaBalance, b: AlphaBalance) -> AlphaBalance {
    let sum = u128::from(u64::from(a)) + u128::from(u64::from(b));
    AlphaBalance::from(u64::try_from(sum).unwrap_or(u64::MAX))
}

/// The subnet-level terms as the per-mechanism path folds them: the fold of
/// `epoch_per_mechanism_with_budgets` for a Null subnet, without its storage writes.
/// Storage does not hold every field of these terms, so the fields are compared here.
fn reference_terms(
    netuid: NetUid,
    miner_budget: AlphaBalance,
    local_dividend_budget: AlphaBalance,
    root_budget: AlphaBalance,
) -> BTreeMap<U256, EpochTerms> {
    let rao_emission = capped_sum(capped_sum(miner_budget, local_dividend_budget), root_budget);
    let miners = SubtensorModule::split_emissions(netuid, miner_budget);
    let local_dividends = SubtensorModule::split_emissions(netuid, local_dividend_budget);
    let roots = SubtensorModule::split_emissions(netuid, root_budget);
    let emissions: Vec<AlphaBalance> = miners
        .iter()
        .zip(&local_dividends)
        .zip(&roots)
        .map(|((miner, local), root)| capped_sum(capped_sum(*miner, *local), *root))
        .collect();
    let fixed = |amount: AlphaBalance| U64F64::from_num(u64::from(amount));
    emissions
        .into_iter()
        .enumerate()
        .flat_map(|(sub_id_usize, sub_emission)| {
            let sub_id_u8: u8 = sub_id_usize.try_into().unwrap_or_default();
            let mining_budget = miners.get(sub_id_usize).copied().unwrap_or_default();
            let local = local_dividends
                .get(sub_id_usize)
                .copied()
                .unwrap_or_default();
            let root = roots.get(sub_id_usize).copied().unwrap_or_default();
            let epoch_output = SubtensorModule::epoch_mechanism_with_budgets(
                netuid,
                MechId::from(sub_id_u8),
                sub_emission,
                mining_budget,
                capped_sum(local, root),
                capped_sum(mining_budget, local),
            );
            // The first mechanism has full weight in an epoch without emission.
            let sub_weight = fixed(sub_emission).safe_div_or(
                fixed(rao_emission),
                U64F64::from_num(if sub_id_u8 == 0 { 1 } else { 0 }),
            );
            epoch_output
                .0
                .into_iter()
                .map(move |(hotkey, terms)| (hotkey, (terms, sub_weight)))
        })
        .fold(BTreeMap::new(), |mut acc, (hotkey, (terms, sub_weight))| {
            let weighted = SubtensorModule::weighted_acc_u16;
            acc.entry(hotkey)
                .and_modify(|acc_terms: &mut EpochTerms| {
                    acc_terms.validator_emission =
                        capped_sum(acc_terms.validator_emission, terms.validator_emission);
                    acc_terms.server_emission =
                        capped_sum(acc_terms.server_emission, terms.server_emission);
                    acc_terms.dividend = weighted(acc_terms.dividend, terms.dividend, sub_weight);
                    acc_terms.stake_weight =
                        weighted(acc_terms.stake_weight, terms.stake_weight, sub_weight);
                    acc_terms.active |= terms.active;
                    acc_terms.emission = capped_sum(acc_terms.emission, terms.emission);
                    acc_terms.consensus =
                        weighted(acc_terms.consensus, terms.consensus, sub_weight);
                    acc_terms.validator_trust =
                        weighted(acc_terms.validator_trust, terms.validator_trust, sub_weight);
                    acc_terms.new_validator_permit |= terms.new_validator_permit;
                    acc_terms.stake = capped_sum(acc_terms.stake, terms.stake);
                })
                .or_insert_with(|| EpochTerms {
                    uid: terms.uid,
                    dividend: weighted(0, terms.dividend, sub_weight),
                    incentive: weighted(0, terms.incentive, sub_weight),
                    validator_emission: terms.validator_emission,
                    server_emission: terms.server_emission,
                    stake_weight: weighted(0, terms.stake_weight, sub_weight),
                    active: terms.active,
                    emission: terms.emission,
                    consensus: weighted(0, terms.consensus, sub_weight),
                    validator_trust: weighted(0, terms.validator_trust, sub_weight),
                    new_validator_permit: terms.new_validator_permit,
                    bond: Vec::new(),
                    stake: terms.stake,
                });
            acc
        })
}

/// Runs the per-mechanism reference and the shared path on the current state and asserts
/// that they leave the same emissions, events and storage. Returns what they leave.
fn assert_equivalent(netuid: NetUid, miner: u64, local: u64, root: u64) -> Outcome {
    assert_eq!(
        SubtensorModule::get_epoch_consensus(netuid),
        EpochConsensus::Null
    );
    let reference = outcome_of(netuid, || {
        SubtensorModule::epoch_per_mechanism_with_budgets(
            netuid,
            miner.into(),
            local.into(),
            root.into(),
        )
    });
    // The production entry point: it must select the shared path for a Null subnet.
    let shared = outcome_of(netuid, || {
        SubtensorModule::epoch_with_mechanism_budgets(
            netuid,
            miner.into(),
            local.into(),
            root.into(),
        )
    });
    assert_eq!(shared.emissions, reference.emissions, "returned emissions");
    assert_eq!(shared.events, reference.events, "events");
    assert_eq!(shared.storage, reference.storage, "epoch storage");
    assert_eq!(shared.root, reference.root, "state root");

    // Every field of the subnet-level terms, including those that no storage item holds.
    let reference_terms = reference_terms(netuid, miner.into(), local.into(), root.into());
    let shared_terms = rolled_back(|| {
        SubtensorModule::null_epoch_terms(netuid, miner.into(), local.into(), root.into())
    });
    assert_eq!(
        format!("{shared_terms:?}"),
        format!("{reference_terms:?}"),
        "subnet-level terms"
    );
    reference
}

/// Miner and validator totals of the emissions that an epoch returned.
fn paid(outcome: &Outcome) -> (u128, u128) {
    outcome
        .emissions
        .iter()
        .fold((0, 0), |(miners, validators), (_, miner, validator)| {
            (
                miners + u128::from(u64::from(*miner)),
                validators + u128::from(u64::from(*validator)),
            )
        })
}

/// True when the mechanism paid by weights, false when it used the equal fallback.
fn paid_by_weights(outcome: &Outcome, mechanism: u8) -> bool {
    let incentive = &outcome.storage.incentive[usize::from(mechanism)];
    incentive
        .iter()
        .any(|value| Some(value) != incentive.first())
}

fn hotkey(uid: u16) -> U256 {
    U256::from(10_000 + u64::from(uid))
}

fn set_stake(netuid: NetUid, uid: u16, alpha: u64) {
    TotalHotkeyAlpha::<Test>::insert(hotkey(uid), netuid, AlphaBalance::from(alpha));
}

/// SplitMix64: a fixed sequence for each seed, independent of any library version.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value below `bound`, which is not zero.
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    /// A value from `low` to `high`, both included.
    fn range(&mut self, low: u64, high: u64) -> u64 {
        low + self.below(high - low + 1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }
}

/// A budget from zero to the largest amount, with the small amounts that round.
fn random_budget(rng: &mut Rng) -> u64 {
    match rng.below(14) {
        0 => 0,
        1 => 1,
        2 => rng.range(2, 60),
        3 => u64::MAX,
        4 => u64::MAX / rng.range(2, 5),
        _ => rng.range(0, 20_000_000_000_000),
    }
}
