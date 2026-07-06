use super::*;
use frame_support::{
    traits::{Get, fungible::Inspect},
    weights::Weight,
};

/// Performs migration to mint SubnetTAO and subnet locked funds into subnet accounts.
///
/// # Arguments
///
/// # Returns
///
/// * `Weight` - The computational weight of this operation.
///
pub fn migrate_subnet_balances<T: Config>() -> Weight {
    let migration_name = b"migrate_subnet_balances".to_vec();
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

    ////////////////////////////////////////////////////////
    // Actual migration

    let balances_total_issuance_before = <T as Config>::Currency::total_issuance();
    let subtensor_total_issuance_before = TotalIssuance::<T>::get();

    // Mint SubnetTAO into subnet accounts
    // The mint_tao will be adding to subtensor TotalIssuance (which is not the intention
    // and will be corrected below). There is no u64 saturation possible, so it is safe to
    // add the whole amount to TI and then reduce back.
    let mut total_subnet_tao = TaoBalance::ZERO;
    SubnetTAO::<T>::iter().for_each(|(netuid, tao)| {
        if let Some(subnet_account) = Pallet::<T>::get_subnet_account_id(netuid) {
            let credit = Pallet::<T>::mint_tao(tao);
            let _ = Pallet::<T>::spend_tao(&subnet_account, credit, tao);
            total_subnet_tao = total_subnet_tao.saturating_add(tao);
            weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 2));
        }
    });

    // Mint SubnetLocked into subnet accounts
    // Currently (3.3.13) we still burn TAO less initial pool value (which is min lock cost) on
    // registrations and record full lock cost (including initial pool TAO) in SubnetLocked.
    // Initial pool TAO is accounted for both in SubnetLocked and SubnetTAO. The double-accounted
    // initial pool TAO equals NetworkMinLockCost, which is 1 TAO per subnet. It is only
    // double-accounted in situation when owner emission is zero and subnet is dissolved, which is
    // only known in the future and is uncertain currently. To make accounting accurate and certain,
    // we stay with pessimistic approach and rather avoid minting more than 21M TAO. This means that
    // subnet accounts will be credited SubnetLocked amount less initial pool TAO, but the
    // TotalIssuance recorded will be increased by the full SubnetLocked amount.
    let mut total_subnet_locked = TaoBalance::ZERO;
    SubnetLocked::<T>::iter().for_each(|(netuid, tao)| {
        if let Some(subnet_account) = Pallet::<T>::get_subnet_account_id(netuid) {
            let initial_pool_tao = NetworkMinLockCost::<T>::get();
            let tao_lock = tao.saturating_sub(initial_pool_tao);
            let credit = Pallet::<T>::mint_tao(tao_lock);
            let _ = Pallet::<T>::spend_tao(&subnet_account, credit, tao_lock);
            total_subnet_locked = total_subnet_locked.saturating_add(tao_lock);
            weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 2));
        }
    });

    // Remark about migrate_restore_subnet_locked migration:
    //
    // In rao release (v2.0.0) the lock was burned (TotalIssuance reduction), in the subsequent
    // migration migrate_restore_subnet_locked in the version v3.2.8 we restored locks into SubnetLocked,
    // but did not increase the TotalIssuance back, which is correct because in v3.2.8 we keep SubnetLocked
    // in non-issued state. This TAO is added to TotalIssuance when subnet is dissolved.
    //
    // mint_tao increases subtensor TotalIssuance, but this is not the intention for SubnetTAO
    // because staked TAO is already accounted for in it subtensor pallet TotalIssuance. Reduce
    // it back.
    //
    // SubnetLocked, in opposite, was not previously included in the subtensor TotalIssuance
    // because we call recycle_tao in subnet registration.
    //
    TotalIssuance::<T>::mutate(|total| *total = total.saturating_sub(total_subnet_tao));

    // Update the total issuance in storage
    let balances_total_issuance = <T as Config>::Currency::total_issuance();
    let subtensor_total_issuance = TotalIssuance::<T>::get();
    weight = weight.saturating_add(T::DbWeight::get().reads(2));
    log::warn!(
        "  balances TI initial     = {}",
        balances_total_issuance_before
    );
    log::warn!("  balances TI final       = {}", balances_total_issuance);
    log::warn!(
        "  subtensor TI initial    = {}",
        subtensor_total_issuance_before
    );
    log::warn!("  subtensor TI final      = {}", subtensor_total_issuance);
    log::warn!("  total_subnet_tao        = {}", total_subnet_tao);
    log::warn!("  total_subnet_locked     = {}", total_subnet_locked);
    if balances_total_issuance != subtensor_total_issuance {
        log::warn!(
            "Balances and Subtensor total issuance still do not match: {} vs {}. Making them match now.",
            balances_total_issuance,
            subtensor_total_issuance
        );
        TotalIssuance::<T>::put(balances_total_issuance);
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
    }

    ////////////////////////////////////////////////////////

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );
    weight
}
