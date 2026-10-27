use crate::{Config, HasMigrationRun, TotalVotingPower, VotingPower};
use alloc::collections::BTreeMap;
use frame_support::{traits::Get, weights::Weight};
use subtensor_runtime_common::NetUid;

const MIGRATION_NAME: &[u8] = b"migrate_total_voting_power";

/// Backfill the per-subnet voting-power aggregate from the existing
/// `VotingPower` entries. The full scan happens once during the runtime
/// upgrade; subsequent reads use `TotalVotingPower` in O(1).
pub fn migrate_total_voting_power<T: Config>() -> Weight {
    let migration_name = MIGRATION_NAME.to_vec();
    let mut reads = 1u64;

    if HasMigrationRun::<T>::get(&migration_name) {
        return T::DbWeight::get().reads(reads);
    }

    let mut totals = BTreeMap::<NetUid, u64>::new();
    for (netuid, _, voting_power) in VotingPower::<T>::iter() {
        reads = reads.saturating_add(1);
        totals
            .entry(netuid)
            .and_modify(|total| *total = total.saturating_add(voting_power))
            .or_insert(voting_power);
    }

    let mut writes = 1u64;
    for (netuid, total) in totals {
        TotalVotingPower::<T>::insert(netuid, total);
        writes = writes.saturating_add(1);
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    T::DbWeight::get().reads_writes(reads, writes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tests::mock::*, *};
    use sp_core::U256;

    #[test]
    fn migration_backfills_total_voting_power_once() {
        new_test_ext(1).execute_with(|| {
            let first_netuid = NetUid::from(1);
            let second_netuid = NetUid::from(2);
            VotingPower::<Test>::insert(first_netuid, U256::from(1), 10);
            VotingPower::<Test>::insert(first_netuid, U256::from(2), 20);
            VotingPower::<Test>::insert(second_netuid, U256::from(3), 7);

            let weight = migrate_total_voting_power::<Test>();

            assert_eq!(TotalVotingPower::<Test>::get(first_netuid), 30);
            assert_eq!(TotalVotingPower::<Test>::get(second_netuid), 7);
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            assert_eq!(
                weight,
                <Test as frame_system::Config>::DbWeight::get().reads_writes(4, 3)
            );

            VotingPower::<Test>::insert(first_netuid, U256::from(4), 100);
            migrate_total_voting_power::<Test>();
            assert_eq!(TotalVotingPower::<Test>::get(first_netuid), 30);
        });
    }
}
