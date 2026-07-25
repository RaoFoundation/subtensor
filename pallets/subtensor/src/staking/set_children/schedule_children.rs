//! Schedule and apply pending child-key sets (`PendingChildKeys`).
use super::*;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// The implementation for the extrinsic do_set_child_singular: Sets a single child.
    /// This function allows a coldkey to set children keys.
    ///
    /// Adds a childkey vector to the PendingChildKeys map and performs a few checks:
    ///    **Signature Verification**: Ensures that the caller has signed the transaction, verifying the coldkey.
    ///    **Root Network Check**: Ensures that the delegation is not on the root network, as child hotkeys are not valid on the root.
    ///    **Network Existence Check**: Ensures that the specified network exists.
    ///    **Ownership Verification**: Ensures that the coldkey owns the hotkey.
    ///    **Hotkey Account Existence Check**: Ensures that the hotkey account already exists.
    ///    **Child count**: Only allow to add up to 5 children per parent
    ///    **Child-Hotkey Distinction**: Ensures that the child is not the same as the hotkey.
    ///    **Minimum stake**: Ensures that the parent key has at least the minimum stake.
    ///    **Proportion check**: Ensure that the sum of the proportions does not exceed u64::MAX.
    ///    **Duplicate check**: Ensure there are no duplicates in the list of children.
    ///
    /// # Events
    /// * `SetChildrenScheduled`: If all checks pass and setting the childkeys is scheduled.
    ///
    /// # Errors
    /// * `MechanismDoesNotExist`: Attempting to register to a non-existent network.
    /// * `RegistrationNotPermittedOnRootSubnet`: Attempting to register a child on the root network.
    /// * `NonAssociatedColdKey`: The coldkey does not own the hotkey or the child is the same as the hotkey.
    /// * `HotKeyAccountNotExists`: The hotkey account does not exist.
    /// * `TooManyChildren`: Too many children in request.
    ///
    pub fn do_schedule_children(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        children: Vec<(u64, T::AccountId)>,
    ) -> DispatchResult {
        // Check that the caller has signed the transaction. (the coldkey of the pairing)
        let coldkey = ensure_signed(origin)?;
        log::trace!(
            "do_set_children( coldkey:{coldkey:?} hotkey:{netuid:?} netuid:{hotkey:?} children:{children:?} )"
        );

        // Ensure the hotkey passes the rate limit.
        ensure!(
            TransactionType::SetChildren.passes_rate_limit_on_subnet::<T>(
                &hotkey, // Specific to a hotkey.
                netuid,  // Specific to a subnet.
            ),
            Error::<T>::TxRateLimitExceeded
        );

        // Check that this delegation is not on the root network. Child hotkeys are not valid on root.
        ensure!(
            !netuid.is_root(),
            Error::<T>::RegistrationNotPermittedOnRootSubnet
        );

        // Check that the network we are trying to create the child on exists.
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        // Check that the coldkey owns the hotkey.
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        // Ensure there are no duplicates in the list of children.
        let mut unique_children = Vec::new();
        for (_, child_i) in &children {
            ensure!(
                !unique_children.contains(child_i),
                Error::<T>::DuplicateChild
            );
            unique_children.push(child_i.clone());
        }

        // Ensure we don't break consistency when these new childkeys are set:
        //  - Ensure that the number of children does not exceed 5
        //  - Each child is not the hotkey.
        //  - The sum of the proportions does not exceed u64::MAX.
        //  - Bipartite separation (no A <-> B relations)
        let relations = Self::load_child_parent_relations(&hotkey, netuid)?;
        relations.ensure_pending_consistency(&children)?;

        // Check that the parent key has at least the minimum own stake
        // if children vector is not empty
        // (checking with check_weights_min_stake wouldn't work because it considers
        // grandparent stake in this case)
        ensure!(
            children.is_empty()
                || Self::get_total_stake_for_hotkey(&hotkey) >= StakeThreshold::<T>::get().into()
                || SubnetOwnerHotkey::<T>::try_get(netuid)
                    .is_ok_and(|owner_hotkey| owner_hotkey.eq(&hotkey)),
            Error::<T>::NotEnoughStakeToSetChildkeys
        );

        // Set last transaction block
        let current_block = Self::get_current_block_as_u64();
        TransactionType::SetChildren.set_last_block_on_subnet::<T>(&hotkey, netuid, current_block);

        // Schedule or immediately apply CK
        Self::schedule_or_apply_ck(netuid, hotkey, children)
    }

    /// If the start call occured, schedule children, otherwise,
    /// apply immediately
    pub(crate) fn schedule_or_apply_ck(
        netuid: NetUid,
        hotkey: T::AccountId,
        children: Vec<(u64, T::AccountId)>,
    ) -> DispatchResult {
        if !SubtokenEnabled::<T>::get(netuid) {
            Self::persist_pending_children_ok(netuid, &hotkey, &children);
            return Ok(());
        }

        // Calculate cool-down block
        let cooldown_block =
            Self::get_current_block_as_u64().saturating_add(PendingChildKeyCooldown::<T>::get());

        // Insert or update PendingChildKeys
        PendingChildKeys::<T>::insert(netuid, hotkey.clone(), (children.clone(), cooldown_block));

        // Log and return.
        log::trace!(
            "SetChildrenScheduled( netuid:{:?}, cooldown_block:{:?}, hotkey:{:?}, children:{:?} )",
            cooldown_block,
            hotkey,
            netuid,
            children.clone()
        );
        Self::deposit_event(Event::SetChildrenScheduled(
            hotkey,
            netuid,
            cooldown_block,
            children,
        ));

        // Ok and return.
        Ok(())
    }

    /// This function executes setting children keys when called during hotkey draining.
    ///
    /// * `netuid`: The u16 network identifier where the child keys will exist.
    ///
    /// # Events
    /// * `SetChildren`: On successfully registering children to a hotkey.
    ///
    /// # Errors
    /// * `MechanismDoesNotExist`: Attempting to register to a non-existent network.
    /// * `RegistrationNotPermittedOnRootSubnet`: Attempting to register a child on the root network.
    /// * `NonAssociatedColdKey`: The coldkey does not own the hotkey or the child is the same as the hotkey.
    /// * `HotKeyAccountNotExists`: The hotkey account does not exist.
    ///
    /// # Note
    /// 1. **Old Children Cleanup**: Removes the hotkey from the parent list of its old children.
    /// 2. **New Children Assignment**: Assigns the new child to the hotkey and updates the parent list for the new child.
    ///
    pub fn do_set_pending_children(netuid: NetUid) {
        let current_block = Self::get_current_block_as_u64();

        // If the childkey cools down before the subnet start call + PendingChildKeyCooldown:
        //   - If Start call happened: Normal track
        //   - If Start call didn't happen: Apply immediately
        // TODO: This check may be removed after all ck are applied after the runtime upgrade
        let start_call_occured = SubtokenEnabled::<T>::get(netuid);

        // Iterate over all pending children of this subnet and set as needed
        let mut to_remove: Vec<T::AccountId> = Vec::new();

        PendingChildKeys::<T>::iter_prefix(netuid).for_each(
            |(hotkey, (children, cool_down_block))| {
                if (cool_down_block < current_block) || !start_call_occured {
                    Self::persist_pending_children_ok(netuid, &hotkey, &children);
                    to_remove.push(hotkey);
                }
            },
        );

        for hotkey in to_remove {
            PendingChildKeys::<T>::remove(netuid, hotkey);
        }
    }

    // If child-parent consistency is broken, fail setting new children silently
    pub(crate) fn persist_pending_children_ok(
        netuid: NetUid,
        hotkey: &T::AccountId,
        children: &Vec<(u64, T::AccountId)>,
    ) {
        let maybe_relations = Self::load_relations_from_pending(hotkey.clone(), children, netuid);
        if let Ok(relations) = maybe_relations {
            let mut _weight: Weight = T::DbWeight::get().reads(0);
            if let Ok(()) = Self::persist_child_parent_relations(relations, netuid, &mut _weight) {
                // Log and emit event.
                log::trace!(
                    "SetChildren( netuid:{:?}, hotkey:{:?}, children:{:?} )",
                    hotkey,
                    netuid,
                    children.clone()
                );
                Self::deposit_event(Event::SetChildren(hotkey.clone(), netuid, children.clone()));
            }
        }
    }
}
