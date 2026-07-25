#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Lock rejection cases.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 4: Lock rejection cases
// =========================================================================

#[test]
fn test_lock_stake_zero_amount() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        assert_noop!(
            SubtensorModule::do_lock_stake(&coldkey, netuid, &hotkey, AlphaBalance::ZERO,),
            Error::<Test>::AmountTooLow
        );
    });
}

#[test]
fn test_lock_stake_exceeds_total_alpha() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let too_much = total + 1.into();

        assert_noop!(
            SubtensorModule::do_lock_stake(&coldkey, netuid, &hotkey, too_much),
            Error::<Test>::InsufficientStakeForLock
        );
    });
}

#[test]
fn test_lock_stake_wrong_hotkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_a = U256::from(2);
        let hotkey_b = U256::from(3);
        let netuid = setup_subnet_with_stake(coldkey, hotkey_a, 100_000_000_000);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey, &hotkey_b
        ));

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey_a,
            1000u64.into(),
        ));

        assert_noop!(
            SubtensorModule::do_lock_stake(&coldkey, netuid, &hotkey_b, 500u64.into(),),
            Error::<Test>::LockHotkeyMismatch
        );
    });
}

#[test]
fn test_lock_stake_topup_exceeds_total() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        // Lock 80% initially
        let initial = total * 8.into() / 10.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, initial
        ));

        // Try to top up the remaining 30% (exceeds total by 10%)
        let topup = total * 3.into() / 10.into();
        assert_noop!(
            SubtensorModule::do_lock_stake(&coldkey, netuid, &hotkey, topup),
            Error::<Test>::InsufficientStakeForLock
        );
    });
}
