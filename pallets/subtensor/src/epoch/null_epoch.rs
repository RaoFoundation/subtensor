//! Null consensus epoch for all mechanisms of one subnet.
//!
//! `epoch_per_mechanism_with_budgets` runs a complete epoch for every mechanism.
//! Under Null consensus, most of that work reads state that no mechanism owns:
//! hotkeys, registration blocks and stake. This module reads that state once per
//! subnet. It then builds each mechanism's result from the elected validator's
//! weight row.
//!
//! Storage, events and returned emissions equal those of the per-mechanism path
//! for every input. `tests::null_epoch_equivalence` compares the two paths.

use super::*;
use crate::epoch::math::*;
use crate::epoch::run_epoch::{EpochTerms, apportion_units};
use alloc::collections::BTreeMap;
use safe_math::*;
use sp_runtime::PerU16;
use substrate_fixed::types::{I64F64, U64F64};
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid, NetUidStorageIndex};

/// Funded budgets of one mechanism.
struct NullMechanismBudgets {
    mecid: MechId,
    mining: AlphaBalance,
    dividend: AlphaBalance,
    /// Paid to miners when no UID has stake. It never includes root rewards.
    fallback: AlphaBalance,
    /// Share of the subnet's emission, for the subnet-level weighted terms.
    weight: U64F64,
}

/// State of one Null epoch that is the same for every mechanism.
struct NullSubnetInputs<AccountId> {
    netuid: NetUid,
    /// Registered UID count.
    n: u16,
    current_block: u64,
    activity_cutoff: u64,
    block_at_registration: Vec<u64>,
    owner_uid: Option<u16>,
    /// Elected validator: the highest unrounded stake, the lowest UID on a tie.
    winner: Option<usize>,
    /// Hotkey whose epoch terms hold the elected validator's UID.
    winner_hotkey: Option<AccountId>,
    commit_reveal: bool,
    /// UIDs with positive stake, ascending, and their dividend shares.
    validator_uids: Vec<usize>,
    validator_shares: Vec<u128>,
    /// UIDs that have epoch terms, ascending. Per-mechanism vectors use this order.
    term_uids: Vec<usize>,
    /// Index in `term_uids` of each UID below `n`.
    term_index: Vec<Option<usize>>,
    /// Reported dividend, stake weight and whole stake of each UID below `n`.
    dividend: Vec<u16>,
    stake_weight: Vec<u16>,
    raw_stake: Vec<u64>,
}

/// Sums over the mechanisms processed so far, by UID, and results that
/// mechanisms share.
struct NullTotals {
    server_emission: Vec<AlphaBalance>,
    validator_emission: Vec<AlphaBalance>,
    active: Vec<bool>,
    /// Incentive of the first mechanism. The subnet-level terms report no other.
    first_incentive: Vec<u16>,
    /// Dividend payouts by budget. They depend on the budget alone, so
    /// mechanisms with equal budgets share them.
    dividend_payouts: BTreeMap<u64, Vec<AlphaBalance>>,
    /// Reported incentive of the equal fallback. It depends on the UID count alone.
    equal_incentive: Option<u16>,
}

/// Results of a function of a `u16`, kept by argument. The reported terms of a
/// subnet repeat a small set of values.
struct U16Cache<F> {
    compute: F,
    /// Index: the argument. `None`: not computed.
    results: Vec<Option<u16>>,
}

impl<F: Fn(u16) -> u16> U16Cache<F> {
    fn new(compute: F) -> Self {
        Self {
            compute,
            results: Vec::new(),
        }
    }

    fn get(&mut self, value: u16) -> u16 {
        let index = usize::from(value);
        if let Some(Some(result)) = self.results.get(index) {
            return *result;
        }
        let result = (self.compute)(value);
        if self.results.len() <= index {
            self.results.resize(index.saturating_add(1), None);
        }
        if let Some(slot) = self.results.get_mut(index) {
            *slot = Some(result);
        }
        result
    }
}

/// `apportion_units` for `count` participants that hold one share each.
///
/// Participant `i` receives `floor((i + 1) * budget / count) - floor(i * budget / count)`.
/// With `budget = quotient * count + remainder`, that is `quotient`, plus one
/// each time the running remainder `(i * remainder) mod count` reaches `count`.
pub(crate) fn apportion_equal_units(count: usize, budget: u64) -> Vec<AlphaBalance> {
    let count_units = count as u64;
    let Some(quotient) = budget.checked_div(count_units) else {
        return Vec::new();
    };
    let remainder = budget.checked_rem(count_units).unwrap_or(0);
    let mut carry = 0u64;
    (0..count)
        .map(|_| {
            carry = carry.saturating_add(remainder);
            if carry >= count_units {
                carry = carry.saturating_sub(count_units);
                AlphaBalance::from(quotient.saturating_add(1))
            } else {
                AlphaBalance::from(quotient)
            }
        })
        .collect()
}

impl<T: Config> Pallet<T> {
    /// Null epoch for every mechanism of `netuid`. Storage, events and returned
    /// emissions equal those of `epoch_per_mechanism_with_budgets`.
    pub(crate) fn null_epoch_with_mechanism_budgets(
        netuid: NetUid,
        miner_budget: AlphaBalance,
        local_dividend_budget: AlphaBalance,
        root_budget: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance, AlphaBalance)> {
        let aggregated =
            Self::null_epoch_terms(netuid, miner_budget, local_dividend_budget, root_budget);

        // State updates from epoch function
        Self::persist_netuid_epoch_terms(netuid, &aggregated);

        // Update voting power EMA for all validators on this subnet
        Self::update_voting_power_for_subnet(netuid, &aggregated);

        // Emission tuples ( hotkeys, server_emission, validator_emission )
        aggregated
            .into_iter()
            .map(|(hotkey, terms)| (hotkey, terms.server_emission, terms.validator_emission))
            .collect()
    }

    /// Runs every mechanism of `netuid`: stores its incentive and consensus and
    /// deposits its event. Returns the subnet-level terms of every hotkey.
    pub(crate) fn null_epoch_terms(
        netuid: NetUid,
        miner_budget: AlphaBalance,
        local_dividend_budget: AlphaBalance,
        root_budget: AlphaBalance,
    ) -> BTreeMap<T::AccountId, EpochTerms> {
        let budgets =
            Self::null_mechanism_budgets(netuid, miner_budget, local_dividend_budget, root_budget);
        let (mut terms_map, inputs) = Self::null_subnet_inputs(netuid);

        let mut totals = NullTotals {
            server_emission: vec![AlphaBalance::ZERO; usize::from(inputs.n)],
            validator_emission: vec![AlphaBalance::ZERO; usize::from(inputs.n)],
            active: Vec::new(),
            first_incentive: vec![0; usize::from(inputs.n)],
            dividend_payouts: BTreeMap::new(),
            equal_incentive: None,
        };
        for (position, budget) in budgets.iter().enumerate() {
            Self::null_mechanism_epoch(&inputs, budget, position == 0, &mut totals);
        }

        // A weighted term has the same value in every mechanism, so its weighted
        // sum over the mechanisms is a function of that value alone.
        let weights: Vec<U64F64> = budgets.iter().map(|budget| budget.weight).collect();
        let mut weighted_sum = U16Cache::new(|value| {
            weights.iter().fold(0u16, |total, weight| {
                Self::weighted_acc_u16(total, value, *weight)
            })
        });
        let mechanism_count = budgets.len() as u64;

        // Without a mechanism, no epoch ran and no UID has terms.
        let Some(first_weight) = weights.first() else {
            return BTreeMap::new();
        };
        let mut first_incentive =
            U16Cache::new(|score| Self::weighted_acc_u16(0, score, *first_weight));
        // Null reports no consensus and no validator trust.
        let unreported = weighted_sum.get(0);
        let by_uid = |values: &[u16], uid: usize| values.get(uid).copied().unwrap_or_default();
        for terms in terms_map.values_mut() {
            let uid = terms.uid;
            terms.dividend = weighted_sum.get(by_uid(&inputs.dividend, uid));
            terms.incentive = first_incentive.get(by_uid(&totals.first_incentive, uid));
            terms.validator_emission = totals
                .validator_emission
                .get(uid)
                .copied()
                .unwrap_or_default();
            terms.server_emission = totals.server_emission.get(uid).copied().unwrap_or_default();
            terms.stake_weight = weighted_sum.get(by_uid(&inputs.stake_weight, uid));
            terms.active = totals.active.get(uid).copied().unwrap_or_default();
            // Saturating sums of non-negative amounts do not depend on their order.
            terms.emission = terms
                .server_emission
                .saturating_add(terms.validator_emission);
            terms.consensus = unreported;
            terms.validator_trust = unreported;
            terms.new_validator_permit = Some(uid) == inputs.winner;
            // Every mechanism adds the UID's stake to the subnet-level terms.
            terms.stake = inputs
                .raw_stake
                .get(uid)
                .copied()
                .unwrap_or_default()
                .saturating_mul(mechanism_count)
                .into();
        }
        terms_map
    }

    /// Splits each funded pool between the mechanisms before any payout is rounded.
    fn null_mechanism_budgets(
        netuid: NetUid,
        miner_budget: AlphaBalance,
        local_dividend_budget: AlphaBalance,
        root_budget: AlphaBalance,
    ) -> Vec<NullMechanismBudgets> {
        let rao_emission = miner_budget
            .saturating_add(local_dividend_budget)
            .saturating_add(root_budget);
        let miners = Self::split_emissions(netuid, miner_budget);
        let local_dividends = Self::split_emissions(netuid, local_dividend_budget);
        let roots = Self::split_emissions(netuid, root_budget);
        miners
            .iter()
            .zip(&local_dividends)
            .zip(&roots)
            .enumerate()
            .map(|(index, ((miner, local), root))| {
                let mecid: u8 = index.try_into().unwrap_or_default();
                let emission = miner.saturating_add(*local).saturating_add(*root);
                NullMechanismBudgets {
                    mecid: MechId::from(mecid),
                    mining: *miner,
                    dividend: local.saturating_add(*root),
                    fallback: miner.saturating_add(*local),
                    // The first mechanism has full weight in an epoch with no emission.
                    weight: U64F64::saturating_from_num(emission).safe_div_or(
                        U64F64::saturating_from_num(rao_emission),
                        U64F64::saturating_from_num(if mecid == 0 { 1 } else { 0 }),
                    ),
                }
            })
            .collect()
    }

    /// Loads the state that every mechanism of `netuid` shares. Returns the epoch
    /// terms of each hotkey, which hold only its UID, and the shared inputs.
    fn null_subnet_inputs(
        netuid: NetUid,
    ) -> (
        BTreeMap<T::AccountId, EpochTerms>,
        NullSubnetInputs<T::AccountId>,
    ) {
        let terms_map: BTreeMap<T::AccountId, EpochTerms> = Keys::<T>::iter_prefix(netuid)
            .map(|(uid, hotkey)| {
                (
                    hotkey,
                    EpochTerms {
                        uid: uid as usize,
                        ..Default::default()
                    },
                )
            })
            .collect();

        // The reads keep the order of the per-mechanism epoch. In the benchmark,
        // registration blocks read after stake cost a few percent more.
        let n = Self::get_subnetwork_n(netuid);
        let current_block: u64 = Self::get_current_block_as_u64();
        let activity_cutoff: u64 = Self::get_activity_cutoff_blocks(netuid);
        let block_at_registration: Vec<u64> = Self::get_block_at_registration(netuid);
        let (total_stake, _alpha_stake, _tao_stake): (Vec<I64F64>, Vec<I64F64>, Vec<I64F64>) =
            Self::get_null_stake_weights_for_network(netuid);
        let min_stake = Self::get_stake_threshold();
        let owner_uid: Option<u16> = Self::get_owner_uid(netuid);

        // Stake below the threshold reports a zero stake weight. It still earns dividends.
        let mut filtered_stake: Vec<I64F64> = total_stake
            .iter()
            .enumerate()
            .map(|(uid, &s)| {
                if owner_uid != Some(uid as u16) && fixed64_to_u64(s) < min_stake {
                    return I64F64::from(0);
                }
                s
            })
            .collect();
        inplace_normalize_64(&mut filtered_stake);
        let stake_weight: Vec<u16> = vec_fixed64_to_fixed32(filtered_stake)
            .iter()
            .map(|xi| fixed_proportion_to_u16(*xi))
            .collect();

        // Strict comparison preserves the first UID when stake ties.
        let winner = Self::null_validator_winner(&total_stake);

        // Dividends follow the original stake precision, independent of reporting.
        let dividend_shares: Vec<u128> = total_stake
            .iter()
            .map(|stake| {
                if *stake <= I64F64::from(0) {
                    0
                } else {
                    stake.to_bits() as u128
                }
            })
            .collect();
        let mut dividend_report: Vec<I64F64> = dividend_shares
            .iter()
            .map(|bits| I64F64::from_bits(*bits as i128))
            .collect();
        inplace_normalize_64(&mut dividend_report);
        let dividend: Vec<u16> = vec_fixed64_to_fixed32(dividend_report)
            .iter()
            .map(|xi| fixed_proportion_to_u16(*xi))
            .collect();
        let raw_stake: Vec<u64> = total_stake
            .iter()
            .map(|s| s.saturating_to_num::<u64>())
            .collect();
        let (validator_uids, validator_shares): (Vec<usize>, Vec<u128>) = dividend_shares
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, share)| *share > 0)
            .unzip();

        let mut term_uids: Vec<usize> = terms_map.values().map(|terms| terms.uid).collect();
        term_uids.sort_unstable();
        let mut term_index: Vec<Option<usize>> = vec![None; usize::from(n)];
        for (index, uid) in term_uids.iter().enumerate() {
            if let Some(slot) = term_index.get_mut(*uid) {
                *slot = Some(index);
            }
        }

        let inputs = NullSubnetInputs {
            netuid,
            n,
            current_block,
            activity_cutoff,
            block_at_registration,
            owner_uid,
            winner,
            winner_hotkey: winner.and_then(|uid| {
                terms_map
                    .iter()
                    .find(|(_, terms)| terms.uid == uid)
                    .map(|(hotkey, _)| hotkey.clone())
            }),
            commit_reveal: Self::get_commit_reveal_weights_enabled(netuid),
            validator_uids,
            validator_shares,
            term_uids,
            term_index,
            dividend,
            stake_weight,
            raw_stake,
        };
        (terms_map, inputs)
    }

    /// Epoch of one mechanism: pays its budgets, adds the payouts and the
    /// activity to `totals`, stores its incentive and consensus, and deposits
    /// its event. `first` is true for the first mechanism of the subnet.
    fn null_mechanism_epoch(
        inputs: &NullSubnetInputs<T::AccountId>,
        budget: &NullMechanismBudgets,
        first: bool,
        totals: &mut NullTotals,
    ) {
        let netuid_index = Self::get_mechanism_storage_index(inputs.netuid, budget.mecid);

        // Activity is the only per-UID input that every mechanism owns.
        let last_update: Vec<u64> = Self::get_last_update(netuid_index);
        if totals.active.len() < last_update.len() {
            totals.active.resize(last_update.len(), false);
        }
        for (active, updated) in totals.active.iter_mut().zip(&last_update) {
            if updated.saturating_add(inputs.activity_cutoff) >= inputs.current_block {
                *active = true;
            }
        }

        // Without stake there are no dividends: the local budget goes to miners.
        let has_validators = !inputs.validator_uids.is_empty();
        let miner_budget = u64::from(if has_validators {
            budget.mining
        } else {
            budget.fallback
        });

        // (UID, reported incentive, miner payout) of every paid miner.
        let shares = Self::null_winner_shares(inputs, netuid_index, &last_update);
        let credits: Vec<(usize, u16, AlphaBalance)> = if shares.is_empty() {
            // Every registered UID participates in the empty-weight fallback.
            let incentive = *totals
                .equal_incentive
                .get_or_insert_with(|| Self::null_equal_incentive(inputs.n));
            apportion_equal_units(usize::from(inputs.n), miner_budget)
                .into_iter()
                .enumerate()
                .map(|(uid, payout)| (uid, incentive, payout))
                .collect()
        } else {
            let units: Vec<u128> = shares
                .iter()
                .map(|(_, weight)| u128::from(*weight))
                .collect();
            let mut report: Vec<I64F64> = units
                .iter()
                .map(|share| I64F64::saturating_from_num(*share))
                .collect();
            inplace_normalize_64(&mut report);
            shares
                .iter()
                .zip(vec_fixed64_to_fixed32(report))
                .zip(apportion_units(&units, miner_budget))
                .map(|(((uid, _), incentive), payout)| {
                    (*uid, fixed_proportion_to_u16(incentive), payout)
                })
                .collect()
        };

        let mut incentive = vec![PerU16::zero(); inputs.term_uids.len()];
        let mut emissions = vec![AlphaBalance::ZERO; inputs.term_uids.len()];
        for (uid, score, payout) in credits {
            if let Some(total) = totals.server_emission.get_mut(uid) {
                *total = total.saturating_add(payout);
            }
            if first && let Some(first_incentive) = totals.first_incentive.get_mut(uid) {
                *first_incentive = score;
            }
            // A UID without a hotkey has no terms: nothing reports or pays its share.
            if let Some(index) = inputs.term_index.get(uid).copied().flatten() {
                if let Some(slot) = incentive.get_mut(index) {
                    *slot = PerU16::from_parts(score);
                }
                if let Some(slot) = emissions.get_mut(index) {
                    *slot = payout;
                }
            }
        }

        if has_validators {
            let dividend_budget = u64::from(budget.dividend);
            let payouts = totals
                .dividend_payouts
                .entry(dividend_budget)
                .or_insert_with(|| apportion_units(&inputs.validator_shares, dividend_budget));
            for (uid, payout) in inputs.validator_uids.iter().zip(payouts.iter()) {
                if let Some(total) = totals.validator_emission.get_mut(*uid) {
                    *total = total.saturating_add(*payout);
                }
            }
        }

        Incentive::<T>::insert(netuid_index, incentive);
        ConsensusByMechanism::<T>::insert(
            netuid_index,
            vec![PerU16::zero(); inputs.term_uids.len()],
        );
        Self::deposit_event(Event::IncentiveAlphaEmittedToMiners {
            netuid: netuid_index,
            emissions,
        });
    }

    /// Positive weights of the elected validator's row for one mechanism, by
    /// ascending UID, after the self-weight, registration and commit masks.
    fn null_winner_shares(
        inputs: &NullSubnetInputs<T::AccountId>,
        netuid_index: NetUidStorageIndex,
        last_update: &[u64],
    ) -> Vec<(usize, u16)> {
        let Some(winner) = inputs.winner else {
            return Vec::new();
        };
        let updated = last_update.get(winner).copied().unwrap_or(0);
        let commit_block = inputs
            .commit_reveal
            .then(|| Self::null_winner_commit_block(inputs, netuid_index));

        // Null loads only the elected validator's row. Historical rows from
        // Yuma or earlier permit holders cannot expand the epoch.
        let mut weight_of: Vec<u16> = vec![0; usize::from(inputs.n)];
        for (target, weight) in Weights::<T>::get(netuid_index, winner as u16) {
            let target = usize::from(target);
            let registered = inputs
                .block_at_registration
                .get(target)
                .copied()
                .unwrap_or(0);
            // Only the subnet owner keeps its self-weight.
            let kept = (target != winner || inputs.owner_uid.map(usize::from) == Some(winner))
                // The row must be newer than the target's registration.
                && updated > registered
                // A pending commit must not be older than the target's registration.
                && commit_block.is_none_or(|block| block >= registered);
            // A target at or above the UID count has no slot. When a stored row
            // repeats a UID, its last entry wins.
            if kept && let Some(slot) = weight_of.get_mut(target) {
                *slot = weight;
            }
        }
        weight_of
            .into_iter()
            .enumerate()
            .filter(|(_, weight)| *weight != 0)
            .collect()
    }

    /// Block of the elected validator's earliest pending commit on one mechanism.
    /// `u64::MAX` means that it has no pending commit.
    fn null_winner_commit_block(
        inputs: &NullSubnetInputs<T::AccountId>,
        netuid_index: NetUidStorageIndex,
    ) -> u64 {
        let mut commit_block = u64::MAX;
        let Some(hotkey) = inputs.winner_hotkey.as_ref() else {
            return commit_block;
        };

        // ---------- v2 ------------------------------------------------------
        // `WeightCommits` tuple: (hash, commit_epoch, commit_block, _).
        if let Some(queue) = WeightCommits::<T>::get(netuid_index, hotkey) {
            for (_, commit_epoch, block, _) in queue.iter() {
                if !Self::is_commit_expired(inputs.netuid, *commit_epoch) {
                    commit_block = commit_block.min(*block);
                    break; // earliest active found
                }
            }
        }

        // ---------- v4 ------------------------------------------------------
        // `TimelockedWeightCommits` is keyed by `commit_epoch`; the value tuple
        // carries the absolute `commit_block` in field 1.
        for (commit_epoch, queue) in TimelockedWeightCommits::<T>::iter_prefix(netuid_index) {
            if Self::is_commit_expired(inputs.netuid, commit_epoch) {
                continue;
            }
            for (who, block, ..) in queue.iter() {
                if who == hotkey {
                    commit_block = commit_block.min(*block);
                }
            }
        }
        commit_block
    }

    /// Reported incentive of each UID when all `count` UIDs hold one share.
    pub(crate) fn null_equal_incentive(count: u16) -> u16 {
        let one = I64F64::saturating_from_num(1u128);
        // The sum and the quotient that `inplace_normalize_64` computes for these shares.
        let total: I64F64 = (0..count).map(|_| one).sum();
        fixed_proportion_to_u16(fixed64_to_fixed32(one.safe_div(total)))
    }
}
