use super::*;
use frame_support::storage::IterableStorageDoubleMap;
use sp_runtime::{PerU16, Percent};
use sp_std::collections::{btree_map::BTreeMap, btree_set::BTreeSet};
use sp_std::{cmp, vec};
use subtensor_runtime_common::NetUid;

/// Maximum UID deletions in one explicit Null pruning transaction.
pub const NULL_PRUNING_BATCH: u16 = 64;

impl<T: Config> Pallet<T> {
    /// Returns the number of filled slots on a network.
    pub fn get_subnetwork_n(netuid: NetUid) -> u16 {
        SubnetworkN::<T>::get(netuid)
    }

    /// Sets value for the element at the given position if it exists.
    pub fn set_element_at<N>(vec: &mut [N], position: usize, value: N) {
        if let Some(element) = vec.get_mut(position) {
            *element = value;
        }
    }

    /// Resets the emission, consensus, incentives, dividends, bonds, and weights of
    /// the neuron to default
    pub fn clear_neuron(netuid: NetUid, neuron_uid: u16) {
        let neuron_index: usize = neuron_uid.into();
        Emission::<T>::mutate(netuid, |v| Self::set_element_at(v, neuron_index, 0.into()));
        Consensus::<T>::mutate(netuid, |v| {
            Self::set_element_at(v, neuron_index, PerU16::zero())
        });
        for mecid in 0..MechanismCountCurrent::<T>::get(netuid).into() {
            let netuid_index = Self::get_mechanism_storage_index(netuid, mecid.into());
            Incentive::<T>::mutate(netuid_index, |v| {
                Self::set_element_at(v, neuron_index, PerU16::zero())
            });
            ConsensusByMechanism::<T>::mutate_exists(netuid_index, |maybe_consensus| {
                if let Some(v) = maybe_consensus {
                    Self::set_element_at(v, neuron_index, PerU16::zero())
                }
            });
            Bonds::<T>::remove(netuid_index, neuron_uid); // Remove bonds for Validator.

            // Clear weights set BY the neuron_uid
            Weights::<T>::remove(netuid_index, neuron_uid);

            // Set weights FOR the neuron_uid to 0
            let all_uids: Vec<u16> = Weights::<T>::iter_key_prefix(netuid_index).collect();
            for uid in all_uids {
                Weights::<T>::mutate(netuid_index, uid, |weight_vec: &mut Vec<(u16, u16)>| {
                    for (weight_uid, w) in weight_vec.iter_mut() {
                        if *weight_uid == neuron_uid {
                            *w = 0;
                        }
                    }
                });
            }
        }
        Dividends::<T>::mutate(netuid, |v| {
            Self::set_element_at(v, neuron_index, PerU16::zero())
        });
        StakeWeight::<T>::mutate(netuid, |v| Self::set_element_at(v, neuron_index, 0));
        ValidatorTrust::<T>::mutate(netuid, |v| {
            Self::set_element_at(v, neuron_index, PerU16::zero())
        });
        ValidatorPermit::<T>::mutate(netuid, |v| Self::set_element_at(v, neuron_index, false));
    }

    /// Replace the neuron under this uid.
    pub fn replace_neuron(
        netuid: NetUid,
        uid_to_replace: u16,
        new_hotkey: &T::AccountId,
        block_number: u64,
    ) {
        log::debug!(
            "replace_neuron( netuid: {netuid:?} | uid_to_replace: {uid_to_replace:?} | new_hotkey: {new_hotkey:?} ) "
        );

        // 1. Get the old hotkey under this position.
        let old_hotkey: T::AccountId = Keys::<T>::get(netuid, uid_to_replace);

        // Do not replace owner hotkey from `SubnetOwnerHotkey`
        if let Ok(sn_owner_hotkey) = SubnetOwnerHotkey::<T>::try_get(netuid)
            && sn_owner_hotkey == old_hotkey.clone()
        {
            log::warn!(
                "replace_neuron: Skipped replacement because neuron is the subnet owner hotkey. \
                    netuid: {netuid:?}, uid_to_replace: {uid_to_replace:?}, new_hotkey: {new_hotkey:?}, owner_hotkey: {sn_owner_hotkey:?}"
            );
            return;
        }

        // Root churn: settle flushable pending basket credits while the hotkey is still
        // on root (earned dividends must deposit, not recycle). After membership drops,
        // a second flush recycles leftover sub-threshold dust the hotkey can no longer
        // merge by earning more.
        if netuid.is_root() {
            let _ = Self::flush_basket_deposits_for_hotkey(&old_hotkey);
            Self::clear_auto_parent_for_root_validator(&old_hotkey);
        }

        T::CommitmentsInterface::purge_neuron(netuid, &old_hotkey);

        // 2. Remove previous set memberships.
        Uids::<T>::remove(netuid, old_hotkey.clone());
        Self::remove_associated_evm_address(netuid, uid_to_replace);
        IsNetworkMember::<T>::remove(old_hotkey.clone(), netuid);
        #[allow(unknown_lints)]
        Keys::<T>::remove(netuid, uid_to_replace);

        if netuid.is_root() {
            let _ = Self::flush_basket_deposits_for_hotkey(&old_hotkey);
        }

        // 3. Create new set memberships.
        Self::set_active_for_uid(netuid, uid_to_replace, true); // Set to active by default.
        Keys::<T>::insert(netuid, uid_to_replace, new_hotkey.clone()); // Make hotkey - uid association.
        Uids::<T>::insert(netuid, new_hotkey.clone(), uid_to_replace); // Make uid - hotkey association.
        BlockAtRegistration::<T>::insert(netuid, uid_to_replace, block_number); // Fill block at registration.
        IsNetworkMember::<T>::insert(new_hotkey.clone(), netuid, true); // Fill network is member.
        // Drop a stale rename edge if this SS58 was previously swapped away.
        Self::clear_stale_hotkey_successor(netuid, new_hotkey);

        // 4. Clear neuron axons, certificates and prometheus info
        Axons::<T>::remove(netuid, &old_hotkey);
        NeuronCertificates::<T>::remove(netuid, &old_hotkey);
        Prometheus::<T>::remove(netuid, &old_hotkey);

        // 5. Reset new neuron's values.
        Self::clear_neuron(netuid, uid_to_replace);

        // 6. Replacement creates a new logical neuron at the reused UID, so the weights timing
        //    state should start from this registration block.
        for mecid in 0..MechanismCountCurrent::<T>::get(netuid).into() {
            let netuid_index = Self::get_mechanism_storage_index(netuid, mecid.into());
            Self::set_last_update_for_uid(netuid_index, uid_to_replace, block_number);
        }
    }

    /// Appends the uid to the network.
    pub fn append_neuron(netuid: NetUid, new_hotkey: &T::AccountId, block_number: u64) {
        // 1. Get the next uid. This is always equal to subnetwork_n.
        let next_uid: u16 = Self::get_subnetwork_n(netuid);
        log::debug!(
            "append_neuron( netuid: {netuid:?} | next_uid: {next_uid:?} | new_hotkey: {new_hotkey:?} ) "
        );

        // 2. Get and increase the uid count.
        SubnetworkN::<T>::insert(netuid, next_uid.saturating_add(1));

        // 3. Expand per-neuron vectors with new position.
        Active::<T>::mutate(netuid, |v| v.push(true));
        Emission::<T>::mutate(netuid, |v| v.push(0.into()));
        Consensus::<T>::mutate(netuid, |v| v.push(PerU16::zero()));
        for mecid in 0..MechanismCountCurrent::<T>::get(netuid).into() {
            let netuid_index = Self::get_mechanism_storage_index(netuid, mecid.into());
            Incentive::<T>::mutate(netuid_index, |v| v.push(PerU16::zero()));
            Self::set_last_update_for_uid(netuid_index, next_uid, block_number);
        }
        Dividends::<T>::mutate(netuid, |v| v.push(PerU16::zero()));
        ValidatorTrust::<T>::mutate(netuid, |v| v.push(PerU16::zero()));
        ValidatorPermit::<T>::mutate(netuid, |v| v.push(false));

        // 4. Insert new account information.
        Keys::<T>::insert(netuid, next_uid, new_hotkey.clone()); // Make hotkey - uid association.
        Uids::<T>::insert(netuid, new_hotkey.clone(), next_uid); // Make uid - hotkey association.
        BlockAtRegistration::<T>::insert(netuid, next_uid, block_number); // Fill block at registration.
        IsNetworkMember::<T>::insert(new_hotkey.clone(), netuid, true); // Fill network is member.
        // Drop a stale rename edge if this SS58 was previously swapped away.
        Self::clear_stale_hotkey_successor(netuid, new_hotkey);
    }

    #[frame_support::transactional]
    pub fn trim_to_max_allowed_uids(netuid: NetUid, max_n: u16) -> DispatchResult {
        ensure!(
            Self::get_epoch_consensus(netuid) != EpochConsensus::Null
                || Self::get_subnetwork_n(netuid).saturating_sub(max_n) <= NULL_PRUNING_BATCH,
            Error::<T>::InvalidValue
        );
        if Self::get_epoch_consensus(netuid) == EpochConsensus::Null
            && Self::get_subnetwork_n(netuid) > max_n
        {
            ensure!(
                Self::clear_null_pruning_commits(netuid).0,
                Error::<T>::InvalidValue
            );
        }
        Self::trim_uids_impl(netuid, max_n)?;
        NullPruningTarget::<T>::remove(netuid);
        Ok(())
    }

    /// Remove at most one batch toward the owner's explicit final target.
    #[frame_support::transactional]
    pub fn trim_null_uids_batch(netuid: NetUid, target: u16) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            Self::get_epoch_consensus(netuid) == EpochConsensus::Null,
            Error::<T>::InvalidValue
        );
        ensure!(
            target >= MinAllowedUids::<T>::get(netuid)
                && target <= MaxAllowedUids::<T>::get(netuid),
            Error::<T>::InvalidValue
        );
        let current = Self::get_subnetwork_n(netuid);
        if current > target {
            let owner = SubnetOwner::<T>::get(netuid);
            let owner_uids = BTreeSet::from_iter(Self::get_immune_owner_uids(netuid, &owner));
            let immune = (0..current)
                .filter(|uid| owner_uids.contains(uid) || Self::get_neuron_is_immune(netuid, *uid))
                .count() as u16;
            ensure!(
                Percent::from_rational(immune, target) < T::MaxImmuneUidsPercentage::get(),
                Error::<T>::TrimmingWouldExceedMaxImmunePercentage
            );
        }
        // A target equal to the current population also permits bounded queue
        // cancellation, without forcing the owner to deregister participants.
        let (clean, cleared_commits) = Self::clear_null_pruning_commits(netuid);
        if !clean {
            NullPruningTarget::<T>::insert(netuid, target);
            Self::deposit_event(Event::NullUidsPruningProgress {
                netuid,
                target,
                remaining: current,
                removed: 0,
                cleared_commits,
            });
            return Ok(());
        }
        let next = current.saturating_sub(NULL_PRUNING_BATCH).max(target);
        Self::trim_uids_impl(netuid, next)?;
        if Self::get_subnetwork_n(netuid) > target {
            NullPruningTarget::<T>::insert(netuid, target);
        } else {
            NullPruningTarget::<T>::remove(netuid);
        }
        let remaining = Self::get_subnetwork_n(netuid);
        Self::deposit_event(Event::NullUidsPruningProgress {
            netuid,
            target,
            remaining,
            removed: current.saturating_sub(remaining),
            cleared_commits,
        });
        Ok(())
    }

    /// Cancel destinations from the pre-compaction UID layout, with a shared
    /// key budget. Orphan legacy commits may outnumber today's registered UIDs;
    /// cleanup-only batches make progress without changing the UID layout.
    fn clear_null_pruning_commits(netuid: NetUid) -> (bool, u32) {
        let mut budget = u32::from(crate::subnets::mechanism::NULL_UID_BUDGET);
        let mut cleared = 0u32;
        for mechanism in 0..u8::from(Self::get_current_mechanism_count(netuid)) {
            let index = Self::get_mechanism_storage_index(netuid, mechanism.into());
            let keys = WeightCommits::<T>::iter_key_prefix(index)
                .take((budget as usize).saturating_add(1))
                .collect::<Vec<_>>();
            let has_more = keys.len() > budget as usize;
            for key in keys.into_iter().take(budget as usize) {
                WeightCommits::<T>::remove(index, key);
                budget = budget.saturating_sub(1);
                cleared = cleared.saturating_add(1);
            }
            if has_more {
                return (false, cleared);
            }
            let keys = TimelockedWeightCommits::<T>::iter_key_prefix(index)
                .take((budget as usize).saturating_add(1))
                .collect::<Vec<_>>();
            let has_more = keys.len() > budget as usize;
            for key in keys.into_iter().take(budget as usize) {
                TimelockedWeightCommits::<T>::remove(index, key);
                budget = budget.saturating_sub(1);
                cleared = cleared.saturating_add(1);
            }
            if has_more {
                return (false, cleared);
            }
        }
        (true, cleared)
    }

    fn trim_uids_impl(netuid: NetUid, max_n: u16) -> DispatchResult {
        // Reasonable limits
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            max_n >= MinAllowedUids::<T>::get(netuid),
            Error::<T>::InvalidValue
        );
        ensure!(
            max_n <= MaxAllowedUids::<T>::get(netuid),
            Error::<T>::InvalidValue
        );

        MaxAllowedUids::<T>::insert(netuid, max_n);

        let current_n = Self::get_subnetwork_n(netuid);
        let is_null = Self::get_epoch_consensus(netuid) == EpochConsensus::Null;
        if current_n > max_n {
            let owner = SubnetOwner::<T>::get(netuid);
            let owner_uids = BTreeSet::from_iter(Self::get_immune_owner_uids(netuid, &owner));

            // Count the number of immune UIDs
            let mut immune_count: u16 = 0;
            for uid in 0..current_n {
                if owner_uids.contains(&{ uid }) || Self::get_neuron_is_immune(netuid, uid) {
                    immune_count = immune_count.saturating_add(1);
                }
            }

            // Ensure the number of immune UIDs is less than 80%
            let immune_percentage = Percent::from_rational(immune_count, max_n);
            ensure!(
                immune_percentage < T::MaxImmuneUidsPercentage::get(),
                Error::<T>::TrimmingWouldExceedMaxImmunePercentage
            );

            // Get all emissions with their UIDs and sort by emission (descending)
            // This ensures we keep the highest emitters and remove the lowest ones
            let mut emissions = Emission::<T>::get(netuid)
                .into_iter()
                .enumerate()
                .collect::<Vec<_>>();
            emissions.sort_by_key(|(_, emission)| cmp::Reverse(*emission));

            let mut removed_uids = BTreeSet::new();
            let mut uids_left_to_process = current_n;
            let mechanisms_count = MechanismCountCurrent::<T>::get(netuid).into();

            // Iterate from the end (lowest emitters) to the beginning
            for i in (0..current_n).rev() {
                if uids_left_to_process == max_n {
                    break; // We've reached the target number of UIDs
                }

                if let Some((uid, _)) = emissions.get(i as usize).cloned() {
                    let neuron_uid = uid as u16;

                    // Skip subnet owner's or temporally immune uids
                    if owner_uids.contains(&neuron_uid)
                        || Self::get_neuron_is_immune(netuid, neuron_uid)
                    {
                        continue;
                    }

                    // Remove hotkey related storage items if hotkey exists
                    if let Ok(hotkey) = Keys::<T>::try_get(netuid, neuron_uid) {
                        T::CommitmentsInterface::purge_neuron(netuid, &hotkey);

                        // Same root-churn finalization as `replace_neuron`: deposit while
                        // still on root, then recycle leftover dust after membership drops.
                        if netuid.is_root() {
                            let _ = Self::flush_basket_deposits_for_hotkey(&hotkey);
                            Self::clear_auto_parent_for_root_validator(&hotkey);
                        }
                        Uids::<T>::remove(netuid, &hotkey);
                        IsNetworkMember::<T>::remove(&hotkey, netuid);
                        LastHotkeyEmissionOnNetuid::<T>::remove(&hotkey, netuid);
                        AlphaDividendsPerSubnet::<T>::remove(netuid, &hotkey);
                        Axons::<T>::remove(netuid, &hotkey);
                        NeuronCertificates::<T>::remove(netuid, &hotkey);
                        Prometheus::<T>::remove(netuid, &hotkey);
                        if netuid.is_root() {
                            let _ = Self::flush_basket_deposits_for_hotkey(&hotkey);
                        }
                    }

                    // Remove all storage items associated with this uid
                    #[allow(unknown_lints)]
                    Keys::<T>::remove(netuid, neuron_uid);
                    BlockAtRegistration::<T>::remove(netuid, neuron_uid);
                    Self::remove_associated_evm_address(netuid, neuron_uid);
                    for mecid in 0..mechanisms_count {
                        let netuid_index = Self::get_mechanism_storage_index(netuid, mecid.into());
                        Weights::<T>::remove(netuid_index, neuron_uid);
                        Bonds::<T>::remove(netuid_index, neuron_uid);
                    }

                    // Remove from emissions array and track as removed
                    emissions.remove(i.into());
                    removed_uids.insert(uid);
                    uids_left_to_process = uids_left_to_process.saturating_sub(1);
                }
            }

            ensure!(uids_left_to_process == max_n, Error::<T>::InvalidValue);

            // Sort remaining emissions by uid to compress uids to the left
            // This ensures consecutive uid indices in the final arrays
            emissions.sort_by_key(|(uid, _)| *uid);

            // Extract the final uids and emissions after trimming and sorting
            let (mut trimmed_uids, mut trimmed_emissions): (Vec<usize>, Vec<AlphaBalance>) =
                emissions.into_iter().unzip();
            if is_null {
                // Fill deleted holes from the tail. At most one batch of surviving
                // hotkeys changes UID; all other keys retain their existing UID.
                let mut original_emission = vec![AlphaBalance::ZERO; usize::from(current_n)];
                for (&uid, &emission) in trimmed_uids.iter().zip(&trimmed_emissions) {
                    if let Some(slot) = original_emission.get_mut(uid) {
                        *slot = emission;
                    }
                }
                let holes = (0..usize::from(max_n))
                    .filter(|uid| removed_uids.contains(uid))
                    .collect::<Vec<_>>();
                let tails = trimmed_uids
                    .iter()
                    .copied()
                    .filter(|uid| *uid >= usize::from(max_n))
                    .collect::<Vec<_>>();
                ensure!(holes.len() == tails.len(), Error::<T>::InvalidValue);
                trimmed_uids = (0..usize::from(max_n)).collect();
                for (hole, tail) in holes.into_iter().zip(tails) {
                    if let Some(slot) = trimmed_uids.get_mut(hole) {
                        *slot = tail;
                    }
                }
                trimmed_emissions = trimmed_uids
                    .iter()
                    .map(|uid| original_emission.get(*uid).copied().unwrap_or_default())
                    .collect();
            }

            // Get all current arrays from storage
            let active = Active::<T>::get(netuid);
            let consensus = Consensus::<T>::get(netuid);
            let dividends = Dividends::<T>::get(netuid);
            let vtrust = ValidatorTrust::<T>::get(netuid);
            let vpermit = ValidatorPermit::<T>::get(netuid);
            let stake_weight = StakeWeight::<T>::get(netuid);

            // Create trimmed arrays by extracting values for kept uids only
            // Pre-allocate vectors with exact capacity for efficiency
            let len = trimmed_uids.len();
            let mut trimmed_active = Vec::with_capacity(len);
            let mut trimmed_consensus = Vec::with_capacity(len);
            let mut trimmed_dividends = Vec::with_capacity(len);
            let mut trimmed_vtrust = Vec::with_capacity(len);
            let mut trimmed_vpermit = Vec::with_capacity(len);
            let mut trimmed_stake_weight = Vec::with_capacity(len);

            // Single iteration to extract values for all kept uids
            for &uid in &trimmed_uids {
                trimmed_active.push(active.get(uid).cloned().unwrap_or_default());
                trimmed_consensus.push(consensus.get(uid).cloned().unwrap_or_default());
                trimmed_dividends.push(dividends.get(uid).cloned().unwrap_or_default());
                trimmed_vtrust.push(vtrust.get(uid).cloned().unwrap_or_default());
                trimmed_vpermit.push(vpermit.get(uid).cloned().unwrap_or_default());
                trimmed_stake_weight.push(stake_weight.get(uid).cloned().unwrap_or_default());
            }

            // Update storage with trimmed arrays
            Emission::<T>::insert(netuid, trimmed_emissions);
            Active::<T>::insert(netuid, trimmed_active);
            Consensus::<T>::insert(netuid, trimmed_consensus);
            Dividends::<T>::insert(netuid, trimmed_dividends);
            ValidatorTrust::<T>::insert(netuid, trimmed_vtrust);
            ValidatorPermit::<T>::insert(netuid, trimmed_vpermit);
            StakeWeight::<T>::insert(netuid, trimmed_stake_weight);

            // Update mechanism-scoped vectors
            for mecid in 0..mechanisms_count {
                let netuid_index = Self::get_mechanism_storage_index(netuid, mecid.into());
                let incentive = Incentive::<T>::get(netuid_index);
                let maybe_consensus = ConsensusByMechanism::<T>::try_get(netuid_index).ok();
                let lastupdate = LastUpdate::<T>::get(netuid_index);
                let mut trimmed_incentive = Vec::with_capacity(trimmed_uids.len());
                let mut trimmed_consensus = maybe_consensus
                    .as_ref()
                    .map(|_| Vec::with_capacity(trimmed_uids.len()));
                let mut trimmed_lastupdate = Vec::with_capacity(trimmed_uids.len());

                for uid in &trimmed_uids {
                    trimmed_incentive.push(incentive.get(*uid).cloned().unwrap_or_default());
                    if let (Some(consensus), Some(trimmed)) =
                        (maybe_consensus.as_ref(), trimmed_consensus.as_mut())
                    {
                        trimmed.push(consensus.get(*uid).cloned().unwrap_or_default());
                    }
                    trimmed_lastupdate.push(lastupdate.get(*uid).cloned().unwrap_or_default());
                }

                Incentive::<T>::insert(netuid_index, trimmed_incentive);
                if let Some(trimmed_consensus) = trimmed_consensus {
                    ConsensusByMechanism::<T>::insert(netuid_index, trimmed_consensus);
                }
                LastUpdate::<T>::insert(netuid_index, trimmed_lastupdate);
            }

            // Create mapping from old uid to new compressed uid
            // This is needed to update connections (weights and bonds) with correct uid references
            let old_to_new_uid: BTreeMap<usize, usize> = trimmed_uids
                .iter()
                .enumerate()
                .map(|(new_uid, &old_uid)| (old_uid, new_uid))
                .collect();

            // Update connections (weights and bonds) for each kept uid
            // This involves three operations per uid:
            // 1. Swap the uid storage to the new compressed position
            // 2. Update all connections to reference the new compressed uids
            // 3. Clear the connections to the trimmed uids
            for (old_uid, new_uid) in &old_to_new_uid {
                let old_neuron_uid = *old_uid as u16;
                let new_neuron_uid = *new_uid as u16;

                if is_null {
                    if old_neuron_uid != new_neuron_uid {
                        Keys::<T>::swap(netuid, old_neuron_uid, netuid, new_neuron_uid);
                        AssociatedEvmAddress::<T>::swap(
                            netuid,
                            old_neuron_uid,
                            netuid,
                            new_neuron_uid,
                        );
                        BlockAtRegistration::<T>::swap(
                            netuid,
                            old_neuron_uid,
                            netuid,
                            new_neuron_uid,
                        );
                        let hotkey = Keys::<T>::get(netuid, new_neuron_uid);
                        Uids::<T>::insert(netuid, hotkey, new_neuron_uid);
                    }
                    continue;
                }

                // Swap uid specific storage items to new compressed positions
                Keys::<T>::swap(netuid, old_neuron_uid, netuid, new_neuron_uid);
                AssociatedEvmAddress::<T>::swap(netuid, old_neuron_uid, netuid, new_neuron_uid);
                BlockAtRegistration::<T>::swap(netuid, old_neuron_uid, netuid, new_neuron_uid);

                for mecid in 0..mechanisms_count {
                    let netuid_index = Self::get_mechanism_storage_index(netuid, mecid.into());

                    // Swap to new position and remap all target uids
                    Weights::<T>::swap(netuid_index, old_neuron_uid, netuid_index, new_neuron_uid);
                    Weights::<T>::mutate(netuid_index, new_neuron_uid, |weights| {
                        weights.retain_mut(|(target_uid, _weight)| {
                            if let Some(new_target_uid) =
                                old_to_new_uid.get(&(*target_uid as usize))
                            {
                                *target_uid = *new_target_uid as u16;
                                true
                            } else {
                                false
                            }
                        })
                    });

                    // Swap to new position and remap all target uids
                    Bonds::<T>::swap(netuid_index, old_neuron_uid, netuid_index, new_neuron_uid);
                    Bonds::<T>::mutate(netuid_index, new_neuron_uid, |bonds| {
                        bonds.retain_mut(|(target_uid, _bond)| {
                            if let Some(new_target_uid) =
                                old_to_new_uid.get(&(*target_uid as usize))
                            {
                                *target_uid = *new_target_uid as u16;
                                true
                            } else {
                                false
                            }
                        })
                    });
                }
            }

            Self::remap_associated_evm_address_index(netuid, &old_to_new_uid);

            if is_null {
                Self::remap_null_pruning_rows(netuid, current_n, &old_to_new_uid, &vpermit)?;
            } else {
                // Clear the UID map for the subnet
                let clear_result = Uids::<T>::clear_prefix(netuid, u32::MAX, None);
                // Shouldn't happen, but possible.
                ensure!(
                    clear_result.maybe_cursor.is_none(),
                    Error::<T>::UidMapCouldNotBeCleared
                );

                // Insert the new UIDs
                for new_uid in old_to_new_uid.values() {
                    // Get the hotkey using Keys map and new UID.
                    let hotkey = Keys::<T>::get(netuid, *new_uid as u16);
                    Uids::<T>::insert(netuid, hotkey, *new_uid as u16);
                }
            }

            // Update the subnet's uid count to reflect the new maximum
            SubnetworkN::<T>::insert(netuid, max_n);
            if is_null {
                let (stakes, _, _) = Self::get_null_stake_weights_for_network(netuid);
                let winner = Self::null_validator_winner(&stakes);
                ValidatorPermit::<T>::insert(
                    netuid,
                    (0..stakes.len())
                        .map(|uid| Some(uid) == winner)
                        .collect::<Vec<_>>(),
                );
            }
        }

        Ok(())
    }

    /// Explicit pruning invalidates queued reveals, which contain pre-compaction
    /// UID destinations. Drop historical nonpermit rows without decoding them;
    /// preserve/remap the elected row and the bounded frozen Yuma bond state.
    fn remap_null_pruning_rows(
        netuid: NetUid,
        old_n: u16,
        mapping: &BTreeMap<usize, usize>,
        permits: &[bool],
    ) -> DispatchResult {
        // Count changes in Null may redistribute capacity, but all frozen
        // Yuma rows together remain within the previous shared Yuma budget.
        let frozen_limit = Self::epoch_uid_budget(EpochConsensus::Yuma);
        let mut frozen_rows_seen = 0usize;
        let mut targets = vec![None; usize::from(old_n)];
        for (&old, &new) in mapping {
            if let Some(slot) = targets.get_mut(old) {
                *slot = Some(new as u16);
            }
        }
        for mechanism in 0..u8::from(Self::get_current_mechanism_count(netuid)) {
            let index = Self::get_mechanism_storage_index(netuid, mechanism.into());
            let keys = Weights::<T>::iter_key_prefix(index)
                .take(usize::from(old_n).saturating_add(1))
                .collect::<Vec<_>>();
            ensure!(keys.len() <= usize::from(old_n), Error::<T>::InvalidValue);
            for old in keys {
                let destination = targets.get(usize::from(old)).copied().flatten();
                if !permits.get(usize::from(old)).copied().unwrap_or(false) || destination.is_none()
                {
                    Weights::<T>::remove(index, old);
                    continue;
                }
                let new = destination.ok_or(Error::<T>::InvalidValue)?;
                let original = Weights::<T>::get(index, old);
                let remapped = original
                    .iter()
                    .filter_map(|(target, weight)| {
                        targets
                            .get(usize::from(*target))
                            .copied()
                            .flatten()
                            .map(|uid| (uid, *weight))
                    })
                    .collect::<Vec<_>>();
                if new != old {
                    Weights::<T>::remove(index, old);
                }
                if new != old || remapped != original {
                    Weights::<T>::insert(index, new, remapped);
                }
            }
            let bonds = Bonds::<T>::iter_key_prefix(index)
                .take(usize::from(frozen_limit).saturating_add(1))
                .collect::<Vec<_>>();
            frozen_rows_seen = frozen_rows_seen.saturating_add(bonds.len());
            ensure!(
                frozen_rows_seen <= usize::from(frozen_limit),
                Error::<T>::InvalidValue
            );
            for old in bonds {
                let original = Bonds::<T>::get(index, old);
                ensure!(
                    original.len() <= usize::from(frozen_limit),
                    Error::<T>::InvalidValue
                );
                let Some(new) = targets.get(usize::from(old)).copied().flatten() else {
                    Bonds::<T>::remove(index, old);
                    continue;
                };
                let remapped = original
                    .iter()
                    .filter_map(|(target, weight)| {
                        targets
                            .get(usize::from(*target))
                            .copied()
                            .flatten()
                            .map(|uid| (uid, *weight))
                    })
                    .collect::<Vec<_>>();
                if new != old {
                    Bonds::<T>::remove(index, old);
                }
                if new != old || remapped != original {
                    Bonds::<T>::insert(index, new, remapped);
                }
            }
        }
        Ok(())
    }

    /// Returns true if the uid is set on the network.
    ///
    pub fn is_uid_exist_on_network(netuid: NetUid, uid: u16) -> bool {
        Keys::<T>::contains_key(netuid, uid)
    }

    /// Returns true if the hotkey holds a slot on the network.
    ///
    pub fn is_hotkey_registered_on_network(netuid: NetUid, hotkey: &T::AccountId) -> bool {
        Uids::<T>::contains_key(netuid, hotkey)
    }

    /// Returs the hotkey under the network uid as a Result. Ok if the uid is taken.
    ///
    pub fn get_hotkey_for_net_and_uid(
        netuid: NetUid,
        neuron_uid: u16,
    ) -> Result<T::AccountId, DispatchError> {
        Keys::<T>::try_get(netuid, neuron_uid)
            .map_err(|_err| Error::<T>::HotKeyNotRegisteredInSubNet.into())
    }

    /// Returns the uid of the hotkey in the network as a Result. Ok if the hotkey has a slot.
    ///
    pub fn get_uid_for_net_and_hotkey(
        netuid: NetUid,
        hotkey: &T::AccountId,
    ) -> Result<u16, DispatchError> {
        Uids::<T>::try_get(netuid, hotkey)
            .map_err(|_err| Error::<T>::HotKeyNotRegisteredInSubNet.into())
    }

    /// Returns the stake of the uid on network or 0 if it doesnt exist.
    ///
    pub fn get_stake_for_uid_and_subnetwork(netuid: NetUid, neuron_uid: u16) -> AlphaBalance {
        if let Ok(hotkey) = Self::get_hotkey_for_net_and_uid(netuid, neuron_uid) {
            Self::get_stake_for_hotkey_on_subnet(&hotkey, netuid)
        } else {
            AlphaBalance::ZERO
        }
    }

    /// Return a list of all networks a hotkey is registered on.
    ///
    pub fn get_registered_networks_for_hotkey(hotkey: &T::AccountId) -> Vec<NetUid> {
        let mut all_networks: Vec<NetUid> = vec![];
        for (network, is_registered) in <IsNetworkMember<T> as IterableStorageDoubleMap<
            T::AccountId,
            NetUid,
            bool,
        >>::iter_prefix(hotkey)
        {
            if is_registered {
                all_networks.push(network)
            }
        }
        all_networks
    }

    /// Return true if a hotkey is registered on any network.
    ///
    pub fn is_hotkey_registered_on_any_network(hotkey: &T::AccountId) -> bool {
        for (_, is_registered) in <IsNetworkMember<T> as IterableStorageDoubleMap<
            T::AccountId,
            NetUid,
            bool,
        >>::iter_prefix(hotkey)
        {
            if is_registered {
                return true;
            }
        }
        false
    }

    /// Return true if a hotkey is registered on specific network.
    ///
    pub fn is_hotkey_registered_on_specific_network(hotkey: &T::AccountId, netuid: NetUid) -> bool {
        IsNetworkMember::<T>::contains_key(hotkey, netuid)
    }
}
