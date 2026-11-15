use super::*;
use crate::staking::lock::{ConvictionModel, LockState};
use frame_support::{
    pallet_prelude::{Identity, OptionQuery},
    storage_alias,
    traits::Get,
    weights::Weight,
};
use scale_info::prelude::string::String;

pub mod deprecated {
    use super::*;

    #[storage_alias]
    pub type OwnerLock<T: Config> = StorageMap<Pallet<T>, Identity, NetUid, LockState, OptionQuery>;

    #[storage_alias]
    pub type DecayingOwnerLock<T: Config> =
        StorageMap<Pallet<T>, Identity, NetUid, LockState, OptionQuery>;
}

/// Gives every hotkey one perpetual and one decaying lock aggregate.
///
/// Locks to the subnet owner hotkey used to be summed in `OwnerLock` and
/// `DecayingOwnerLock`, selected by the owner hotkey at the time of each
/// operation. Those maps are removed, and `HotkeyLock` and `DecayingHotkeyLock`
/// are rebuilt from the individual locks, so every lock is counted once, under
/// the hotkey it targets.
pub fn migrate_one_lock_bucket_per_hotkey<T: Config>() -> Weight {
    let migration_name = b"migrate_one_lock_bucket_per_hotkey".to_vec();
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

    for removal in [
        HotkeyLock::<T>::clear(u32::MAX, None),
        DecayingHotkeyLock::<T>::clear(u32::MAX, None),
        deprecated::OwnerLock::<T>::clear(u32::MAX, None),
        deprecated::DecayingOwnerLock::<T>::clear(u32::MAX, None),
    ] {
        weight = weight.saturating_add(
            T::DbWeight::get().reads_writes(removal.loops as u64, removal.backend as u64),
        );
    }

    let now = Pallet::<T>::get_current_block_as_u64();
    let unlock_rate = UnlockRate::<T>::get();
    let maturity_rate = MaturityRate::<T>::get();
    let mut scanned_locks = 0u64;
    for ((coldkey, netuid, hotkey), lock) in Lock::<T>::iter() {
        scanned_locks = scanned_locks.saturating_add(1);
        // The locks of a subnet under dissolution are removed by its cleanup.
        if !Pallet::<T>::if_subnet_exist(netuid) {
            continue;
        }
        let rolled = ConvictionModel::roll_forward_lock(
            lock,
            now,
            unlock_rate,
            maturity_rate,
            Pallet::<T>::is_subnet_owner_hotkey(netuid, &hotkey),
            Pallet::<T>::is_perpetual_lock(&coldkey, netuid),
        )
        .0;
        Pallet::<T>::add_aggregate_lock(&coldkey, &hotkey, netuid, rolled);
    }
    weight = weight.saturating_add(
        T::DbWeight::get().reads_writes(scanned_locks.saturating_mul(14), scanned_locks),
    );

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully. Rebuilt aggregates from {:?} locks.",
        String::from_utf8_lossy(&migration_name),
        scanned_locks,
    );

    weight
}
