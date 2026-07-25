//! Legacy `epoch` / `epoch_dense` test entrypoints and persistence of epoch vectors into storage.

use super::*;
use alloc::collections::BTreeMap;
use sp_runtime::PerU16;
use sp_std::vec::Vec;
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid};

impl<T: Config> Pallet<T> {
    /// Test helper: run [`Self::epoch_mechanism`] for `MechId::MAIN` and persist terms.
    pub fn epoch(
        netuid: NetUid,
        rao_emission: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance, AlphaBalance)> {
        // Run mechanism-style epoch
        let output = Self::epoch_mechanism(netuid, MechId::MAIN, rao_emission);

        // Persist values in legacy format
        Self::persist_mechanism_epoch_terms(netuid, MechId::MAIN, output.as_map());
        Self::persist_netuid_epoch_terms(netuid, output.as_map());

        // Remap and return
        output
            .into_iter()
            .map(|(hotkey, terms)| (hotkey, terms.server_emission, terms.validator_emission))
            .collect()
    }

    /// Test helper: dense-matrix epoch for `MechId::MAIN`.
    pub fn epoch_dense(
        netuid: NetUid,
        rao_emission: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance, AlphaBalance)> {
        Self::epoch_dense_mechanism_for_tests(netuid, MechId::MAIN, rao_emission)
    }

    /// Write mechanism-scoped `Incentive` / `Bonds` and emit `IncentiveAlphaEmittedToMiners`.
    pub fn persist_mechanism_epoch_terms(
        netuid: NetUid,
        mecid: MechId,
        output: &BTreeMap<T::AccountId, EpochTerms>,
    ) {
        let netuid_index = Self::get_mechanism_storage_index(netuid, mecid);
        let mut terms_sorted: sp_std::vec::Vec<&EpochTerms> = output.values().collect();
        terms_sorted.sort_unstable_by_key(|t| t.uid);

        let incentive = collect_sorted_epoch_field!(terms_sorted, incentive);
        let bonds: Vec<Vec<(u16, u16)>> = terms_sorted
            .iter()
            .cloned()
            .map(|t| t.bond.clone())
            .collect::<sp_std::vec::Vec<_>>();

        // Epoch math stays in raw u16; wrap into PerU16 only at the storage boundary.
        let incentive: Vec<PerU16> = incentive.into_iter().map(PerU16::from_parts).collect();
        Incentive::<T>::insert(netuid_index, incentive);

        let server_emission = collect_sorted_epoch_field!(terms_sorted, server_emission);
        Self::deposit_event(Event::IncentiveAlphaEmittedToMiners {
            netuid: netuid_index,
            emissions: server_emission,
        });

        bonds
            .into_iter()
            .enumerate()
            .for_each(|(uid_usize, bond_vec)| {
                let uid: u16 = uid_usize.try_into().unwrap_or_default();
                Bonds::<T>::insert(netuid_index, uid, bond_vec);
            });
    }

    /// Write netuid-scoped active/emission/consensus/dividend/validator vectors from epoch terms.
    pub fn persist_netuid_epoch_terms(netuid: NetUid, output: &BTreeMap<T::AccountId, EpochTerms>) {
        let mut terms_sorted: sp_std::vec::Vec<&EpochTerms> = output.values().collect();
        terms_sorted.sort_unstable_by_key(|t| t.uid);

        let active = collect_sorted_epoch_field!(terms_sorted, active);
        let emission = collect_sorted_epoch_field!(terms_sorted, emission);
        let consensus = collect_sorted_epoch_field!(terms_sorted, consensus);
        let dividend = collect_sorted_epoch_field!(terms_sorted, dividend);
        let validator_trust = collect_sorted_epoch_field!(terms_sorted, validator_trust);
        let new_validator_permit = collect_sorted_epoch_field!(terms_sorted, new_validator_permit);
        let stake_weight = collect_sorted_epoch_field!(terms_sorted, stake_weight);

        // Epoch math stays in raw u16; wrap into PerU16 only at the storage boundary.
        let consensus: Vec<PerU16> = consensus.into_iter().map(PerU16::from_parts).collect();
        let dividend: Vec<PerU16> = dividend.into_iter().map(PerU16::from_parts).collect();
        let validator_trust: Vec<PerU16> = validator_trust
            .into_iter()
            .map(PerU16::from_parts)
            .collect();

        Active::<T>::insert(netuid, active.clone());
        Emission::<T>::insert(netuid, emission);
        Consensus::<T>::insert(netuid, consensus);
        Dividends::<T>::insert(netuid, dividend);
        ValidatorTrust::<T>::insert(netuid, validator_trust);
        ValidatorPermit::<T>::insert(netuid, new_validator_permit);
        StakeWeight::<T>::insert(netuid, stake_weight);
    }
}
