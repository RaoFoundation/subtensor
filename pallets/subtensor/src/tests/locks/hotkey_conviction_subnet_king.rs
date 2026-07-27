#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Hotkey conviction and subnet king.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 9: Hotkey conviction and subnet king
// =========================================================================

#[test]
fn test_hotkey_conviction_single_locker() {
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

        // Initially conviction is 0 (just created)
        let c = SubtensorModule::hotkey_conviction(&hotkey, netuid);
        assert_eq!(c, U64F64::from_num(0));

        // After time, conviction grows
        step_block(1000);
        let c = SubtensorModule::hotkey_conviction(&hotkey, netuid);
        assert!(c > U64F64::from_num(0));
    });
}

#[test]
fn test_hotkey_conviction_multiple_lockers() {
    new_test_ext(1).execute_with(|| {
        let coldkey1 = U256::from(1);
        let coldkey2 = U256::from(5);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey1, hotkey, 100_000_000_000);

        // Also give coldkey2 stake on same hotkey
        add_balance_to_coldkey_account(&coldkey2, 100_000_000_000u64.into());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey2, &hotkey
        ));
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey2,
            netuid,
            50_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey1,
            netuid,
            &hotkey,
            3000u64.into(),
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey2,
            netuid,
            &hotkey,
            2000u64.into(),
        ));

        step_block(500);

        let total_conviction = SubtensorModule::hotkey_conviction(&hotkey, netuid);
        let c1 = SubtensorModule::get_conviction(&coldkey1, netuid);
        let c2 = SubtensorModule::get_conviction(&coldkey2, netuid);

        // Total conviction should be approximately sum of individual convictions
        let diff = if total_conviction > (c1 + c2) {
            total_conviction - (c1 + c2)
        } else {
            (c1 + c2) - total_conviction
        };
        assert!(diff < U64F64::from_num(1));
    });
}

#[test]
fn test_mixed_perpetual_owner_and_decaying_non_owner_locks_roll_forward() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let owner_hotkey = U256::from(1002);
        let staker_coldkey = U256::from(1);
        let staker_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(staker_coldkey, staker_hotkey, 100_000_000_000);

        add_balance_to_coldkey_account(&owner_coldkey, 100_000_000_000u64.into());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_coldkey,
            &owner_hotkey
        ));
        SubtensorModule::stake_into_subnet(
            &owner_hotkey,
            &owner_coldkey,
            netuid,
            100_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        let owner_lock_amount = AlphaBalance::from(10_000u64);
        let staker_lock_amount = AlphaBalance::from(20_000u64);
        assert_ok!(SubtensorModule::do_lock_stake(
            &owner_coldkey,
            netuid,
            &owner_hotkey,
            owner_lock_amount,
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &staker_coldkey,
            netuid,
            &staker_hotkey,
            staker_lock_amount,
        ));
        assert_ok!(SubtensorModule::do_set_perpetual_lock(
            &owner_coldkey,
            netuid,
            true,
        ));

        System::set_block_number(System::block_number() + UnlockRate::<Test>::get());

        let owner_lock = roll_forward_lock(
            OwnerLock::<Test>::get(netuid).unwrap(),
            SubtensorModule::get_current_block_as_u64(),
            true,
            true,
        );
        let staker_lock = roll_forward_lock(
            HotkeyLock::<Test>::get(netuid, staker_hotkey).unwrap(),
            SubtensorModule::get_current_block_as_u64(),
            false,
            false,
        );

        assert_eq!(owner_lock.locked_mass, owner_lock_amount);
        assert_eq!(
            owner_lock.conviction,
            U64F64::from_num(u64::from(owner_lock_amount))
        );
        assert!(staker_lock.locked_mass < staker_lock_amount);
        assert!(staker_lock.conviction > U64F64::from_num(0));
    });
}

#[test]
fn test_total_conviction_equals_sum_of_participating_aggregate_convictions() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let owner_hotkey = U256::from(1002);
        let staker_coldkey = U256::from(1);
        let staker_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(staker_coldkey, staker_hotkey, 100_000_000_000);

        add_balance_to_coldkey_account(&owner_coldkey, 100_000_000_000u64.into());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_coldkey,
            &owner_hotkey
        ));
        SubtensorModule::stake_into_subnet(
            &owner_hotkey,
            &owner_coldkey,
            netuid,
            100_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        assert_ok!(SubtensorModule::do_lock_stake(
            &owner_coldkey,
            netuid,
            &owner_hotkey,
            10_000u64.into(),
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &staker_coldkey,
            netuid,
            &staker_hotkey,
            20_000u64.into(),
        ));
        assert_ok!(SubtensorModule::do_set_perpetual_lock(
            &owner_coldkey,
            netuid,
            true,
        ));

        step_block(1_000);

        let owner_conviction = SubtensorModule::hotkey_conviction(&owner_hotkey, netuid);
        let staker_conviction = SubtensorModule::hotkey_conviction(&staker_hotkey, netuid);
        let expected = owner_conviction.saturating_add(staker_conviction);
        let total = SubtensorModule::get_total_conviction(netuid);
        let diff = if total > expected {
            total - expected
        } else {
            expected - total
        };

        assert!(diff < U64F64::from_num(1));
    });
}

#[test]
fn test_total_conviction_equals_sum_of_individual_lock_convictions_for_many_lockers() {
    new_test_ext(1).execute_with(|| {
        let first_coldkey = U256::from(1);
        let first_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(first_coldkey, first_hotkey, 100_000_000_000);

        let mut lockers = vec![(first_coldkey, first_hotkey)];
        for i in 1..10u64 {
            let coldkey = U256::from(10 + i);
            let hotkey = U256::from(100 + (i % 3));
            add_balance_to_coldkey_account(&coldkey, 100_000_000_000u64.into());
            assert_ok!(SubtensorModule::create_account_if_non_existent(
                &coldkey, &hotkey
            ));
            SubtensorModule::stake_into_subnet(
                &hotkey,
                &coldkey,
                netuid,
                50_000_000_000u64.into(),
                <Test as Config>::SwapInterface::max_price(),
                false,
            )
            .unwrap();
            lockers.push((coldkey, hotkey));
        }

        for (index, (coldkey, hotkey)) in lockers.iter().enumerate() {
            assert_ok!(SubtensorModule::do_lock_stake(
                coldkey,
                netuid,
                hotkey,
                AlphaBalance::from(1_000u64 + index as u64),
            ));
        }

        step_block(1_000);

        let now = SubtensorModule::get_current_block_as_u64();
        let individual_sum = Lock::<Test>::iter()
            .filter(|((_coldkey, lock_netuid, _hotkey), _lock)| *lock_netuid == netuid)
            .map(|((coldkey, _netuid, hotkey), lock)| {
                roll_forward_individual_lock(&coldkey, netuid, &hotkey, lock, now).conviction
            })
            .fold(U64F64::from_num(0), |acc, conviction| {
                acc.saturating_add(conviction)
            });
        let total = SubtensorModule::get_total_conviction(netuid);
        let diff = if total > individual_sum {
            total - individual_sum
        } else {
            individual_sum - total
        };

        assert!(diff < U64F64::from_num(1));
    });
}

#[test]
fn test_subnet_king_single_hotkey() {
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

        step_block(100);

        let king = SubtensorModule::subnet_king(netuid);
        assert_eq!(king, Some(hotkey));
    });
}

#[test]
fn test_subnet_king_highest_conviction_wins() {
    new_test_ext(1).execute_with(|| {
        let coldkey1 = U256::from(1);
        let coldkey2 = U256::from(5);
        let hotkey_a = U256::from(2);
        let hotkey_b = U256::from(3);

        let netuid = setup_subnet_with_stake(coldkey1, hotkey_a, 100_000_000_000);

        add_balance_to_coldkey_account(&coldkey2, 100_000_000_000u64.into());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey2, &hotkey_b
        ));
        SubtensorModule::stake_into_subnet(
            &hotkey_b,
            &coldkey2,
            netuid,
            50_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        // coldkey1 locks more to hotkey_a
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey1,
            netuid,
            &hotkey_a,
            8000u64.into(),
        ));
        // coldkey2 locks less to hotkey_b
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey2,
            netuid,
            &hotkey_b,
            2000u64.into(),
        ));

        step_block(500);

        let king = SubtensorModule::subnet_king(netuid);
        assert_eq!(king, Some(hotkey_a));
    });
}

#[test]
fn test_subnet_king_no_locks() {
    new_test_ext(1).execute_with(|| {
        let netuid = subtensor_runtime_common::NetUid::from(99);
        let king = SubtensorModule::subnet_king(netuid);
        assert_eq!(king, None);
    });
}

#[test]
fn test_change_subnet_owner_if_needed_reassigns_to_subnet_king() {
    new_test_ext(1).execute_with(|| {
        // Start with the subnet's existing owner, then create a different hotkey owner
        // that can become subnet king.
        let old_owner_coldkey = U256::from(1);
        let old_owner_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(old_owner_coldkey, old_owner_hotkey, 100_000_000_000);
        SubnetOwner::<Test>::insert(netuid, old_owner_coldkey);
        SubnetOwnerHotkey::<Test>::insert(netuid, old_owner_hotkey);

        let new_owner_coldkey = U256::from(5);
        let king_hotkey = U256::from(6);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &new_owner_coldkey,
            &king_hotkey
        ));

        // Make the subnet old enough and set alpha out so 1_000 conviction is exactly
        // the 10% minimum required to trigger reassignment.
        let now = crate::staking::lock::ONE_YEAR + 1;
        System::set_block_number(now);
        NetworkRegisteredAt::<Test>::insert(netuid, 1);
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(10_000u64));

        // Seed matching individual and aggregate lock rows for the future king.
        let locked_mass = AlphaBalance::from(1_000u64);
        Lock::<Test>::insert(
            (new_owner_coldkey, netuid, king_hotkey),
            LockState {
                locked_mass,
                conviction: U64F64::from_num(1_000),
                last_update: now,
            },
        );
        HotkeyLock::<Test>::insert(
            netuid,
            king_hotkey,
            LockState {
                locked_mass,
                conviction: U64F64::from_num(1_000),
                last_update: now,
            },
        );

        // Reassignment should select the king hotkey and its owning coldkey.
        SubtensorModule::change_subnet_owner_if_needed(netuid);

        assert_eq!(SubnetOwner::<Test>::get(netuid), new_owner_coldkey);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), king_hotkey);

        // The new owner's aggregate conviction is progressed to locked mass.
        let owner_lock = Lock::<Test>::get((new_owner_coldkey, netuid, king_hotkey)).unwrap();
        assert_eq!(owner_lock.conviction, U64F64::from_num(1_000));

        let king_lock = OwnerLock::<Test>::get(netuid).unwrap();
        assert_eq!(king_lock.conviction, U64F64::from_num(1_000));
    });
}

#[test]
fn test_run_coinbase_reassigns_subnet_owner_by_conviction_on_epoch() {
    new_test_ext(1).execute_with(|| {
        let old_owner_coldkey = U256::from(1);
        let old_owner_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(old_owner_coldkey, old_owner_hotkey, 100_000_000_000);
        SubnetOwner::<Test>::insert(netuid, old_owner_coldkey);
        SubnetOwnerHotkey::<Test>::insert(netuid, old_owner_hotkey);

        let new_owner_coldkey = U256::from(5);
        let king_hotkey = U256::from(6);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &new_owner_coldkey,
            &king_hotkey
        ));

        let now = crate::staking::lock::ONE_YEAR + 1;
        System::set_block_number(now);
        NetworkRegisteredAt::<Test>::insert(netuid, 1);
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(10_000u64));
        SubtensorModule::set_tempo_unchecked(netuid, 1);
        LastEpochBlock::<Test>::insert(netuid, now.saturating_sub(1));
        PendingEpochAt::<Test>::insert(netuid, 0);

        let locked_mass = AlphaBalance::from(1_000u64);
        Lock::<Test>::insert(
            (new_owner_coldkey, netuid, king_hotkey),
            LockState {
                locked_mass,
                conviction: U64F64::from_num(1_000),
                last_update: now,
            },
        );
        HotkeyLock::<Test>::insert(
            netuid,
            king_hotkey,
            LockState {
                locked_mass,
                conviction: U64F64::from_num(1_000),
                last_update: now,
            },
        );

        assert_eq!(SubnetOwner::<Test>::get(netuid), old_owner_coldkey);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), old_owner_hotkey);

        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));

        assert_eq!(SubnetOwner::<Test>::get(netuid), new_owner_coldkey);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), king_hotkey);
        assert_eq!(LastEpochBlock::<Test>::get(netuid), now);
    });
}

#[test]
fn test_change_subnet_owner_rebuilds_old_owner_hotkey_by_lock_mode() {
    new_test_ext(1).execute_with(|| {
        let old_owner_coldkey = U256::from(1);
        let old_owner_hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(old_owner_coldkey, old_owner_hotkey, 100_000_000_000);
        SubnetOwner::<Test>::insert(netuid, old_owner_coldkey);
        SubnetOwnerHotkey::<Test>::insert(netuid, old_owner_hotkey);

        let perpetual_coldkey = U256::from(3);
        let decaying_coldkey = U256::from(4);
        let king_coldkey = U256::from(5);
        let king_hotkey = U256::from(6);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &king_coldkey,
            &king_hotkey
        ));
        register_ok_neuron(netuid, king_hotkey, king_coldkey, 0);

        let now = crate::staking::lock::ONE_YEAR + 1;
        System::set_block_number(now);
        NetworkRegisteredAt::<Test>::insert(netuid, 1);
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(10_000u64));
        DecayingLock::<Test>::insert(perpetual_coldkey, netuid, false);

        Lock::<Test>::insert(
            (perpetual_coldkey, netuid, old_owner_hotkey),
            LockState {
                locked_mass: 400u64.into(),
                conviction: U64F64::from_num(400),
                last_update: now,
            },
        );
        Lock::<Test>::insert(
            (decaying_coldkey, netuid, old_owner_hotkey),
            LockState {
                locked_mass: 300u64.into(),
                conviction: U64F64::from_num(300),
                last_update: now,
            },
        );
        OwnerLock::<Test>::insert(
            netuid,
            LockState {
                locked_mass: 400u64.into(),
                conviction: U64F64::from_num(400),
                last_update: now,
            },
        );
        DecayingOwnerLock::<Test>::insert(
            netuid,
            LockState {
                locked_mass: 300u64.into(),
                conviction: U64F64::from_num(300),
                last_update: now,
            },
        );
        Lock::<Test>::insert(
            (king_coldkey, netuid, king_hotkey),
            LockState {
                locked_mass: 1_000u64.into(),
                conviction: U64F64::from_num(1_000),
                last_update: now,
            },
        );
        HotkeyLock::<Test>::insert(
            netuid,
            king_hotkey,
            LockState {
                locked_mass: 1_000u64.into(),
                conviction: U64F64::from_num(1_000),
                last_update: now,
            },
        );

        SubtensorModule::change_subnet_owner_if_needed(netuid);

        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), king_hotkey);
        assert_eq!(
            HotkeyLock::<Test>::get(netuid, old_owner_hotkey)
                .unwrap()
                .locked_mass,
            400u64.into()
        );
        assert_eq!(
            DecayingHotkeyLock::<Test>::get(netuid, old_owner_hotkey)
                .unwrap()
                .locked_mass,
            300u64.into()
        );
        assert_eq!(
            OwnerLock::<Test>::get(netuid).unwrap().locked_mass,
            1_000u64.into()
        );
    });
}

#[test]
fn test_swap_hotkey_locks_moves_owner_hotkey_aggregate_to_owner_lock() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1);
        let old_owner_hotkey = U256::from(2);
        let new_owner_hotkey = U256::from(3);
        let locking_coldkey = U256::from(4);
        let netuid = setup_subnet_with_stake(owner_coldkey, old_owner_hotkey, 100_000_000_000);
        SubnetOwner::<Test>::insert(netuid, owner_coldkey);
        SubnetOwnerHotkey::<Test>::insert(netuid, old_owner_hotkey);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_coldkey,
            &new_owner_hotkey
        ));

        let now = SubtensorModule::get_current_block_as_u64();
        Lock::<Test>::insert(
            (locking_coldkey, netuid, old_owner_hotkey),
            LockState {
                locked_mass: 500u64.into(),
                conviction: U64F64::from_num(500),
                last_update: now,
            },
        );
        SubtensorModule::add_locking_coldkey(&old_owner_hotkey, netuid, &locking_coldkey);
        OwnerLock::<Test>::insert(
            netuid,
            LockState {
                locked_mass: 500u64.into(),
                conviction: U64F64::from_num(500),
                last_update: now,
            },
        );

        SubtensorModule::swap_hotkey_locks(&old_owner_hotkey, &new_owner_hotkey);

        assert!(Lock::<Test>::get((locking_coldkey, netuid, old_owner_hotkey)).is_none());
        assert!(Lock::<Test>::get((locking_coldkey, netuid, new_owner_hotkey)).is_some());
        assert!(HotkeyLock::<Test>::get(netuid, new_owner_hotkey).is_none());
        assert!(DecayingHotkeyLock::<Test>::get(netuid, new_owner_hotkey).is_none());
        assert_eq!(
            OwnerLock::<Test>::get(netuid).unwrap().locked_mass,
            500u64.into()
        );
        assert!(!LockingColdkeys::<Test>::contains_key((
            netuid,
            old_owner_hotkey,
            locking_coldkey
        )));
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid,
            new_owner_hotkey,
            locking_coldkey
        )));
    });
}

#[test]
fn test_change_subnet_owner_if_needed_does_not_reassign_when_required_condition_is_missing() {
    let assert_owner_unchanged =
        |alpha_out: u64, registered_at: u64, owner_conviction: u64, king_conviction: u64| {
            new_test_ext(1).execute_with(|| {
                let owner_coldkey = U256::from(1001);
                let owner_hotkey = U256::from(1002);
                let staker_coldkey = U256::from(1);
                let staker_hotkey = U256::from(2);
                let netuid =
                    setup_subnet_with_stake(staker_coldkey, staker_hotkey, 100_000_000_000);

                let king_coldkey = U256::from(5);
                let king_hotkey = U256::from(6);
                assert_ok!(SubtensorModule::create_account_if_non_existent(
                    &king_coldkey,
                    &king_hotkey
                ));

                let now = crate::staking::lock::ONE_YEAR + 10;
                System::set_block_number(now);
                NetworkRegisteredAt::<Test>::insert(netuid, registered_at);
                SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(alpha_out));

                let locked_mass = AlphaBalance::from(1_000u64);
                HotkeyLock::<Test>::insert(
                    netuid,
                    owner_hotkey,
                    LockState {
                        locked_mass,
                        conviction: U64F64::from_num(owner_conviction),
                        last_update: now,
                    },
                );
                HotkeyLock::<Test>::insert(
                    netuid,
                    king_hotkey,
                    LockState {
                        locked_mass,
                        conviction: U64F64::from_num(king_conviction),
                        last_update: now,
                    },
                );

                SubtensorModule::change_subnet_owner_if_needed(netuid);

                assert_eq!(SubnetOwner::<Test>::get(netuid), owner_coldkey);
                assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), owner_hotkey);
            });
        };

    // Missing condition 1: total conviction is below 10% of SubnetAlphaOut.
    assert_owner_unchanged(30_000, 1, 500, 1_000);

    // Missing condition 2: subnet is younger than one year.
    assert_owner_unchanged(20_000, crate::staking::lock::ONE_YEAR, 500, 1_000);

    // Missing condition 3: challenger is not the subnet king because owner's conviction is higher.
    assert_owner_unchanged(20_000, 1, 2_000, 1_000);
}
