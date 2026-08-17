#![allow(clippy::unwrap_used)]

use crate::{Config, HasMigrationRun, TotalAlphaStaked, TotalHotkeyAlpha};
use alloc::collections::BTreeMap;
use frame_support::{traits::Get, weights::Weight};
use sp_runtime::traits::Zero;
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

pub(crate) const MIGRATION_NAME: &[u8] = b"migrate_total_alpha_staked";

/// Backfill the per-subnet staked-alpha aggregate from the existing per-hotkey totals.
pub fn migrate_total_alpha_staked<T: Config>() -> Weight {
    let migration_name = MIGRATION_NAME.to_vec();
    let mut reads = 1u64;

    if HasMigrationRun::<T>::get(&migration_name) {
        return T::DbWeight::get().reads(reads);
    }

    let mut totals = BTreeMap::<NetUid, AlphaBalance>::new();
    for (_, netuid, alpha) in TotalHotkeyAlpha::<T>::iter() {
        reads = reads.saturating_add(1);
        totals
            .entry(netuid)
            .and_modify(|total| *total = total.saturating_add(alpha))
            .or_insert(alpha);
    }

    let mut writes = 1u64;
    for (netuid, total) in totals {
        if !total.is_zero() {
            TotalAlphaStaked::<T>::insert(netuid, total);
            writes = writes.saturating_add(1);
        }
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
    fn migration_backfills_each_subnet_once() {
        new_test_ext(1).execute_with(|| {
            let first_netuid = NetUid::from(2);
            let second_netuid = NetUid::from(3);
            TotalHotkeyAlpha::<Test>::insert(U256::from(1), first_netuid, AlphaBalance::from(10));
            TotalHotkeyAlpha::<Test>::insert(U256::from(2), first_netuid, AlphaBalance::from(20));
            TotalHotkeyAlpha::<Test>::insert(U256::from(3), second_netuid, AlphaBalance::from(7));

            let weight = migrate_total_alpha_staked::<Test>();

            assert_eq!(TotalAlphaStaked::<Test>::get(first_netuid), 30.into());
            assert_eq!(TotalAlphaStaked::<Test>::get(second_netuid), 7.into());
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            assert_eq!(
                weight,
                <Test as frame_system::Config>::DbWeight::get().reads_writes(4, 3)
            );

            TotalHotkeyAlpha::<Test>::insert(U256::from(4), first_netuid, AlphaBalance::from(100));
            migrate_total_alpha_staked::<Test>();
            assert_eq!(TotalAlphaStaked::<Test>::get(first_netuid), 30.into());
        });
    }
}
