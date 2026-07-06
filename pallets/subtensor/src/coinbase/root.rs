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
use crate::CommitmentsInterface;
use safe_math::*;
use substrate_fixed::types::{I64F64, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

impl<T: Config> Pallet<T> {
    /// Fetches the total count of root network validators
    ///
    /// This function retrieves the total number of root network validators.
    ///
    /// # Returns:
    /// * 'u16': The total number of root network validators
    ///
    pub fn get_num_root_validators() -> u16 {
        Self::get_subnetwork_n(NetUid::ROOT)
    }

    /// Fetches the max validators count of root network.
    ///
    /// This function retrieves the max validators count of root network.
    ///
    /// # Returns:
    /// * 'u16': The max validators count of root network.
    ///
    pub fn get_max_root_validators() -> u16 {
        Self::get_max_allowed_uids(NetUid::ROOT)
    }

    /// Checks for any UIDs in the given list that are either equal to the root netuid or exceed the total number of subnets.
    ///
    /// It's important to check for invalid UIDs to ensure data integrity and avoid referencing nonexistent subnets.
    ///
    /// # Arguments:
    /// * 'uids': A reference to a vector of UIDs to check.
    ///
    /// # Returns:
    /// * 'bool': 'true' if any of the UIDs are invalid, 'false' otherwise.
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
    /// This function is responsible for registering the hotkey of a user.
    /// The root key with the least stake if pruned in the event of a filled network.
    ///
    /// # Arguments:
    /// * 'origin': Represents the origin of the call.
    /// * 'hotkey': The hotkey that the user wants to register to the root network.
    ///
    /// # Returns:
    /// * 'DispatchResult': A result type indicating success or failure of the registration.
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

        // Declare a variable to hold the root UID.
        let subnetwork_uid: u16;

        // --- 8. Check if the root net is below its allowed size.
        // max allowed is senate size.
        if current_num_root_validators < Self::get_max_root_validators() {
            // --- 12.1.1 We can append to the subnetwork as it's not full.
            subnetwork_uid = current_num_root_validators;

            // --- 12.1.2 Add the new account and make them a member of the Senate.
            Self::append_neuron(NetUid::ROOT, &hotkey, current_block_number);
            log::debug!("add new neuron: {hotkey:?} on uid {subnetwork_uid:?}");
        } else {
            // --- 13.1.1 The network is full. Perform replacement.
            // Find the neuron with the lowest stake value to replace.
            let mut lowest_stake = AlphaBalance::MAX;
            let mut lowest_uid: u16 = 0;

            // Iterate over all keys in the root network to find the neuron with the lowest stake.
            for (uid_i, hotkey_i) in Keys::<T>::iter_prefix(NetUid::ROOT) {
                let stake_i = Self::get_stake_for_hotkey_on_subnet(&hotkey_i, NetUid::ROOT);
                if stake_i < lowest_stake {
                    lowest_stake = stake_i;
                    lowest_uid = uid_i;
                }
            }
            subnetwork_uid = lowest_uid;
            let replaced_hotkey: T::AccountId =
                Self::get_hotkey_for_net_and_uid(NetUid::ROOT, subnetwork_uid)?;

            // --- 13.1.2 The new account has a higher stake than the one being replaced.
            ensure!(
                lowest_stake < Self::get_stake_for_hotkey_on_subnet(&hotkey, NetUid::ROOT),
                Error::<T>::StakeTooLowForRoot
            );

            // --- 13.1.3 The new account has a higher stake than the one being replaced.
            // Replace the neuron account with new information.
            Self::replace_neuron(NetUid::ROOT, lowest_uid, &hotkey, current_block_number);

            log::debug!(
                "replace neuron: {replaced_hotkey:?} with {hotkey:?} on uid {subnetwork_uid:?}"
            );
        }

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
            hotkey,
        ));

        // --- 16. Finish and return success.
        Ok(())
    }

    /// Facilitates the removal of a user's subnetwork.
    ///
    /// # Args:
    /// * 'origin': ('T::RuntimeOrigin'): The calling origin. Must be signed.
    /// * 'netuid': ('u16'): The unique identifier of the network to be removed.
    ///
    /// # Event:
    /// * 'NetworkRemoved': Emitted when a network is successfully removed.
    ///
    /// # Raises:
    /// * 'MechanismDoesNotExist': If the specified network does not exist.
    /// * 'NotSubnetOwner': If the caller does not own the specified subnet.
    ///
    pub fn do_dissolve_network(netuid: NetUid) -> dispatch::DispatchResult {
        // --- The network exists?
        ensure!(
            Self::if_subnet_exist(netuid) && netuid != NetUid::ROOT,
            Error::<T>::SubnetNotExists
        );

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

        Self::finalize_all_subnet_root_dividends(netuid);

        // --- Perform the cleanup before removing the network.
        Self::destroy_alpha_in_out_stakes(netuid)?;
        T::SwapInterface::clear_protocol_liquidity(netuid);
        T::CommitmentsInterface::purge_netuid(netuid);

        // --- Remove the network
        Self::remove_network(netuid);

        // --- Emit the NetworkRemoved event
        log::info!("NetworkRemoved( netuid:{netuid:?} )");
        Self::deposit_event(Event::NetworkRemoved(netuid));

        Ok(())
    }

    pub fn remove_network(netuid: NetUid) {
        // --- 1. Get the owner and remove from SubnetOwner.
        let owner_coldkey: T::AccountId = SubnetOwner::<T>::get(netuid);
        SubnetOwner::<T>::remove(netuid);

        // --- 2. Remove network count.
        SubnetworkN::<T>::remove(netuid);

        // --- 3. Remove netuid from added networks.
        NetworksAdded::<T>::remove(netuid);

        // --- 4. Decrement the network counter.
        TotalNetworks::<T>::mutate(|n: &mut u16| *n = n.saturating_sub(1));

        // --- 5. Remove various network-related storages.
        NetworkRegisteredAt::<T>::remove(netuid);

        // --- 6. Remove incentive mechanism memory.
        let _ = Uids::<T>::clear_prefix(netuid, u32::MAX, None);
        let keys = Keys::<T>::iter_prefix(netuid).collect::<Vec<_>>();
        let _ = Keys::<T>::clear_prefix(netuid, u32::MAX, None);

        // --- 8. Iterate over stored weights and fill the matrix.
        for (uid_i, weights_i) in Weights::<T>::iter_prefix(NetUidStorageIndex::ROOT) {
            // Create a new vector to hold modified weights.
            let mut modified_weights = weights_i.clone();
            for (subnet_id, weight) in modified_weights.iter_mut() {
                // If the root network had a weight pointing to this netuid, set it to 0
                if subnet_id == &u16::from(netuid) {
                    *weight = 0;
                }
            }
            Weights::<T>::insert(NetUidStorageIndex::ROOT, uid_i, modified_weights);
        }

        // --- 9. Remove various network-related parameters.
        Active::<T>::remove(netuid);
        Emission::<T>::remove(netuid);

        Consensus::<T>::remove(netuid);
        Dividends::<T>::remove(netuid);
        ValidatorPermit::<T>::remove(netuid);
        ValidatorTrust::<T>::remove(netuid);

        for (_uid, key) in keys {
            IsNetworkMember::<T>::remove(key, netuid);
        }

        // --- 10. Erase network parameters.
        Tempo::<T>::remove(netuid);
        Kappa::<T>::remove(netuid);
        Difficulty::<T>::remove(netuid);
        MaxAllowedUids::<T>::remove(netuid);
        ImmunityPeriod::<T>::remove(netuid);
        ActivityCutoff::<T>::remove(netuid);
        ActivityCutoffFactorMilli::<T>::remove(netuid);
        LastEpochBlock::<T>::remove(netuid);
        PendingEpochAt::<T>::remove(netuid);
        SubnetEpochIndex::<T>::remove(netuid);
        MinAllowedWeights::<T>::remove(netuid);
        RegistrationsThisInterval::<T>::remove(netuid);
        POWRegistrationsThisInterval::<T>::remove(netuid);
        BurnRegistrationsThisInterval::<T>::remove(netuid);

        // --- 11. AMM / price / accounting.
        // SubnetTAO, SubnetAlpha{In,InProvided,Out} are already cleared during dissolve/destroy.
        SubnetAlphaInEmission::<T>::remove(netuid);
        SubnetAlphaOutEmission::<T>::remove(netuid);
        SubnetTaoInEmission::<T>::remove(netuid);
        SubnetVolume::<T>::remove(netuid);
        SubnetMovingPrice::<T>::remove(netuid);
        SubnetTaoFlow::<T>::remove(netuid);
        SubnetEmaTaoFlow::<T>::remove(netuid);
        SubnetProtocolFlow::<T>::remove(netuid);
        SubnetEmaProtocolFlow::<T>::remove(netuid);
        SubnetExcessTao::<T>::remove(netuid);
        SubnetRootSellTao::<T>::remove(netuid);

        // --- 13. Token / mechanism / registration toggles.
        TokenSymbol::<T>::remove(netuid);
        SubnetMechanism::<T>::remove(netuid);
        SubnetOwnerHotkey::<T>::remove(netuid);
        NetworkRegistrationAllowed::<T>::remove(netuid);
        NetworkPowRegistrationAllowed::<T>::remove(netuid);

        // --- 14. Locks & toggles.
        TransferToggle::<T>::remove(netuid);
        SubnetLocked::<T>::remove(netuid);
        LargestLocked::<T>::remove(netuid);

        // --- 15. Mechanism step / emissions bookkeeping.
        FirstEmissionBlockNumber::<T>::remove(netuid);
        PendingValidatorEmission::<T>::remove(netuid);
        PendingServerEmission::<T>::remove(netuid);
        PendingRootAlphaDivs::<T>::remove(netuid);
        PendingOwnerCut::<T>::remove(netuid);
        MinerBurned::<T>::remove(netuid);
        BlocksSinceLastStep::<T>::remove(netuid);
        LastMechansimStepBlock::<T>::remove(netuid);
        LastAdjustmentBlock::<T>::remove(netuid);

        // --- 16. Serving / rho / curves, and other per-net controls.
        ServingRateLimit::<T>::remove(netuid);
        Rho::<T>::remove(netuid);
        AlphaSigmoidSteepness::<T>::remove(netuid);

        MaxAllowedValidators::<T>::remove(netuid);
        BondsMovingAverage::<T>::remove(netuid);
        BondsPenalty::<T>::remove(netuid);
        BondsResetOn::<T>::remove(netuid);
        WeightsSetRateLimit::<T>::remove(netuid);
        ValidatorPruneLen::<T>::remove(netuid);
        ScalingLawPower::<T>::remove(netuid);
        TargetRegistrationsPerInterval::<T>::remove(netuid);
        CommitRevealWeightsEnabled::<T>::remove(netuid);

        BurnHalfLife::<T>::remove(netuid);
        BurnIncreaseMult::<T>::remove(netuid);

        Burn::<T>::remove(netuid);
        MinBurn::<T>::remove(netuid);
        MaxBurn::<T>::remove(netuid);
        MinDifficulty::<T>::remove(netuid);
        MaxDifficulty::<T>::remove(netuid);
        RegistrationsThisBlock::<T>::remove(netuid);
        EMAPriceHalvingBlocks::<T>::remove(netuid);
        RAORecycledForRegistration::<T>::remove(netuid);
        MaxRegistrationsPerBlock::<T>::remove(netuid);
        WeightsVersionKey::<T>::remove(netuid);

        // --- 17. Subtoken / feature flags.
        LiquidAlphaOn::<T>::remove(netuid);
        Yuma3On::<T>::remove(netuid);
        AlphaValues::<T>::remove(netuid);
        SubtokenEnabled::<T>::remove(netuid);
        OwnerCutAutoLockEnabled::<T>::remove(netuid);
        ImmuneOwnerUidsLimit::<T>::remove(netuid);

        // --- 18. Consensus aux vectors.
        StakeWeight::<T>::remove(netuid);
        LoadedEmission::<T>::remove(netuid);

        // --- 19. DMAPs where netuid is the FIRST key: clear by prefix.
        let _ = BlockAtRegistration::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = Axons::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = NeuronCertificates::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = Prometheus::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = AlphaDividendsPerSubnet::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = PendingChildKeys::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = AssociatedEvmAddress::<T>::clear_prefix(netuid, u32::MAX, None);

        // Commit-reveal / weights commits (all per-net prefixes):
        let mechanisms: u8 = MechanismCountCurrent::<T>::get(netuid).into();
        for subid in 0..mechanisms {
            let netuid_index = Self::get_mechanism_storage_index(netuid, subid.into());
            LastUpdate::<T>::remove(netuid_index);
            Incentive::<T>::remove(netuid_index);
            let _ = WeightCommits::<T>::clear_prefix(netuid_index, u32::MAX, None);
            let _ = TimelockedWeightCommits::<T>::clear_prefix(netuid_index, u32::MAX, None);
            let _ = CRV3WeightCommits::<T>::clear_prefix(netuid_index, u32::MAX, None);
            let _ = CRV3WeightCommitsV2::<T>::clear_prefix(netuid_index, u32::MAX, None);
            let _ = Bonds::<T>::clear_prefix(netuid_index, u32::MAX, None);
            let _ = Weights::<T>::clear_prefix(netuid_index, u32::MAX, None);
        }
        RevealPeriodEpochs::<T>::remove(netuid);
        MechanismCountCurrent::<T>::remove(netuid);
        MechanismEmissionSplit::<T>::remove(netuid);

        // Last hotkey swap (DMAP where netuid is FIRST key → easy)
        let _ = LastHotkeySwapOnNetuid::<T>::clear_prefix(netuid, u32::MAX, None);

        // --- 20. Identity maps across versions (netuid-scoped).
        if SubnetIdentitiesV3::<T>::contains_key(netuid) {
            SubnetIdentitiesV3::<T>::remove(netuid);
            Self::deposit_event(Event::SubnetIdentityRemoved(netuid));
        }

        // --- 21. DMAP / NMAP where netuid is NOT the first key → iterate & remove.

        // ChildkeyTake: (hot, netuid) → u16
        {
            let to_rm: sp_std::vec::Vec<T::AccountId> = ChildkeyTake::<T>::iter()
                .filter_map(|(hot, n, _)| if n == netuid { Some(hot) } else { None })
                .collect();
            for hot in to_rm {
                ChildkeyTake::<T>::remove(&hot, netuid);
            }
        }
        // ChildKeys: (parent, netuid) → Vec<...>
        {
            let to_rm: sp_std::vec::Vec<T::AccountId> = ChildKeys::<T>::iter()
                .filter_map(|(parent, n, _)| if n == netuid { Some(parent) } else { None })
                .collect();
            for parent in to_rm {
                ChildKeys::<T>::remove(&parent, netuid);
            }
        }
        // ParentKeys: (child, netuid) → Vec<...>
        {
            let to_rm: sp_std::vec::Vec<T::AccountId> = ParentKeys::<T>::iter()
                .filter_map(|(child, n, _)| if n == netuid { Some(child) } else { None })
                .collect();
            for child in to_rm {
                ParentKeys::<T>::remove(&child, netuid);
            }
        }
        // LastHotkeyEmissionOnNetuid: (hot, netuid) → α
        {
            let to_rm: sp_std::vec::Vec<T::AccountId> = LastHotkeyEmissionOnNetuid::<T>::iter()
                .filter_map(|(hot, n, _)| if n == netuid { Some(hot) } else { None })
                .collect();
            for hot in to_rm {
                LastHotkeyEmissionOnNetuid::<T>::remove(&hot, netuid);
            }
        }
        // TotalHotkeyAlphaLastEpoch: (hot, netuid) → ...
        // (TotalHotkeyAlpha and TotalHotkeyShares were already removed during dissolve.)
        {
            let to_rm_alpha_last: sp_std::vec::Vec<T::AccountId> =
                TotalHotkeyAlphaLastEpoch::<T>::iter()
                    .filter_map(|(hot, n, _)| if n == netuid { Some(hot) } else { None })
                    .collect();
            for hot in to_rm_alpha_last {
                TotalHotkeyAlphaLastEpoch::<T>::remove(&hot, netuid);
            }
        }
        // TransactionKeyLastBlock NMAP: (hot, netuid, name) → u64
        {
            let to_rm: sp_std::vec::Vec<(T::AccountId, u16)> = TransactionKeyLastBlock::<T>::iter()
                .filter_map(
                    |((hot, n, name), _)| if n == netuid { Some((hot, name)) } else { None },
                )
                .collect();
            for (hot, name) in to_rm {
                TransactionKeyLastBlock::<T>::remove((hot, netuid, name));
            }
        }
        // --- 22. Subnet leasing: remove mapping and any lease-scoped state linked to this netuid.
        if let Some(lease_id) = SubnetUidToLeaseId::<T>::take(netuid) {
            SubnetLeases::<T>::remove(lease_id);
            let _ = SubnetLeaseShares::<T>::clear_prefix(lease_id, u32::MAX, None);
            AccumulatedLeaseDividends::<T>::remove(lease_id);
        }

        // --- 23: Locks cleanup
        Self::destroy_lock_maps(netuid);

        // --- Final removal logging.
        log::debug!(
            "remove_network: netuid={netuid}, owner={owner_coldkey:?} removed successfully"
        );
    }

    #[allow(clippy::arithmetic_side_effects)]
    /// This function calculates the lock cost for a network based on the last lock amount, minimum lock cost, last lock block, and current block.
    /// The lock cost is calculated using the formula:
    /// lock_cost = (last_lock * mult) - (last_lock / lock_reduction_interval) * (current_block - last_lock_block)
    /// where:
    /// - last_lock is the last lock amount for the network
    /// - mult is the multiplier which increases lock cost each time a registration occurs
    /// - last_lock_block is the block number at which the last lock occurred
    /// - lock_reduction_interval the number of blocks before the lock returns to previous value.
    /// - current_block is the current block number
    /// - DAYS is the number of blocks in a day
    /// - min_lock is the minimum lock cost for the network
    ///
    /// If the calculated lock cost is less than the minimum lock cost, the minimum lock cost is returned.
    ///
    /// # Returns:
    ///  * 'u64':
    ///     - The lock cost for the network.
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
