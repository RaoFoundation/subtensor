//! Lock storage helpers: locking-coldkey index, accept-locked-alpha flags, and
//! read/write of [`super::ConvictionModel`] into `Lock` / aggregate maps.
use super::*;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    pub fn add_locking_coldkey(hotkey: &T::AccountId, netuid: NetUid, coldkey: &T::AccountId) {
        LockingColdkeys::<T>::insert((netuid, hotkey, coldkey), ());
    }

    pub fn maybe_remove_locking_coldkey(
        hotkey: &T::AccountId,
        netuid: NetUid,
        coldkey: &T::AccountId,
    ) {
        LockingColdkeys::<T>::remove((netuid, hotkey, coldkey));
    }

    pub fn account_rejects_locked_alpha(coldkey: &T::AccountId) -> bool {
        AccountFlags::<T>::get(coldkey) & crate::ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA != 1
    }

    pub fn set_accept_locked_alpha(coldkey: &T::AccountId, enabled: bool) {
        AccountFlags::<T>::mutate_exists(coldkey, |maybe_flags| {
            let mut flags = maybe_flags.unwrap_or_default();
            if enabled {
                flags |= crate::ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA;
            } else {
                flags &= !crate::ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA;
            }
            *maybe_flags = if flags == 0 { None } else { Some(flags) };
        });
    }

    pub fn ensure_can_receive_locked_alpha(
        coldkey: &T::AccountId,
        amount: AlphaBalance,
    ) -> DispatchResult {
        let rejects_locked_alpha = Self::account_rejects_locked_alpha(coldkey);
        Self::ensure_can_receive_locked_alpha_with_flag(rejects_locked_alpha, amount)
    }

    pub(crate) fn ensure_can_receive_locked_alpha_with_flag(
        rejects_locked_alpha: bool,
        amount: AlphaBalance,
    ) -> DispatchResult {
        if amount.is_zero() {
            return Ok(());
        }
        ensure!(!rejects_locked_alpha, Error::<T>::AccountRejectsLockedAlpha);
        Ok(())
    }

    pub fn insert_lock_state(
        coldkey: &T::AccountId,
        netuid: NetUid,
        hotkey: &T::AccountId,
        lock_state: LockState,
    ) {
        if lock_state.is_zero() {
            Self::maybe_remove_locking_coldkey(hotkey, netuid, coldkey);
            // If there is no record previously, this is a no-op
            Lock::<T>::remove((coldkey, netuid, hotkey));
        } else {
            Self::add_locking_coldkey(hotkey, netuid, coldkey);
            Lock::<T>::insert((coldkey, netuid, hotkey), lock_state);
        }
    }

    pub fn insert_hotkey_lock_state(netuid: NetUid, hotkey: &T::AccountId, lock_state: LockState) {
        if !lock_state.locked_mass.is_zero()
            || lock_state.conviction > U64F64::saturating_from_num(0)
        {
            HotkeyLock::<T>::insert(netuid, hotkey, lock_state);
        } else {
            HotkeyLock::<T>::remove(netuid, hotkey);
        }
    }

    pub fn insert_decaying_hotkey_lock_state(
        netuid: NetUid,
        hotkey: &T::AccountId,
        lock_state: LockState,
    ) {
        if !lock_state.locked_mass.is_zero()
            || lock_state.conviction > U64F64::saturating_from_num(0)
        {
            DecayingHotkeyLock::<T>::insert(netuid, hotkey, lock_state);
        } else {
            DecayingHotkeyLock::<T>::remove(netuid, hotkey);
        }
    }

    pub fn insert_owner_lock_state(netuid: NetUid, lock_state: LockState) {
        if !lock_state.locked_mass.is_zero()
            || lock_state.conviction > U64F64::saturating_from_num(0)
        {
            OwnerLock::<T>::insert(netuid, lock_state);
        } else {
            OwnerLock::<T>::remove(netuid);
        }
    }

    pub fn insert_decaying_owner_lock_state(netuid: NetUid, lock_state: LockState) {
        if !lock_state.locked_mass.is_zero()
            || lock_state.conviction > U64F64::saturating_from_num(0)
        {
            DecayingOwnerLock::<T>::insert(netuid, lock_state);
        } else {
            DecayingOwnerLock::<T>::remove(netuid);
        }
    }

    pub(crate) fn is_subnet_owner_hotkey(netuid: NetUid, hotkey: &T::AccountId) -> bool {
        hotkey == &SubnetOwnerHotkey::<T>::get(netuid)
    }

    pub(crate) fn is_perpetual_lock(coldkey: &T::AccountId, netuid: NetUid) -> bool {
        DecayingLock::<T>::get(coldkey, netuid) == Some(false)
    }

    pub(crate) fn empty_lock(now: u64) -> LockState {
        LockState {
            locked_mass: AlphaBalance::ZERO,
            conviction: U64F64::saturating_from_num(0),
            last_update: now,
        }
    }

    pub(crate) fn read_conviction_model_for_hotkey(
        coldkey: &T::AccountId,
        netuid: NetUid,
        hotkey: &T::AccountId,
        now: u64,
    ) -> ConvictionModel {
        ConvictionModel::new(
            Self::is_subnet_owner_hotkey(netuid, hotkey),
            Self::is_perpetual_lock(coldkey, netuid),
            Lock::<T>::get((coldkey, netuid, hotkey)).unwrap_or_else(|| Self::empty_lock(now)),
            HotkeyLock::<T>::get(netuid, hotkey).unwrap_or_else(|| Self::empty_lock(now)),
            DecayingHotkeyLock::<T>::get(netuid, hotkey).unwrap_or_else(|| Self::empty_lock(now)),
            OwnerLock::<T>::get(netuid).unwrap_or_else(|| Self::empty_lock(now)),
            DecayingOwnerLock::<T>::get(netuid).unwrap_or_else(|| Self::empty_lock(now)),
        )
    }

    pub(crate) fn read_conviction_model(
        coldkey: &T::AccountId,
        netuid: NetUid,
        now: u64,
    ) -> Option<(T::AccountId, ConvictionModel)> {
        Lock::<T>::iter_prefix((coldkey, netuid))
            .next()
            .map(|(hotkey, _lock)| {
                let model = Self::read_conviction_model_for_hotkey(coldkey, netuid, &hotkey, now);
                (hotkey, model)
            })
    }

    pub(crate) fn save_conviction_model(
        coldkey: &T::AccountId,
        netuid: NetUid,
        hotkey: &T::AccountId,
        model: ConvictionModel,
    ) {
        if model.individual_lock_dirty() {
            Self::insert_lock_state(coldkey, netuid, hotkey, model.individual_lock().clone());
        }
        if model.agg_perpetual_general_dirty() {
            Self::insert_hotkey_lock_state(netuid, hotkey, model.agg_perpetual_general().clone());
        }
        if model.agg_decaying_general_dirty() {
            Self::insert_decaying_hotkey_lock_state(
                netuid,
                hotkey,
                model.agg_decaying_general().clone(),
            );
        }
        if model.agg_perpetual_owner_dirty() {
            Self::insert_owner_lock_state(netuid, model.agg_perpetual_owner().clone());
        }
        if model.agg_decaying_owner_dirty() {
            Self::insert_decaying_owner_lock_state(netuid, model.agg_decaying_owner().clone());
        }
    }

    pub fn do_set_perpetual_lock(
        coldkey: &T::AccountId,
        netuid: NetUid,
        enabled: bool,
    ) -> DispatchResult {
        ensure!(Self::subnet_exists(netuid), Error::<T>::SubnetNotExists);

        let now = Self::get_current_block_as_u64();
        let current_enabled = Self::is_perpetual_lock(coldkey, netuid);

        if let Some((hotkey, mut model)) = Self::read_conviction_model(coldkey, netuid, now) {
            model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
            let rolled = model.individual_lock().clone();
            Self::save_conviction_model(coldkey, netuid, &hotkey, model);

            if current_enabled != enabled {
                Self::reduce_aggregate_lock(
                    coldkey,
                    &hotkey,
                    netuid,
                    rolled.locked_mass,
                    rolled.conviction,
                );
            }
        }

        if enabled {
            DecayingLock::<T>::insert(coldkey, netuid, false);
        } else {
            DecayingLock::<T>::remove(coldkey, netuid);
        }

        if current_enabled != enabled
            && let Some((hotkey, model)) = Self::read_conviction_model(coldkey, netuid, now)
        {
            Self::add_aggregate_lock(coldkey, &hotkey, netuid, model.individual_lock().clone());
        }
        Self::deposit_event(Event::PerpetualLockUpdated {
            coldkey: coldkey.clone(),
            netuid,
            enabled,
        });
        Ok(())
    }
}
