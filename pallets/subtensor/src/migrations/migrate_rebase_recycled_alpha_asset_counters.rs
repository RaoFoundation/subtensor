use super::*;
use frame_support::traits::Get;
use frame_system::pallet_prelude::BlockNumberFor;
use pallet_alpha_assets::AlphaAssetsInterface;
use scale_info::prelude::string::String;
use sp_runtime::traits::Zero;
use subtensor_runtime_common::{AlphaBalance, NetUid};

pub(crate) const MIGRATION_NAME: &[u8] = b"migrate_rebase_recycled_alpha_asset_counters";

/// `(netuid, current_generation_registered_at, issuance, burned, recycled)` offsets
/// inherited from a prior subnet generation.
///
/// These values were read from mainnet archive state at the end of the block in which
/// each current generation was registered. This captures old-generation emission and
/// recycling performed earlier in that block, while the registration itself did not
/// reset these counters. The registration-block guard prevents applying an offset after
/// the netuid is recycled again.
pub(crate) const RECYCLED_ALPHA_COUNTER_OFFSETS: &[(u16, u64, u64, u64, u64)] = &[
    (116, 8_294_730, 10_946_000_000_000, 0, 0),
    (92, 8_352_006, 68_575_136_222_730, 0, 0),
    (40, 8_409_860, 126_076_000_000_000, 51_803_789_083_976, 0),
    (16, 8_460_646, 176_862_000_000_000, 42_912_779_090_897, 0),
    (58, 8_511_017, 227_233_000_000_000, 93_219_350_226_399, 0),
    (
        99,
        8_572_056,
        317_241_269_686_137,
        591_633_301_496,
        1_518_370_122_169,
    ),
    (
        90,
        8_618_670,
        345_144_192_991_428,
        116_392_269_522_858,
        80_950_014_273,
    ),
    (86, 8_693_284, 415_723_558_271_648, 167_757_356_198_720, 0),
    (103, 8_762_380, 529_768_402_624_963, 78_741_954_591_065, 0),
    (
        70,
        8_825_571,
        589_757_917_904_445,
        148_169_094_182,
        3_904_269_338_445,
    ),
];

/// Removes prior-generation offsets from alpha-asset counters for affected mainnet
/// netuids while preserving everything accumulated by their current generations.
pub fn migrate_rebase_recycled_alpha_asset_counters<T: Config>() -> Weight {
    let migration_name = MIGRATION_NAME.to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(MIGRATION_NAME)
        );
        return weight;
    }

    let genesis_hash = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
    weight.saturating_accrue(T::DbWeight::get().reads(1));
    let mainnet_genesis =
        hex_literal::hex!("2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03");
    let mut corrected = 0u64;

    if genesis_hash.as_ref() == mainnet_genesis {
        for &(netuid, expected_registered_at, issuance, burned, recycled) in
            RECYCLED_ALPHA_COUNTER_OFFSETS
        {
            let netuid = NetUid::from(netuid);
            let registered_at = NetworkRegisteredAt::<T>::get(netuid);
            weight.saturating_accrue(T::DbWeight::get().reads(1));

            // The constants describe one exact generation. If the slot has moved again,
            // leave it untouched rather than applying an offset to the wrong asset.
            if registered_at != expected_registered_at {
                log::error!(
                    "Skipping alpha counter rebase for netuid {:?}: registered_at={}, expected={}",
                    netuid,
                    registered_at,
                    expected_registered_at,
                );
                continue;
            }

            T::AlphaAssets::rebase_alpha_counters(
                netuid,
                AlphaBalance::from(issuance),
                AlphaBalance::from(burned),
                AlphaBalance::from(recycled),
            );
            let updated_counters = [issuance, burned, recycled]
                .into_iter()
                .filter(|offset| *offset != 0)
                .count() as u64;
            weight.saturating_accrue(
                T::DbWeight::get().reads_writes(updated_counters, updated_counters),
            );
            corrected = corrected.saturating_add(1);
        }
    } else {
        log::info!(
            "Migration '{}' skipped outside mainnet",
            String::from_utf8_lossy(MIGRATION_NAME)
        );
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed with {} alpha counter corrections",
        String::from_utf8_lossy(MIGRATION_NAME),
        corrected,
    );
    weight
}
