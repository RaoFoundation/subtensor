//! Whitelisted, rate-limited TAO transfers for the `SmallTransfer` proxy.
//!
//! The proxy filter is a pure function of the call, so the per-coldkey state a
//! `SmallTransfer` delegate must respect (the allowed destination and the
//! one-call-per-block limit) is checked here, inside the only call that proxy
//! type may dispatch. `pallet_proxy` dispatches as the delegating coldkey, so
//! every check below is keyed on that coldkey, not on the delegate.
use super::*;
use frame_support::{
    dispatch, ensure,
    traits::{fungible::Mutate, tokens::Preservation},
};
use frame_system::{ensure_signed, pallet_prelude::OriginFor};
use subtensor_runtime_common::SMALL_TRANSFER_LIMIT;

impl<T: Config> Pallet<T> {
    /// Sets (or clears, with `None`) the single account `small_transfer` may
    /// pay from the signer's balance.
    pub fn do_set_small_transfer_destination(
        origin: OriginFor<T>,
        destination: Option<T::AccountId>,
    ) -> dispatch::DispatchResult {
        let coldkey = ensure_signed(origin)?;

        match &destination {
            Some(destination) => SmallTransferDestination::<T>::insert(&coldkey, destination),
            None => SmallTransferDestination::<T>::remove(&coldkey),
        }

        Self::deposit_event(Event::SmallTransferDestinationSet {
            coldkey,
            destination,
        });
        Ok(())
    }

    /// Transfers `amount` TAO (strictly below [`SMALL_TRANSFER_LIMIT`]) from the
    /// signer to its whitelisted destination, at most once per block.
    pub fn do_small_transfer(
        origin: OriginFor<T>,
        destination: T::AccountId,
        amount: TaoBalance,
    ) -> dispatch::DispatchResult {
        let coldkey = ensure_signed(origin)?;

        ensure!(
            amount < SMALL_TRANSFER_LIMIT,
            Error::<T>::SmallTransferAmountTooHigh
        );
        ensure!(
            SmallTransferDestination::<T>::get(&coldkey).as_ref() == Some(&destination),
            Error::<T>::SmallTransferDestinationNotAllowed
        );
        ensure!(
            TransactionType::SmallTransfer.passes_rate_limit::<T>(&coldkey),
            Error::<T>::SmallTransferRateLimitExceeded
        );

        <T as Config>::Currency::transfer(&coldkey, &destination, amount, Preservation::Preserve)?;

        TransactionType::SmallTransfer.set_last_block_on_subnet::<T>(
            &coldkey,
            NetUid::ROOT,
            Self::get_current_block_as_u64(),
        );
        Ok(())
    }
}
