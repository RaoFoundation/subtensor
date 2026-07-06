use super::*;
use frame_support::pallet_prelude::Weight;
use sp_io::KillStorageResult;
use sp_io::hashing::twox_128;
use sp_io::storage::clear_prefix;
use sp_std::vec::Vec;

pub mod migrate_swapv3_to_balancer;

pub(crate) fn remove_prefix<T: Config>(module: &str, old_map: &str, weight: &mut Weight) {
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&twox_128(module.as_bytes()));
    prefix.extend_from_slice(&twox_128(old_map.as_bytes()));

    let removal_results = clear_prefix(&prefix, Some(u32::MAX));
    let removed_entries_count = match removal_results {
        KillStorageResult::AllRemoved(removed) => removed as u64,
        KillStorageResult::SomeRemaining(removed) => {
            log::info!("Failed To Remove Some Items During migration");
            removed as u64
        }
    };

    *weight = (*weight).saturating_add(T::DbWeight::get().writes(removed_entries_count));
}
