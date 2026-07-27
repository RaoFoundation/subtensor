//! Epoch-slot drain of pending subnet emissions and tempo scheduling helpers.

use super::*;
use alloc::collections::{BTreeMap, BTreeSet};
use subtensor_runtime_common::{AlphaBalance, NetUid};

impl<T: Config> Pallet<T> {
    /// Subnets whose epoch slot is due *this* block but is deferred by the per-block
    /// cap (`MaxEpochsPerBlock`).
    pub fn epochs_deferred_this_block(subnets: &[NetUid], current_block: u64) -> BTreeSet<NetUid> {
        let cap = Self::get_max_epochs_per_block() as u32;
        let mut deferred: BTreeSet<NetUid> = BTreeSet::new();
        let mut epochs_run_this_block: u32 = 0;

        for &netuid in subnets.iter() {
            if !Self::should_run_epoch(netuid, current_block) {
                continue;
            }
            // Per-block cap — due subnets beyond the limit are deferred.
            if epochs_run_this_block >= cap {
                deferred.insert(netuid);
                continue;
            }
            if Self::epoch_keys_have_unique_hotkeys(netuid) {
                epochs_run_this_block = epochs_run_this_block.saturating_add(1);
            }
        }
        deferred
    }

    /// On each subnet whose epoch is due this block (respecting `MaxEpochsPerBlock`), drain
    /// pending server/validator/root/owner alpha into the return map and advance the epoch
    /// schedule (`LastEpochBlock`, `SubnetEpochIndex`, clear `PendingEpochAt`).
    ///
    /// Deferred or inconsistent-input subnets keep accumulating pending emissions.
    pub fn drain_pending_subnet_emissions(
        subnets: &[NetUid],
        current_block: u64,
    ) -> BTreeMap<NetUid, (AlphaBalance, AlphaBalance, AlphaBalance, AlphaBalance)> {
        // Map of netuid to (pending_server_alpha, pending_validator_alpha, pending_root_alpha, pending_owner_cut).
        let mut emissions_to_distribute: BTreeMap<
            NetUid,
            (AlphaBalance, AlphaBalance, AlphaBalance, AlphaBalance),
        > = BTreeMap::new();
        // Per-block cap on number of epochs that may run; the rest are deferred 1 block forward
        // by setting `PendingEpochAt`.
        let max_epochs_per_block = Self::get_max_epochs_per_block() as u32;
        let mut epochs_run_this_block: u32 = 0;

        for &netuid in subnets.iter() {
            // Keep the scheduler age bounded per subnet. `tempo + 1` is enough to
            // record that a due epoch missed its slot while avoiding an unbounded
            // public counter when the epoch is repeatedly deferred or its input
            // state remains inconsistent.
            let tempo = Self::get_tempo(netuid);
            let max_blocks_since_last_step = u64::from(tempo).saturating_add(1);
            BlocksSinceLastStep::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(1).min(max_blocks_since_last_step)
            });

            if !Self::should_run_epoch_given_tempo(netuid, current_block, tempo) {
                continue;
            }

            // Per-block cap — defer if already at limit.
            if epochs_run_this_block >= max_epochs_per_block {
                let next_block = current_block.saturating_add(1);
                PendingEpochAt::<T>::insert(netuid, next_block);
                Self::deposit_event(Event::EpochDeferred {
                    netuid,
                    from_block: current_block,
                    to_block: next_block,
                });
                continue;
            }

            if Self::epoch_keys_have_unique_hotkeys(netuid) {
                // Reset blocks-since counter; LastMechansimStepBlock is written
                // post-distribute (see the caller), so bonds masking can read the
                // previous successful run.
                BlocksSinceLastStep::<T>::insert(netuid, 0);

                // Get and drain the subnet pending emission.
                let pending_server_alpha = PendingServerEmission::<T>::get(netuid);
                PendingServerEmission::<T>::insert(netuid, AlphaBalance::ZERO);

                let pending_validator_alpha = PendingValidatorEmission::<T>::get(netuid);
                PendingValidatorEmission::<T>::insert(netuid, AlphaBalance::ZERO);

                // Get and drain the pending Alpha for root divs.
                let pending_root_alpha = PendingRootAlphaDivs::<T>::get(netuid);
                PendingRootAlphaDivs::<T>::insert(netuid, AlphaBalance::ZERO);

                // Get and drain the pending owner cut.
                let owner_cut = PendingOwnerCut::<T>::get(netuid);
                PendingOwnerCut::<T>::insert(netuid, AlphaBalance::ZERO);

                // Save the emissions to distribute.
                emissions_to_distribute.insert(
                    netuid,
                    (
                        pending_server_alpha,
                        pending_validator_alpha,
                        pending_root_alpha,
                        owner_cut,
                    ),
                );
                epochs_run_this_block = epochs_run_this_block.saturating_add(1);

                // Change subnet owner based on conviction.
                Self::change_subnet_owner_if_needed(netuid);
            } else {
                // Schedule advances below; execution skipped. Pending emissions accumulate
                // and will be drained by the next successful epoch.
                Self::deposit_event(Event::EpochSkipped {
                    netuid,
                    block: current_block,
                });
            }

            // Advance the schedule unconditionally — the slot is consumed.
            LastEpochBlock::<T>::insert(netuid, current_block);
            PendingEpochAt::<T>::insert(netuid, 0);
            SubnetEpochIndex::<T>::mutate(netuid, |idx| *idx = idx.saturating_add(1));
        }
        emissions_to_distribute
    }

    /// For each drained subnet, run [`Pallet::distribute_emission`] and stamp
    /// [`LastMechansimStepBlock`] (note the historical misspelling of that storage item).
    pub fn distribute_emissions_to_subnets(
        emissions_to_distribute: &BTreeMap<
            NetUid,
            (AlphaBalance, AlphaBalance, AlphaBalance, AlphaBalance),
        >,
    ) {
        let current_block = Self::get_current_block_as_u64();
        for (
            &netuid,
            &(pending_server_alpha, pending_validator_alpha, pending_root_alpha, pending_owner_cut),
        ) in emissions_to_distribute.iter()
        {
            // Distribute the emission to the subnet.
            Self::distribute_emission(
                netuid,
                pending_server_alpha,
                pending_validator_alpha,
                pending_root_alpha,
                pending_owner_cut,
            );
            LastMechansimStepBlock::<T>::insert(netuid, current_block);
        }
    }

    /// Checks if the epoch should run for a given subnet based on the current block.
    ///
    /// # Arguments
    /// * `netuid`: The unique identifier of the subnet.
    ///
    /// # Returns
    /// * `bool`: True if the epoch should run, false otherwise.
    pub fn should_run_epoch(netuid: NetUid, current_block: u64) -> bool {
        let tempo = Self::get_tempo(netuid);
        Self::should_run_epoch_given_tempo(netuid, current_block, tempo)
    }

    /// Same predicate as [`Pallet::should_run_epoch`], using an already-loaded tempo so
    /// callers that also need the tempo do not charge a duplicate storage read.
    fn should_run_epoch_given_tempo(netuid: NetUid, current_block: u64, tempo: u16) -> bool {
        if tempo == 0 {
            return false;
        }
        let pending = PendingEpochAt::<T>::get(netuid);
        if pending > 0 && current_block >= pending {
            return true;
        }
        if BlocksSinceLastStep::<T>::get(netuid) > u64::from(tempo) {
            return true;
        }
        let last = LastEpochBlock::<T>::get(netuid);
        let blocks_since = current_block.saturating_sub(last);
        blocks_since >= tempo as u64
    }

    /// Returns the number of blocks remaining before the next automatic epoch under the
    /// stateful scheduler (period `tempo`, anchored on `LastEpochBlock`). Does NOT account for:
    ///     - `PendingEpochAt` (owner-triggered manual fire — could happen sooner),
    ///     - `BlocksSinceLastStep > tempo` safety-net,
    ///     - per-block-cap defer (could push the actual fire one or more blocks later)
    /// Used by the admin-freeze-window predicate and external tooling. Returns `u64::MAX` when
    /// `tempo == 0` (legacy defensive short-circuit).
    pub fn blocks_until_next_auto_epoch(netuid: NetUid, tempo: u16, block_number: u64) -> u64 {
        if tempo == 0 {
            return u64::MAX;
        }
        let last = LastEpochBlock::<T>::get(netuid);
        // Period is `tempo`: next firing at `last + tempo`.
        let next_auto = last.saturating_add(tempo as u64);
        next_auto.saturating_sub(block_number)
    }

    /// Returns the absolute block number at which the next epoch is expected to fire for the
    /// given subnet, considering both the automatic schedule (`LastEpochBlock + tempo`) and
    /// any owner-triggered `PendingEpochAt`. Returns `None` if `tempo == 0` (subnet does not run).
    /// Does NOT account for the per-block cap deferral or the `BlocksSinceLastStep > tempo`
    /// safety-net (which can fire earlier under extreme drift).
    pub fn get_next_epoch_start_block(netuid: NetUid) -> Option<u64> {
        let tempo = Self::get_tempo(netuid);
        if tempo == 0 {
            return None;
        }
        let last = LastEpochBlock::<T>::get(netuid);
        let auto_next = last.saturating_add(tempo as u64);

        let pending = PendingEpochAt::<T>::get(netuid);
        if pending > 0 {
            Some(auto_next.min(pending))
        } else {
            Some(auto_next)
        }
    }
}
