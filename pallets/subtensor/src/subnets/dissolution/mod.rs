//! Subnet dissolution and deferred network-registration queue.
//!
//! Search anchors:
//! - `do_dissolve_network` — queue a subnet for multi-block storage cleanup
//! - `remove_data_for_dissolved_networks` — `on_idle` orchestrator over the dissolve queue
//! - `clean_up_data_for_one_dissolved_network` — phase machine for one netuid
//! - `process_network_registration_queue` — register one queued network when a slot frees
//! - [`DissolveCleanupPhase`] / [`DissolveCleanupStatus`] — persisted resume state
//! - `remove_network_*` — weight-metered storage purge helpers

mod cleanup_status;
mod phased_cleanup;
mod purge_network_storage;

pub use cleanup_status::{DissolveCleanupPhase, DissolveCleanupStatus};

use super::*;
use subtensor_runtime_common::NetUid;
use subtensor_swap_interface::SwapHandler;

impl<T: Config> Pallet<T> {
    /// Mark `netuid` dissolved: remove from [`NetworksAdded`] and enqueue cleanup.
    ///
    /// Storage wipe continues asynchronously via
    /// [`Self::remove_data_for_dissolved_networks`] / [`Self::clean_up_data_for_one_dissolved_network`].
    /// Emits [`Event::NetworkRemoved`]. Fails if the subnet is missing, is root, or
    /// is already queued.
    pub fn do_dissolve_network(netuid: NetUid) -> dispatch::DispatchResult {
        // --- The network exists?
        ensure!(
            Self::if_subnet_exist(netuid) && netuid != NetUid::ROOT,
            Error::<T>::SubnetNotExists
        );

        // Since TotalStake is updated on this level, purge reservoirs here into reserves and TotalStake
        let reservoir_tao = T::SwapInterface::protocol_tao_reservoir(netuid);
        let reservoir_alpha = T::SwapInterface::protocol_alpha_reservoir(netuid);
        T::SwapInterface::clear_protocol_liquidity_reservoirs(netuid);
        Self::increase_provided_tao_reserve(netuid, reservoir_tao);
        Self::increase_provided_alpha_reserve(netuid, reservoir_alpha);
        if !reservoir_tao.is_zero() {
            TotalStake::<T>::mutate(|total| {
                *total = total.saturating_add(reservoir_tao);
            });
        }

        let mut dissolved_networks = DissolveCleanupQueue::<T>::get();
        ensure!(
            !dissolved_networks.contains(&netuid),
            Error::<T>::NetworkDissolveAlreadyQueued
        );

        // Just remove the network from the added networks, it is used to check if the network is existed.
        NetworksAdded::<T>::remove(netuid);
        // Reduce the total networks count.
        TotalNetworks::<T>::mutate(|n: &mut u16| *n = n.saturating_sub(1));
        TotalStake::<T>::mutate(|total| *total = total.saturating_sub(SubnetTAO::<T>::get(netuid)));

        dissolved_networks.push(netuid);
        DissolveCleanupQueue::<T>::set(dissolved_networks);

        log::debug!("NetworkRemoved( netuid:{netuid:?} )");

        // --- Emit the NetworkRemoved event
        Self::deposit_event(Event::NetworkRemoved(netuid));

        Ok(())
    }

    /// `on_idle` entry: resume or start dissolve cleanup within `remaining_weight`.
    pub fn remove_data_for_dissolved_networks(remaining_weight: Weight) -> Weight {
        let w = T::DbWeight::get().writes(1);
        let r = T::DbWeight::get().reads(1);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(remaining_weight);

        // complete unfinished network cleanup at first if any
        if let Some(mut status) = CurrentDissolveCleanupStatus::<T>::get() {
            let (cleanup_completed, weight) =
                Self::clean_up_data_for_one_dissolved_network(&mut weight_meter, &mut status);
            if cleanup_completed {
                DissolveCleanupQueue::<T>::mutate(|queue| {
                    queue.retain(|queued_netuid| *queued_netuid != status.netuid);
                });
                CurrentDissolveCleanupStatus::<T>::kill();
                return weight.saturating_add(T::DbWeight::get().writes(2));
            }
            return weight;
        }

        if !weight_meter.can_consume(r) {
            return weight_meter.consumed();
        }
        weight_meter.consume(r);

        let dissolved_networks = DissolveCleanupQueue::<T>::get();
        if let Some(netuid) = dissolved_networks.first() {
            if !weight_meter.can_consume(w) {
                return weight_meter.consumed();
            }
            weight_meter.consume(w);

            let mut status = DissolveCleanupStatus::new(*netuid);
            CurrentDissolveCleanupStatus::<T>::set(Some(status.clone()));

            let (cleanup_completed, _weight) =
                Self::clean_up_data_for_one_dissolved_network(&mut weight_meter, &mut status);

            if cleanup_completed {
                DissolveCleanupQueue::<T>::mutate(|queue| {
                    queue.retain(|queued_netuid| *queued_netuid != status.netuid);
                });
                CurrentDissolveCleanupStatus::<T>::kill();
                weight_meter.consume(T::DbWeight::get().writes(2));
            }
        }

        weight_meter.consumed()
    }

    // try use all weight available to clean up data for one dissolved network based on the status

    /// Try to finalize one queued [`NetworkRegistrationInfo`] after a dissolve frees a slot.
    pub fn process_network_registration_queue() -> Weight {
        let db_weight = T::DbWeight::get();
        let queue = NetworkRegistrationQueue::<T>::get();
        let mut weight = db_weight.reads(1);

        for (index, info) in queue.iter().enumerate() {
            // just complete one registration at a time since on_idle just complete one network dissolve cleanup
            // if one registration fails, then try next one. it could be not align with the order of registration in the queue
            match Self::set_new_network_state(
                &info.coldkey,
                &info.hotkey,
                info.mechid,
                info.identity.clone(),
                info.lock_amount,
                info.median_subnet_alpha_price,
                Some(info.lock_id),
            ) {
                Ok(post_info) => {
                    NetworkRegistrationQueue::<T>::mutate(|queue| queue.remove(index));
                    weight.saturating_accrue(db_weight.reads_writes(1, 1));
                    weight.saturating_accrue(post_info.actual_weight.unwrap_or_else(Weight::zero));
                    return weight;
                }
                Err(_) => {
                    log::error!(
                        "Failed to set new network state for coldkey: {:?}, hotkey: {:?}",
                        info.coldkey,
                        info.hotkey
                    );
                    continue;
                }
            }
        }

        weight
    }
}
