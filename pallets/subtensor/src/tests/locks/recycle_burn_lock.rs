#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Recycle/burn alpha checks against lock.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 14: Recycle/burn alpha checks against lock
// =========================================================================

#[test]
fn test_recycle_alpha_checks_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(&coldkey, netuid, &hotkey, total));

        step_block(1);

        // Unstake should be blocked
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

        // recycle_alpha checks lock and should fail if it would reduce alpha below locked amount
        let recycle_amount = alpha / 2.into();
        assert_noop!(
            SubtensorModule::do_recycle_alpha(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                recycle_amount,
                netuid,
            ),
            Error::<Test>::StakeUnavailable
        );

        // Alpha is not below locked_mass
        let total_after = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let locked = SubtensorModule::get_current_locked(&coldkey, netuid);
        assert!(total_after >= locked);
    });
}

#[test]
fn test_burn_alpha_checks_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, total
        ));

        step_block(1);

        // burn_alpha checks lock and should fail if it would reduce alpha below locked amount
        let alpha = get_alpha(&hotkey, &coldkey, netuid);
        let burn_amount = alpha / 2.into();
        assert_noop!(
            SubtensorModule::do_burn_alpha(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                burn_amount,
                netuid,
            ),
            Error::<Test>::StakeUnavailable
        );

        // Alpha is not below locked_mass
        let total_after = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let locked = SubtensorModule::get_current_locked(&coldkey, netuid);
        assert!(total_after >= locked);
    });
}
