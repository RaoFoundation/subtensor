use crate::{Config, HasMigrationRun, Pallet, TotalAlphaStaked, TotalHotkeyAlpha};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{pallet_prelude::OptionQuery, storage_alias, traits::Get, weights::Weight};
use scale_info::TypeInfo;
use scale_info::prelude::string::String;
use sp_runtime::traits::Zero;
use sp_std::vec::Vec;
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

pub(crate) const MIGRATION_NAME: &[u8] = b"migrate_total_alpha_staked";

/// Persistent cursor for the bounded `TotalHotkeyAlpha` backfill.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub struct TotalAlphaStakedProgress {
    /// Last raw `TotalHotkeyAlpha` key processed. Empty starts at the first entry.
    pub cursor: Vec<u8>,
}

#[storage_alias]
pub type TotalAlphaStakedMigration<T: Config> =
    StorageValue<Pallet<T>, TotalAlphaStakedProgress, OptionQuery>;

/// True while the backfill cursor exists.
pub fn in_progress<T: Config>() -> bool {
    TotalAlphaStakedMigration::<T>::exists()
}

/// Apply a live `TotalHotkeyAlpha` mutation to `TotalAlphaStaked`.
///
/// During the paged backfill, only keys already behind the cursor are applied.
/// Unscanned keys are left alone so the later page adds their current value
/// once. After the cursor is gone, every mutation applies.
pub fn apply_live_delta<T: Config>(
    hotkey: &T::AccountId,
    netuid: NetUid,
    previous: AlphaBalance,
    new: AlphaBalance,
) {
    if previous == new || !should_apply_live_delta::<T>(hotkey, netuid) {
        return;
    }
    TotalAlphaStaked::<T>::mutate(netuid, |total| {
        *total = total.saturating_sub(previous).saturating_add(new);
    });
}

fn should_apply_live_delta<T: Config>(hotkey: &T::AccountId, netuid: NetUid) -> bool {
    let Some(progress) = TotalAlphaStakedMigration::<T>::get() else {
        return true;
    };
    if progress.cursor.is_empty() {
        return false;
    }
    TotalHotkeyAlpha::<T>::hashed_key_for(hotkey, netuid) <= progress.cursor
}

fn item_weight<T: Config>() -> Weight {
    // Map iteration read plus the per-netuid aggregate mutate.
    T::DbWeight::get().reads_writes(2, 1)
}

/// Schedule the backfill. Does no map walk in the upgrade block.
pub fn migrate_total_alpha_staked<T: Config>() -> Weight {
    let mut weight = T::DbWeight::get().reads(2);
    if HasMigrationRun::<T>::get(MIGRATION_NAME) || TotalAlphaStakedMigration::<T>::exists() {
        return weight;
    }

    TotalAlphaStakedMigration::<T>::put(TotalAlphaStakedProgress { cursor: Vec::new() });
    weight.saturating_accrue(T::DbWeight::get().writes(1));
    log::info!(
        "Migration '{}' scheduled for bounded on_idle execution",
        String::from_utf8_lossy(MIGRATION_NAME)
    );
    weight
}

/// Continue the backfill using no more than `limit`.
pub fn continue_total_alpha_staked<T: Config>(limit: Weight) -> Weight {
    let pass_overhead = T::DbWeight::get().reads_writes(1, 2);
    if !pass_overhead.all_lte(limit) {
        return Weight::zero();
    }

    let Some(mut progress) = TotalAlphaStakedMigration::<T>::get() else {
        return T::DbWeight::get().reads(1);
    };

    let work_limit = limit.saturating_sub(pass_overhead);
    let per_item = item_weight::<T>();
    let mut work_weight = Weight::zero();
    let mut last_key: Option<Vec<u8>> = None;

    let iter = if progress.cursor.is_empty() {
        TotalHotkeyAlpha::<T>::iter()
    } else {
        TotalHotkeyAlpha::<T>::iter_from(progress.cursor.clone())
    };

    for (hotkey, netuid, alpha) in iter {
        if !work_weight.saturating_add(per_item).all_lte(work_limit)
            && let Some(key) = last_key
        {
            progress.cursor = key;
            TotalAlphaStakedMigration::<T>::put(progress);
            return pass_overhead.saturating_add(work_weight);
        }

        work_weight.saturating_accrue(per_item);
        if !alpha.is_zero() {
            TotalAlphaStaked::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(alpha);
            });
        }
        last_key = Some(TotalHotkeyAlpha::<T>::hashed_key_for(&hotkey, netuid));
    }

    HasMigrationRun::<T>::insert(MIGRATION_NAME, true);
    TotalAlphaStakedMigration::<T>::kill();
    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(MIGRATION_NAME)
    );
    pass_overhead.saturating_add(work_weight)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tests::mock::*, *};
    use sp_core::U256;
    use subtensor_runtime_common::{AlphaBalance, NetUid};

    fn huge_limit() -> Weight {
        Weight::from_parts(u64::MAX, u64::MAX)
    }

    fn run_migration() {
        migrate_total_alpha_staked::<Test>();
        continue_total_alpha_staked::<Test>(huge_limit());
    }

    #[test]
    fn migration_backfills_each_subnet_once() {
        new_test_ext(1).execute_with(|| {
            let first_netuid = NetUid::from(2);
            let second_netuid = NetUid::from(3);
            TotalHotkeyAlpha::<Test>::insert(U256::from(1), first_netuid, AlphaBalance::from(10));
            TotalHotkeyAlpha::<Test>::insert(U256::from(2), first_netuid, AlphaBalance::from(20));
            TotalHotkeyAlpha::<Test>::insert(U256::from(3), second_netuid, AlphaBalance::from(7));

            run_migration();

            assert_eq!(TotalAlphaStaked::<Test>::get(first_netuid), 30.into());
            assert_eq!(TotalAlphaStaked::<Test>::get(second_netuid), 7.into());
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            assert!(!in_progress::<Test>());

            TotalHotkeyAlpha::<Test>::insert(U256::from(4), first_netuid, AlphaBalance::from(100));
            run_migration();
            assert_eq!(TotalAlphaStaked::<Test>::get(first_netuid), 30.into());
        });
    }

    #[test]
    fn upgrade_block_only_writes_the_cursor() {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(2);
            TotalHotkeyAlpha::<Test>::insert(U256::from(1), netuid, AlphaBalance::from(10));

            migrate_total_alpha_staked::<Test>();

            assert!(in_progress::<Test>());
            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            assert!(TotalAlphaStaked::<Test>::get(netuid).is_zero());
        });
    }

    #[test]
    fn live_deltas_apply_only_behind_the_cursor() {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(2);
            let first = U256::from(1);
            let second = U256::from(2);
            TotalHotkeyAlpha::<Test>::insert(first, netuid, AlphaBalance::from(10));
            TotalHotkeyAlpha::<Test>::insert(second, netuid, AlphaBalance::from(20));

            migrate_total_alpha_staked::<Test>();
            let one_item = <Test as frame_system::Config>::DbWeight::get().reads_writes(3, 3);
            continue_total_alpha_staked::<Test>(one_item);

            let Some(progress) = TotalAlphaStakedMigration::<Test>::get() else {
                panic!("cursor remains after a partial page");
            };
            let cursor = progress.cursor;
            assert!(!cursor.is_empty());

            let (behind, ahead) =
                if TotalHotkeyAlpha::<Test>::hashed_key_for(first, netuid) <= cursor {
                    (first, second)
                } else {
                    (second, first)
                };
            assert_eq!(
                TotalAlphaStaked::<Test>::get(netuid),
                TotalHotkeyAlpha::<Test>::get(behind, netuid)
            );

            apply_live_delta::<Test>(
                &behind,
                netuid,
                TotalHotkeyAlpha::<Test>::get(behind, netuid),
                AlphaBalance::from(40),
            );
            TotalHotkeyAlpha::<Test>::insert(behind, netuid, AlphaBalance::from(40));
            assert_eq!(TotalAlphaStaked::<Test>::get(netuid), 40.into());

            apply_live_delta::<Test>(
                &ahead,
                netuid,
                TotalHotkeyAlpha::<Test>::get(ahead, netuid),
                AlphaBalance::from(50),
            );
            TotalHotkeyAlpha::<Test>::insert(ahead, netuid, AlphaBalance::from(50));
            assert_eq!(TotalAlphaStaked::<Test>::get(netuid), 40.into());

            continue_total_alpha_staked::<Test>(huge_limit());

            assert_eq!(TotalAlphaStaked::<Test>::get(netuid), 90.into());
            assert!(!in_progress::<Test>());
        });
    }
}
