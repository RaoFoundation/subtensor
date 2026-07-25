#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Move/transfer invariant enforcement.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 7: Move/transfer invariant enforcement
// =========================================================================

#[test]
fn test_move_stake_same_coldkey_same_subnet_allowed() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_a = U256::from(2);
        let hotkey_b = U256::from(3);
        let netuid = setup_subnet_with_stake(coldkey, hotkey_a, 100_000_000_000);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey, &hotkey_b
        ));

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        // Lock the full amount to hotkey_a
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey_a, total
        ));

        // Move from hotkey_a to hotkey_b on same subnet — total coldkey alpha unchanged
        let alpha = get_alpha(&hotkey_a, &coldkey, netuid);
        let move_amount = alpha / 2.into();
        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey_a,
            hotkey_b,
            netuid,
            netuid,
            move_amount,
        ));
    });
}

#[test]
fn test_do_transfer_stake_same_subnet_transfers_lock_to_destination_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey_sender = U256::from(1);
        let coldkey_receiver = U256::from(5);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey_sender, hotkey, 100_000_000_000);
        DecayingLock::<Test>::insert(coldkey_receiver, netuid, false);
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(coldkey_receiver),
            false,
        ));

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_sender, netuid);
        let lock_half = total / 2.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey_sender,
            netuid,
            &hotkey,
            lock_half,
        ));

        let sender_lock_before =
            Lock::<Test>::get((coldkey_sender, netuid, hotkey)).expect("sender lock should exist");
        let hotkey_lock_before =
            HotkeyLock::<Test>::get(netuid, hotkey).expect("hotkey lock should exist");

        step_block(1);

        let transfer_amount = total;
        assert_ok!(SubtensorModule::do_transfer_stake(
            RuntimeOrigin::signed(coldkey_sender),
            coldkey_receiver,
            hotkey,
            netuid,
            netuid,
            transfer_amount,
        ));

        let expected_sender_lock = roll_forward_lock(
            sender_lock_before,
            SubtensorModule::get_current_block_as_u64(),
            false,
            true,
        );

        assert!(Lock::<Test>::get((coldkey_sender, netuid, hotkey)).is_none());

        let receiver_lock = Lock::<Test>::get((coldkey_receiver, netuid, hotkey))
            .expect("receiver lock should exist after transfer");
        assert_eq!(receiver_lock.locked_mass, expected_sender_lock.locked_mass);
        assert!(receiver_lock.conviction > U64F64::from_num(0));
        assert!(receiver_lock.conviction <= expected_sender_lock.conviction);

        let hotkey_lock_after =
            HotkeyLock::<Test>::get(netuid, hotkey).expect("hotkey lock should remain");
        let expected_hotkey_lock = roll_forward_lock(
            hotkey_lock_before,
            SubtensorModule::get_current_block_as_u64(),
            false,
            true,
        );
        assert_eq!(
            hotkey_lock_after.locked_mass,
            expected_hotkey_lock.locked_mass
        );
    });
}

// Regression test: a same-subnet transfer that changes the hotkey must move the
// individual lock and the aggregate lock to the destination hotkey. Before the
// fix the recipient's lock (and aggregate conviction) stayed on the origin
// hotkey while the stake landed on the destination hotkey.
#[test]
fn test_do_transfer_stake_and_hotkey_same_subnet_moves_lock_to_destination_hotkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey_sender = U256::from(1);
        let coldkey_receiver = U256::from(5);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(6);
        let netuid = setup_subnet_with_stake(coldkey_sender, origin_hotkey, 100_000_000_000);

        // The destination hotkey is owned by the receiving coldkey, so origin and
        // destination hotkeys have different owners.
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey_receiver,
            &destination_hotkey
        ));
        DecayingLock::<Test>::insert(coldkey_receiver, netuid, false);
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(coldkey_receiver),
            false,
        ));

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_sender, netuid);
        let lock_half = total / 2.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey_sender,
            netuid,
            &origin_hotkey,
            lock_half,
        ));

        let sender_lock_before = Lock::<Test>::get((coldkey_sender, netuid, origin_hotkey))
            .expect("sender lock should exist");

        step_block(1);

        // Transfer the whole position (unlocked and locked halves) to the
        // destination coldkey and hotkey.
        assert_ok!(SubtensorModule::do_transfer_stake_and_hotkey(
            RuntimeOrigin::signed(coldkey_sender),
            coldkey_receiver,
            origin_hotkey,
            destination_hotkey,
            netuid,
            netuid,
            total,
        ));

        let expected_sender_lock = roll_forward_lock(
            sender_lock_before,
            SubtensorModule::get_current_block_as_u64(),
            false,
            true,
        );

        // The sender's lock is fully transferred away.
        assert!(Lock::<Test>::get((coldkey_sender, netuid, origin_hotkey)).is_none());

        // The receiver's lock follows the stake to the destination hotkey and
        // does not stay stranded on the origin hotkey.
        assert!(Lock::<Test>::get((coldkey_receiver, netuid, origin_hotkey)).is_none());
        let receiver_lock = Lock::<Test>::get((coldkey_receiver, netuid, destination_hotkey))
            .expect("receiver lock should exist on the destination hotkey");
        assert_eq!(receiver_lock.locked_mass, expected_sender_lock.locked_mass);

        // The hotkeys are owned by different coldkeys, so the transferred
        // conviction is forfeited, mirroring do_move_lock.
        assert_eq!(receiver_lock.conviction, U64F64::from_num(0));

        // The aggregate lock moves off the origin hotkey and onto the destination hotkey.
        assert!(
            HotkeyLock::<Test>::get(netuid, origin_hotkey)
                .map(|lock| lock.locked_mass)
                .unwrap_or(AlphaBalance::ZERO)
                .is_zero()
        );
        let destination_hotkey_lock = HotkeyLock::<Test>::get(netuid, destination_hotkey)
            .expect("destination hotkey aggregate lock should exist");
        assert_eq!(
            destination_hotkey_lock.locked_mass,
            expected_sender_lock.locked_mass
        );
    });
}

// When origin and destination hotkeys share an owning coldkey, the transferred
// conviction follows the lock to the destination hotkey instead of being forfeited.
#[test]
fn test_do_transfer_stake_and_hotkey_same_owner_preserves_conviction() {
    new_test_ext(1).execute_with(|| {
        let coldkey_sender = U256::from(1);
        let coldkey_receiver = U256::from(5);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(6);
        let netuid = setup_subnet_with_stake(coldkey_sender, origin_hotkey, 100_000_000_000);

        // Both hotkeys are owned by the sending coldkey.
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey_sender,
            &destination_hotkey
        ));
        DecayingLock::<Test>::insert(coldkey_receiver, netuid, false);
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(coldkey_receiver),
            false,
        ));

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_sender, netuid);
        let lock_half = total / 2.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey_sender,
            netuid,
            &origin_hotkey,
            lock_half,
        ));

        let sender_lock_before = Lock::<Test>::get((coldkey_sender, netuid, origin_hotkey))
            .expect("sender lock should exist");

        step_block(1);

        assert_ok!(SubtensorModule::do_transfer_stake_and_hotkey(
            RuntimeOrigin::signed(coldkey_sender),
            coldkey_receiver,
            origin_hotkey,
            destination_hotkey,
            netuid,
            netuid,
            total,
        ));

        let expected_sender_lock = roll_forward_lock(
            sender_lock_before,
            SubtensorModule::get_current_block_as_u64(),
            false,
            true,
        );

        let receiver_lock = Lock::<Test>::get((coldkey_receiver, netuid, destination_hotkey))
            .expect("receiver lock should exist on the destination hotkey");
        assert_eq!(receiver_lock.locked_mass, expected_sender_lock.locked_mass);

        // Same-owner hotkey change: the conviction moved with the lock.
        assert!(receiver_lock.conviction > U64F64::from_num(0));
        assert!(receiver_lock.conviction <= expected_sender_lock.conviction);
    });
}

// The LockHotkeyMismatch guard is checked against the hotkey the stake lands on:
// a recipient with an existing lock can only receive locked alpha onto that same
// hotkey, and transfers targeting any other hotkey are rejected.
#[test]
fn test_do_transfer_stake_and_hotkey_locked_requires_destination_match_receiver_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey_sender = U256::from(1);
        let coldkey_receiver = U256::from(5);
        let origin_hotkey = U256::from(2);
        let receiver_lock_hotkey = U256::from(6);
        let other_hotkey = U256::from(7);
        let netuid = setup_subnet_with_stake(coldkey_sender, origin_hotkey, 100_000_000_000);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey_receiver,
            &receiver_lock_hotkey
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey_receiver,
            &other_hotkey
        ));
        DecayingLock::<Test>::insert(coldkey_receiver, netuid, false);
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(coldkey_receiver),
            false,
        ));

        // The receiver already has an active lock on receiver_lock_hotkey.
        let receiver_locked = AlphaBalance::from(1_000_000u64);
        SubtensorModule::insert_lock_state(
            &coldkey_receiver,
            netuid,
            &receiver_lock_hotkey,
            LockState {
                locked_mass: receiver_locked,
                conviction: U64F64::from_num(0),
                last_update: SubtensorModule::get_current_block_as_u64(),
            },
        );

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_sender, netuid);
        let lock_half = total / 2.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey_sender,
            netuid,
            &origin_hotkey,
            lock_half,
        ));
        let sender_lock_before = Lock::<Test>::get((coldkey_sender, netuid, origin_hotkey))
            .expect("sender lock should exist");

        step_block(1);

        // Locked alpha targeting a hotkey other than the receiver's lock hotkey fails.
        assert_noop!(
            SubtensorModule::do_transfer_stake_and_hotkey(
                RuntimeOrigin::signed(coldkey_sender),
                coldkey_receiver,
                origin_hotkey,
                other_hotkey,
                netuid,
                netuid,
                total,
            ),
            Error::<Test>::LockHotkeyMismatch
        );

        // Targeting the receiver's lock hotkey succeeds even though it differs
        // from the origin hotkey (the pre-fix check compared against the origin
        // hotkey and would have rejected this).
        assert_ok!(SubtensorModule::do_transfer_stake_and_hotkey(
            RuntimeOrigin::signed(coldkey_sender),
            coldkey_receiver,
            origin_hotkey,
            receiver_lock_hotkey,
            netuid,
            netuid,
            total,
        ));

        let expected_sender_lock = roll_forward_lock(
            sender_lock_before,
            SubtensorModule::get_current_block_as_u64(),
            false,
            true,
        );
        let receiver_lock = Lock::<Test>::get((coldkey_receiver, netuid, receiver_lock_hotkey))
            .expect("receiver lock should exist on its lock hotkey");
        assert_eq!(
            receiver_lock.locked_mass,
            receiver_locked.saturating_add(expected_sender_lock.locked_mass)
        );
    });
}

#[test]
fn test_move_stake_cross_subnet_blocked_by_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid_a = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let subnet_owner2_ck = U256::from(2001);
        let subnet_owner2_hk = U256::from(2002);
        let netuid_b = add_dynamic_network(&subnet_owner2_hk, &subnet_owner2_ck);
        setup_reserves(
            netuid_b,
            (100_000_000_000u64 * 1_000_000).into(),
            (100_000_000_000u64 * 10_000_000).into(),
        );

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid_a);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid_a, &hotkey, total
        ));

        step_block(1);

        let alpha = get_alpha(&hotkey, &coldkey, netuid_a);
        assert_noop!(
            SubtensorModule::do_move_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                hotkey,
                netuid_a,
                netuid_b,
                alpha,
            ),
            Error::<Test>::StakeUnavailable
        );
    });
}

#[test]
fn test_do_transfer_stake_rejects_locked_alpha_to_flagged_destination() {
    new_test_ext(1).execute_with(|| {
        let coldkey_sender = U256::from(1);
        let coldkey_receiver = U256::from(5);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey_sender, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_sender, netuid);
        let lock_half = total / 2.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey_sender,
            netuid,
            &hotkey,
            lock_half,
        ));
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(coldkey_receiver),
            true,
        ));

        let sender_lock_before =
            Lock::<Test>::get((coldkey_sender, netuid, hotkey)).expect("sender lock should exist");
        let sender_alpha_before =
            SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_sender, netuid);
        let receiver_alpha_before =
            SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_receiver, netuid);

        assert_noop!(
            SubtensorModule::do_transfer_stake(
                RuntimeOrigin::signed(coldkey_sender),
                coldkey_receiver,
                hotkey,
                netuid,
                netuid,
                total,
            ),
            Error::<Test>::AccountRejectsLockedAlpha
        );

        assert_eq!(
            Lock::<Test>::get((coldkey_sender, netuid, hotkey)),
            Some(sender_lock_before)
        );
        assert!(Lock::<Test>::get((coldkey_receiver, netuid, hotkey)).is_none());
        assert_eq!(
            SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_sender, netuid),
            sender_alpha_before
        );
        assert_eq!(
            SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_receiver, netuid),
            receiver_alpha_before
        );
    });
}

#[test]
fn test_do_transfer_stake_allows_unlocked_alpha_to_flagged_destination() {
    new_test_ext(1).execute_with(|| {
        let coldkey_sender = U256::from(1);
        let coldkey_receiver = U256::from(5);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey_sender, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_sender, netuid);
        let lock_half = total / 2.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey_sender,
            netuid,
            &hotkey,
            lock_half,
        ));
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(coldkey_receiver),
            true,
        ));

        let unlocked_transfer = lock_half / 2.into();
        assert_ok!(SubtensorModule::do_transfer_stake(
            RuntimeOrigin::signed(coldkey_sender),
            coldkey_receiver,
            hotkey,
            netuid,
            netuid,
            unlocked_transfer,
        ));

        assert!(Lock::<Test>::get((coldkey_receiver, netuid, hotkey)).is_none());
        assert_eq!(
            SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_receiver, netuid),
            unlocked_transfer
        );
    });
}

#[test]
fn test_transfer_stake_cross_coldkey_allowed_partial() {
    new_test_ext(1).execute_with(|| {
        let coldkey_sender = U256::from(1);
        let coldkey_receiver = U256::from(5);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey_sender, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey_sender, netuid);
        let lock_half = total / 2.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey_sender,
            netuid,
            &hotkey,
            lock_half,
        ));

        let sender_lock_before =
            Lock::<Test>::get((coldkey_sender, netuid, hotkey)).expect("sender lock should exist");

        step_block(1);

        // Transfer the unlocked portion
        let alpha = get_alpha(&hotkey, &coldkey_sender, netuid);
        let transfer_amount = alpha / 4.into(); // well within the unlocked half
        assert_ok!(SubtensorModule::do_transfer_stake(
            RuntimeOrigin::signed(coldkey_sender),
            coldkey_receiver,
            hotkey,
            netuid,
            netuid,
            transfer_amount,
        ));

        let sender_lock_after =
            Lock::<Test>::get((coldkey_sender, netuid, hotkey)).expect("sender lock should remain");
        assert_eq!(
            sender_lock_after.locked_mass,
            roll_forward_lock(sender_lock_before, 2, false, true).locked_mass
        );
        assert!(Lock::<Test>::get((coldkey_receiver, netuid, hotkey)).is_none());
    });
}
