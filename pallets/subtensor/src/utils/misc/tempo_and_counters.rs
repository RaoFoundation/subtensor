//! Tempo writes, registration/adjustment counters, and current-block helper.
use super::*;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    // ========================
    // ==== Global Setters ====
    // ========================
    /// Unchecked tempo write used by tests, precompiles, and internal helpers.
    /// Does NOT reset `LastEpochBlock` — that is the responsibility of
    /// `AdminUtils::sudo_set_tempo` (owner-or-root), which performs the cycle
    /// reset explicitly via `apply_tempo_with_cycle_reset`.
    pub fn set_tempo_unchecked(netuid: NetUid, tempo: u16) {
        Tempo::<T>::insert(netuid, tempo);
        Self::deposit_event(Event::TempoSet(netuid, tempo));
    }

    /// Sets `Tempo` and resets the state-based scheduler anchor `LastEpochBlock`
    /// to the current block
    pub fn apply_tempo_with_cycle_reset(netuid: NetUid, tempo: u16) {
        Self::set_tempo_unchecked(netuid, tempo);
        let now = Self::get_current_block_as_u64();
        LastEpochBlock::<T>::insert(netuid, now);
    }

    pub fn set_last_adjustment_block(netuid: NetUid, last_adjustment_block: u64) {
        LastAdjustmentBlock::<T>::insert(netuid, last_adjustment_block);
    }
    pub fn set_blocks_since_last_step(netuid: NetUid, blocks_since_last_step: u64) {
        BlocksSinceLastStep::<T>::insert(netuid, blocks_since_last_step);
    }
    pub fn set_registrations_this_block(netuid: NetUid, registrations_this_block: u16) {
        RegistrationsThisBlock::<T>::insert(netuid, registrations_this_block);
    }
    pub fn set_last_mechanism_step_block(netuid: NetUid, last_mechanism_step_block: u64) {
        LastMechansimStepBlock::<T>::insert(netuid, last_mechanism_step_block);
    }
    pub fn set_registrations_this_interval(netuid: NetUid, registrations_this_interval: u16) {
        RegistrationsThisInterval::<T>::insert(netuid, registrations_this_interval);
    }
    pub fn set_pow_registrations_this_interval(
        netuid: NetUid,
        pow_registrations_this_interval: u16,
    ) {
        POWRegistrationsThisInterval::<T>::insert(netuid, pow_registrations_this_interval);
    }
    pub fn set_burn_registrations_this_interval(
        netuid: NetUid,
        burn_registrations_this_interval: u16,
    ) {
        BurnRegistrationsThisInterval::<T>::insert(netuid, burn_registrations_this_interval);
    }

    // ========================
    // ==== Global Getters ====
    // ========================
    pub fn get_current_block_as_u64() -> u64 {
        TryInto::try_into(<frame_system::Pallet<T>>::block_number())
            .ok()
            .expect("blockchain will not exceed 2^64 blocks; QED.")
    }
}
