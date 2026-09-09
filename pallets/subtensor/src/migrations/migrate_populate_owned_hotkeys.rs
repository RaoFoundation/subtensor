use super::*;
use frame_support::{
    pallet_prelude::{Identity, OptionQuery},
    storage_alias,
    traits::Get,
    weights::Weight,
};
use log::info;
use sp_std::vec::Vec;

const LOG_TARGET_1: &str = "migrate_populate_owned";

/// Module containing deprecated storage format for LoadedEmission
pub mod deprecated_loaded_emission_format {
    use super::*;

    #[storage_alias]
    pub(super) type LoadedEmission<T: Config> =
        StorageMap<Pallet<T>, Identity, u16, Vec<(AccountIdOf<T>, u64)>, OptionQuery>;
}

/// Migrate the OwnedHotkeys map to the new storage format
pub fn migrate_populate_owned<T: Config>() -> Weight {
    // Setup migration weight
    let mut weight = T::DbWeight::get().reads(1);
    let migration_name = "Populate OwnedHotkeys map";

    // Check if this migration is needed (if OwnedHotkeys map is empty)
    let migrate = OwnedHotkeys::<T>::iter().next().is_none();

    // Only runs if the migration is needed
    if migrate {
        info!(target: LOG_TARGET_1, ">>> Starting Migration: {migration_name}");

        let mut longest_hotkey_vector: usize = 0;
        let mut longest_coldkey: Option<T::AccountId> = None;
        let mut keys_touched: u64 = 0;
        let mut storage_reads: u64 = 0;
        let mut storage_writes: u64 = 0;

        // Iterate through all Owner entries
        Owner::<T>::iter().for_each(|(hotkey, coldkey)| {
            storage_reads = storage_reads.saturating_add(1); // Read from Owner storage
            let mut hotkeys = OwnedHotkeys::<T>::get(&coldkey);
            storage_reads = storage_reads.saturating_add(1); // Read from OwnedHotkeys storage

            // Add the hotkey if it's not already in the vector
            if !hotkeys.contains(&hotkey) {
                hotkeys.push(hotkey);
                keys_touched = keys_touched.saturating_add(1);

                // Update longest hotkey vector info
                if longest_hotkey_vector < hotkeys.len() {
                    longest_hotkey_vector = hotkeys.len();
                    longest_coldkey = Some(coldkey.clone());
                }

                // Update the OwnedHotkeys storage
                OwnedHotkeys::<T>::insert(&coldkey, hotkeys);
                storage_writes = storage_writes.saturating_add(1); // Write to OwnedHotkeys storage
            }

            // Accrue weight for reads and writes
            weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 1));
        });

        // Log migration results
        info!(
            target: LOG_TARGET_1,
            "Migration {migration_name} finished. Keys touched: {keys_touched}, Longest hotkey vector: {longest_hotkey_vector}, Storage reads: {storage_reads}, Storage writes: {storage_writes}"
        );
        if let Some(c) = longest_coldkey {
            info!(target: LOG_TARGET_1, "Longest hotkey vector is controlled by: {c:?}");
        }

        weight
    } else {
        info!(target: LOG_TARGET_1, "Migration {migration_name} already done!");
        Weight::zero()
    }
}
