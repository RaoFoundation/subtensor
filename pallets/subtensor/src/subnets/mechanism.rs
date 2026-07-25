//! Sub-subnet (mechanism) indexing, emission splits, and multi-mechanism epoch.
//!
//! Mechanisms share a parent `netuid` but store weights/bonds/incentive under a
//! packed [`NetUidStorageIndex`] = `netuid + mech_id * GLOBAL_MAX_SUBNET_COUNT`.

use super::*;
use crate::epoch::run_epoch::EpochTerms;
use alloc::collections::BTreeMap;
use safe_math::*;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid, NetUidStorageIndex};

/// Theoretical maximum of subnets on bittensor. This value is used in indexed
/// storage of epoch values for sub-subnets as
///
/// `storage_index = netuid + sub_id * GLOBAL_MAX_SUBNET_COUNT`
///
/// For sub_id = 0 this index results in netuid and provides backward compatibility
/// for subnets with default sub-subnet count of 1.
///
/// Changing this value will require a migration of all epoch maps.
///
pub const GLOBAL_MAX_SUBNET_COUNT: u16 = 4096;

// Theoretical maximum number of mechanisms per subnet
// GLOBAL_MAX_SUBNET_COUNT * MAX_MECHANISM_COUNT_PER_SUBNET should be 0x10000
pub const MAX_MECHANISM_COUNT_PER_SUBNET: u8 = 16;

impl<T: Config> Pallet<T> {
    /// Pack `(netuid, mechanism_id)` into the storage index used by weights/bonds maps.
    pub fn get_mechanism_storage_index(netuid: NetUid, sub_id: MechId) -> NetUidStorageIndex {
        u16::from(sub_id)
            .saturating_mul(GLOBAL_MAX_SUBNET_COUNT)
            .saturating_add(u16::from(netuid))
            .into()
    }

    /// Extract parent `netuid` from a mechanism storage index (`index % GLOBAL_MAX_SUBNET_COUNT`).
    pub fn get_netuid(netuid_index: NetUidStorageIndex) -> NetUid {
        if let Some(netuid) = u16::from(netuid_index).checked_rem(GLOBAL_MAX_SUBNET_COUNT) {
            NetUid::from(netuid)
        } else {
            // Because GLOBAL_MAX_SUBNET_COUNT is not zero, this never happens
            NetUid::ROOT
        }
    }

    /// Decode `(netuid, mech_id)` from a storage index and ensure that mechanism exists.
    pub fn get_netuid_and_subid(
        netuid_index: NetUidStorageIndex,
    ) -> Result<(NetUid, MechId), Error<T>> {
        let maybe_netuid = u16::from(netuid_index).checked_rem(GLOBAL_MAX_SUBNET_COUNT);
        if let Some(netuid_u16) = maybe_netuid {
            let netuid = NetUid::from(netuid_u16);

            // Make sure the base subnet exists
            ensure!(
                Self::if_subnet_exist(netuid),
                Error::<T>::MechanismDoesNotExist
            );

            // Extract sub_id
            let sub_id_u8 = u8::try_from(u16::from(netuid_index).safe_div(GLOBAL_MAX_SUBNET_COUNT))
                .map_err(|_| Error::<T>::MechanismDoesNotExist)?;
            let sub_id = MechId::from(sub_id_u8);

            if MechanismCountCurrent::<T>::get(netuid) > sub_id {
                Ok((netuid, sub_id))
            } else {
                Err(Error::<T>::MechanismDoesNotExist.into())
            }
        } else {
            Err(Error::<T>::MechanismDoesNotExist.into())
        }
    }

    /// Current mechanism count for `netuid` (defaults via storage to at least one).
    pub fn get_current_mechanism_count(netuid: NetUid) -> MechId {
        MechanismCountCurrent::<T>::get(netuid)
    }

    /// Error if `netuid` is missing or `sub_id` is outside `[0, mechanism_count)`.
    pub fn ensure_mechanism_exists(netuid: NetUid, sub_id: MechId) -> DispatchResult {
        // Make sure the base subnet exists
        ensure!(
            Self::if_subnet_exist(netuid),
            Error::<T>::MechanismDoesNotExist
        );

        // Make sure the mechanism limit is not exceeded
        ensure!(
            MechanismCountCurrent::<T>::get(netuid) > sub_id,
            Error::<T>::MechanismDoesNotExist
        );
        Ok(())
    }

    /// Cap `max_uids * mechanism_count` so multi-mechanism subnets cannot bloat past default max UIDs.
    pub fn ensure_max_uids_over_all_mechanisms(
        max_uids: u16,
        mechanism_count: MechId,
    ) -> DispatchResult {
        let max_uids_over_all_mechanisms =
            max_uids.saturating_mul(u8::from(mechanism_count) as u16);
        ensure!(
            max_uids_over_all_mechanisms <= DefaultMaxAllowedUids::<T>::get(),
            Error::<T>::TooManyUIDsPerMechanism
        );
        Ok(())
    }

    /// Set the desired value of mechanism count for a subnet identified
    /// by netuid
    pub fn do_set_mechanism_count(netuid: NetUid, mechanism_count: MechId) -> DispatchResult {
        // Make sure the subnet exists
        ensure!(
            Self::if_subnet_exist(netuid),
            Error::<T>::MechanismDoesNotExist
        );

        // Count cannot be zero
        ensure!(mechanism_count > 0.into(), Error::<T>::InvalidValue);

        // Make sure we are not exceeding the max sub-subnet count
        ensure!(
            mechanism_count <= MaxMechanismCount::<T>::get(),
            Error::<T>::InvalidValue
        );

        // Prevent chain bloat: Require max UIDs to be limited
        let max_uids = MaxAllowedUids::<T>::get(netuid);
        Self::ensure_max_uids_over_all_mechanisms(max_uids, mechanism_count)?;

        // Make sure we are not allowing numbers that will break the math
        ensure!(
            mechanism_count <= MechId::from(MAX_MECHANISM_COUNT_PER_SUBNET),
            Error::<T>::InvalidValue
        );

        Self::update_mechanism_counts_if_needed(netuid, mechanism_count);

        Ok(())
    }

    /// Set the global maximum number of mechanisms per subnet
    pub fn do_set_max_mechanism_count(max_mechanism_count: MechId) -> DispatchResult {
        // Max count cannot be zero
        ensure!(max_mechanism_count > 0.into(), Error::<T>::InvalidValue);

        // Make sure we are not allowing numbers that will break the math
        ensure!(
            max_mechanism_count <= MechId::from(MAX_MECHANISM_COUNT_PER_SUBNET),
            Error::<T>::InvalidValue
        );

        MaxMechanismCount::<T>::set(max_mechanism_count);

        Ok(())
    }

    /// Update current count for a subnet identified by netuid
    /// * Cleans up all sub-subnet maps if count is reduced
    ///
    pub fn update_mechanism_counts_if_needed(netuid: NetUid, new_count: MechId) {
        let old_count = u8::from(MechanismCountCurrent::<T>::get(netuid));
        let new_count_u8 = u8::from(new_count);
        if old_count != new_count_u8 {
            if old_count > new_count_u8 {
                for mecid in new_count_u8..old_count {
                    let netuid_index =
                        Self::get_mechanism_storage_index(netuid, MechId::from(mecid));

                    // Cleanup Weights
                    let _ = Weights::<T>::clear_prefix(netuid_index, u32::MAX, None);

                    // Cleanup Incentive
                    Incentive::<T>::remove(netuid_index);

                    // Cleanup LastUpdate
                    LastUpdate::<T>::remove(netuid_index);

                    // Cleanup Bonds
                    let _ = Bonds::<T>::clear_prefix(netuid_index, u32::MAX, None);

                    // Cleanup WeightCommits
                    let _ = WeightCommits::<T>::clear_prefix(netuid_index, u32::MAX, None);

                    // Cleanup TimelockedWeightCommits
                    let _ =
                        TimelockedWeightCommits::<T>::clear_prefix(netuid_index, u32::MAX, None);
                }
            }

            MechanismCountCurrent::<T>::insert(netuid, MechId::from(new_count));

            // Reset split back to even
            MechanismEmissionSplit::<T>::remove(netuid);
        }
    }

    /// Set or clear the per-mechanism emission split; values must sum to `u16::MAX` when `Some`.
    pub fn do_set_emission_split(netuid: NetUid, maybe_split: Option<Vec<u16>>) -> DispatchResult {
        // Make sure the subnet exists
        ensure!(
            Self::if_subnet_exist(netuid),
            Error::<T>::MechanismDoesNotExist
        );

        if let Some(split) = maybe_split {
            // Check the length
            ensure!(!split.is_empty(), Error::<T>::InvalidValue);
            ensure!(
                split.len() == u8::from(MechanismCountCurrent::<T>::get(netuid)) as usize,
                Error::<T>::InvalidValue
            );

            // Check that values add up to 65535
            let total: u64 = split.iter().map(|s| *s as u64).sum();
            ensure!(total == u16::MAX as u64, Error::<T>::InvalidValue);

            MechanismEmissionSplit::<T>::insert(netuid, split);
        } else {
            MechanismEmissionSplit::<T>::remove(netuid);
        }

        Ok(())
    }

    /// Split alpha emission in sub-subnet proportions
    /// stored in MechanismEmissionSplit
    ///
    pub fn split_emissions(netuid: NetUid, alpha: AlphaBalance) -> Vec<AlphaBalance> {
        let mechanism_count = u64::from(MechanismCountCurrent::<T>::get(netuid));
        let maybe_split = MechanismEmissionSplit::<T>::get(netuid)
            .filter(|split| split.len() == mechanism_count as usize);

        // Unset or legacy malformed split means even distribution
        let mut result: Vec<AlphaBalance> = if let Some(split) = maybe_split {
            split
                .iter()
                .map(|s| {
                    AlphaBalance::from(
                        (u64::from(alpha) as u128)
                            .saturating_mul(*s as u128)
                            .safe_div(u16::MAX as u128) as u64,
                    )
                })
                .collect()
        } else {
            let per_mechanism = u64::from(alpha).safe_div(mechanism_count);
            vec![AlphaBalance::from(per_mechanism); mechanism_count as usize]
        };

        // If there's any rounding error or lost due to truncation emission, credit it to mechanism 0
        let rounding_err =
            u64::from(alpha).saturating_sub(result.iter().map(|s| u64::from(*s)).sum());
        if let Some(cell) = result.first_mut() {
            *cell = cell.saturating_add(AlphaBalance::from(rounding_err));
        }
        result
    }

    /// `existing + added * weight` for consolidating u16 epoch terms across mechanisms.
    fn weighted_accumulate_u16(existing: u16, added: u16, weight: U64F64) -> u16 {
        U64F64::saturating_from_num(existing)
            .saturating_add(U64F64::saturating_from_num(added).saturating_mul(weight))
            .saturating_to_num::<u16>()
    }

    /// `existing + added * weight` for consolidating alpha epoch terms across mechanisms.
    fn weighted_accumulate_alpha(
        existing: AlphaBalance,
        added: AlphaBalance,
        weight: U64F64,
    ) -> AlphaBalance {
        U64F64::saturating_from_num(existing)
            .saturating_add(U64F64::saturating_from_num(added).saturating_mul(weight))
            .saturating_to_num::<u64>()
            .into()
    }

    /// Splits rao_emission between different sub-subnets using `split_emissions` function.
    ///
    /// Runs the epoch function for each sub-subnet and consolidates hotkey_emission
    /// into a single vector.
    ///
    pub fn epoch_with_mechanisms(
        netuid: NetUid,
        rao_emission: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance, AlphaBalance)> {
        let aggregated: BTreeMap<T::AccountId, EpochTerms> =
            Self::split_emissions(netuid, rao_emission)
                .into_iter()
                .enumerate()
                // Run epoch function for each mechanism to distribute its portion of emissions
                .flat_map(|(sub_id_usize, sub_emission)| {
                    let sub_id_u8: u8 = sub_id_usize.try_into().unwrap_or_default();
                    let sub_id = MechId::from(sub_id_u8);

                    // Run epoch function on the mechanism emission
                    let epoch_output = Self::epoch_mechanism(netuid, sub_id, sub_emission);
                    Self::persist_mechanism_epoch_terms(netuid, sub_id, epoch_output.as_map());

                    // Calculate mechanism weight from the split emission (not the other way because preserving
                    // emission accuracy is the priority)
                    // For zero emission the first mechanism gets full weight
                    let sub_weight = U64F64::saturating_from_num(sub_emission).safe_div_or(
                        U64F64::saturating_from_num(rao_emission),
                        U64F64::saturating_from_num(if sub_id_u8 == 0 { 1 } else { 0 }),
                    );

                    // Produce an iterator of (hotkey, (terms, sub_weight)) tuples
                    epoch_output
                        .0
                        .into_iter()
                        .map(move |(hotkey, terms)| (hotkey, (terms, sub_weight)))
                })
                // Consolidate the hotkey emissions into a single BTreeMap
                .fold(BTreeMap::new(), |mut acc, (hotkey, (terms, sub_weight))| {
                    acc.entry(hotkey)
                        .and_modify(|acc_terms| {
                            // Server and validator emission come from mechanism emission and need to be added up
                            acc_terms.validator_emission = acc_terms
                                .validator_emission
                                .saturating_add(terms.validator_emission);
                            acc_terms.server_emission = acc_terms
                                .server_emission
                                .saturating_add(terms.server_emission);

                            // The rest of the terms need to be aggregated as weighted sum
                            acc_terms.dividend = Self::weighted_accumulate_u16(
                                acc_terms.dividend,
                                terms.dividend,
                                sub_weight,
                            );
                            acc_terms.stake_weight = Self::weighted_accumulate_u16(
                                acc_terms.stake_weight,
                                terms.stake_weight,
                                sub_weight,
                            );
                            acc_terms.active |= terms.active;
                            acc_terms.emission = Self::weighted_accumulate_alpha(
                                acc_terms.emission,
                                terms.emission,
                                sub_weight,
                            );
                            acc_terms.consensus = Self::weighted_accumulate_u16(
                                acc_terms.consensus,
                                terms.consensus,
                                sub_weight,
                            );
                            acc_terms.validator_trust = Self::weighted_accumulate_u16(
                                acc_terms.validator_trust,
                                terms.validator_trust,
                                sub_weight,
                            );
                            acc_terms.new_validator_permit |= terms.new_validator_permit;
                            acc_terms.stake = acc_terms.stake.saturating_add(terms.stake);
                        })
                        .or_insert_with(|| {
                            // weighted insert for the first sub-subnet seen for this hotkey
                            EpochTerms {
                                uid: terms.uid,
                                dividend: Self::weighted_accumulate_u16(
                                    0,
                                    terms.dividend,
                                    sub_weight,
                                ),
                                incentive: Self::weighted_accumulate_u16(
                                    0,
                                    terms.incentive,
                                    sub_weight,
                                ),
                                validator_emission: terms.validator_emission,
                                server_emission: terms.server_emission,
                                stake_weight: Self::weighted_accumulate_u16(
                                    0,
                                    terms.stake_weight,
                                    sub_weight,
                                ),
                                active: terms.active, // booleans are ORed across subs
                                emission: Self::weighted_accumulate_alpha(
                                    0u64.into(),
                                    terms.emission,
                                    sub_weight,
                                ),
                                consensus: Self::weighted_accumulate_u16(
                                    0,
                                    terms.consensus,
                                    sub_weight,
                                ),
                                validator_trust: Self::weighted_accumulate_u16(
                                    0,
                                    terms.validator_trust,
                                    sub_weight,
                                ),
                                new_validator_permit: terms.new_validator_permit,
                                bond: Vec::new(), // aggregated map doesn't use bonds; keep empty
                                stake: terms.stake,
                            }
                        });
                    acc
                });

        // State updates from epoch function
        Self::persist_netuid_epoch_terms(netuid, &aggregated);

        // Update voting power EMA for all validators on this subnet
        Self::update_voting_power_for_subnet(netuid, &aggregated);

        // Remap BTreeMap back to Vec<(T::AccountId, AlphaBalance, AlphaBalance)> format
        // for processing emissions in run_coinbase
        // Emission tuples ( hotkeys, server_emission, validator_emission )
        aggregated
            .into_iter()
            .map(|(hotkey, terms)| (hotkey, terms.server_emission, terms.validator_emission))
            .collect()
    }
}
