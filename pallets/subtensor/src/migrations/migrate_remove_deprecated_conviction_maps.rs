use super::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{
    pallet_prelude::{Blake2_128Concat, Identity, NMapKey, OptionQuery, ValueQuery},
    storage_alias,
    traits::Get,
    weights::Weight,
};
use scale_info::{TypeInfo, prelude::string::String};
use substrate_fixed::types::U64F64;

pub mod deprecated {
    use super::*;

    /// Deprecated lock state for a coldkey on a subnet.
    #[crate::freeze_struct("13703236126f1b2b")]
    #[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo)]
    pub struct LockState {
        /// Locked amount, stays constant unless user makes changes.
        pub locked_mass: AlphaBalance,
        /// Unlocked amount, gradually decays over time.
        pub unlocked_mass: AlphaBalance,
        /// Matured decaying score.
        pub conviction: U64F64,
        /// Block number of last roll-forward.
        pub last_update: u64,
    }

    #[storage_alias]
    pub type Lock<T: Config> = StorageNMap<
        Pallet<T>,
        (
            NMapKey<Blake2_128Concat, AccountIdOf<T>>,
            NMapKey<Identity, NetUid>,
            NMapKey<Blake2_128Concat, AccountIdOf<T>>,
        ),
        LockState,
        OptionQuery,
    >;

    #[storage_alias]
    pub type HotkeyLock<T: Config> = StorageDoubleMap<
        Pallet<T>,
        Identity,
        NetUid,
        Blake2_128Concat,
        AccountIdOf<T>,
        LockState,
        OptionQuery,
    >;

    #[storage_alias]
    pub type MaturityRate<T: Config> = StorageValue<Pallet<T>, u64, ValueQuery>;

    #[storage_alias]
    pub type UnlockRate<T: Config> = StorageValue<Pallet<T>, u64, ValueQuery>;
}

/// This migration removes the conviction v1 maps that were deprecated before they were
/// deployed on mainnet. They existed briefly on testnet and contain some values that need
/// to be cleaned before deploying conviction v2.
pub fn migrate_remove_deprecated_conviction_maps<T: Config>() -> Weight {
    let migration_name = b"migrate_remove_deprecated_conviction_maps".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{:?}' has already run. Skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    let lock_removal = deprecated::Lock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(
        T::DbWeight::get().reads_writes(lock_removal.loops as u64, lock_removal.backend as u64),
    );

    let hotkey_lock_removal = deprecated::HotkeyLock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        hotkey_lock_removal.loops as u64,
        hotkey_lock_removal.backend as u64,
    ));

    deprecated::MaturityRate::<T>::kill();
    deprecated::UnlockRate::<T>::kill();
    weight = weight.saturating_add(T::DbWeight::get().writes(2));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully. Removed Lock entries: {:?}, HotkeyLock entries: {:?}.",
        String::from_utf8_lossy(&migration_name),
        lock_removal.backend,
        hotkey_lock_removal.backend,
    );

    weight
}
