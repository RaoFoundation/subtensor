#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! tao-in refund deployment block + subnet hotkey lock-swap repair.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_tao_in_refund_deployment_block() {
    new_test_ext(1).execute_with(|| {
        let deployment_block: u64 = 42;
        let migration_name = b"migrate_tao_in_refund_deployment_block".to_vec();

        TaoInRefundDeploymentBlock::<Test>::put(0);
        HasMigrationRun::<Test>::remove(&migration_name);

        run_to_block(deployment_block);
        crate::migrations::migrate_tao_in_refund_deployment_block::migrate_tao_in_refund_deployment_block::<Test>();

        assert_eq!(TaoInRefundDeploymentBlock::<Test>::get(), deployment_block);
        assert!(HasMigrationRun::<Test>::get(&migration_name));

        run_to_block(deployment_block.saturating_add(1));
        crate::migrations::migrate_tao_in_refund_deployment_block::migrate_tao_in_refund_deployment_block::<Test>();

        assert_eq!(TaoInRefundDeploymentBlock::<Test>::get(), deployment_block);
    });
}

#[test]
fn test_migrate_fix_subnet_hotkey_lock_swaps_moves_or_discards_conflicts() {
    new_test_ext(1).execute_with(|| {
        let migration_name = b"migrate_fix_subnet_hotkey_lock_swaps".to_vec();
        let old_hotkey =
            decode_account_id32::<Test>("5Ca8L8PkbqXUtzohKtSM3i1naGQxANGLx51kJsEPNB14Admz")
                .expect("old hotkey should decode");
        let new_hotkey =
            decode_account_id32::<Test>("5Evgh9QTXJLxYLusVy3tcY5S6Z3GgRSNDb9AzXUchX5dco3P")
                .expect("new hotkey should decode");
        let netuid = NetUid::from(28);
        let coldkey_to_move = U256::from(1);
        let coldkey_with_conflict = U256::from(2);
        let chained_coldkey =
            decode_account_id32::<Test>("5EWUPMenvyvHdEGUHfUhSTeTDJDLzLkKZq74LFLRWtzcqZiS")
                .expect("chained coldkey should decode");
        let chained_first_hotkey =
            decode_account_id32::<Test>("5H3Kuy7L7DBSy7BS2c9EBayJYGkHV1pzWtnJm3iXvThT4VUJ")
                .expect("chained first hotkey should decode");
        let chained_middle_hotkey =
            decode_account_id32::<Test>("5CSiRF3sMKt1c3MT4KsRLBWENGkymVE7wA2zUDPsYy6JtpGE")
                .expect("chained middle hotkey should decode");
        let chained_final_hotkey =
            decode_account_id32::<Test>("5EsnHJK89FgF55EYwXtqhUwLu3c14xakyQ8PWoomcFwpxk5e")
                .expect("chained final hotkey should decode");
        let chained_netuid = NetUid::from(97);

        HasMigrationRun::<Test>::remove(&migration_name);

        let moved_lock = LockState {
            locked_mass: AlphaBalance::from(10_u64),
            conviction: U64F64::from_num(3),
            last_update: 11,
        };
        let discarded_lock = LockState {
            locked_mass: AlphaBalance::from(20_u64),
            conviction: U64F64::from_num(5),
            last_update: 12,
        };
        let existing_destination_lock = LockState {
            locked_mass: AlphaBalance::from(77_u64),
            conviction: U64F64::from_num(7),
            last_update: 10,
        };
        let chained_lock = LockState {
            locked_mass: AlphaBalance::from(33_u64),
            conviction: U64F64::from_num(4),
            last_update: 13,
        };

        Lock::<Test>::insert(
            (coldkey_to_move, netuid, old_hotkey),
            moved_lock.clone(),
        );
        LockingColdkeys::<Test>::insert((netuid, old_hotkey, coldkey_to_move), ());
        Lock::<Test>::insert(
            (coldkey_with_conflict, netuid, old_hotkey),
            discarded_lock.clone(),
        );
        LockingColdkeys::<Test>::insert((netuid, old_hotkey, coldkey_with_conflict), ());
        Lock::<Test>::insert(
            (coldkey_with_conflict, netuid, new_hotkey),
            existing_destination_lock.clone(),
        );
        LockingColdkeys::<Test>::insert((netuid, new_hotkey, coldkey_with_conflict), ());
        DecayingLock::<Test>::insert(coldkey_to_move, netuid, false);
        DecayingLock::<Test>::insert(coldkey_with_conflict, netuid, false);
        DecayingLock::<Test>::insert(chained_coldkey, chained_netuid, false);
        HotkeyLock::<Test>::insert(
            netuid,
            old_hotkey,
            LockState {
                locked_mass: AlphaBalance::from(30_u64),
                conviction: U64F64::from_num(8),
                last_update: 12,
            },
        );
        HotkeyLock::<Test>::insert(netuid, new_hotkey, existing_destination_lock.clone());
        Lock::<Test>::insert(
            (chained_coldkey, chained_netuid, chained_first_hotkey),
            chained_lock.clone(),
        );
        LockingColdkeys::<Test>::insert(
            (chained_netuid, chained_first_hotkey, chained_coldkey),
            (),
        );
        HotkeyLock::<Test>::insert(chained_netuid, chained_first_hotkey, chained_lock.clone());

        let weight =
            crate::migrations::migrate_fix_subnet_hotkey_lock_swaps::migrate_fix_subnet_hotkey_lock_swaps::<Test>();

        assert!(!weight.is_zero(), "migration weight should be non-zero");
        assert!(HasMigrationRun::<Test>::get(&migration_name));
        assert!(Lock::<Test>::get((coldkey_to_move, netuid, old_hotkey)).is_none());
        assert!(Lock::<Test>::get((coldkey_with_conflict, netuid, old_hotkey)).is_none());
        assert!(!LockingColdkeys::<Test>::contains_key((
            netuid,
            old_hotkey,
            coldkey_to_move
        )));
        assert!(!LockingColdkeys::<Test>::contains_key((
            netuid,
            old_hotkey,
            coldkey_with_conflict
        )));
        assert_eq!(
            Lock::<Test>::get((coldkey_to_move, netuid, new_hotkey)),
            Some(moved_lock.clone())
        );
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid,
            new_hotkey,
            coldkey_to_move
        )));
        assert_eq!(
            Lock::<Test>::get((coldkey_with_conflict, netuid, new_hotkey)),
            Some(existing_destination_lock.clone())
        );
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid,
            new_hotkey,
            coldkey_with_conflict
        )));
        assert!(HotkeyLock::<Test>::get(netuid, old_hotkey).is_none());

        let new_aggregate = HotkeyLock::<Test>::get(netuid, new_hotkey)
            .expect("new aggregate should exist");
        assert_eq!(
            new_aggregate.locked_mass,
            existing_destination_lock
                .locked_mass
                .saturating_add(moved_lock.locked_mass)
        );
        assert_eq!(
            new_aggregate.conviction,
            existing_destination_lock
                .conviction
                .saturating_add(moved_lock.conviction)
        );
        assert!(Lock::<Test>::get((
            chained_coldkey,
            chained_netuid,
            chained_first_hotkey
        ))
        .is_none());
        assert!(Lock::<Test>::get((
            chained_coldkey,
            chained_netuid,
            chained_middle_hotkey
        ))
        .is_none());
        assert!(!LockingColdkeys::<Test>::contains_key((
            chained_netuid,
            chained_first_hotkey,
            chained_coldkey
        )));
        assert!(!LockingColdkeys::<Test>::contains_key((
            chained_netuid,
            chained_middle_hotkey,
            chained_coldkey
        )));
        assert_eq!(
            Lock::<Test>::get((chained_coldkey, chained_netuid, chained_final_hotkey)),
            Some(chained_lock.clone())
        );
        assert!(LockingColdkeys::<Test>::contains_key((
            chained_netuid,
            chained_final_hotkey,
            chained_coldkey
        )));
        assert!(HotkeyLock::<Test>::get(chained_netuid, chained_first_hotkey).is_none());
        assert!(HotkeyLock::<Test>::get(chained_netuid, chained_middle_hotkey).is_none());
        assert_eq!(
            HotkeyLock::<Test>::get(chained_netuid, chained_final_hotkey),
            Some(chained_lock)
        );
    });
}
