use super::*;
use crate::{BalancerAlphaReservoir, BalancerTaoReservoir, HasMigrationRun};
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;

const MIGRATION_NAME: &[u8] = b"migrate_swap_storage_cleanup_v2";

/// Removes the abandoned Swap V3 prefixes without replaying the obsolete initialization logic.
///
/// Mainnet has 2,661 legacy rows across these prefixes, so this small cleanup is intentionally
/// completed in the runtime-upgrade block. The much larger Subtensor cleanup is separately
/// bounded across `on_idle` blocks.
pub fn migrate_swap_storage_cleanup_v2<T: Config>() -> Weight {
    let migration_name = BoundedVec::truncate_from(MIGRATION_NAME.to_vec());
    let mut weight = T::DbWeight::get().reads(1);
    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    for storage_name in [
        "AlphaSqrtPrice",
        "CurrentTick",
        "EnabledUserLiquidity",
        "FeeGlobalTao",
        "FeeGlobalAlpha",
        "LastPositionId",
        "ScrapReservoirTao",
        "ScrapReservoirAlpha",
        "Ticks",
        "TickIndexBitmapWords",
        "SwapV3Initialized",
        "CurrentLiquidity",
        "Positions",
    ] {
        remove_prefix::<T>("Swap", storage_name, &mut weight);
    }

    // ValueQuery returns zero for an absent row. Avoid retaining the explicit zero reservoirs
    // created by the old update path while preserving any genuinely pending balance.
    let mut reservoir_reads = 0_u64;
    let zero_tao: sp_std::vec::Vec<_> = BalancerTaoReservoir::<T>::iter()
        .filter_map(|(netuid, value)| {
            reservoir_reads = reservoir_reads.saturating_add(1);
            value.is_zero().then_some(netuid)
        })
        .collect();
    let zero_alpha: sp_std::vec::Vec<_> = BalancerAlphaReservoir::<T>::iter()
        .filter_map(|(netuid, value)| {
            reservoir_reads = reservoir_reads.saturating_add(1);
            value.is_zero().then_some(netuid)
        })
        .collect();
    weight.saturating_accrue(T::DbWeight::get().reads(reservoir_reads));
    for netuid in zero_tao {
        BalancerTaoReservoir::<T>::remove(netuid);
        weight.saturating_accrue(T::DbWeight::get().writes(1));
    }
    for netuid in zero_alpha {
        BalancerAlphaReservoir::<T>::remove(netuid);
        weight.saturating_accrue(T::DbWeight::get().writes(1));
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));
    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(MIGRATION_NAME)
    );
    weight
}
