// The MIT License (MIT)
// Copyright © 2023 Yuma Rao

// Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
// documentation files (the “Software”), to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software,
// and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all copies or substantial portions of
// the Software.

// THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO
// THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use super::*;
use safe_math::*;
use substrate_fixed::types::{I64F64, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
impl<T: Config> Pallet<T> {
    /// Fetches the total count of root network validators
    ///
    /// This function retrieves the total number of root network validators.
    ///
    /// # Returns
    /// * `u16`: The total number of root network validators
    ///
    pub fn get_num_root_validators() -> u16 {
        Self::get_subnetwork_n(NetUid::ROOT)
    }

    /// Fetches the max validators count of root network.
    ///
    /// This function retrieves the max validators count of root network.
    ///
    /// # Returns
    /// * `u16`: The max validators count of root network.
    ///
    pub fn get_max_root_validators() -> u16 {
        Self::get_max_allowed_uids(NetUid::ROOT)
    }

    /// Checks whether any netuid in the given list does not exist.
    ///
    /// It's important to check for invalid netuids to ensure data integrity and avoid referencing nonexistent subnets.
    ///
    /// # Arguments
    /// * `netuids`: A reference to a slice of netuids to check.
    ///
    /// # Returns
    /// * `bool`: 'true' if any of the netuids are invalid, 'false' otherwise.
    ///
    pub fn contains_invalid_root_uids(netuids: &[NetUid]) -> bool {
        for netuid in netuids {
            if !Self::if_subnet_exist(*netuid) {
                log::debug!("contains_invalid_root_uids: netuid {netuid:?} does not exist");
                return true;
            }
        }
        false
    }

    /// Registers a user's hotkey to the root network.
    ///
    /// Admission is burn-based: the coldkey pays the root burn price
    /// (`Burn(0)`, demand-priced like subnet registration — bumped on every
    /// registration, decaying back toward the floor each block). No prior
    /// stake is required. When the network is full, the lowest-staked
    /// non-immune member is pruned to make room. A just-registered seat is
    /// immune for `ImmunityPeriod` blocks.
    ///
    /// After a successful registration, the hotkey is auto-childkeyed to
    /// every existing subnet owner (unless the validator opted out of
    /// auto parent delegation).
    ///
    /// # Arguments
    /// * `origin`: Represents the origin of the call.
    /// * `hotkey`: The hotkey that the user wants to register to the root network.
    ///
    /// # Returns
    /// * `DispatchResult`: A result type indicating success or failure of the registration.
    ///
    pub fn do_root_register(origin: OriginFor<T>, hotkey: T::AccountId) -> DispatchResult {
        // --- 0. Get the unique identifier (UID) for the root network.
        let current_block_number: u64 = Self::get_current_block_as_u64();
        ensure!(
            Self::if_subnet_exist(NetUid::ROOT),
            Error::<T>::RootNetworkDoesNotExist
        );

        // --- 1. Ensure that the call originates from a signed source and retrieve the caller's account ID (coldkey).
        let coldkey = ensure_signed(origin)?;
        log::debug!("do_root_register( coldkey: {coldkey:?}, hotkey: {hotkey:?} )");

        // --- 2. Ensure that the number of registrations in this block doesn't exceed the allowed limit.
        ensure!(
            Self::get_registrations_this_block(NetUid::ROOT)
                < Self::get_max_registrations_per_block(NetUid::ROOT),
            Error::<T>::TooManyRegistrationsThisBlock
        );

        // --- 3. Ensure that the number of registrations in this interval doesn't exceed thrice the target limit.
        ensure!(
            Self::get_registrations_this_interval(NetUid::ROOT)
                < Self::get_target_registrations_per_interval(NetUid::ROOT).saturating_mul(3),
            Error::<T>::TooManyRegistrationsThisInterval
        );

        // --- 4. Check if the hotkey is already registered. If so, error out.
        ensure!(
            !Uids::<T>::contains_key(NetUid::ROOT, &hotkey),
            Error::<T>::HotKeyAlreadyRegisteredInSubNet
        );

        // --- 6. Create a network account for the user if it doesn't exist.
        Self::create_account_if_non_existent(&coldkey, &hotkey)?;

        // --- 7. Fetch the current size of the subnetwork.
        let current_num_root_validators: u16 = Self::get_num_root_validators();

        // --- 8. Resolve the slot: append while below capacity (max allowed is
        // senate size), otherwise prune the lowest-staked *non-immune* member.
        // A just-registered seat is immune (`ImmunityPeriod`) so it can attract
        // stake before the next registration can evict it. Resolution only
        // reads state, so the burn charged below can never be taken for a
        // registration that fails.
        let maybe_replacement: Option<(u16, T::AccountId)> =
            if current_num_root_validators < Self::get_max_root_validators() {
                None
            } else {
                let mut lowest_stake = AlphaBalance::MAX;
                let mut lowest_uid: Option<u16> = None;
                for (uid_i, hotkey_i) in Keys::<T>::iter_prefix(NetUid::ROOT) {
                    if Self::get_neuron_is_immune(NetUid::ROOT, uid_i) {
                        continue;
                    }
                    let stake_i = Self::get_stake_for_hotkey_on_subnet(&hotkey_i, NetUid::ROOT);
                    if lowest_uid.is_none() || stake_i < lowest_stake {
                        lowest_stake = stake_i;
                        lowest_uid = Some(uid_i);
                    }
                }
                let lowest_uid = lowest_uid.ok_or(Error::<T>::NoNeuronIdAvailable)?;
                let replaced_hotkey: T::AccountId =
                    Self::get_hotkey_for_net_and_uid(NetUid::ROOT, lowest_uid)?;
                Some((lowest_uid, replaced_hotkey))
            };

        // --- 9. Charge the burn: recycled out of issuance, priced by demand.
        // `recycle_tao` refuses to drop the coldkey below the existential
        // deposit, so an underfunded caller fails here before any mutation.
        let burn_cost: TaoBalance = Self::get_burn(NetUid::ROOT);
        if !burn_cost.is_zero() {
            Self::recycle_tao(&coldkey, burn_cost.into())?;
            Self::increase_rao_recycled(NetUid::ROOT, burn_cost);
        }
        Self::bump_registration_price_after_registration(NetUid::ROOT);

        // --- 10. Insert the neuron.
        let subnetwork_uid: u16 = match maybe_replacement {
            None => {
                let uid = current_num_root_validators;
                Self::append_neuron(NetUid::ROOT, &hotkey, current_block_number);
                log::debug!("add new neuron: {hotkey:?} on uid {uid:?}");
                uid
            }
            Some((lowest_uid, replaced_hotkey)) => {
                Self::replace_neuron(NetUid::ROOT, lowest_uid, &hotkey, current_block_number);
                log::debug!(
                    "replace neuron: {replaced_hotkey:?} with {hotkey:?} on uid {lowest_uid:?}"
                );
                lowest_uid
            }
        };

        // --- 13. Force all members on root to become a delegate.
        if !Self::hotkey_is_delegate(&hotkey) {
            Self::delegate_hotkey(&hotkey, 11_796); // 18% cut defaulted.
        }

        // --- 14. Update the registration counters for both the block and interval.
        #[allow(clippy::arithmetic_side_effects)]
        // note this RA + clippy false positive is a known substrate issue
        RegistrationsThisInterval::<T>::mutate(NetUid::ROOT, |val| *val += 1);
        #[allow(clippy::arithmetic_side_effects)]
        // note this RA + clippy false positive is a known substrate issue
        RegistrationsThisBlock::<T>::mutate(NetUid::ROOT, |val| *val += 1);

        // --- 15. Log and announce the successful registration.
        log::debug!(
            "RootRegistered(netuid:{:?} uid:{:?} hotkey:{:?})",
            NetUid::ROOT,
            subnetwork_uid,
            hotkey
        );
        Self::deposit_event(Event::NeuronRegistered(
            NetUid::ROOT,
            subnetwork_uid,
            hotkey.clone(),
        ));

        // --- 16. Auto-childkey this root validator to every subnet owner.
        if let Err(e) = Self::do_set_subnet_owners_for_root_validator(&hotkey) {
            log::warn!("Failed to auto-childkey root validator {hotkey:?} to subnet owners: {e:?}");
        }

        // --- 17. Finish and return success.
        Ok(())
    }

    #[allow(clippy::arithmetic_side_effects)]
    /// This function calculates the lock cost for a network based on the last lock amount, minimum lock cost, last lock block, and current block.
    /// The lock cost is calculated using the formula:
    /// lock_cost = (last_lock * mult) - (last_lock / lock_reduction_interval) * (current_block - last_lock_block)
    /// where:
    /// * last_lock is the last lock amount for the network
    /// * mult is the multiplier which increases lock cost each time a registration occurs
    /// * last_lock_block is the block number at which the last lock occurred
    /// * lock_reduction_interval the number of blocks before the lock returns to previous value.
    /// * current_block is the current block number
    /// * DAYS is the number of blocks in a day
    /// * min_lock is the minimum lock cost for the network
    ///
    /// If the calculated lock cost is less than the minimum lock cost, the minimum lock cost is returned.
    ///
    /// # Returns
    /// * `u64`: The lock cost for the network.
    ///
    pub fn get_network_lock_cost() -> TaoBalance {
        let last_lock = Self::get_network_last_lock();
        let min_lock = Self::get_network_min_lock();
        let last_lock_block = Self::get_network_last_lock_block();
        let current_block = Self::get_current_block_as_u64();
        let lock_reduction_interval = Self::get_lock_reduction_interval();
        let mult: TaoBalance = if last_lock_block == 0 { 1 } else { 2 }.into();

        let mut lock_cost = last_lock.saturating_mul(mult).saturating_sub(
            last_lock
                .to_u64()
                .safe_div(lock_reduction_interval)
                .saturating_mul(current_block.saturating_sub(last_lock_block))
                .into(),
        );

        if lock_cost < min_lock {
            lock_cost = min_lock;
        }

        log::debug!(
            "last_lock: {last_lock:?}, min_lock: {min_lock:?}, last_lock_block: {last_lock_block:?}, lock_reduction_interval: {lock_reduction_interval:?}, current_block: {current_block:?}, mult: {mult:?} lock_cost: {lock_cost:?}"
        );

        lock_cost
    }

    pub fn get_network_registered_block(netuid: NetUid) -> u64 {
        NetworkRegisteredAt::<T>::get(netuid)
    }
    pub fn get_registered_subnet_counter(netuid: NetUid) -> u64 {
        RegisteredSubnetCounter::<T>::get(netuid)
    }
    pub fn get_network_immunity_period() -> u64 {
        NetworkImmunityPeriod::<T>::get()
    }
    pub fn set_network_immunity_period(net_immunity_period: u64) {
        NetworkImmunityPeriod::<T>::set(net_immunity_period);
        Self::deposit_event(Event::NetworkImmunityPeriodSet(net_immunity_period));
    }
    pub fn set_start_call_delay(delay: u64) {
        StartCallDelay::<T>::set(delay);
        Self::deposit_event(Event::StartCallDelaySet(delay));
    }
    pub fn set_network_min_lock(net_min_lock: TaoBalance) {
        NetworkMinLockCost::<T>::set(net_min_lock);
        Self::deposit_event(Event::NetworkMinLockCostSet(net_min_lock));
    }
    pub fn get_network_min_lock() -> TaoBalance {
        NetworkMinLockCost::<T>::get()
    }
    pub fn set_network_last_lock(net_last_lock: TaoBalance) {
        NetworkLastLockCost::<T>::set(net_last_lock);
    }
    pub fn get_network_last_lock() -> TaoBalance {
        NetworkLastLockCost::<T>::get()
    }
    pub fn get_network_last_lock_block() -> u64 {
        Self::get_rate_limited_last_block(&RateLimitKey::NetworkLastRegistered)
    }
    pub fn set_network_last_lock_block(block: u64) {
        Self::set_rate_limited_last_block(&RateLimitKey::NetworkLastRegistered, block);
    }
    pub fn set_lock_reduction_interval(interval: u64) {
        NetworkLockReductionInterval::<T>::set(interval);
        Self::deposit_event(Event::NetworkLockCostReductionIntervalSet(interval));
    }
    pub fn get_lock_reduction_interval() -> u64 {
        let interval: I64F64 =
            I64F64::saturating_from_num(NetworkLockReductionInterval::<T>::get());
        let block_emission: I64F64 = I64F64::saturating_from_num(
            Self::calculate_block_emission()
                .unwrap_or(1_000_000_000.into())
                .to_u64(),
        );
        let halving: I64F64 = block_emission
            .checked_div(I64F64::saturating_from_num(1_000_000_000))
            .unwrap_or(I64F64::saturating_from_num(0.0));
        let halved_interval: I64F64 = interval.saturating_mul(halving);
        halved_interval.saturating_to_num::<u64>()
    }
    pub fn get_rate_limited_last_block(rate_limit_key: &RateLimitKey<T::AccountId>) -> u64 {
        LastRateLimitedBlock::<T>::get(rate_limit_key)
    }
    pub fn set_rate_limited_last_block(rate_limit_key: &RateLimitKey<T::AccountId>, block: u64) {
        LastRateLimitedBlock::<T>::insert(rate_limit_key, block);
    }
    pub fn remove_rate_limited_last_block(rate_limit_key: &RateLimitKey<T::AccountId>) {
        LastRateLimitedBlock::<T>::remove(rate_limit_key);
    }

    pub fn get_network_to_prune() -> Option<NetUid> {
        let current_block: u64 = Self::get_current_block_as_u64();

        let mut candidate_netuid: Option<NetUid> = None;
        let mut candidate_price: U64F64 = U64F64::saturating_from_num(u128::MAX);
        let mut candidate_timestamp: u64 = u64::MAX;

        for (netuid, added) in NetworksAdded::<T>::iter() {
            if !added || netuid == NetUid::ROOT {
                continue;
            }

            let registered_at = NetworkRegisteredAt::<T>::get(netuid);

            // Skip immune networks.
            if current_block < registered_at.saturating_add(Self::get_network_immunity_period()) {
                continue;
            }

            let price: U64F64 = Self::get_moving_alpha_price(netuid);

            // If tie on price, earliest registration wins.
            if price < candidate_price
                || (price == candidate_price && registered_at < candidate_timestamp)
            {
                candidate_netuid = Some(netuid);
                candidate_price = price;
                candidate_timestamp = registered_at;
            }
        }

        candidate_netuid
    }
}
