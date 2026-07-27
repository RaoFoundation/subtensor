use super::*;

pub fn get_unactive_sn_netuids<T: Config>(
    pool_initial_alpha: AlphaBalance,
) -> (Vec<NetUid>, Weight) {
    // Loop over all subnets, if the AlphaIssuance is > pool_initial_alpha
    // but FirstEmissionBlockNumber is None
    // then this subnet should be reset
    let mut weight = T::DbWeight::get().reads(1);
    let unactive_netuids = Pallet::<T>::get_all_subnet_netuids()
        .iter()
        .filter(|&netuid| !netuid.is_root())
        .filter(|&netuid| {
            let alpha_issuance = Pallet::<T>::get_alpha_issuance(*netuid);
            let first_emission_block_number = FirstEmissionBlockNumber::<T>::get(*netuid);
            alpha_issuance != pool_initial_alpha && first_emission_block_number.is_none()
        })
        .copied()
        .collect::<Vec<_>>();
    weight = weight
        .saturating_add(T::DbWeight::get().reads(unactive_netuids.len().saturating_mul(3) as u64));

    (unactive_netuids, weight)
}

/// Clears pending emission counters for subnets that have alpha issuance but never received a first emission block
/// (`PendingServerEmission`, `PendingValidatorEmission`, root/owner pending cuts, and in/out emission tallies).
///
/// Idempotency key (frozen): `migrate_reset_unactive_sn`.
pub fn migrate_reset_unactive_sn<T: Config>() -> Weight {
    let migration_name = b"migrate_reset_unactive_sn".to_vec();
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

    // From init_new_network
    let pool_initial_tao: TaoBalance = Pallet::<T>::get_network_min_lock();
    let pool_initial_alpha: AlphaBalance = pool_initial_tao.to_u64().into();

    let (unactive_netuids, w) = get_unactive_sn_netuids::<T>(pool_initial_alpha);
    weight = weight.saturating_add(w);

    for netuid in unactive_netuids.iter() {
        // Reset the subnet as it shouldn't have any emissions
        PendingServerEmission::<T>::remove(*netuid);
        PendingValidatorEmission::<T>::remove(*netuid);
        PendingRootAlphaDivs::<T>::remove(*netuid);
        PendingOwnerCut::<T>::remove(*netuid);
        SubnetTaoInEmission::<T>::remove(*netuid);
        SubnetAlphaInEmission::<T>::remove(*netuid);
        SubnetAlphaOutEmission::<T>::remove(*netuid);
        weight = weight.saturating_add(T::DbWeight::get().writes(7));
    }

    // Mark Migration as Completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
