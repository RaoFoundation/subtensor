use super::*;
use frame_support::weights::Weight;
use share_pool::SafeFloat;
use sp_core::Get;
use sp_std::collections::btree_set::BTreeSet;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{MechId, NetUid, Token};

impl<T: Config> Pallet<T> {
    /// Swaps the hotkey of a coldkey account.
    ///
    /// # Arguments
    ///
    /// * `origin` - The origin of the transaction, and also the coldkey account.
    /// * `old_hotkey` - The old hotkey to be swapped.
    /// * `new_hotkey` - The new hotkey to replace the old one.
    /// * `netuid` - The hotkey swap in a subnet or all subnets.
    /// * `keep_stake` - If `true`, stake remains on the old hotkey and the rest metadata
    ///
    /// # Returns
    ///
    /// * `DispatchResultWithPostInfo` - The result of the dispatch.
    ///
    /// # Errors
    ///
    /// * `NonAssociatedColdKey` - If the coldkey does not own the old hotkey.
    /// * `NewHotKeyIsSameWithOld` - If the new hotkey is the same as the old hotkey.
    /// * `HotKeyAlreadyRegisteredInSubNet` - If the new hotkey is already registered in the subnet.
    /// * `NewHotKeyNotCleanForRootSwap` - If the swap touches root and the new hotkey
    ///   has outstanding `RootClaimable` entries or non-zero root stake.
    /// * `NotEnoughBalanceToPaySwapHotKey` - If there is not enough balance to pay for the swap.
    pub fn do_swap_hotkey(
        origin: OriginFor<T>,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid: Option<NetUid>,
        keep_stake: bool,
    ) -> DispatchResultWithPostInfo {
        // // 1. Ensure the origin is signed and get the coldkey
        let coldkey = ensure_signed(origin)?;

        // 2. Ensure the coldkey owns the old hotkey
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, old_hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        // 3. If the new hotkey already exists globally, ensure the coldkey owns it
        if Self::hotkey_account_exists(new_hotkey) {
            ensure!(
                Self::coldkey_owns_hotkey(&coldkey, new_hotkey),
                Error::<T>::NonAssociatedColdKey
            );
        }

        // 4. Initialize the weight for this operation
        let mut weight = T::DbWeight::get().reads(2);

        // 5. Ensure the new hotkey is different from the old one
        ensure!(old_hotkey != new_hotkey, Error::<T>::NewHotKeyIsSameWithOld);

        // 6. Get the current block number
        let block: u64 = Self::get_current_block_as_u64();

        match netuid {
            // 8. Ensure the hotkey is not registered on the network before, if netuid is provided
            Some(netuid) => {
                ensure!(
                    !Self::is_hotkey_registered_on_specific_network(new_hotkey, netuid),
                    Error::<T>::HotKeyAlreadyRegisteredInSubNet
                );
            }
            // 8.1 Ensure the new hotkey is not already registered on any network, only if netuid is none
            None => {
                ensure!(
                    !Self::is_hotkey_registered_on_any_network(new_hotkey),
                    Error::<T>::HotKeyAlreadyRegisteredInSubNet
                );
            }
        }

        // 8.2 If the swap touches the root subnet, require that new_hotkey is clean
        // on root (no outstanding claimable rate and no existing root stake). Merging
        // a non-empty rate-book would either violate total conservation or misallocate
        // dividends across coldkeys that never staked on old_hotkey.
        let touches_root = match netuid {
            None => true,
            Some(n) => n == NetUid::ROOT,
        };
        if touches_root {
            ensure!(
                RootClaimable::<T>::get(new_hotkey).is_empty()
                    && Self::get_stake_for_hotkey_on_subnet(new_hotkey, NetUid::ROOT).is_zero()
                    && RootClaimed::<T>::iter_prefix((NetUid::ROOT, new_hotkey))
                        .next()
                        .is_none(),
                Error::<T>::NewHotKeyNotCleanForRootSwap
            );
        }

        // 8. Swap LastTxBlockDelegateTake
        let last_tx_block_delegate_take: u64 = Self::get_last_tx_block_delegate_take(old_hotkey);
        Self::set_last_tx_block_delegate_take(new_hotkey, last_tx_block_delegate_take);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        // 11. Swap LastTxBlockChildKeyTake
        let last_tx_block_child_key_take: u64 = Self::get_last_tx_block_childkey_take(old_hotkey);
        Self::set_last_tx_block_childkey(new_hotkey, last_tx_block_child_key_take);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        // 12. fork for swap hotkey on a specific subnet case after do the common check
        if let Some(netuid) = netuid {
            return Self::swap_hotkey_on_subnet(
                &coldkey, old_hotkey, new_hotkey, netuid, weight, keep_stake,
            );
        };

        // Start to do everything for swap hotkey on all subnets case
        // 12.1 Enforce the per-subnet hotkey-swap cooldown on the all-subnets path too.
        // The all-subnets swap moves the identity on every subnet the old hotkey actively
        // participates in, so it must respect (and record) `LastHotkeySwapOnNetuid` for each
        // of those subnets, exactly like the per-subnet path.
        //
        // "Participates in" is membership OR being a parent (has childkeys). Note a parent
        // need not be a registered member — `do_set_children` has no membership requirement —
        // so the childkey case genuinely adds coverage beyond `IsNetworkMember`, and
        // `parent_child_swap_hotkey` re-homes those childkeys even on non-member subnets.
        //
        // We deliberately do NOT gate on the *child* side (`ParentKeys`): being someone's
        // child is set by the parent via `do_set_children` WITHOUT the child's consent, so a
        // third party could otherwise add a victim's hotkey as a child on an arbitrary subnet
        // and impose swap-cooldowns on it — a griefing vector. The swap still migrates the
        // child relationship for correctness; it simply isn't cooldown-gated.
        let hotkey_swap_interval = T::HotkeySwapOnSubnetInterval::get();
        let all_netuids = Self::get_all_subnet_netuids();
        // Up to 2 reads per subnet during filtering (membership + childkeys), plus the
        // subnet-list read itself.
        weight.saturating_accrue(
            T::DbWeight::get().reads(
                (all_netuids.len() as u64)
                    .saturating_mul(2)
                    .saturating_add(1),
            ),
        );
        let affected_netuids: Vec<NetUid> = all_netuids
            .into_iter()
            .filter(|netuid| {
                IsNetworkMember::<T>::get(old_hotkey, *netuid)
                    || !ChildKeys::<T>::get(old_hotkey, *netuid).is_empty()
            })
            .collect();
        for netuid in affected_netuids.iter() {
            let last_hotkey_swap_block = LastHotkeySwapOnNetuid::<T>::get(*netuid, &coldkey);
            // Only enforce the cooldown when a prior swap was recorded on this subnet.
            // A first swap (no recorded timestamp) must not be gated by chain age — that
            // would block the very first swap and never closes a bypass, since any swap
            // records the timestamp and subsequent swaps within the interval are rejected.
            if last_hotkey_swap_block != 0 {
                ensure!(
                    last_hotkey_swap_block.saturating_add(hotkey_swap_interval) < block,
                    Error::<T>::HotKeySwapOnSubnetIntervalNotPassed
                );
            }
            weight.saturating_accrue(T::DbWeight::get().reads(1));
        }

        // Start to do everything for swap hotkey on all subnets case
        // 13. Get the cost for swapping the key
        let swap_cost = Self::get_key_swap_cost();
        log::debug!("Swap cost: {swap_cost:?}");

        // 14. Ensure the coldkey has enough balance to pay for the swap
        ensure!(
            Self::can_remove_balance_from_coldkey_account(&coldkey, swap_cost.into()),
            Error::<T>::NotEnoughBalanceToPaySwapHotKey
        );

        weight.saturating_accrue(T::DbWeight::get().reads_writes(3, 0));

        // 15. Remove the swap cost from the coldkey's account + Recycle the tokens
        Self::recycle_tao(&coldkey, swap_cost.into())?;
        weight.saturating_accrue(T::DbWeight::get().reads_writes(0, 2));

        // 16. Perform the hotkey swap
        Self::perform_hotkey_swap_on_all_subnets(
            old_hotkey,
            new_hotkey,
            &coldkey,
            &mut weight,
            keep_stake,
        )?;

        // 16.1 Record the per-subnet swap cooldown for every affected subnet, so a
        // subsequent per-subnet (or all-subnets) swap on the same subnet within the
        // interval is correctly rejected.
        for netuid in affected_netuids.iter() {
            LastHotkeySwapOnNetuid::<T>::insert(*netuid, &coldkey, block);
            weight.saturating_accrue(T::DbWeight::get().writes(1));
        }

        // 17. Emit an event for the hotkey swap
        Self::deposit_event(Event::HotkeySwapped {
            coldkey,
            old_hotkey: old_hotkey.clone(),
            new_hotkey: new_hotkey.clone(),
        });

        // 19. Return the weight of the operation
        Ok(Some(weight).into())
    }

    /// Performs the hotkey swap operation, transferring all associated data and state from the old hotkey to the new hotkey.
    ///
    /// This function executes a series of steps to ensure a complete transfer of all relevant information:
    /// 1. Swaps the owner of the hotkey.
    /// 2. Updates the list of owned hotkeys for the coldkey.
    /// 3. Transfers the total hotkey stake.
    /// 4. Moves all stake-related data for the interval.
    /// 5. Updates the last transaction block for the new hotkey.
    /// 6. Transfers the delegate take information.
    /// 7. Updates delegate information.
    /// 8. For each subnet:
    ///    - Updates network membership status.
    ///    - Transfers UID and key information.
    ///    - Moves Prometheus data.
    ///    - Updates axon information.
    ///    - Transfers weight commits.
    ///    - Updates loaded emission data.
    /// 9. Transfers all stake information, including updating staking hotkeys for each coldkey.
    ///
    /// Throughout the process, the function accumulates the computational weight of operations performed.
    ///
    /// # Arguments
    /// * `old_hotkey` - The AccountId of the current hotkey to be replaced.
    /// * `new_hotkey` - The AccountId of the new hotkey to replace the old one.
    /// * `coldkey` - The AccountId of the coldkey that owns both hotkeys.
    /// * `weight` - A mutable reference to the Weight, updated as operations are performed.
    ///
    /// # Returns
    /// * `DispatchResult` - Ok(()) if the swap was successful, or an error if any operation failed.
    ///
    /// # Note
    /// This function performs extensive storage reads and writes, which can be computationally expensive.
    /// The accumulated weight should be carefully considered in the context of block limits.
    pub fn perform_hotkey_swap_on_all_subnets(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        weight: &mut Weight,
        keep_stake: bool,
    ) -> DispatchResult {
        // 1. keep the old hotkey alpha values for the case where hotkey staked by multiple coldkeys.
        let old_alpha_values: Vec<(T::AccountId, NetUid, SafeFloat)> =
            Self::alpha_iter_single_prefix(old_hotkey).collect();
        weight.saturating_accrue(T::DbWeight::get().reads(old_alpha_values.len() as u64));

        // 2. Swap the stake locks
        let (reads, writes) = Self::swap_hotkey_locks(old_hotkey, new_hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(reads, writes));

        // 3. Swap owner.
        // Owner( hotkey ) -> coldkey -- the coldkey that owns the hotkey.
        Owner::<T>::remove(old_hotkey);
        Self::set_hotkey_owner(coldkey, new_hotkey)?;
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        // 4. Swap OwnedHotkeys.
        // OwnedHotkeys( coldkey ) -> Vec<hotkey> -- the hotkeys that the coldkey owns.
        let mut hotkeys = OwnedHotkeys::<T>::get(coldkey);
        // Add the new key if needed.
        if !hotkeys.contains(new_hotkey) {
            hotkeys.push(new_hotkey.clone());
        }

        // 5. Remove the old key.
        hotkeys.retain(|hk| *hk != *old_hotkey);
        OwnedHotkeys::<T>::insert(coldkey, hotkeys);

        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        // 6. execute the hotkey swap on all subnets
        for netuid in Self::get_all_subnet_netuids() {
            Self::perform_hotkey_swap_on_one_subnet(
                old_hotkey, new_hotkey, weight, netuid, keep_stake,
            )?;
        }

        // 7. Swap LastTxBlock
        // LastTxBlock( hotkey ) --> u64 -- the last transaction block for the hotkey.
        Self::remove_last_tx_block(old_hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));

        // 8. Swap LastTxBlockDelegateTake
        // LastTxBlockDelegateTake( hotkey ) --> u64 -- the last transaction block for the hotkey delegate take.
        Self::remove_last_tx_block_delegate_take(old_hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));

        // 9. Swap LastTxBlockChildKeyTake
        // LastTxBlockChildKeyTake( hotkey ) --> u64 -- the last transaction block for the hotkey child key take.
        Self::remove_last_tx_block_childkey(old_hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));

        // 10. Swap delegates.
        // Delegates( hotkey ) -> take value -- the hotkey delegate take value.
        if Delegates::<T>::contains_key(old_hotkey) {
            let old_delegate_take = Delegates::<T>::get(old_hotkey);
            Delegates::<T>::remove(old_hotkey);
            Delegates::<T>::insert(new_hotkey, old_delegate_take);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));
        }

        // 11. Alphas already update in perform_hotkey_swap_on_one_subnet
        // Update the StakingHotkeys for the case where hotkey staked by multiple coldkeys.
        if !keep_stake {
            for (coldkey, _netuid, alpha_share) in old_alpha_values {
                // Swap StakingHotkeys.
                // StakingHotkeys( coldkey ) --> Vec<hotkey> -- the hotkeys that the coldkey stakes.
                if !alpha_share.is_zero() {
                    let mut staking_hotkeys = StakingHotkeys::<T>::get(&coldkey);
                    weight.saturating_accrue(T::DbWeight::get().reads(1));
                    if staking_hotkeys.contains(old_hotkey) {
                        staking_hotkeys.retain(|hk| *hk != *old_hotkey && *hk != *new_hotkey);
                        if !staking_hotkeys.contains(new_hotkey) {
                            staking_hotkeys.push(new_hotkey.clone());
                        }
                        StakingHotkeys::<T>::insert(&coldkey, staking_hotkeys);
                        weight.saturating_accrue(T::DbWeight::get().writes(1));
                    }
                }
            }
        }

        // Return successful after swapping all the relevant terms.
        Ok(())
    }

    #[allow(unused)]
    fn swap_hotkey_on_subnet(
        coldkey: &T::AccountId,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid: NetUid,
        init_weight: Weight,
        keep_stake: bool,
    ) -> DispatchResultWithPostInfo {
        // 1. Ensure coldkey not swap hotkey too frequently
        let mut weight: Weight = init_weight;
        let block: u64 = Self::get_current_block_as_u64();
        let hotkey_swap_interval = T::HotkeySwapOnSubnetInterval::get();
        let last_hotkey_swap_block = LastHotkeySwapOnNetuid::<T>::get(netuid, coldkey);

        ensure!(
            last_hotkey_swap_block.saturating_add(hotkey_swap_interval) < block,
            Error::<T>::HotKeySwapOnSubnetIntervalNotPassed
        );
        weight.saturating_accrue(T::DbWeight::get().reads_writes(3, 0));

        // Check that new hotkey is a non-system hotkey
        ensure!(
            Self::is_subnet_account_id(new_hotkey).is_none(),
            Error::<T>::CannotUseSystemAccount
        );

        // 2. Ensure the hotkey not registered on the network before.
        ensure!(
            !Self::is_hotkey_registered_on_specific_network(new_hotkey, netuid),
            Error::<T>::HotKeyAlreadyRegisteredInSubNet
        );

        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 0));

        // 3. Get the cost for swapping the key on the subnet
        let swap_cost = T::KeySwapOnSubnetCost::get();
        log::debug!("Swap cost in subnet {netuid:?}: {swap_cost:?}");
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 0));

        // 4. Ensure the coldkey has enough balance to pay for the swap
        ensure!(
            Self::can_remove_balance_from_coldkey_account(coldkey, swap_cost),
            Error::<T>::NotEnoughBalanceToPaySwapHotKey
        );

        // 5. Remove the swap cost from the coldkey's account + Recycle the tokens
        Self::recycle_tao(coldkey, swap_cost)?;
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        // 7. Swap owner.
        // Owner( hotkey ) -> coldkey -- the coldkey that owns the hotkey.
        // Owner::<T>::remove(old_hotkey);
        Owner::<T>::insert(new_hotkey, coldkey.clone());
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        // 8. Swap OwnedHotkeys.
        // OwnedHotkeys( coldkey ) -> Vec<hotkey> -- the hotkeys that the coldkey owns.
        let mut hotkeys = OwnedHotkeys::<T>::get(coldkey);
        // Add the new key if needed.
        if !hotkeys.contains(new_hotkey) {
            hotkeys.push(new_hotkey.clone());
            OwnedHotkeys::<T>::insert(coldkey, hotkeys);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        // 9. Swap the stake locks
        let (reads, writes) = Self::swap_hotkey_locks_on_subnet(old_hotkey, new_hotkey, netuid);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(reads, writes));

        // 10. Perform the hotkey swap
        Self::perform_hotkey_swap_on_one_subnet(
            old_hotkey,
            new_hotkey,
            &mut weight,
            netuid,
            keep_stake,
        )?;

        // 10. Record the per-subnet swap block for the HotkeySwapOnSubnetInterval gate.
        //     The generic LastTxBlock setter is dropped together with its removed check.
        LastHotkeySwapOnNetuid::<T>::insert(netuid, coldkey, block);
        weight.saturating_accrue(T::DbWeight::get().writes(1));

        // 12. Emit an event for the hotkey swap
        Self::deposit_event(Event::HotkeySwappedOnSubnet {
            coldkey: coldkey.clone(),
            old_hotkey: old_hotkey.clone(),
            new_hotkey: new_hotkey.clone(),
            netuid,
        });

        Ok(Some(weight).into())
    }

    // do hotkey swap public part for both swap all subnets and just swap one subnet
    pub fn perform_hotkey_swap_on_one_subnet(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        weight: &mut Weight,
        netuid: NetUid,
        keep_stake: bool,
    ) -> DispatchResult {
        // 3. Swap all subnet specific info.

        // 3.1 Remove the previous hotkey and insert the new hotkey from membership.
        // IsNetworkMember( hotkey, netuid ) -> bool -- is the hotkey a subnet member.
        let is_network_member: bool = IsNetworkMember::<T>::get(old_hotkey, netuid);
        IsNetworkMember::<T>::remove(old_hotkey, netuid);
        IsNetworkMember::<T>::insert(new_hotkey, netuid, is_network_member);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));

        // 3.2 Swap Uids + Keys.
        // Keys( netuid, hotkey ) -> uid -- the uid the hotkey has in the network if it is a member.
        // Uids( netuid, hotkey ) -> uid -- the uids that the hotkey has.
        if is_network_member {
            // 3.2.1 Swap the UIDS
            if let Ok(old_uid) = Uids::<T>::try_get(netuid, old_hotkey) {
                Uids::<T>::remove(netuid, old_hotkey);
                Uids::<T>::insert(netuid, new_hotkey, old_uid);
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));

                // 3.2.2 Swap the keys.
                Keys::<T>::insert(netuid, old_uid, new_hotkey.clone());
                weight.saturating_accrue(T::DbWeight::get().reads_writes(0, 1));
            }
        }

        // 3.3 Swap Prometheus.
        // Prometheus( netuid, hotkey ) -> prometheus -- the prometheus data that a hotkey has in the network.
        if is_network_member
            && let Ok(old_prometheus_info) = Prometheus::<T>::try_get(netuid, old_hotkey)
        {
            Prometheus::<T>::remove(netuid, old_hotkey);
            Prometheus::<T>::insert(netuid, new_hotkey, old_prometheus_info);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 3.4. Swap axons.
        // Axons( netuid, hotkey ) -> axon -- the axon that the hotkey has.
        if is_network_member && let Ok(old_axon_info) = Axons::<T>::try_get(netuid, old_hotkey) {
            Axons::<T>::remove(netuid, old_hotkey);
            Axons::<T>::insert(netuid, new_hotkey, old_axon_info);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 3.5 Swap WeightCommits
        // WeightCommits( hotkey ) --> Vec<u64> -- the weight commits for the hotkey.
        if is_network_member {
            for mecid in 0..MechanismCountCurrent::<T>::get(netuid).into() {
                let netuid_index = Self::get_mechanism_storage_index(netuid, MechId::from(mecid));
                if let Ok(old_weight_commits) =
                    WeightCommits::<T>::try_get(netuid_index, old_hotkey)
                {
                    WeightCommits::<T>::remove(netuid_index, old_hotkey);
                    WeightCommits::<T>::insert(netuid_index, new_hotkey, old_weight_commits);
                    weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
                }
            }
        }

        // 3.6. Swap the subnet loaded emission.
        // LoadedEmission( netuid ) --> Vec<(hotkey, u64)> -- the loaded emission for the subnet.
        if is_network_member && let Some(mut old_loaded_emission) = LoadedEmission::<T>::get(netuid)
        {
            for emission in old_loaded_emission.iter_mut() {
                if emission.0 == *old_hotkey {
                    emission.0 = new_hotkey.clone();
                }
            }
            LoadedEmission::<T>::remove(netuid);
            LoadedEmission::<T>::insert(netuid, old_loaded_emission);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 3.7. Swap neuron TLS certificates.
        // NeuronCertificates( netuid, hotkey ) -> Vec<u8> -- the neuron certificate for the hotkey.
        if is_network_member
            && let Ok(old_neuron_certificates) =
                NeuronCertificates::<T>::try_get(netuid, old_hotkey)
        {
            NeuronCertificates::<T>::remove(netuid, old_hotkey);
            NeuronCertificates::<T>::insert(netuid, new_hotkey, old_neuron_certificates);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 3.8. Swap ChildkeyTake.
        // ChildkeyTake( hotkey, netuid ) --> u16 -- the per-subnet childkey take for the hotkey.
        // Only migrate when an explicit value exists, to preserve the storage default
        // semantics (don't materialize a floor value for hotkeys that never set a take).
        // `take` reads + removes the old row in one operation, avoiding orphaned storage.
        if ChildkeyTake::<T>::contains_key(old_hotkey, netuid) {
            let childkey_take = ChildkeyTake::<T>::take(old_hotkey, netuid);
            ChildkeyTake::<T>::insert(new_hotkey, netuid, childkey_take);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 4. Swap ChildKeys.
        // 5. Swap ParentKeys.
        // 6. Swap PendingChildKeys.
        Self::parent_child_swap_hotkey(old_hotkey, new_hotkey, netuid, weight)?;

        // Also check for others with our hotkey as a child
        for (hotkey, (children, cool_down_block)) in PendingChildKeys::<T>::iter_prefix(netuid) {
            weight.saturating_accrue(T::DbWeight::get().reads(1));

            if let Some(potential_idx) =
                children.iter().position(|(_, child)| *child == *old_hotkey)
            {
                let mut new_children = children.clone();
                let entry_to_remove = new_children.remove(potential_idx);
                new_children.push((entry_to_remove.0, new_hotkey.clone())); // Keep the proportion.

                PendingChildKeys::<T>::remove(netuid, hotkey.clone());
                PendingChildKeys::<T>::insert(netuid, hotkey, (new_children, cool_down_block));
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
            }
        }

        // 6.4 Swap AutoStakeDestination
        if let Ok(old_auto_stake_coldkeys) =
            AutoStakeDestinationColdkeys::<T>::try_get(old_hotkey, netuid)
        {
            // Move the vector from old hotkey to new hotkey.
            for coldkey in &old_auto_stake_coldkeys {
                AutoStakeDestination::<T>::insert(coldkey, netuid, new_hotkey);
            }
            AutoStakeDestinationColdkeys::<T>::remove(old_hotkey, netuid);
            AutoStakeDestinationColdkeys::<T>::insert(new_hotkey, netuid, old_auto_stake_coldkeys);
        }

        // 7. Swap SubnetOwnerHotkey
        // SubnetOwnerHotkey( netuid ) --> hotkey -- the hotkey that is the owner of the subnet.
        if let Ok(old_subnet_owner_hotkey) = SubnetOwnerHotkey::<T>::try_get(netuid) {
            weight.saturating_accrue(T::DbWeight::get().reads(1));
            if old_subnet_owner_hotkey == *old_hotkey {
                Self::set_subnet_owner_hotkey(netuid, new_hotkey)?;
                weight.saturating_accrue(T::DbWeight::get().writes(1));
            }
        }

        // 8. Swap dividend records
        if !keep_stake {
            // 8.1 Swap TotalHotkeyAlphaLastEpoch
            let old_alpha = TotalHotkeyAlphaLastEpoch::<T>::take(old_hotkey, netuid);
            let new_total_hotkey_alpha = TotalHotkeyAlphaLastEpoch::<T>::get(new_hotkey, netuid);
            TotalHotkeyAlphaLastEpoch::<T>::insert(
                new_hotkey,
                netuid,
                old_alpha.saturating_add(new_total_hotkey_alpha),
            );
            weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));

            // 8.2 Swap AlphaDividendsPerSubnet
            let old_hotkey_alpha_dividends = AlphaDividendsPerSubnet::<T>::get(netuid, old_hotkey);
            let new_hotkey_alpha_dividends = AlphaDividendsPerSubnet::<T>::get(netuid, new_hotkey);
            AlphaDividendsPerSubnet::<T>::remove(netuid, old_hotkey);
            AlphaDividendsPerSubnet::<T>::insert(
                netuid,
                new_hotkey,
                old_hotkey_alpha_dividends.saturating_add(new_hotkey_alpha_dividends),
            );
            weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));

            // 8.3 Swap TaoDividendsPerSubnet
            // Tao dividends were removed

            // 8.4 Swap VotingPower
            // VotingPower( netuid, hotkey ) --> u64 -- the voting power EMA for the hotkey.
            Self::swap_voting_power_for_hotkey(old_hotkey, new_hotkey, netuid);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));

            // Swap Alpha
            // Alpha( hotkey, coldkey, netuid ) -> alpha
            let old_alpha_values: Vec<((T::AccountId, NetUid), U64F64)> =
                Alpha::<T>::iter_prefix((old_hotkey,)).collect();
            weight.saturating_accrue(T::DbWeight::get().reads(old_alpha_values.len() as u64));
            weight.saturating_accrue(T::DbWeight::get().writes(old_alpha_values.len() as u64));

            let old_alpha_values_v2: Vec<((T::AccountId, NetUid), SafeFloat)> =
                AlphaV2::<T>::iter_prefix((old_hotkey,)).collect();
            weight.saturating_accrue(T::DbWeight::get().reads(old_alpha_values_v2.len() as u64));
            weight.saturating_accrue(T::DbWeight::get().writes(old_alpha_values_v2.len() as u64));

            // Insert the new alpha values.
            // Deduplicate coldkeys staking to old_hotkey from alpha and alpha_v2
            let unique_coldkeys: BTreeSet<T::AccountId> = old_alpha_values
                .into_iter()
                .map(|((coldkey, netuid_alpha), _)| (coldkey, netuid_alpha))
                .chain(
                    old_alpha_values_v2
                        .into_iter()
                        .map(|((coldkey, netuid_alpha), _)| (coldkey, netuid_alpha)),
                )
                .filter(|(_, netuid_alpha)| *netuid_alpha == netuid)
                .map(|(coldkey, _)| coldkey)
                .collect();

            // For each coldkey remove their stake from old_hotkey and add to new_hotkey
            for coldkey in unique_coldkeys {
                let alpha_old =
                    Self::get_stake_for_hotkey_and_coldkey_on_subnet(old_hotkey, &coldkey, netuid);
                Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                    old_hotkey, &coldkey, netuid, alpha_old,
                );
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    new_hotkey, &coldkey, netuid, alpha_old,
                );
                weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));

                let mut staking_hotkeys = StakingHotkeys::<T>::get(&coldkey);
                weight.saturating_accrue(T::DbWeight::get().reads(1));

                if staking_hotkeys.contains(old_hotkey) && !staking_hotkeys.contains(new_hotkey) {
                    staking_hotkeys.push(new_hotkey.clone());
                    StakingHotkeys::<T>::insert(&coldkey, staking_hotkeys);
                    weight.saturating_accrue(T::DbWeight::get().writes(1));
                }
            }

            if netuid == NetUid::ROOT {
                // 9. Transfer root claimable and root claimed only for the root subnet
                // NOTE: we shouldn't transfer root claimable and root claimed for other subnets,
                // otherwise root stakers won't be able to receive dividends.
                Self::transfer_root_claimable_for_new_hotkey(old_hotkey, new_hotkey);
                weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));

                // After transfer, new_hotkey has the full RootClaimable map.
                // We use it to know which subnets have outstanding claims.
                let subnets: Vec<NetUid> = RootClaimable::<T>::get(new_hotkey)
                    .keys()
                    .copied()
                    .collect();
                weight.saturating_accrue(T::DbWeight::get().reads(1));

                for subnet in subnets {
                    let claimed_coldkeys: Vec<T::AccountId> =
                        RootClaimed::<T>::iter_prefix((subnet, old_hotkey))
                            .map(|(coldkey, _)| coldkey)
                            .collect();
                    weight
                        .saturating_accrue(T::DbWeight::get().reads(claimed_coldkeys.len() as u64));

                    for coldkey in claimed_coldkeys {
                        Self::transfer_root_claimed_for_new_keys(
                            subnet, old_hotkey, new_hotkey, &coldkey, &coldkey,
                        );
                        weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));
                    }
                }

                // Transfer AutoParentDelegationEnabled flag from old_hotkey to new_hotkey.
                // Only migrate if it was explicitly set, to preserve the storage default semantics.
                if AutoParentDelegationEnabled::<T>::contains_key(old_hotkey) {
                    let enabled = AutoParentDelegationEnabled::<T>::take(old_hotkey);
                    AutoParentDelegationEnabled::<T>::insert(new_hotkey, enabled);
                    weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
                }
            }
        }

        Ok(())
    }
}
