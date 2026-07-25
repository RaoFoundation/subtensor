#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Unstake invariant enforcement.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 6: Unstake invariant enforcement
// =========================================================================

#[test]
fn test_unstake_allowed_when_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let alpha = get_alpha(&hotkey, &coldkey, netuid);
        assert!(alpha > AlphaBalance::ZERO);

        assert_ok!(SubtensorModule::do_remove_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            alpha,
        ));
    });
}

#[test]
fn test_unstake_allowed_up_to_available() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let lock_amount = total / 2.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount
        ));

        // Unstake the unlocked half
        let alpha = get_alpha(&hotkey, &coldkey, netuid);
        let available_alpha: u64 = (alpha.to_u64()) / 2;
        // Need to step a block to pass rate limiter
        step_block(1);
        assert_ok!(SubtensorModule::do_remove_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            available_alpha.into(),
        ));
    });
}

#[test]
fn test_unstake_rolls_forward_existing_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let lock_amount = AlphaBalance::from(1_000_000_000u64);

        DecayingLock::<Test>::remove(coldkey, netuid);
        let lock_block = SubtensorModule::get_current_block_as_u64();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        step_block(100);
        let now = SubtensorModule::get_current_block_as_u64();
        let expected = roll_forward_decaying_hotkey_lock(
            LockState {
                locked_mass: lock_amount,
                conviction: U64F64::from_num(0),
                last_update: lock_block,
            },
            now,
        );

        assert_ok!(SubtensorModule::do_remove_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            lock_amount,
        ));

        assert_eq!(
            Lock::<Test>::get((coldkey, netuid, hotkey)).expect("lock should remain"),
            expected
        );
        let aggregate =
            DecayingHotkeyLock::<Test>::get(netuid, hotkey).expect("aggregate should remain");
        assert_eq!(aggregate.locked_mass, expected.locked_mass);
        assert_eq!(aggregate.last_update, now);
    });
}

#[test]
fn test_unstake_roll_forward_collects_decaying_lock_dust_from_hotkey_aggregate() {
    new_test_ext(1).execute_with(|| {
        const ONE_ALPHA: u64 = 1_000_000_000;
        const DUST_ALPHA: u64 = 100;
        const STAKE_TAO_RAO: u64 = 1_000 * 1_000_000_000;

        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let coldkey_1 = U256::from(2001);
        let coldkey_2 = U256::from(2002);
        let hotkey_1 = U256::from(3001);
        let hotkey_2 = U256::from(3002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        setup_reserves(
            netuid,
            (STAKE_TAO_RAO * 1_000).into(),
            (STAKE_TAO_RAO * 10_000).into(),
        );
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey_1, &hotkey_1
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey_1, &hotkey_2
        ));

        for coldkey in [coldkey_1, coldkey_2] {
            add_balance_to_coldkey_account(&coldkey, STAKE_TAO_RAO.into());
            SubtensorModule::stake_into_subnet(
                &hotkey_1,
                &coldkey,
                netuid,
                STAKE_TAO_RAO.into(),
                <Test as Config>::SwapInterface::max_price(),
                false,
            )
            .unwrap();
        }

        let lock_block = SubtensorModule::get_current_block_as_u64();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey_1,
            netuid,
            &hotkey_2,
            ONE_ALPHA.into(),
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey_2,
            netuid,
            &hotkey_2,
            DUST_ALPHA.into(),
        ));

        assert_eq!(
            DecayingHotkeyLock::<Test>::get(netuid, hotkey_2)
                .expect("decaying aggregate should exist")
                .locked_mass,
            AlphaBalance::from(ONE_ALPHA + DUST_ALPHA)
        );

        step_block(100);
        let now = SubtensorModule::get_current_block_as_u64();
        let rolled_large_lock = roll_forward_decaying_hotkey_lock(
            LockState {
                locked_mass: ONE_ALPHA.into(),
                conviction: U64F64::from_num(0),
                last_update: lock_block,
            },
            now,
        );

        assert_ok!(SubtensorModule::do_remove_stake(
            RuntimeOrigin::signed(coldkey_1),
            hotkey_1,
            netuid,
            ONE_ALPHA.into(),
        ));
        assert_eq!(
            Lock::<Test>::get((coldkey_1, netuid, hotkey_2)).expect("coldkey1 lock should remain"),
            rolled_large_lock
        );
        assert_eq!(
            DecayingHotkeyLock::<Test>::get(netuid, hotkey_2)
                .expect("decaying aggregate should remain")
                .locked_mass,
            rolled_large_lock
                .locked_mass
                .saturating_add(AlphaBalance::from(DUST_ALPHA))
        );

        assert_ok!(SubtensorModule::do_remove_stake(
            RuntimeOrigin::signed(coldkey_2),
            hotkey_1,
            netuid,
            ONE_ALPHA.into(),
        ));
        assert_eq!(
            DecayingHotkeyLock::<Test>::get(netuid, hotkey_2)
                .expect("decaying aggregate should remain")
                .locked_mass,
            rolled_large_lock.locked_mass
        );
    });
}

#[test]
fn test_unstake_blocked_by_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        // Lock the entire amount
        assert_ok!(SubtensorModule::do_lock_stake(&coldkey, netuid, &hotkey, total));

        step_block(1);

        let alpha = get_alpha(&hotkey, &coldkey, netuid);
        assert_noop!(
            SubtensorModule::do_remove_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                alpha,
            ),
            Error::<Test>::StakeUnavailable
        );
    });
}
