use sp_runtime::PerU16;

use super::*;

impl<T: Config> Pallet<T> {
    /// The implementation for the extrinsic decrease_take
    ///
    /// # Arguments
    /// * `origin`: The signature of the caller's coldkey.
    ///
    /// * `hotkey`: The hotkey we are delegating (must be owned by the coldkey).
    ///
    /// * `take`: The stake proportion that this hotkey takes from delegations for subnet ID.
    ///
    /// # Events
    /// * `TakeDecreased`: On successfully setting a decreased take for this hotkey.
    ///
    /// # Errors
    /// * `NotRegistered`: The hotkey we are delegating is not registered on the network.
    ///
    /// * `NonAssociatedColdKey`: The hotkey we are delegating is not owned by the calling coldket.
    ///
    /// * `DelegateTakeTooLow`: The delegate is setting a take which is not lower than the previous.
    ///
    pub fn do_decrease_take(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        take: PerU16,
    ) -> dispatch::DispatchResult {
        // --- 1. We check the coldkey signature.
        let coldkey = ensure_signed(origin)?;
        log::debug!("do_decrease_take( origin:{coldkey:?} hotkey:{hotkey:?}, take:{take:?} )");

        // --- 2. Ensure we are delegating a known key.
        //        Ensure that the coldkey is the owner.
        Self::do_take_checks(&coldkey, &hotkey)?;

        // --- 3. Ensure we are always strictly decreasing, never increasing take
        if let Ok(current_take) = Delegates::<T>::try_get(&hotkey) {
            ensure!(take < current_take, Error::<T>::DelegateTakeTooLow);
        }

        // --- 3.1 Ensure take is within the min ..= InitialDefaultDelegateTake (18%) range
        let min_take = MinDelegateTake::<T>::get();
        ensure!(take >= min_take, Error::<T>::DelegateTakeTooLow);

        // --- 4. Set the new take value.
        Delegates::<T>::insert(hotkey.clone(), take);

        // --- 5. Set last block for rate limiting
        let block: u64 = Self::get_current_block_as_u64();
        Self::set_last_tx_block_delegate_take(&hotkey, block);

        // --- 6. Emit the take value.
        log::debug!("TakeDecreased( coldkey:{coldkey:?}, hotkey:{hotkey:?}, take:{take:?} )");
        Self::deposit_event(Event::TakeDecreased(coldkey, hotkey, take));

        // --- 6. Ok and return.
        Ok(())
    }
}
