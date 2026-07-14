use super::*;
use crate::root_registered::OnRootRegistrationChange;

impl<T: Config> Pallet<T> {
    pub fn coldkey_has_root_hotkey(coldkey: &T::AccountId) -> bool {
        RootRegisteredHotkeyCount::<T>::get(coldkey) > 0
    }

    pub fn increment_root_registered_hotkey_count(coldkey: &T::AccountId) {
        let was_zero = RootRegisteredHotkeyCount::<T>::get(coldkey) == 0;
        RootRegisteredHotkeyCount::<T>::mutate(coldkey, |c| *c = c.saturating_add(1));
        if was_zero {
            Self::init_root_registered_ema(coldkey);
            T::OnRootRegistrationChange::on_added(coldkey);
        }
    }

    pub fn decrement_root_registered_hotkey_count(coldkey: &T::AccountId) {
        let mut became_zero = false;
        RootRegisteredHotkeyCount::<T>::mutate_exists(coldkey, |c| {
            let prev = c.unwrap_or(0);
            let next = prev.saturating_sub(1);
            became_zero = prev > 0 && next == 0;
            *c = if next == 0 { None } else { Some(next) };
        });
        if became_zero {
            Self::clear_root_registered_ema(coldkey);
            T::OnRootRegistrationChange::on_removed(coldkey);
        }
    }
}
