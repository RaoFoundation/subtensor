use super::*;

use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use sp_runtime::traits::SaturatedConversion;

/// Captures the runtime-upgrade block used as the TAO-in refund behavior cutover.
pub fn migrate_tao_in_refund_deployment_block<T: Config>() -> Weight {
    let migration_name = b"migrate_tao_in_refund_deployment_block".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{:?}' has already run. Skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    log::info!(
        "Running migration '{:?}'",
        String::from_utf8_lossy(&migration_name)
    );

    let deployment_block: u64 = frame_system::Pallet::<T>::block_number().saturated_into::<u64>();

    TaoInRefundDeploymentBlock::<T>::put(deployment_block);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully. TaoInRefundDeploymentBlock: {}.",
        String::from_utf8_lossy(&migration_name),
        deployment_block
    );

    weight
}
