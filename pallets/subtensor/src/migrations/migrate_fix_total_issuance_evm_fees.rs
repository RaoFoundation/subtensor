use super::*;
use frame_support::traits::fungible::Inspect;
use frame_support::weights::Weight;

pub fn migrate_fix_total_issuance_evm_fees<T: Config>() -> Weight {
    let migration_names: [&[u8]; 2] = [
        // Fix testnet TotalIssuance after the earlier EVM fees issue caused the
        // Subtensor pallet's accounting to diverge from the balances pallet.
        b"migrate_fix_total_issuance_evm_fees",
        // Fix Subtensor TotalIssuance after dust collection caused accounting drift.
        b"migrate_fix_total_issuance_after_dust_collection",
    ];
    let mut weight = T::DbWeight::get().reads(migration_names.len() as u64);

    let Some(migration_name) = migration_names
        .iter()
        .map(|name| name.to_vec())
        .find(|name| !HasMigrationRun::<T>::get(name))
    else {
        log::info!("All total issuance fix migrations have already run. Skipping.");
        return weight;
    };

    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    // All migration instances reset Subtensor TotalIssuance to the authoritative
    // Balances pallet total issuance.
    let balances_total_issuance = <T as Config>::Currency::total_issuance();
    let subtensor_total_issuance_before = TotalIssuance::<T>::get();
    TotalIssuance::<T>::put(balances_total_issuance);
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 1));

    log::info!(
        "Subtensor TotalIssuance fixed for EVM fees issue: previous: {}, new: {}",
        subtensor_total_issuance_before,
        balances_total_issuance
    );

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
