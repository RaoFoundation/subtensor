use crate::{Config, HasMigrationRun, NetworksAdded, SubnetTAO, TotalStake};
use frame_support::{traits::Get, weights::Weight};
use subtensor_runtime_common::{TaoBalance, Token};

const MIGRATION_NAME: &[u8] = b"migrate_resync_total_stake";

/// Set `TotalStake` to the sum of `SubnetTAO` over live subnets.
///
/// `TotalStake` used to grow by the gross TAO of every buy while `SubnetTAO` grew by the
/// TAO that entered the reserve (gross minus the swap fee), so it drifted above the sum
/// of the reserves it summarises. The two now move together; this one-shot resync removes
/// the accumulated difference. `TotalIssuance` is not touched.
pub fn migrate_resync_total_stake<T: Config>() -> Weight {
    let migration_name = MIGRATION_NAME.to_vec();
    let mut reads = 1u64;

    if HasMigrationRun::<T>::get(&migration_name) {
        return T::DbWeight::get().reads(reads);
    }

    let mut total = TaoBalance::ZERO;
    for (netuid, added) in NetworksAdded::<T>::iter() {
        reads = reads.saturating_add(1);
        if added {
            reads = reads.saturating_add(1);
            total = total.saturating_add(SubnetTAO::<T>::get(netuid));
        }
    }

    let previous = TotalStake::<T>::get();
    reads = reads.saturating_add(1);
    TotalStake::<T>::put(total);
    log::info!(
        "migrate_resync_total_stake: TotalStake {previous:?} -> {total:?} (sum of live SubnetTAO)"
    );

    HasMigrationRun::<T>::insert(&migration_name, true);
    T::DbWeight::get().reads_writes(reads, 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::mock::*;
    use subtensor_runtime_common::NetUid;

    #[test]
    fn migration_resyncs_total_stake_to_live_subnet_tao_once() {
        new_test_ext(1).execute_with(|| {
            let live_a = NetUid::from(1);
            let live_b = NetUid::from(2);
            let dissolving = NetUid::from(3);
            NetworksAdded::<Test>::insert(live_a, true);
            NetworksAdded::<Test>::insert(live_b, true);
            NetworksAdded::<Test>::insert(dissolving, false);
            SubnetTAO::<Test>::insert(live_a, TaoBalance::from(1_000_u64));
            SubnetTAO::<Test>::insert(live_b, TaoBalance::from(250_u64));
            SubnetTAO::<Test>::insert(dissolving, TaoBalance::from(99_u64));
            TotalStake::<Test>::put(TaoBalance::from(5_000_u64));

            let weight = migrate_resync_total_stake::<Test>();
            assert_eq!(TotalStake::<Test>::get(), TaoBalance::from(1_250_u64));
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            assert_eq!(
                weight,
                <Test as frame_system::Config>::DbWeight::get().reads_writes(7, 2)
            );

            TotalStake::<Test>::put(TaoBalance::from(7_u64));
            let weight = migrate_resync_total_stake::<Test>();
            assert_eq!(TotalStake::<Test>::get(), TaoBalance::from(7_u64));
            assert_eq!(
                weight,
                <Test as frame_system::Config>::DbWeight::get().reads(1)
            );
        });
    }
}
