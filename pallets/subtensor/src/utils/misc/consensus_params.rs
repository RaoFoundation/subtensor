//! Yuma-consensus vector getters/setters and per-UID consensus fields.
use super::*;
use sp_core::U256;
use sp_runtime::PerU16;
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex};

impl<T: Config> Pallet<T> {
    // ==============================
    // ==== YumaConsensus params ====
    // ==============================
    /// Deprecated: Rank is no longer computed during epoch. Always returns empty.
    pub fn get_rank(_netuid: NetUid) -> Vec<u16> {
        Vec::new()
    }
    /// Deprecated: Trust is no longer computed during epoch. Always returns empty.
    pub fn get_trust(_netuid: NetUid) -> Vec<u16> {
        Vec::new()
    }
    pub fn get_active(netuid: NetUid) -> Vec<bool> {
        Active::<T>::get(netuid)
    }
    pub fn get_emission(netuid: NetUid) -> Vec<AlphaBalance> {
        Emission::<T>::get(netuid)
    }
    pub fn get_consensus(netuid: NetUid) -> Vec<u16> {
        Consensus::<T>::get(netuid)
            .into_iter()
            .map(PerU16::deconstruct)
            .collect()
    }
    pub fn get_incentive(netuid: NetUidStorageIndex) -> Vec<u16> {
        Incentive::<T>::get(netuid)
            .into_iter()
            .map(PerU16::deconstruct)
            .collect()
    }
    pub fn get_dividends(netuid: NetUid) -> Vec<u16> {
        Dividends::<T>::get(netuid)
            .into_iter()
            .map(PerU16::deconstruct)
            .collect()
    }
    /// Fetch LastUpdate for `netuid` and ensure its length is at least `get_subnetwork_n(netuid)`,
    /// padding with zeros if needed. Returns the (possibly padded) vector.
    pub fn get_last_update(netuid_index: NetUidStorageIndex) -> Vec<u64> {
        let netuid = Self::netuid_from_mechanism_storage_index(netuid_index);
        let target_len = Self::get_subnetwork_n(netuid) as usize;
        let mut v = LastUpdate::<T>::get(netuid_index);
        if v.len() < target_len {
            v.resize(target_len, 0);
        }
        v
    }
    /// Deprecated: PruningScores is no longer computed during epoch. Always returns empty.
    pub fn get_pruning_score(_netuid: NetUid) -> Vec<u16> {
        Vec::new()
    }
    pub fn get_validator_trust(netuid: NetUid) -> Vec<u16> {
        ValidatorTrust::<T>::get(netuid)
            .into_iter()
            .map(PerU16::deconstruct)
            .collect()
    }
    pub fn get_validator_permit(netuid: NetUid) -> Vec<bool> {
        ValidatorPermit::<T>::get(netuid)
    }

    // ==================================
    // ==== YumaConsensus UID params ====
    // ==================================
    pub fn set_last_update_for_uid(netuid: NetUidStorageIndex, uid: u16, last_update: u64) {
        let mut updated_last_update_vec = Self::get_last_update(netuid);
        let Some(updated_last_update) = updated_last_update_vec.get_mut(uid as usize) else {
            return;
        };
        *updated_last_update = last_update;
        LastUpdate::<T>::insert(netuid, updated_last_update_vec);
    }
    pub fn set_active_for_uid(netuid: NetUid, uid: u16, active: bool) {
        let mut updated_active_vec = Self::get_active(netuid);
        let Some(updated_active) = updated_active_vec.get_mut(uid as usize) else {
            return;
        };
        *updated_active = active;
        Active::<T>::insert(netuid, updated_active_vec);
    }
    pub fn set_validator_permit_for_uid(netuid: NetUid, uid: u16, validator_permit: bool) {
        let mut updated_validator_permits = Self::get_validator_permit(netuid);
        let Some(updated_validator_permit) = updated_validator_permits.get_mut(uid as usize) else {
            return;
        };
        *updated_validator_permit = validator_permit;
        ValidatorPermit::<T>::insert(netuid, updated_validator_permits);
    }
    pub fn set_stake_threshold(min_stake: u64) {
        StakeThreshold::<T>::put(min_stake);
        Self::deposit_event(Event::StakeThresholdSet(min_stake));
    }

    /// Deprecated: Rank is no longer computed. Always returns 0.
    pub fn get_rank_for_uid(_netuid: NetUid, _uid: u16) -> u16 {
        0
    }
    /// Deprecated: Trust is no longer computed. Always returns 0.
    pub fn get_trust_for_uid(_netuid: NetUid, _uid: u16) -> u16 {
        0
    }
    pub fn get_emission_for_uid(netuid: NetUid, uid: u16) -> AlphaBalance {
        let vec = Emission::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or_default()
    }
    pub fn get_active_for_uid(netuid: NetUid, uid: u16) -> bool {
        let vec = Active::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(false)
    }
    pub fn get_consensus_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = Consensus::<T>::get(netuid);
        vec.get(uid as usize)
            .copied()
            .unwrap_or_default()
            .deconstruct()
    }
    pub fn get_incentive_for_uid(netuid: NetUidStorageIndex, uid: u16) -> u16 {
        let vec = Incentive::<T>::get(netuid);
        vec.get(uid as usize)
            .copied()
            .unwrap_or_default()
            .deconstruct()
    }
    pub fn get_dividends_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = Dividends::<T>::get(netuid);
        vec.get(uid as usize)
            .copied()
            .unwrap_or_default()
            .deconstruct()
    }
    pub fn get_last_update_for_uid(netuid: NetUidStorageIndex, uid: u16) -> u64 {
        let vec = LastUpdate::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    /// Deprecated: PruningScores is no longer computed. Always returns u16::MAX.
    pub fn get_pruning_score_for_uid(_netuid: NetUid, _uid: u16) -> u16 {
        u16::MAX
    }
    pub fn get_validator_trust_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = ValidatorTrust::<T>::get(netuid);
        vec.get(uid as usize)
            .copied()
            .unwrap_or_default()
            .deconstruct()
    }
    pub fn get_validator_permit_for_uid(netuid: NetUid, uid: u16) -> bool {
        let vec = ValidatorPermit::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(false)
    }
    pub fn get_stake_threshold() -> u64 {
        StakeThreshold::<T>::get()
    }

    // ============================
    // ==== Subnetwork Getters ====
    // ============================
    pub fn get_tempo(netuid: NetUid) -> u16 {
        Tempo::<T>::get(netuid)
    }
    pub fn get_last_adjustment_block(netuid: NetUid) -> u64 {
        LastAdjustmentBlock::<T>::get(netuid)
    }
    pub fn get_blocks_since_last_step(netuid: NetUid) -> u64 {
        BlocksSinceLastStep::<T>::get(netuid)
    }
    pub fn get_difficulty(netuid: NetUid) -> U256 {
        U256::from(Self::get_difficulty_as_u64(netuid))
    }
    pub fn get_registrations_this_block(netuid: NetUid) -> u16 {
        RegistrationsThisBlock::<T>::get(netuid)
    }
    pub fn get_last_mechanism_step_block(netuid: NetUid) -> u64 {
        LastMechansimStepBlock::<T>::get(netuid)
    }
    pub fn get_registrations_this_interval(netuid: NetUid) -> u16 {
        RegistrationsThisInterval::<T>::get(netuid)
    }
    pub fn get_pow_registrations_this_interval(netuid: NetUid) -> u16 {
        POWRegistrationsThisInterval::<T>::get(netuid)
    }
    pub fn get_burn_registrations_this_interval(netuid: NetUid) -> u16 {
        BurnRegistrationsThisInterval::<T>::get(netuid)
    }
    pub fn get_neuron_block_at_registration(netuid: NetUid, neuron_uid: u16) -> u64 {
        BlockAtRegistration::<T>::get(netuid, neuron_uid)
    }
    /// Returns the minimum number of non-immortal & non-immune UIDs that must remain in a subnet.
    pub fn get_min_non_immune_uids(netuid: NetUid) -> u16 {
        MinNonImmuneUids::<T>::get(netuid)
    }

    /// Sets the minimum number of non-immortal & non-immune UIDs that must remain in a subnet.
    pub fn set_min_non_immune_uids(netuid: NetUid, min: u16) {
        MinNonImmuneUids::<T>::insert(netuid, min);
        Self::deposit_event(Event::MinNonImmuneUidsSet(netuid, min));
    }
}
