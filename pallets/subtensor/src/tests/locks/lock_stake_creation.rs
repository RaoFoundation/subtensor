#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Green-path — basic lock creation.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 1: Green-path — basic lock creation
// =========================================================================

#[test]
fn test_lock_stake_creates_new_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let alpha = get_alpha(&hotkey, &coldkey, netuid);
        let lock_amount = alpha.to_u64() / 2;

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount.into(),
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).expect("Lock should exist");
        assert_eq!(lock.locked_mass, lock_amount.into());
        assert_eq!(lock.conviction, U64F64::from_num(0));
        assert_eq!(
            lock.last_update,
            SubtensorModule::get_current_block_as_u64()
        );

        // Hotkey lock should also be created
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey);
        assert!(hotkey_lock.is_some());
        assert_eq!(hotkey_lock.unwrap().locked_mass, lock_amount.into());
    });
}

#[test]
fn test_lock_stake_defaults_to_decaying_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        DecayingLock::<Test>::remove(coldkey, netuid);

        let lock_amount: AlphaBalance = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        assert!(DecayingLock::<Test>::get(coldkey, netuid).is_none());
        assert!(HotkeyLock::<Test>::get(netuid, hotkey).is_none());

        let decaying_hotkey_lock = DecayingHotkeyLock::<Test>::get(netuid, hotkey)
            .expect("default lock should use decaying aggregate");
        assert_eq!(decaying_hotkey_lock.locked_mass, lock_amount);
    });
}

#[test]
fn test_lock_stake_by_subnet_owner_coldkey_gets_immediate_conviction() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1);
        let owner_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(owner_coldkey, owner_hotkey, 300_000_000_000);
        SubnetOwner::<Test>::insert(netuid, owner_coldkey);
        SubnetOwnerHotkey::<Test>::insert(netuid, owner_hotkey);

        let lock_amount: AlphaBalance = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &owner_coldkey,
            netuid,
            &owner_hotkey,
            lock_amount,
        ));

        let lock = Lock::<Test>::get((owner_coldkey, netuid, owner_hotkey))
            .expect("lock to owner hotkey should exist");
        assert_eq!(lock.locked_mass, lock_amount);
        assert_eq!(lock.conviction, U64F64::saturating_from_num(5000));
        let owner_lock = OwnerLock::<Test>::get(netuid).expect("owner lock should exist");
        assert_eq!(owner_lock.locked_mass, lock_amount);
        assert_eq!(owner_lock.conviction, U64F64::saturating_from_num(5000));
    });
}

#[test]
fn test_lock_to_subnet_owner_hotkey_gets_immediate_conviction_for_non_owner_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let staker_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, staker_hotkey, 300_000_000_000);
        let owner_hotkey = SubnetOwnerHotkey::<Test>::get(netuid);

        let lock_amount: AlphaBalance = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &owner_hotkey,
            lock_amount,
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, owner_hotkey))
            .expect("lock to owner hotkey should exist");
        assert_eq!(lock.locked_mass, lock_amount);
        assert_eq!(lock.conviction, U64F64::saturating_from_num(5000));

        let owner_lock = OwnerLock::<Test>::get(netuid).expect("owner lock should exist");
        assert_eq!(owner_lock.locked_mass, lock_amount);
        assert_eq!(owner_lock.conviction, U64F64::saturating_from_num(5000));
        assert!(
            HotkeyLock::<Test>::get(netuid, owner_hotkey).is_none(),
            "lock to owner hotkey should use OwnerLock, not HotkeyLock"
        );
    });
}

#[test]
fn test_decaying_lock_to_subnet_owner_hotkey_keeps_decaying_mass() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let staker_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, staker_hotkey, 300_000_000_000);
        let owner_hotkey = SubnetOwnerHotkey::<Test>::get(netuid);

        assert_ok!(SubtensorModule::do_set_perpetual_lock(
            &coldkey, netuid, false,
        ));

        let lock_amount: AlphaBalance = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &owner_hotkey,
            lock_amount,
        ));

        step_block(1_000);
        let now = SubtensorModule::get_current_block_as_u64();
        let rolled = roll_forward_individual_lock(
            &coldkey,
            netuid,
            &owner_hotkey,
            Lock::<Test>::get((coldkey, netuid, owner_hotkey)).unwrap(),
            now,
        );

        assert!(rolled.locked_mass < lock_amount);
        assert_eq!(
            rolled.conviction,
            U64F64::saturating_from_num(u64::from(rolled.locked_mass))
        );
        assert_eq!(
            SubtensorModule::hotkey_conviction(&owner_hotkey, netuid),
            rolled.conviction
        );
        assert!(
            OwnerLock::<Test>::get(netuid).is_none(),
            "decaying lock to owner hotkey should not use perpetual OwnerLock"
        );
        assert!(
            DecayingOwnerLock::<Test>::get(netuid).is_some(),
            "decaying lock to owner hotkey should use DecayingOwnerLock"
        );
    });
}

#[test]
fn test_lock_by_subnet_owner_coldkey_to_non_owner_hotkey_matures_normally() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1);
        let non_owner_hotkey = U256::from(2);
        let owner_hotkey = U256::from(3);
        let netuid = setup_subnet_with_stake(owner_coldkey, non_owner_hotkey, 300_000_000_000);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_coldkey,
            &owner_hotkey
        ));
        SubnetOwner::<Test>::insert(netuid, owner_coldkey);
        SubnetOwnerHotkey::<Test>::insert(netuid, owner_hotkey);

        let lock_amount: AlphaBalance = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &owner_coldkey,
            netuid,
            &non_owner_hotkey,
            lock_amount,
        ));

        let lock = Lock::<Test>::get((owner_coldkey, netuid, non_owner_hotkey))
            .expect("lock to non-owner hotkey should exist");
        assert_eq!(lock.locked_mass, lock_amount);
        assert_eq!(lock.conviction, U64F64::saturating_from_num(0));
        assert!(
            OwnerLock::<Test>::get(netuid).is_none(),
            "owner coldkey lock to a non-owner hotkey should not use OwnerLock"
        );

        let hotkey_lock =
            HotkeyLock::<Test>::get(netuid, non_owner_hotkey).expect("hotkey lock should exist");
        assert_eq!(hotkey_lock.locked_mass, lock_amount);
        assert_eq!(hotkey_lock.conviction, U64F64::saturating_from_num(0));
    });
}

#[test]
fn test_lock_stake_topup_by_subnet_owner_coldkey_gets_immediate_conviction() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1);
        let owner_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(owner_coldkey, owner_hotkey, 100_000_000_000);
        SubnetOwner::<Test>::insert(netuid, owner_coldkey);
        SubnetOwnerHotkey::<Test>::insert(netuid, owner_hotkey);

        let first_lock: AlphaBalance = 5000u64.into();
        let second_lock: AlphaBalance = 7000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &owner_coldkey,
            netuid,
            &owner_hotkey,
            first_lock,
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &owner_coldkey,
            netuid,
            &owner_hotkey,
            second_lock,
        ));

        let expected_locked = first_lock + second_lock;
        let lock = Lock::<Test>::get((owner_coldkey, netuid, owner_hotkey))
            .expect("lock to owner hotkey should exist");
        assert_eq!(lock.locked_mass, expected_locked);
        assert_eq!(
            lock.conviction,
            U64F64::saturating_from_num(u64::from(expected_locked))
        );

        let owner_lock = OwnerLock::<Test>::get(netuid).expect("owner lock should exist");
        assert_eq!(owner_lock.locked_mass, expected_locked);
        assert_eq!(
            owner_lock.conviction,
            U64F64::saturating_from_num(u64::from(expected_locked))
        );
    });
}

#[test]
fn test_set_perpetual_lock_toggles_owner_lock_decay() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1);
        let owner_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(owner_coldkey, owner_hotkey, 100_000_000_000);
        SubnetOwner::<Test>::insert(netuid, owner_coldkey);
        SubnetOwnerHotkey::<Test>::insert(netuid, owner_hotkey);

        let lock_amount: AlphaBalance = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &owner_coldkey,
            netuid,
            &owner_hotkey,
            lock_amount,
        ));

        assert_ok!(SubtensorModule::set_perpetual_lock(
            RuntimeOrigin::signed(owner_coldkey),
            netuid,
            true,
        ));
        step_block(100);
        assert_eq!(
            SubtensorModule::get_current_locked(&owner_coldkey, netuid),
            lock_amount
        );

        assert_ok!(SubtensorModule::set_perpetual_lock(
            RuntimeOrigin::signed(owner_coldkey),
            netuid,
            false,
        ));
        step_block(100);
        assert!(SubtensorModule::get_current_locked(&owner_coldkey, netuid) < lock_amount);
    });
}

#[test]
fn test_set_perpetual_lock_is_per_coldkey_and_rolls_lock_at_boundary() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 300_000_000_000);

        let lock_amount: AlphaBalance = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        assert_ok!(SubtensorModule::set_perpetual_lock(
            RuntimeOrigin::signed(coldkey),
            netuid,
            false,
        ));
        System::set_block_number(System::block_number() + UnlockRate::<Test>::get() / 10);
        assert_ok!(SubtensorModule::set_perpetual_lock(
            RuntimeOrigin::signed(coldkey),
            netuid,
            true,
        ));

        let locked_at_boundary = SubtensorModule::get_current_locked(&coldkey, netuid);
        assert!(locked_at_boundary < lock_amount);

        System::set_block_number(System::block_number() + UnlockRate::<Test>::get() / 10);
        assert_eq!(
            SubtensorModule::get_current_locked(&coldkey, netuid),
            locked_at_boundary
        );

        assert_ok!(SubtensorModule::set_perpetual_lock(
            RuntimeOrigin::signed(coldkey),
            netuid,
            false,
        ));
        System::set_block_number(System::block_number() + UnlockRate::<Test>::get() / 10);
        assert!(SubtensorModule::get_current_locked(&coldkey, netuid) < locked_at_boundary);
    });
}

#[test]
fn test_mixed_perpetual_and_decaying_non_owner_locks_same_hotkey_update_aggregates() {
    new_test_ext(1).execute_with(|| {
        let perpetual_coldkey = U256::from(1);
        let decaying_coldkey = U256::from(3);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(perpetual_coldkey, hotkey, 100_000_000_000);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &decaying_coldkey,
            &hotkey
        ));
        add_balance_to_coldkey_account(&decaying_coldkey, 100_000_000_000u64.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &decaying_coldkey,
            netuid,
            100_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        let lock_amount: AlphaBalance = 10_000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &perpetual_coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &decaying_coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));
        assert_ok!(SubtensorModule::do_set_perpetual_lock(
            &decaying_coldkey,
            netuid,
            false,
        ));

        step_block(1_000);
        let now = SubtensorModule::get_current_block_as_u64();

        let perpetual_lock = roll_forward_individual_lock(
            &perpetual_coldkey,
            netuid,
            &hotkey,
            Lock::<Test>::get((perpetual_coldkey, netuid, hotkey)).unwrap(),
            now,
        );
        let decaying_lock = roll_forward_individual_lock(
            &decaying_coldkey,
            netuid,
            &hotkey,
            Lock::<Test>::get((decaying_coldkey, netuid, hotkey)).unwrap(),
            now,
        );
        let perpetual_hotkey_lock =
            roll_forward_hotkey_lock(HotkeyLock::<Test>::get(netuid, hotkey).unwrap(), now);
        let decaying_hotkey_lock = roll_forward_decaying_hotkey_lock(
            DecayingHotkeyLock::<Test>::get(netuid, hotkey).unwrap(),
            now,
        );

        assert_eq!(perpetual_lock.locked_mass, lock_amount);
        assert_eq!(perpetual_hotkey_lock.locked_mass, lock_amount);
        assert!(decaying_lock.locked_mass < lock_amount);
        assert_eq!(decaying_hotkey_lock.locked_mass, decaying_lock.locked_mass);
        assert_eq!(
            SubtensorModule::hotkey_conviction(&hotkey, netuid),
            perpetual_hotkey_lock
                .conviction
                .saturating_add(decaying_hotkey_lock.conviction)
        );
    });
}

#[test]
#[ignore]
fn plot_perpetual_decay_perpetual_lock_curve() {
    new_test_ext(1).execute_with(|| {
        const ALPHA: u64 = 1_000_000_000;
        const ALPHA_F64: f64 = ALPHA as f64;

        let owner_coldkey = U256::from(1);
        let owner_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(owner_coldkey, owner_hotkey, 300_000_000_000);
        SubnetOwner::<Test>::insert(netuid, owner_coldkey);
        SubnetOwnerHotkey::<Test>::insert(netuid, owner_hotkey);
        MaturityRate::<Test>::put(300u64);
        UnlockRate::<Test>::put(200u64);

        let lock_amount: AlphaBalance = (1_000u64 * ALPHA).into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &owner_coldkey,
            netuid,
            &owner_hotkey,
            lock_amount,
        ));
        assert_ok!(SubtensorModule::do_set_perpetual_lock(
            &owner_coldkey,
            netuid,
            true,
        ));

        println!("block,locked_mass,conviction");
        for block in 0..=2_000u64 {
            System::set_block_number(block);

            if block == 1_000 {
                assert_ok!(SubtensorModule::do_set_perpetual_lock(
                    &owner_coldkey,
                    netuid,
                    false,
                ));
            } else if block == 1_200 {
                assert_ok!(SubtensorModule::do_set_perpetual_lock(
                    &owner_coldkey,
                    netuid,
                    true,
                ));
            }

            let lock = Lock::<Test>::get((owner_coldkey, netuid, owner_hotkey)).unwrap();
            let rolled =
                roll_forward_individual_lock(&owner_coldkey, netuid, &owner_hotkey, lock, block);
            SubtensorModule::insert_lock_state(
                &owner_coldkey,
                netuid,
                &owner_hotkey,
                rolled.clone(),
            );
            SubtensorModule::insert_owner_lock_state(netuid, rolled.clone());
            println!(
                "{},{},{}",
                block,
                u64::from(rolled.locked_mass) as f64 / ALPHA_F64,
                rolled.conviction.to_num::<f64>() / ALPHA_F64
            );
        }
    });
}

#[test]
#[ignore]
fn plot_decaying_non_owner_lock_curve() {
    new_test_ext(1).execute_with(|| {
        const ALPHA: u64 = 1_000_000_000;
        const ALPHA_F64: f64 = ALPHA as f64;

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 300_000_000_000);
        MaturityRate::<Test>::put(300u64);
        UnlockRate::<Test>::put(200u64);
        System::set_block_number(0);

        let lock_amount: AlphaBalance = (1_000u64 * ALPHA).into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));
        assert_ok!(SubtensorModule::do_set_perpetual_lock(
            &coldkey, netuid, false,
        ));

        println!("block,locked_mass,conviction");
        for block in 0..=2_000u64 {
            System::set_block_number(block);

            let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();
            let rolled = roll_forward_individual_lock(&coldkey, netuid, &hotkey, lock, block);
            SubtensorModule::insert_lock_state(&coldkey, netuid, &hotkey, rolled.clone());
            SubtensorModule::insert_hotkey_lock_state(netuid, &hotkey, rolled.clone());
            println!(
                "{},{},{}",
                block,
                u64::from(rolled.locked_mass) as f64 / ALPHA_F64,
                rolled.conviction.to_num::<f64>() / ALPHA_F64
            );
        }
    });
}

#[test]
#[ignore]
fn plot_perpetual_decay_perpetual_non_owner_lock_curve() {
    new_test_ext(1).execute_with(|| {
        const ALPHA: u64 = 1_000_000_000;
        const ALPHA_F64: f64 = ALPHA as f64;

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 1_000_000_000_000);
        MaturityRate::<Test>::put(300u64);
        UnlockRate::<Test>::put(200u64);
        System::set_block_number(0);

        let lock_amount: AlphaBalance = (1_000u64 * ALPHA).into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));
        assert_ok!(SubtensorModule::do_set_perpetual_lock(
            &coldkey, netuid, true,
        ));

        println!("block,locked_mass,conviction");
        for block in 0..=2_000u64 {
            System::set_block_number(block);

            if block == 1_000 {
                assert_ok!(SubtensorModule::do_set_perpetual_lock(
                    &coldkey, netuid, false,
                ));
            } else if block == 1_200 {
                assert_ok!(SubtensorModule::do_set_perpetual_lock(
                    &coldkey, netuid, true,
                ));
            }

            let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();
            let rolled = roll_forward_individual_lock(&coldkey, netuid, &hotkey, lock, block);
            SubtensorModule::insert_lock_state(&coldkey, netuid, &hotkey, rolled.clone());
            if DecayingLock::<Test>::get(coldkey, netuid) == Some(false) {
                SubtensorModule::insert_hotkey_lock_state(netuid, &hotkey, rolled.clone());
            } else {
                SubtensorModule::insert_decaying_hotkey_lock_state(netuid, &hotkey, rolled.clone());
            }
            println!(
                "{},{},{}",
                block,
                u64::from(rolled.locked_mass) as f64 / ALPHA_F64,
                rolled.conviction.to_num::<f64>() / ALPHA_F64
            );

            // Add more lock (emulate owner auto-lock)
            let auto_lock_amount: AlphaBalance = 200_000_000_u64.into();
            assert_ok!(SubtensorModule::do_lock_stake(
                &coldkey,
                netuid,
                &hotkey,
                auto_lock_amount,
            ));
        }
    });
}

#[test]
fn test_lock_stake_emits_event() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let lock_amount: u64 = 1000;

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount.into(),
        ));

        System::assert_last_event(
            Event::StakeLocked {
                coldkey,
                hotkey,
                netuid,
                amount: lock_amount.into(),
            }
            .into(),
        );
    });
}

#[test]
fn test_lock_stake_full_amount() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total_alpha = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        assert!(!total_alpha.is_zero());

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            total_alpha,
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();
        assert_eq!(lock.locked_mass, total_alpha);
    });
}
