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
/// operation. Those maps are removed and the owner hotkey's aggregates are
/// rebuilt from the locks to it, so each of them is counted once. Nothing is
/// lost there: the conviction of a lock to the owner hotkey is its locked mass.
///
/// The aggregates of every other hotkey are kept as they are. They can hold
/// conviction that its individual locks do not, such as the conviction a former
/// owner hotkey kept when it handed the subnet over.
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
        deprecated::OwnerLock::<T>::clear(u32::MAX, None),
        deprecated::DecayingOwnerLock::<T>::clear(u32::MAX, None),
    ] {
        weight = weight.saturating_add(
            T::DbWeight::get().reads_writes(removal.loops as u64, removal.backend as u64),
        );
    }

    // The locks of a subnet under dissolution are removed by its cleanup.
    let netuids = Pallet::<T>::get_all_subnet_netuids();
    for netuid in netuids.iter() {
        let owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);
        HotkeyLock::<T>::remove(netuid, &owner_hotkey);
        DecayingHotkeyLock::<T>::remove(netuid, &owner_hotkey);
    }
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        (netuids.len() as u64).saturating_mul(2),
        (netuids.len() as u64).saturating_mul(2),
    ));

    let now = Pallet::<T>::get_current_block_as_u64();
    let unlock_rate = UnlockRate::<T>::get();
    let maturity_rate = MaturityRate::<T>::get();
    let mut scanned_locks = 0u64;
    for ((coldkey, netuid, hotkey), lock) in Lock::<T>::iter() {
        scanned_locks = scanned_locks.saturating_add(1);
        if !Pallet::<T>::is_subnet_owner_hotkey(netuid, &hotkey)
            || !Pallet::<T>::if_subnet_exist(netuid)
        {
            continue;
        }
        let rolled = ConvictionModel::roll_forward_lock(
            lock,
            now,
            unlock_rate,
            maturity_rate,
            true,
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
        "Migration '{:?}' completed successfully. Scanned {:?} locks for the owner hotkey aggregates.",
        String::from_utf8_lossy(&migration_name),
        scanned_locks,
    );

    weight
}
