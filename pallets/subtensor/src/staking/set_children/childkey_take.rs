//! Childkey take (fee) getters/setters and auto-parent-delegation flag.
use super::*;
use sp_runtime::PerU16;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    pub fn get_children(hotkey: &T::AccountId, netuid: NetUid) -> Vec<(u64, T::AccountId)> {
        ChildKeys::<T>::get(hotkey, netuid)
    }

    pub fn get_parents(child: &T::AccountId, netuid: NetUid) -> Vec<(u64, T::AccountId)> {
        ParentKeys::<T>::get(child, netuid)
    }

    /// Sets the childkey take for a given hotkey.
    ///
    /// This function allows a coldkey to set the childkey take for a given hotkey.
    /// The childkey take determines the proportion of stake that the hotkey keeps for itself
    /// when distributing stake to its children.
    ///
    /// # Arguments
    /// * `coldkey`: The coldkey that owns the hotkey.
    ///
    /// * `hotkey`: The hotkey for which the childkey take will be set.
    ///
    /// * `take`: The new childkey take value. This is a ratio represented in parts per 65535,
    ///   where 65535 represents 100%.
    ///
    /// # Returns
    /// * `DispatchResult`: The result of the operation.
    ///
    /// # Errors
    /// * `NonAssociatedColdKey`: The coldkey does not own the hotkey.
    /// * `InvalidChildkeyTake`: The provided take value is invalid (greater than the maximum allowed take).
    /// * `TxChildkeyTakeRateLimitExceeded`: The rate limit for changing childkey take has been exceeded.
    pub fn do_set_childkey_take(
        coldkey: T::AccountId,
        hotkey: T::AccountId,
        netuid: NetUid,
        take: PerU16,
    ) -> DispatchResult {
        // Ensure the coldkey owns the hotkey
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        ensure!(Self::subnet_exists(netuid), Error::<T>::SubnetNotExists);

        // Ensure the take value is valid
        ensure!(
            take.deconstruct() >= Self::get_effective_min_childkey_take(netuid)
                && take.deconstruct() <= Self::get_max_childkey_take(),
            Error::<T>::InvalidChildkeyTake
        );

        let current_take = Self::get_childkey_take(&hotkey, netuid);
        // Check the rate limit for increasing childkey take case
        if take.deconstruct() > current_take {
            // Ensure the hotkey passes the rate limit.
            ensure!(
                TransactionType::SetChildkeyTake.passes_rate_limit_on_subnet::<T>(
                    &hotkey, // Specific to a hotkey.
                    netuid,  // Specific to a subnet.
                ),
                Error::<T>::TxChildkeyTakeRateLimitExceeded
            );
        }

        // Set last transaction block
        let current_block = Self::get_current_block_as_u64();
        TransactionType::SetChildkeyTake.set_last_block_on_subnet::<T>(
            &hotkey,
            netuid,
            current_block,
        );

        // Set the new childkey take value for the given hotkey and network
        ChildkeyTake::<T>::insert(hotkey.clone(), netuid, take);

        // Update the last transaction block
        TransactionType::SetChildkeyTake.set_last_block_on_subnet::<T>(
            &hotkey,
            netuid,
            current_block,
        );

        // Emit the event
        Self::deposit_event(Event::ChildKeyTakeSet(hotkey.clone(), take));
        log::debug!("Childkey take set for hotkey: {hotkey:?} and take: {take:?}");
        Ok(())
    }

    /// Gets the childkey take for a given hotkey.
    ///
    /// This function retrieves the current childkey take value for a specified hotkey.
    /// If no specific take value has been set, it returns the default childkey take.
    ///
    /// # Arguments
    /// * `hotkey` (&T::AccountId): The hotkey for which to retrieve the childkey take.
    ///
    /// # Returns
    /// * `u16`: The childkey take value, scaled so `u16::MAX` represents 100%.
    pub fn get_childkey_take(hotkey: &T::AccountId, netuid: NetUid) -> u16 {
        ChildkeyTake::<T>::get(hotkey, netuid)
            .deconstruct()
            .max(Self::get_effective_min_childkey_take(netuid))
    }

    pub fn get_auto_parent_delegation_enabled(root_validator_hotkey: &T::AccountId) -> bool {
        AutoParentDelegationEnabled::<T>::get(root_validator_hotkey)
    }
}
