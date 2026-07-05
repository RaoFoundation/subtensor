use super::*;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;

pub fn migrate_associated_evm_address_index<T: Config>() -> Weight {
    let migration_name = b"migrate_associated_evm_address_index".to_vec();
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

    let mut migrated = 0_u64;
    let mut overflowed = 0_u64;
    for (netuid, uid, (evm_key, block_associated)) in AssociatedEvmAddress::<T>::iter() {
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        AssociatedUidsByEvmAddress::<T>::mutate(netuid, evm_key, |uids| {
            if let Some((_, stored_block)) =
                uids.iter_mut().find(|(stored_uid, _)| *stored_uid == uid)
            {
                *stored_block = block_associated;
                return;
            }

            if uids.try_push((uid, block_associated)).is_err() {
                overflowed = overflowed.saturating_add(1);
            }
        });
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        migrated = migrated.saturating_add(1);
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully. {} associations indexed, {} skipped.",
        String::from_utf8_lossy(&migration_name),
        migrated,
        overflowed
    );

    weight
}
