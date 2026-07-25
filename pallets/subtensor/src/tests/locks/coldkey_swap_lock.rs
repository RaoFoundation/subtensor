#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Coldkey swap interaction.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 11: Coldkey swap interaction
// =========================================================================

#[test]
fn test_coldkey_swap_swaps_lock() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(10);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(old_coldkey, hotkey, 100_000_000_000);

        assert_ok!(SubtensorModule::do_lock_stake(
            &old_coldkey,
            netuid,
            &hotkey,
            5000u64.into(),
        ));
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(new_coldkey),
            false,
        ));

        // Perform coldkey swap
        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey));

        // Lock removed on old coldkey
        assert!(
            Lock::<Test>::iter_prefix((old_coldkey, netuid))
                .next()
                .is_none()
        );
        assert!(!DecayingLock::<Test>::contains_key(old_coldkey, netuid));
        // New coldkey now has the lock
        assert!(Lock::<Test>::get((new_coldkey, netuid, hotkey)).is_some());
        assert_eq!(DecayingLock::<Test>::get(new_coldkey, netuid), Some(false));
        assert!(HotkeyLock::<Test>::contains_key(netuid, hotkey));
        assert!(!DecayingHotkeyLock::<Test>::contains_key(netuid, hotkey));
    });
}

#[test]
fn test_coldkey_swap_lock_blocks_unstake() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(10);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(old_coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&old_coldkey, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &old_coldkey,
            netuid,
            &hotkey,
            total,
        ));
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(new_coldkey),
            false,
        ));

        // Swap coldkey
        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey));

        step_block(1);

        // New coldkey should not be able to unstake
        let alpha = get_alpha(&hotkey, &new_coldkey, netuid);
        assert!(alpha > AlphaBalance::ZERO);
        assert_noop!(
            SubtensorModule::do_remove_stake(
                RuntimeOrigin::signed(new_coldkey),
                hotkey,
                netuid,
                alpha,
            ),
            Error::<Test>::StakeUnavailable
        );
    });
}

#[test]
// Conviction-only destination lock state is not active, so direct coldkey lock transfer is allowed.
fn test_coldkey_swap_allows_destination_conviction_only_lock() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(10);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(20);
        let netuid = subtensor_runtime_common::NetUid::from(1);

        let old_conviction = U64F64::from_num(777);
        let new_conviction = U64F64::from_num(111);

        SubtensorModule::insert_lock_state(
            &old_coldkey,
            netuid,
            &old_hotkey,
            LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction: old_conviction,
                last_update: SubtensorModule::get_current_block_as_u64(),
            },
        );
        DecayingLock::<Test>::insert(old_coldkey, netuid, false);
        SubtensorModule::insert_lock_state(
            &new_coldkey,
            netuid,
            &new_hotkey,
            LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction: new_conviction,
                last_update: SubtensorModule::get_current_block_as_u64(),
            },
        );

        assert_ok!(SubtensorModule::swap_coldkey_locks(
            &old_coldkey,
            &new_coldkey
        ));

        assert!(
            Lock::<Test>::iter_prefix((old_coldkey, netuid))
                .next()
                .is_none()
        );
        assert!(Lock::<Test>::get((new_coldkey, netuid, new_hotkey)).is_some());

        let swapped_lock = Lock::<Test>::get((new_coldkey, netuid, old_hotkey))
            .expect("source lock should be transferred");
        assert_eq!(swapped_lock.locked_mass, AlphaBalance::ZERO);
        assert_eq!(swapped_lock.conviction, old_conviction);
        assert_eq!(Lock::<Test>::iter_prefix((new_coldkey, netuid)).count(), 2);
        assert!(DecayingLock::<Test>::get(old_coldkey, netuid).is_none());
        assert_eq!(DecayingLock::<Test>::get(new_coldkey, netuid), Some(false));
    });
}

#[test]
// When the destination already has an active lock, coldkey lock transfer should fail
// before mutating either coldkey's lock state.
fn test_coldkey_swap_rejects_destination_lock() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(10);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(20);
        let netuid = subtensor_runtime_common::NetUid::from(1);

        let old_locked = AlphaBalance::from(7_000u64);
        let old_conviction = U64F64::from_num(77);

        let new_locked = AlphaBalance::from(999u64);
        let new_conviction = U64F64::from_num(11);

        SubtensorModule::insert_lock_state(
            &old_coldkey,
            netuid,
            &old_hotkey,
            LockState {
                locked_mass: old_locked,
                conviction: old_conviction,
                last_update: SubtensorModule::get_current_block_as_u64(),
            },
        );
        SubtensorModule::insert_lock_state(
            &new_coldkey,
            netuid,
            &new_hotkey,
            LockState {
                locked_mass: new_locked,
                conviction: new_conviction,
                last_update: SubtensorModule::get_current_block_as_u64(),
            },
        );

        assert_noop!(
            SubtensorModule::swap_coldkey_locks(&old_coldkey, &new_coldkey),
            Error::<Test>::ActiveLockExists
        );

        let source_lock = Lock::<Test>::get((old_coldkey, netuid, old_hotkey))
            .expect("source lock should remain after failed transfer");
        assert_eq!(source_lock.locked_mass, old_locked);
        assert_eq!(source_lock.conviction, old_conviction);
        let destination_lock = Lock::<Test>::get((new_coldkey, netuid, new_hotkey))
            .expect("destination lock should remain after failed transfer");
        assert_eq!(destination_lock.locked_mass, new_locked);
        assert_eq!(destination_lock.conviction, new_conviction);
        assert!(
            Lock::<Test>::get((new_coldkey, netuid, old_hotkey)).is_none(),
            "source lock should not be inserted under destination coldkey"
        );
        assert_eq!(Lock::<Test>::iter_prefix((new_coldkey, netuid)).count(), 1);
    });
}

#[test]
fn test_coldkey_swap_rejects_locked_alpha_to_flagged_destination() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(10);
        let old_hotkey = U256::from(2);
        let netuid = subtensor_runtime_common::NetUid::from(1);

        let old_locked = AlphaBalance::from(7_000u64);
        let old_conviction = U64F64::from_num(77);

        SubtensorModule::insert_lock_state(
            &old_coldkey,
            netuid,
            &old_hotkey,
            LockState {
                locked_mass: old_locked,
                conviction: old_conviction,
                last_update: SubtensorModule::get_current_block_as_u64(),
            },
        );
        DecayingLock::<Test>::insert(old_coldkey, netuid, false);
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(new_coldkey),
            true,
        ));

        assert_noop!(
            SubtensorModule::swap_coldkey_locks(&old_coldkey, &new_coldkey),
            Error::<Test>::AccountRejectsLockedAlpha
        );

        let source_lock = Lock::<Test>::get((old_coldkey, netuid, old_hotkey))
            .expect("source lock should remain after failed transfer");
        assert_eq!(source_lock.locked_mass, old_locked);
        assert_eq!(source_lock.conviction, old_conviction);
        assert!(
            Lock::<Test>::iter_prefix((new_coldkey, netuid))
                .next()
                .is_none()
        );
        assert_eq!(DecayingLock::<Test>::get(old_coldkey, netuid), Some(false));
        assert!(DecayingLock::<Test>::get(new_coldkey, netuid).is_none());
    });
}

#[test]
// The public coldkey swap extrinsic runs inside a storage layer, so a late failure rolls back the earlier writes.
fn test_failed_coldkey_swap_extrinsic_rolls_back_state_changes() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let new_coldkey = U256::from(3);
        let blocked_hotkey = U256::from(4);
        let netuid = setup_subnet_with_stake(old_coldkey, old_hotkey, 100_000_000_000);

        let original_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &old_coldkey,
            netuid,
        );
        assert!(!original_stake.is_zero());
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &new_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );

        // Seed a lock directly on the destination coldkey so the swap reaches ActiveLockExists
        // without tripping the earlier "already associated" guard.
        SubtensorModule::insert_lock_state(
            &new_coldkey,
            netuid,
            &blocked_hotkey,
            LockState {
                locked_mass: 1_000u64.into(),
                conviction: U64F64::from_num(0),
                last_update: SubtensorModule::get_current_block_as_u64(),
            },
        );

        assert_noop!(
            SubtensorModule::swap_coldkey(
                RuntimeOrigin::root(),
                old_coldkey,
                new_coldkey,
                TaoBalance::ZERO,
            ),
            Error::<Test>::ActiveLockExists
        );

        // The failed extrinsic should roll back the earlier stake transfer.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &old_coldkey,
                netuid
            ),
            original_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &new_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
    });
}
