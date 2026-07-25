#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Multi-subnet locks.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 8: Multi-subnet locks
// =========================================================================

#[test]
fn test_lock_on_multiple_subnets() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_a = U256::from(2);
        let hotkey_b = U256::from(3);

        let netuid_a = setup_subnet_with_stake(coldkey, hotkey_a, 100_000_000_000);

        let subnet_owner2_ck = U256::from(2001);
        let subnet_owner2_hk = U256::from(2002);
        let netuid_b = add_dynamic_network(&subnet_owner2_hk, &subnet_owner2_ck);
        setup_reserves(
            netuid_b,
            (100_000_000_000u64 * 1_000_000).into(),
            (100_000_000_000u64 * 10_000_000).into(),
        );
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey, &hotkey_b
        ));
        add_balance_to_coldkey_account(&coldkey, 100_000_000_000u64.into());
        SubtensorModule::stake_into_subnet(
            &hotkey_b,
            &coldkey,
            netuid_b,
            100_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        DecayingLock::<Test>::insert(coldkey, netuid_b, false);

        // Lock on subnet A to hotkey_a
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid_a,
            &hotkey_a,
            1000u64.into(),
        ));

        // Lock on subnet B to hotkey_b (different hotkey is fine — different subnet)
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid_b,
            &hotkey_b,
            2000u64.into(),
        ));

        let lock_a = Lock::<Test>::get((coldkey, netuid_a, hotkey_a)).unwrap();
        let lock_b = Lock::<Test>::get((coldkey, netuid_b, hotkey_b)).unwrap();
        assert_eq!(lock_a.locked_mass, 1000u64.into());
        assert_eq!(lock_b.locked_mass, 2000u64.into());

        // Hotkey locks should also be separate
        let hotkey_lock_a = HotkeyLock::<Test>::get(netuid_a, hotkey_a).unwrap();
        let hotkey_lock_b = HotkeyLock::<Test>::get(netuid_b, hotkey_b).unwrap();
        assert_eq!(hotkey_lock_a.locked_mass, 1000u64.into());
        assert_eq!(hotkey_lock_b.locked_mass, 2000u64.into());
    });
}

#[test]
fn test_unstake_one_subnet_does_not_affect_other() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid_a = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        // Lock on subnet A
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid_a,
            &hotkey,
            5000u64.into(),
        ));

        // Subnet B — no lock, just stake
        let subnet_owner2_ck = U256::from(2001);
        let subnet_owner2_hk = U256::from(2002);
        let netuid_b = add_dynamic_network(&subnet_owner2_hk, &subnet_owner2_ck);
        setup_reserves(
            netuid_b,
            (100_000_000_000u64 * 1_000_000).into(),
            (100_000_000_000u64 * 10_000_000).into(),
        );
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey, &hotkey
        ));
        add_balance_to_coldkey_account(&coldkey, 100_000_000_000u64.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid_b,
            100_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        step_block(1);

        // Unstake from subnet B — should succeed (no lock there)
        let alpha_b = get_alpha(&hotkey, &coldkey, netuid_b);
        assert_ok!(SubtensorModule::do_remove_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid_b,
            alpha_b,
        ));

        // Lock on subnet A unaffected
        let lock_a = Lock::<Test>::get((coldkey, netuid_a, hotkey)).unwrap();
        assert_eq!(lock_a.locked_mass, 5000u64.into());

        // Hotkey lock on subnet A also unaffected
        let hotkey_lock_a = HotkeyLock::<Test>::get(netuid_a, hotkey).unwrap();
        assert_eq!(hotkey_lock_a.locked_mass, 5000u64.into());
    });
}
