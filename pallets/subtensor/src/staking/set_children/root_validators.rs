//! Auto-schedule childkeys for root validators onto a non-root subnet.
use super::*;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    ////////////////////////////////////////////////////////////
    // State cleaners (for use in migration)
    // TODO: Deprecate when the state is clean for a while

    /// Establishes parent-child relationships between all root validators and
    /// a subnet owner's hotkey on the specified subnet.
    ///
    /// For each validator on the root network (netuid 0), this function calls
    /// `do_schedule_children` to schedule the subnet owner hotkey as a child
    /// of that root validator on the given subnet, with full proportion (u64::MAX).
    ///
    /// # Arguments
    /// * `netuid`: The subnet on which to establish relationships.
    ///
    /// # Returns
    /// * `DispatchResult`: Ok if at least the setup completes; individual
    ///   scheduling failures per validator are logged but do not abort the loop.
    pub fn do_set_root_validators_for_subnet(netuid: NetUid) -> DispatchResult {
        // Cannot set children on root network itself.
        ensure!(
            !netuid.is_root(),
            Error::<T>::RegistrationNotPermittedOnRootSubnet
        );

        // Subnet must exist.
        ensure!(Self::subnet_exists(netuid), Error::<T>::SubnetNotExists);

        // Get the subnet owner hotkey.
        let subnet_owner_hotkey =
            SubnetOwnerHotkey::<T>::try_get(netuid).map_err(|_| Error::<T>::SubnetNotExists)?;

        // Iterate over all root validators and schedule each one as a parent
        // of the subnet owner hotkey.
        for (_uid, root_validator_hotkey) in Keys::<T>::iter_prefix(NetUid::ROOT) {
            // Skip if the root validator is the subnet owner hotkey itself
            // (cannot be both parent and child).
            if root_validator_hotkey == subnet_owner_hotkey {
                continue;
            }

            // Skip if root validator disabled auto parent delegation via AutoParentDelegationEnabled flag
            if !Self::get_auto_parent_delegation_enabled(&root_validator_hotkey) {
                continue;
            }

            // Look up the coldkey that owns this root validator hotkey.
            let coldkey = Self::get_owning_coldkey_for_hotkey(&root_validator_hotkey);

            // Build a signed origin from the coldkey.
            let origin: <T as frame_system::Config>::RuntimeOrigin =
                frame_system::RawOrigin::Signed(coldkey).into();

            // Schedule the subnet owner hotkey as a child with full proportion.
            let children = vec![(u64::MAX, subnet_owner_hotkey.clone())];

            if let Err(e) =
                Self::do_schedule_children(origin, root_validator_hotkey.clone(), netuid, children)
            {
                log::warn!(
                    "Failed to schedule children for root validator {:?} on netuid {:?}: {:?}",
                    root_validator_hotkey,
                    netuid,
                    e
                );
            }
        }

        Ok(())
    }
}
