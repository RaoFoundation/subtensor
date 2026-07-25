#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Subnet dissolution.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 15: Subnet dissolution
// =========================================================================

#[test]
fn test_subnet_dissolution_orphans_locks() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            5000u64.into(),
        ));
        assert!(Lock::<Test>::get((coldkey, netuid, hotkey)).is_some());

        // Dissolve the subnet
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        run_block_idle();

        // All Alpha entries are gone
        assert_eq!(
            SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid),
            AlphaBalance::ZERO
        );

        // Lock entries are not orphaned
        let lock = Lock::<Test>::get((coldkey, netuid, hotkey));
        assert!(lock.is_none());

        // Hotkey lock is also removed
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey);
        assert!(hotkey_lock.is_none());
    });
}

#[test]
fn test_subnet_dissolution_and_netuid_reuse() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_old = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey_old, 100_000_000_000);

        // Lock on the old subnet
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey_old,
            5000u64.into(),
        ));

        // Dissolve old subnet
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        run_block_idle();

        // No stale lock from old subnet remains
        let stale_lock = Lock::<Test>::get((coldkey, netuid, hotkey_old));
        assert!(stale_lock.is_none());

        // No stale hotkey lock remains
        let stale_hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey_old);
        assert!(stale_hotkey_lock.is_none());
    });
}
