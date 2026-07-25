use super::*;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;

/// Backfills [`ColdkeyCollateralHotkeys`] from existing [`MinerCollateral`] rows so coldkeys can look up
/// their collateralized hotkeys without scanning the collateral map.
///
/// Idempotency key (frozen): `migrate_coldkey_collateral_hotkeys`.
pub fn migrate_coldkey_collateral_hotkeys<T: Config>() -> Weight {
    let migration_name = b"migrate_coldkey_collateral_hotkeys".to_vec();
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

    let mut indexed = 0_u64;
    let mut overflowed = 0_u64;

    for ((netuid, hotkey, coldkey), _state) in MinerCollateral::<T>::iter() {
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        let mut overflow = false;
        ColdkeyCollateralHotkeys::<T>::mutate(netuid, &coldkey, |hotkeys| {
            if hotkeys.contains(&hotkey) {
                return;
            }
            if hotkeys.try_push(hotkey.clone()).is_err() {
                overflow = true;
            }
        });
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        if overflow {
            overflowed = overflowed.saturating_add(1);
            log::warn!(
                "migrate_coldkey_collateral_hotkeys: coldkey at cap; left unindexed \
                 (netuid={netuid:?}, hotkey={hotkey:?}, coldkey={coldkey:?})"
            );
        } else {
            indexed = indexed.saturating_add(1);
        }
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed. {} positions indexed, {} over-cap left unindexed.",
        String::from_utf8_lossy(&migration_name),
        indexed,
        overflowed,
    );

    weight
}
