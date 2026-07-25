use super::*;
use frame_support::{storage_alias, traits::Get, weights::Weight};
use substrate_fixed::types::U96F32;

pub mod deprecated_pending_emission_format {
    use super::*;

    #[storage_alias]
    pub(super) type PendingEmission<T: Config> =
        StorageMap<Pallet<T>, Identity, NetUid, AlphaBalance, ValueQuery>;
}

/// Splits legacy `PendingEmission` into `PendingServerEmission` / `PendingValidatorEmission`
/// (50/50 with root-alpha adjustment), then clears the old map.
///
/// Idempotency key (frozen): `migrate_pending_emissions`.
pub fn migrate_pending_emissions<T: Config>() -> Weight {
    let migration_name = b"migrate_pending_emissions".to_vec();
    let mut weight: Weight = T::DbWeight::get().reads(1);

    // Skip if already executed
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            target: "runtime",
            "Migration '{}' already run - skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }
    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    // Pull from PendingEmission and distribute to PendingValidatorEmission and PendingServerEmission
    for (netuid, pending_emission) in
        deprecated_pending_emission_format::PendingEmission::<T>::iter()
    {
        // Split up the pending emission into server and validator emission
        // Server emission is pending+root_alpha times the 50% miner cut.
        let root_alpha: U96F32 =
            U96F32::saturating_from_num(PendingRootAlphaDivs::<T>::get(netuid).to_u64());
        let server_emission_float: U96F32 = U96F32::saturating_from_num(pending_emission.to_u64())
            .saturating_add(root_alpha)
            .saturating_div(U96F32::saturating_from_num(2));
        let server_emission: AlphaBalance = server_emission_float.saturating_to_num::<u64>().into();
        let validator_emission = pending_emission.saturating_sub(server_emission);

        PendingValidatorEmission::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(validator_emission)
        });
        PendingServerEmission::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(server_emission)
        });

        weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 2));
    }

    // Kill the map
    let removal_result =
        deprecated_pending_emission_format::PendingEmission::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(
        T::DbWeight::get().reads_writes(removal_result.loops as u64, removal_result.backend as u64),
    );

    // Mark Migration as Completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
