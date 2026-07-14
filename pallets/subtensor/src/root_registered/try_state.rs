use alloc::collections::{BTreeMap, BTreeSet};

use super::*;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// Stored per-coldkey count equals the actual number of owned hotkeys registered on root.
    pub(crate) fn check_root_registered_hotkey_count() -> Result<(), sp_runtime::TryRuntimeError> {
        let mut expected: BTreeMap<T::AccountId, u32> = BTreeMap::new();
        for (_uid, hotkey) in Keys::<T>::iter_prefix(NetUid::ROOT) {
            let owner = Owner::<T>::get(&hotkey);
            expected
                .entry(owner)
                .and_modify(|c| *c = c.saturating_add(1))
                .or_insert(1);
        }

        for (coldkey, stored) in RootRegisteredHotkeyCount::<T>::iter() {
            let expected_count = expected.remove(&coldkey).unwrap_or(0);
            ensure!(
                stored == expected_count,
                "RootRegisteredHotkeyCount mismatch for coldkey",
            );
        }

        ensure!(
            expected.is_empty(),
            "RootRegisteredHotkeyCount missing entries for coldkeys with root hotkeys",
        );

        Ok(())
    }

    /// External inspector's coldkey set matches `RootRegisteredHotkeyCount`; skipped when unwired.
    pub(crate) fn check_root_registered_matches_inspector()
    -> Result<(), sp_runtime::TryRuntimeError> {
        let Some(actual_members) = T::RootRegisteredInspector::members() else {
            return Ok(());
        };
        let actual: BTreeSet<T::AccountId> = actual_members.into_iter().collect();
        let expected: BTreeSet<T::AccountId> = RootRegisteredHotkeyCount::<T>::iter()
            .map(|(coldkey, _)| coldkey)
            .collect();
        ensure!(
            actual == expected,
            "RootRegisteredInspector members do not match root-registered coldkey set",
        );
        Ok(())
    }

    /// `RootRegisteredEma` and `RootRegisteredHotkeyCount` always share the same key set.
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn check_root_registered_ema_matches_count()
    -> Result<(), sp_runtime::TryRuntimeError> {
        let ema_keys: BTreeSet<T::AccountId> =
            RootRegisteredEma::<T>::iter().map(|(c, _)| c).collect();
        let count_keys: BTreeSet<T::AccountId> = RootRegisteredHotkeyCount::<T>::iter()
            .map(|(c, _)| c)
            .collect();
        ensure!(
            ema_keys == count_keys,
            "RootRegisteredEma keys do not match RootRegisteredHotkeyCount keys",
        );
        Ok(())
    }
}
