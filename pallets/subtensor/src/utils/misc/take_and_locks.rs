//! Delegate-take ownership checks and subnet locked-balance / recycle-or-burn helpers.
use super::*;
use crate::Error;
use sp_runtime::Saturating;
use subtensor_runtime_common::{NetUid, TaoBalance};

impl<T: Config> Pallet<T> {
    // ========================
    // ===== Take checks ======
    // ========================
    pub fn do_take_checks(coldkey: &T::AccountId, hotkey: &T::AccountId) -> Result<(), Error<T>> {
        // Ensure we are delegating a known key.
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure that the coldkey is the owner.
        ensure!(
            Self::coldkey_owns_hotkey(coldkey, hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        Ok(())
    }

    // ========================
    // === Token Management ===
    // ========================
    pub fn set_subnet_locked_balance(netuid: NetUid, amount: TaoBalance) {
        SubnetLocked::<T>::insert(netuid, amount);
    }
    pub fn get_subnet_locked_balance(netuid: NetUid) -> TaoBalance {
        SubnetLocked::<T>::get(netuid)
    }
    pub fn get_total_subnet_locked() -> TaoBalance {
        let mut total_subnet_locked: u64 = 0;
        for (_, locked) in SubnetLocked::<T>::iter() {
            total_subnet_locked.saturating_accrue(locked.into());
        }
        total_subnet_locked.into()
    }

    pub fn set_recycle_or_burn(netuid: NetUid, recycle_or_burn: RecycleOrBurnEnum) {
        RecycleOrBurn::<T>::insert(netuid, recycle_or_burn);
    }
}
