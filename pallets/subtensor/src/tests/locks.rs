#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use approx::assert_abs_diff_eq;
use frame_support::dispatch::{GetDispatchInfo, Pays};
use frame_support::weights::Weight;
use frame_support::{assert_noop, assert_ok};
use safe_math::FixedExt;
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance};
use subtensor_swap_interface::SwapHandler;

use super::mock::*;
use crate::staking::lock::{ConvictionModel, LockState};
use crate::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup_subnet_with_stake(
    coldkey: U256,
    hotkey: U256,
    stake_tao: u64,
) -> subtensor_runtime_common::NetUid {
    let subnet_owner_coldkey = U256::from(1001);
    let subnet_owner_hotkey = U256::from(1002);
    let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

    let amount: TaoBalance = (stake_tao).into();
    setup_reserves(
        netuid,
        (stake_tao * 1_000_000).into(),
        (stake_tao * 10_000_000).into(),
    );

    assert_ok!(SubtensorModule::create_account_if_non_existent(
        &coldkey, &hotkey
    ));
    add_balance_to_coldkey_account(&coldkey, amount);
    SubtensorModule::stake_into_subnet(
        &hotkey,
        &coldkey,
        netuid,
        amount,
        <Test as Config>::SwapInterface::max_price(),
        false,
    )
    .unwrap();
    DecayingLock::<Test>::insert(coldkey, netuid, false);

    netuid
}

fn get_alpha(
    hotkey: &U256,
    coldkey: &U256,
    netuid: subtensor_runtime_common::NetUid,
) -> AlphaBalance {
    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid)
}

fn roll_forward_lock(
    lock: LockState,
    now: u64,
    owner_lock: bool,
    perpetual_lock: bool,
) -> LockState {
    ConvictionModel::roll_forward_lock(
        lock,
        now,
        UnlockRate::<Test>::get(),
        MaturityRate::<Test>::get(),
        owner_lock,
        perpetual_lock,
    )
    .0
}

fn roll_forward_individual_lock(
    coldkey: &U256,
    netuid: subtensor_runtime_common::NetUid,
    hotkey: &U256,
    lock: LockState,
    now: u64,
) -> LockState {
    roll_forward_lock(
        lock,
        now,
        hotkey == &SubnetOwnerHotkey::<Test>::get(netuid),
        DecayingLock::<Test>::get(coldkey, netuid) == Some(false),
    )
}

#[test]
fn test_account_flags_default_to_zero_and_reject_locked_alpha_setter_pays_fee() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);

        assert_eq!(AccountFlags::<Test>::get(coldkey), 0);
        assert!(!AccountFlags::<Test>::contains_key(coldkey));
        assert!(SubtensorModule::account_rejects_locked_alpha(&coldkey));

        let call =
            RuntimeCall::SubtensorModule(crate::Call::set_reject_locked_alpha { enabled: true });
        assert_eq!(call.get_dispatch_info().pays_fee, Pays::Yes);

        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(coldkey),
            false,
        ));
        assert_eq!(
            AccountFlags::<Test>::get(coldkey),
            ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA
        );
        assert!(AccountFlags::<Test>::contains_key(coldkey));
        assert!(!SubtensorModule::account_rejects_locked_alpha(&coldkey));

        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(coldkey),
            true,
        ));
        assert_eq!(AccountFlags::<Test>::get(coldkey), 0);
        assert!(!AccountFlags::<Test>::contains_key(coldkey));
        assert!(SubtensorModule::account_rejects_locked_alpha(&coldkey));
    });
}

fn roll_forward_hotkey_lock(lock: LockState, now: u64) -> LockState {
    roll_forward_lock(lock, now, false, true)
}

fn roll_forward_decaying_hotkey_lock(lock: LockState, now: u64) -> LockState {
    roll_forward_lock(lock, now, false, false)
}

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

// =========================================================================
// GROUP 2: Green-path — lock queries
// =========================================================================

#[test]
fn test_get_current_locked_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = subtensor_runtime_common::NetUid::from(1);
        assert_eq!(
            SubtensorModule::get_current_locked(&coldkey, netuid),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_get_conviction_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = subtensor_runtime_common::NetUid::from(1);
        assert_eq!(
            SubtensorModule::get_conviction(&coldkey, netuid),
            U64F64::from_num(0)
        );
    });
}

#[test]
fn test_get_coldkey_lock_rolls_forward() {
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

        let initial_lock =
            SubtensorModule::get_coldkey_lock(&coldkey, netuid).expect("coldkey lock should exist");
        assert_eq!(initial_lock.conviction, U64F64::from_num(0));

        step_block(1000);

        let rolled_lock =
            SubtensorModule::get_coldkey_lock(&coldkey, netuid).expect("coldkey lock should exist");
        assert_eq!(rolled_lock.locked_mass, initial_lock.locked_mass);
        assert!(rolled_lock.conviction > initial_lock.conviction);
        assert_eq!(
            rolled_lock.last_update,
            SubtensorModule::get_current_block_as_u64()
        );
    });
}

#[test]
fn test_get_coldkey_lock_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = subtensor_runtime_common::NetUid::from(1);

        assert!(SubtensorModule::get_coldkey_lock(&coldkey, netuid).is_none());
    });
}

#[test]
fn test_available_to_unstake_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let available = SubtensorModule::available_to_unstake(&coldkey, netuid);
        assert_eq!(available, total);
    });
}

#[test]
fn test_available_to_unstake_with_lock() {
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
            lock_amount,
        ));

        let available = SubtensorModule::available_to_unstake(&coldkey, netuid);
        assert_eq!(available, total - lock_amount);
    });
}

#[test]
fn test_available_to_unstake_fully_locked() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, total,
        ));

        let available = SubtensorModule::available_to_unstake(&coldkey, netuid);
        assert_eq!(available, AlphaBalance::ZERO);
    });
}

#[test]
fn test_stake_availability_for_coldkeys_empty_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let result = SubtensorModule::get_stake_availability_for_coldkeys(Vec::new(), None);
        assert!(result.is_empty());
    });
}

#[test]
fn test_stake_availability_for_coldkeys_empty_netuids() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(Vec::new()));
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&coldkey));
        assert!(result.get(&coldkey).unwrap().is_empty());
    });
}

#[test]
fn test_stake_availability_for_coldkeys_filters_empty_rows() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));

        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&coldkey));
        assert!(result.get(&coldkey).unwrap().is_empty());
    });
}

#[test]
fn test_stake_availability_for_coldkeys_stake_without_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));

        assert_eq!(result.len(), 1);
        let availability = result.get(&coldkey).unwrap().get(&netuid).unwrap();
        assert_eq!(availability.total(), total);
        assert_eq!(availability.locked(), AlphaBalance::ZERO);
        assert_eq!(availability.available(), total);
    });
}

#[test]
fn test_stake_availability_for_coldkeys_partial_lock() {
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
            lock_amount,
        ));

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));
        let availability = result.get(&coldkey).unwrap().get(&netuid).unwrap();

        assert_eq!(availability.total(), total);
        assert_eq!(
            availability.locked(),
            SubtensorModule::get_current_locked(&coldkey, netuid)
        );
        assert_eq!(availability.available(), total - availability.locked());
    });
}

#[test]
fn test_stake_availability_for_coldkeys_fully_locked() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, total,
        ));

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));
        let availability = result.get(&coldkey).unwrap().get(&netuid).unwrap();

        assert_eq!(availability.total(), total);
        assert_eq!(availability.locked(), total);
        assert_eq!(availability.available(), AlphaBalance::ZERO);
    });
}

#[test]
fn test_stake_availability_for_coldkeys_preserves_coldkey_grouping() {
    new_test_ext(1).execute_with(|| {
        let coldkey_a = U256::from(1);
        let hotkey_a = U256::from(2);
        let coldkey_b = U256::from(3);
        let hotkey_b = U256::from(4);
        let netuid_a = setup_subnet_with_stake(coldkey_a, hotkey_a, 100_000_000_000);
        let netuid_b = setup_subnet_with_stake(coldkey_b, hotkey_b, 100_000_000_000);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey_a, coldkey_b],
            Some(vec![netuid_a, netuid_b]),
        );

        assert_eq!(result.len(), 2);
        assert_eq!(result.get(&coldkey_a).unwrap().len(), 1);
        assert!(result.get(&coldkey_a).unwrap().contains_key(&netuid_a));
        assert_eq!(result.get(&coldkey_b).unwrap().len(), 1);
        assert!(result.get(&coldkey_b).unwrap().contains_key(&netuid_b));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_none_netuids_uses_all_subnets() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], None);

        assert_eq!(result.len(), 1);
        assert!(result.get(&coldkey).unwrap().contains_key(&netuid));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_one_coldkey_two_subnets() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_a = U256::from(2);
        let hotkey_b = U256::from(3);
        let netuid_a = setup_subnet_with_stake(coldkey, hotkey_a, 100_000_000_000);
        let netuid_b = setup_subnet_with_stake(coldkey, hotkey_b, 100_000_000_000);
        let total_a = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid_a);
        let total_b = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid_b);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![netuid_a, netuid_b]),
        );

        assert_eq!(result.len(), 1);
        let subnets = result.get(&coldkey).unwrap();
        assert_eq!(subnets.len(), 2);
        assert!(subnets.contains_key(&netuid_a));
        assert!(subnets.contains_key(&netuid_b));

        let row_a = subnets.get(&netuid_a).unwrap();
        assert_eq!(row_a.total(), total_a);
        assert_eq!(row_a.locked(), AlphaBalance::ZERO);
        assert_eq!(row_a.available(), total_a);

        let row_b = subnets.get(&netuid_b).unwrap();
        assert_eq!(row_b.total(), total_b);
        assert_eq!(row_b.locked(), AlphaBalance::ZERO);
        assert_eq!(row_b.available(), total_b);
    });
}

#[test]
fn test_stake_availability_for_coldkeys_filters_to_requested_netuid() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_a = U256::from(2);
        let hotkey_b = U256::from(3);
        let netuid_a = setup_subnet_with_stake(coldkey, hotkey_a, 100_000_000_000);
        let netuid_b = setup_subnet_with_stake(coldkey, hotkey_b, 100_000_000_000);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![netuid_b]),
        );

        assert_eq!(result.len(), 1);
        let subnets = result.get(&coldkey).unwrap();
        assert_eq!(subnets.len(), 1);
        assert!(subnets.contains_key(&netuid_b));
        assert!(!subnets.contains_key(&netuid_a));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_dedups_netuids() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![netuid, netuid]),
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result.get(&coldkey).unwrap().len(), 1);
        assert!(result.get(&coldkey).unwrap().contains_key(&netuid));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_skips_nonexistent_netuid() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let nonexistent = subtensor_runtime_common::NetUid::from(99);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![nonexistent]),
        );
        assert_eq!(result.len(), 1);
        assert!(result.get(&coldkey).unwrap().is_empty());

        // Mix real + fake requires at least two subnets on chain so len(requested) <= subnet_count.
        let subnet_owner_coldkey = U256::from(2001);
        let subnet_owner_hotkey = U256::from(2002);
        let _other_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![netuid, nonexistent]),
        );
        assert_eq!(result.len(), 1);
        let subnets = result.get(&coldkey).unwrap();
        assert_eq!(subnets.len(), 1);
        assert!(subnets.contains_key(&netuid));
        assert!(!subnets.contains_key(&nonexistent));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_rejects_oversized_netuid_list() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let subnet_count = SubtensorModule::get_all_subnet_netuids().len();
        let requested: Vec<subtensor_runtime_common::NetUid> = (0..=subnet_count as u16)
            .map(subtensor_runtime_common::NetUid::from)
            .collect();

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(requested));
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&coldkey));
        assert!(result.get(&coldkey).unwrap().is_empty());

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));
        assert_eq!(result.get(&coldkey).unwrap().len(), 1);
        assert!(result.get(&coldkey).unwrap().contains_key(&netuid));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_uses_rolled_forward_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let lock_amount = total / 2.into();

        DecayingLock::<Test>::remove(coldkey, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));
        let raw_lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();

        step_block(1000);

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));
        let availability = result.get(&coldkey).unwrap().get(&netuid).unwrap();
        let rolled_locked = SubtensorModule::get_current_locked(&coldkey, netuid);

        assert!(rolled_locked < raw_lock.locked_mass);
        assert_eq!(availability.locked(), rolled_locked);
        assert_eq!(availability.available(), total - rolled_locked);
    });
}

// =========================================================================
// GROUP 3: Incremental locks (top-up)
// =========================================================================

#[test]
fn test_lock_stake_topup() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let first_lock = 1000u64;
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            first_lock.into()
        ));

        step_block(100);

        let second_lock = 500u64;
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            second_lock.into()
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();
        // locked_mass should be decayed(first_lock) + second_lock
        // Since tau is large (216000), decay over 100 blocks is small; locked_mass ~ 1000 + 500
        assert!(lock.locked_mass > 1490.into());
        assert!(lock.locked_mass < 1501.into());
        // conviction should have grown from the time the first lock was active
        assert!(lock.conviction > U64F64::from_num(0));
        assert_eq!(
            lock.last_update,
            SubtensorModule::get_current_block_as_u64()
        );

        // Hotkey lock should also be created
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey).unwrap();
        assert!(hotkey_lock.locked_mass > 1490.into());
        assert_eq!(hotkey_lock.locked_mass, lock.locked_mass);
        assert!(hotkey_lock.conviction > U64F64::from_num(0));
    });
}

#[test]
fn test_lock_stake_topup_multiple_times() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let chunk = 500u64.into();

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, chunk
        ));
        step_block(50);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, chunk
        ));
        step_block(50);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, chunk
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();
        // After three top-ups with small decay, should be close to 1500
        assert!(lock.locked_mass > 1490.into());
        assert!(lock.locked_mass <= 1500.into());
        assert!(lock.conviction > U64F64::from_num(0));

        // Hotkey lock should also be updated
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey).unwrap();
        assert!(hotkey_lock.locked_mass > 1490.into());
        assert_eq!(hotkey_lock.locked_mass, lock.locked_mass);
        assert!(hotkey_lock.conviction > U64F64::from_num(0));
    });
}

#[test]
fn test_lock_stake_topup_same_block() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let first = 1000u64.into();
        let second = 500u64.into();

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, first
        ));
        // No block advancement — same block top-up
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, second
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();
        // dt=0 means no decay, simple addition
        assert_eq!(lock.locked_mass, first + second);
        assert_eq!(lock.conviction, U64F64::from_num(0));

        // Hotkey lock should also be updated
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey).unwrap();
        assert_eq!(hotkey_lock.locked_mass, first + second);
        assert_eq!(hotkey_lock.conviction, U64F64::from_num(0));
    });
}

#[test]
fn test_locking_coldkeys_added_once_by_lock_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            100u64.into(),
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            50u64.into(),
        ));

        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey
        )));
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, hotkey)).count(),
            1
        );
    });
}

#[test]
fn test_locking_coldkeys_removed_when_lock_is_fully_reduced() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let amount = 100u64.into();

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, amount
        ));
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey
        )));

        SubtensorModule::force_reduce_lock(&coldkey, netuid, amount);

        assert!(Lock::<Test>::get((coldkey, netuid, hotkey)).is_none());
        assert!(!LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey
        )));
    });
}

#[test]
fn test_lock_state_is_zero_uses_dust_threshold() {
    let below_threshold = LockState {
        locked_mass: AlphaBalance::from(99u64),
        conviction: U64F64::from_num(99),
        last_update: 0,
    };
    let locked_mass_at_threshold = LockState {
        locked_mass: AlphaBalance::from(100u64),
        conviction: U64F64::from_num(99),
        last_update: 0,
    };
    let conviction_at_threshold = LockState {
        locked_mass: AlphaBalance::from(99u64),
        conviction: U64F64::from_num(100),
        last_update: 0,
    };

    assert!(below_threshold.is_zero());
    assert!(!locked_mass_at_threshold.is_zero());
    assert!(!conviction_at_threshold.is_zero());
}

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

// =========================================================================
// GROUP 5: ConvictionModel roll-forward math
// =========================================================================

#[test]
fn test_exp_decay_zero_dt() {
    new_test_ext(1).execute_with(|| {
        let result = ConvictionModel::exp_decay(0, 216000);
        assert_eq!(result, U64F64::from_num(1));
    });
}

#[test]
fn test_exp_decay_zero_tau() {
    new_test_ext(1).execute_with(|| {
        let result = ConvictionModel::exp_decay(1000, 0);
        assert_eq!(result, U64F64::from_num(0));
    });
}

#[test]
fn test_exp_decay_one_tau() {
    new_test_ext(1).execute_with(|| {
        let tau = 216000u64;
        let result = ConvictionModel::exp_decay(tau, tau);
        // exp(-1) ~= 0.36787944
        let expected = U64F64::from_num(0.36787944f64);
        let diff = if result > expected {
            result - expected
        } else {
            expected - result
        };
        assert!(diff < U64F64::from_num(0.001));
    });
}

#[test]
fn test_exp_decay_clamps_large_dt_to_min_ratio() {
    new_test_ext(1).execute_with(|| {
        let tau = 216000u64;
        let clamped_result = ConvictionModel::exp_decay(40 * tau, tau);
        let oversized_result = ConvictionModel::exp_decay(100 * tau, tau);

        let diff = if oversized_result > clamped_result {
            oversized_result - clamped_result
        } else {
            clamped_result - oversized_result
        };

        assert!(diff < U64F64::from_num(0.000000001));
        assert!(oversized_result > U64F64::from_num(0));
    });
}

#[test]
fn test_roll_forward_individual_lock_uses_lock_owner_and_decay_mode() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let owner_hotkey = SubnetOwnerHotkey::<Test>::get(netuid);
        DecayingLock::<Test>::remove(coldkey, netuid);

        let lock = LockState {
            locked_mass: 10_000u64.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let now = 1_000u64;

        let rolled =
            roll_forward_individual_lock(&coldkey, netuid, &owner_hotkey, lock.clone(), now);
        let expected = ConvictionModel::roll_forward_lock(
            lock,
            now,
            UnlockRate::<Test>::get(),
            MaturityRate::<Test>::get(),
            true,
            false,
        )
        .0;

        assert_eq!(rolled, expected);
    });
}

#[test]
fn test_roll_forward_hotkey_lock_uses_perpetual_general_mode() {
    new_test_ext(1).execute_with(|| {
        let lock = LockState {
            locked_mass: 10_000u64.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let now = 1_000u64;

        let rolled = roll_forward_hotkey_lock(lock.clone(), now);
        let expected = ConvictionModel::roll_forward_lock(
            lock,
            now,
            UnlockRate::<Test>::get(),
            MaturityRate::<Test>::get(),
            false,
            true,
        )
        .0;

        assert_eq!(rolled, expected);
    });
}

#[test]
fn test_roll_forward_decaying_hotkey_lock_uses_decaying_general_mode() {
    new_test_ext(1).execute_with(|| {
        let lock = LockState {
            locked_mass: 10_000u64.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let now = 1_000u64;

        let rolled = roll_forward_decaying_hotkey_lock(lock.clone(), now);
        let expected = ConvictionModel::roll_forward_lock(
            lock,
            now,
            UnlockRate::<Test>::get(),
            MaturityRate::<Test>::get(),
            false,
            false,
        )
        .0;

        assert_eq!(rolled, expected);
    });
}

#[test]
fn test_roll_forward_locked_mass_decays() {
    new_test_ext(1).execute_with(|| {
        let lock_amount = 10000u64;
        let lock = LockState {
            locked_mass: lock_amount.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let rolled = roll_forward_lock(lock, UnlockRate::<Test>::get(), false, false);

        assert!(rolled.locked_mass < lock_amount.into());
        assert!(rolled.locked_mass > AlphaBalance::ZERO);
    });
}

#[test]
fn test_roll_forward_conviction_uses_unequal_rate_closed_form() {
    new_test_ext(1).execute_with(|| {
        let locked_mass = 10_000u64;
        let dt = 10_000u64;
        let unlock_rate = 200_000u64;
        let maturity_rate = 240_000u64;
        UnlockRate::<Test>::set(unlock_rate);
        MaturityRate::<Test>::set(maturity_rate);
        assert_ne!(unlock_rate, maturity_rate);

        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let rolled = roll_forward_lock(lock, dt, false, false);

        let unlock_decay = ConvictionModel::exp_decay(dt, unlock_rate);
        let maturity_decay = ConvictionModel::exp_decay(dt, maturity_rate);
        let gamma = U64F64::from_num(unlock_rate)
            .saturating_mul(maturity_decay.saturating_sub(unlock_decay))
            .safe_div(U64F64::from_num(maturity_rate.saturating_sub(unlock_rate)));
        let expected = U64F64::from_num(locked_mass).saturating_mul(gamma);

        assert_abs_diff_eq!(
            rolled.conviction.to_num::<f64>(),
            expected.to_num::<f64>(),
            epsilon = 0.0000001
        );
    });
}

#[test]
fn test_roll_forward_adjacent_large_rates_and_large_mass_match_f64_closed_form() {
    new_test_ext(1).execute_with(|| {
        let unlock_rate = 1_142_108u64;
        let maturity_rate = unlock_rate + 1;
        let locked_mass = 21_000_000_000_000_000u64;
        let dt = unlock_rate;
        UnlockRate::<Test>::put(unlock_rate);
        MaturityRate::<Test>::put(maturity_rate);

        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let rolled = roll_forward_lock(lock, dt, false, false);

        let decay_x = (-(dt as f64) / unlock_rate as f64).exp();
        let decay_z = (-(dt as f64) / maturity_rate as f64).exp();
        let gamma =
            unlock_rate as f64 * (decay_x - decay_z) / (unlock_rate as f64 - maturity_rate as f64);
        let expected_conviction = locked_mass as f64 * gamma;
        let expected_locked_mass = locked_mass as f64 * decay_x;

        assert_abs_diff_eq!(
            rolled.conviction.to_num::<f64>(),
            expected_conviction,
            epsilon = 50_000.0
        );
        assert_abs_diff_eq!(
            u64::from(rolled.locked_mass) as f64,
            expected_locked_mass,
            epsilon = 2_000.0
        );
    });
}

#[test]
fn test_roll_forward_scales_linearly_with_locked_mass() {
    new_test_ext(1).execute_with(|| {
        let dt = 25_000u64;
        let base_mass = 10_000u64;
        let base = LockState {
            locked_mass: base_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let double = LockState {
            locked_mass: (base_mass * 2).into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };

        let rolled_base = roll_forward_lock(base, dt, false, false);
        let rolled_double = roll_forward_lock(double, dt, false, false);

        assert_abs_diff_eq!(
            u64::from(rolled_double.locked_mass) as f64,
            (u64::from(rolled_base.locked_mass) * 2) as f64,
            epsilon = 1.0
        );
        assert_abs_diff_eq!(
            rolled_double.conviction.to_num::<f64>(),
            rolled_base.conviction.to_num::<f64>() * 2.0,
            epsilon = 0.0000001
        );
    });
}

#[test]
fn test_roll_forward_chunked_update_matches_single_update() {
    new_test_ext(1).execute_with(|| {
        let lock = LockState {
            locked_mass: 1_000_000_000u64.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let mid = 10_000u64;
        let end = 20_000u64;

        let rolled_once = roll_forward_lock(lock.clone(), end, false, false);
        let rolled_twice = roll_forward_lock(
            roll_forward_lock(lock, mid, false, false),
            end,
            false,
            false,
        );

        assert_abs_diff_eq!(
            u64::from(rolled_twice.locked_mass) as f64,
            u64::from(rolled_once.locked_mass) as f64,
            epsilon = 1.0
        );
        assert_abs_diff_eq!(
            rolled_twice.conviction.to_num::<f64>(),
            rolled_once.conviction.to_num::<f64>(),
            epsilon = 0.1
        );
    });
}

#[test]
fn test_roll_forward_conviction_stays_below_original_mass_for_one_shot_lock() {
    new_test_ext(1).execute_with(|| {
        let locked_mass = 10_000u64;
        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let cap = U64F64::from_num(locked_mass);

        for dt in [
            1_000u64,
            10_000u64,
            UnlockRate::<Test>::get(),
            MaturityRate::<Test>::get(),
            MaturityRate::<Test>::get().saturating_mul(5),
        ] {
            let rolled = roll_forward_lock(lock.clone(), dt, false, false);
            assert!(rolled.conviction <= cap);
        }
    });
}

#[test]
fn test_roll_forward_decaying_conviction_peak_is_below_original_lock() {
    new_test_ext(1).execute_with(|| {
        UnlockRate::<Test>::set(200_000u64);
        MaturityRate::<Test>::set(240_000u64);

        let locked_mass = 10_000u64;
        let unlock_rate = UnlockRate::<Test>::get() as f64;
        let maturity_rate = MaturityRate::<Test>::get() as f64;
        assert_ne!(unlock_rate, maturity_rate);

        let peak_block = ((unlock_rate * maturity_rate) / (unlock_rate - maturity_rate)
            * (unlock_rate / maturity_rate).ln())
        .round() as u64;
        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };

        let rolled = roll_forward_lock(lock, peak_block, false, false);

        assert!(rolled.conviction < U64F64::from_num(locked_mass));
    });
}

#[test]
fn test_roll_forward_perpetual_mass_does_not_decay_and_conviction_matures() {
    new_test_ext(1).execute_with(|| {
        let locked_mass = 10_000u64;
        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };

        let rolled = roll_forward_lock(lock, MaturityRate::<Test>::get(), false, true);

        assert_eq!(rolled.locked_mass, locked_mass.into());
        assert!(rolled.conviction > U64F64::from_num(0));
        assert!(rolled.conviction < U64F64::from_num(locked_mass));
    });
}

#[test]
fn test_roll_forward_perpetual_conviction_never_exceeds_lock() {
    new_test_ext(1).execute_with(|| {
        let locked_mass = 10_000u64;
        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };

        for dt in [
            1u64,
            1_000u64,
            MaturityRate::<Test>::get(),
            MaturityRate::<Test>::get().saturating_mul(10),
            MaturityRate::<Test>::get().saturating_mul(1_000),
        ] {
            let rolled = roll_forward_lock(lock.clone(), dt, false, true);
            assert_eq!(rolled.locked_mass, locked_mass.into());
            assert!(rolled.conviction <= U64F64::from_num(locked_mass));
        }
    });
}

#[test]
fn test_roll_forward_conviction_converges_to_zero() {
    new_test_ext(1).execute_with(|| {
        let lock_amount = 10000u64;
        let lock = LockState {
            locked_mass: lock_amount.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };

        let c0 = lock.conviction;
        assert_eq!(c0, U64F64::from_num(0));

        let rolled = roll_forward_lock(lock.clone(), 100, false, false);
        let c1 = rolled.conviction;
        assert!(c1 > U64F64::from_num(0));

        let rolled = roll_forward_lock(lock.clone(), 1_100, false, false);
        let c2 = rolled.conviction;
        assert!(c2 > c1);

        let tau = MaturityRate::<Test>::get();
        let c_late = roll_forward_lock(lock, tau * 1000, false, false).conviction;
        assert_abs_diff_eq!(c_late.to_num::<f64>(), 0., epsilon = 0.0000001);
    });
}

#[test]
fn test_roll_forward_normalizes_dust_to_zero() {
    new_test_ext(1).execute_with(|| {
        let lock = LockState {
            locked_mass: 99u64.into(),
            conviction: U64F64::from_num(99),
            last_update: 100,
        };

        let rolled = roll_forward_lock(lock, 100, false, false);

        assert_eq!(rolled.locked_mass, AlphaBalance::ZERO);
        assert_eq!(rolled.conviction, U64F64::from_num(0));
        assert_eq!(rolled.last_update, 100);
    });
}

#[test]
fn test_roll_forward_no_change_when_now_equals_last_update() {
    new_test_ext(1).execute_with(|| {
        let lock = LockState {
            locked_mass: 5000.into(),
            conviction: U64F64::from_num(1234),
            last_update: 100,
        };
        let rolled = roll_forward_lock(lock.clone(), 100, false, false);
        assert_eq!(rolled.locked_mass, lock.locked_mass);
        assert_eq!(rolled.conviction, lock.conviction);
        assert_eq!(rolled.last_update, 100);
    });
}

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

// =========================================================================
// GROUP 10: Lock force-reduction
// =========================================================================

#[test]
fn test_reduce_lock_removes_dust() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let lock_amount = AlphaBalance::from(50u64);

        // Lock a small amount
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        // Advance many taus so everything decays well below dust (100)
        let tau = UnlockRate::<Test>::get();
        let target = System::block_number() + tau * 50;
        System::set_block_number(target);

        // Remove full lock amount
        SubtensorModule::force_reduce_lock(&coldkey, netuid, lock_amount);

        assert!(Lock::<Test>::get((coldkey, netuid, hotkey)).is_none());
        assert!(HotkeyLock::<Test>::get(netuid, hotkey).is_none());
    });
}

#[test]
fn test_reduce_lock_partial_reduction() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let lock_amount = AlphaBalance::from(1_000u64);
        let reduce_amount = AlphaBalance::from(400u64);
        let now = SubtensorModule::get_current_block_as_u64();

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        let conviction = U64F64::from_num(1_000);
        Lock::<Test>::insert(
            (coldkey, netuid, hotkey),
            LockState {
                locked_mass: lock_amount,
                conviction,
                last_update: now,
            },
        );
        HotkeyLock::<Test>::insert(
            netuid,
            hotkey,
            LockState {
                locked_mass: lock_amount,
                conviction,
                last_update: now,
            },
        );

        SubtensorModule::force_reduce_lock(&coldkey, netuid, reduce_amount);

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).expect("lock should remain");
        assert_eq!(lock.locked_mass, 600u64.into());
        assert_abs_diff_eq!(
            lock.conviction.to_num::<f64>(),
            600.,
            epsilon = 0.0000000001
        );

        let hotkey_lock =
            HotkeyLock::<Test>::get(netuid, hotkey).expect("hotkey lock should remain");
        assert_eq!(hotkey_lock.locked_mass, 600u64.into());
        assert_abs_diff_eq!(
            hotkey_lock.conviction.to_num::<f64>(),
            600.,
            epsilon = 0.0000000001
        );
    });
}

#[test]
fn test_reduce_lock_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = subtensor_runtime_common::NetUid::from(1);
        // Should be a no-op, no panic
        SubtensorModule::force_reduce_lock(&coldkey, netuid, 100u64.into());
        assert!(
            Lock::<Test>::iter_prefix((coldkey, netuid))
                .next()
                .is_none()
        );
    });
}

#[test]
fn test_reduce_lock_two_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let coldkey1 = U256::from(1);
        let coldkey2 = U256::from(3);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey1, hotkey, 100_000_000_000);

        // Add stake on coldkey 2
        add_balance_to_coldkey_account(&coldkey2, 100_000_000_000u64.into());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey2, &hotkey
        ));
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey2,
            netuid,
            100_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        DecayingLock::<Test>::insert(coldkey2, netuid, false);

        // Mock a non-zero conviction for both coldkeys
        let lock1 = Lock::<Test>::get((coldkey1, netuid, hotkey)).unwrap_or(LockState {
            locked_mass: 0.into(),
            conviction: U64F64::from_num(1234),
            last_update: System::block_number(),
        });
        let lock2 = Lock::<Test>::get((coldkey2, netuid, hotkey)).unwrap_or(LockState {
            locked_mass: 0.into(),
            conviction: U64F64::from_num(1234),
            last_update: System::block_number(),
        });
        Lock::<Test>::insert((coldkey1, netuid, hotkey), lock1);
        Lock::<Test>::insert((coldkey2, netuid, hotkey), lock2);
        HotkeyLock::<Test>::insert(
            netuid,
            hotkey,
            LockState {
                locked_mass: 0.into(),
                conviction: U64F64::from_num(1234 * 2),
                last_update: System::block_number(),
            },
        );

        // Lock a small amount from both coldkeys
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey1,
            netuid,
            &hotkey,
            50u64.into(),
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey2,
            netuid,
            &hotkey,
            50u64.into(),
        ));

        SubtensorModule::force_reduce_lock(&coldkey1, netuid, 50u64.into());

        // Should only clean up coldkey1's lock, not coldkey2's
        assert!(
            Lock::<Test>::iter_prefix((coldkey1, netuid))
                .next()
                .is_none()
        );
        assert!(Lock::<Test>::get((coldkey2, netuid, hotkey)).is_some());

        // Hotkey lock should reduce according to coldkey1 lock
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey).unwrap();
        assert_eq!(hotkey_lock.locked_mass, 50u64.into());

        // Conviction should be reduced by coldkey1's lock conviction,
        // but not fully reset because coldkey2 still has a lock
        assert!(hotkey_lock.conviction == U64F64::from_num(1234));
    });
}

#[test]
fn test_force_reduce_lock_does_not_over_reduce_hotkey_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey1 = U256::from(1);
        let coldkey2 = U256::from(3);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey1, hotkey, 100_000_000_000);
        let now = SubtensorModule::get_current_block_as_u64();

        Lock::<Test>::insert(
            (coldkey1, netuid, hotkey),
            LockState {
                locked_mass: 1_000u64.into(),
                conviction: U64F64::from_num(1_000),
                last_update: now,
            },
        );
        Lock::<Test>::insert(
            (coldkey2, netuid, hotkey),
            LockState {
                locked_mass: 5_000u64.into(),
                conviction: U64F64::from_num(2_000),
                last_update: now,
            },
        );
        HotkeyLock::<Test>::insert(
            netuid,
            hotkey,
            LockState {
                locked_mass: 6_000u64.into(),
                conviction: U64F64::from_num(3_000),
                last_update: now,
            },
        );

        SubtensorModule::force_reduce_lock(&coldkey1, netuid, 2_000u64.into());

        assert!(Lock::<Test>::get((coldkey1, netuid, hotkey)).is_none());
        assert!(Lock::<Test>::get((coldkey2, netuid, hotkey)).is_some());

        let hotkey_lock =
            HotkeyLock::<Test>::get(netuid, hotkey).expect("hotkey lock should remain");
        assert_eq!(hotkey_lock.locked_mass, 5_000u64.into());
        assert_eq!(hotkey_lock.conviction, U64F64::from_num(2_000));
    });
}

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

// =========================================================================
// GROUP 12: Hotkey swap interaction
// =========================================================================

#[test]
fn test_hotkey_swap_swaps_locks_and_convictions() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(20);
        let netuid = setup_subnet_with_stake(coldkey, old_hotkey, 100_000_000_000);
        Owner::<Test>::insert(old_hotkey, coldkey);
        Owner::<Test>::insert(new_hotkey, coldkey);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &old_hotkey,
            5000u64.into(),
        ));
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, old_hotkey, coldkey
        )));
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, old_hotkey)).count(),
            1
        );

        // Mock a non-zero conviction
        let mut lock = Lock::<Test>::get((coldkey, netuid, old_hotkey)).unwrap();
        lock.conviction = U64F64::from_num(1234);
        Lock::<Test>::insert((coldkey, netuid, old_hotkey), lock);
        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, old_hotkey).unwrap();
        hotkey_lock.conviction = U64F64::from_num(1234);
        HotkeyLock::<Test>::insert(netuid, old_hotkey, hotkey_lock);

        // Perform hotkey swap
        let mut weight = Weight::zero();
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        // Lock references new_hotkey, conviction is not reset
        let lock = Lock::<Test>::get((coldkey, netuid, new_hotkey)).unwrap();
        assert_eq!(lock.locked_mass, 5000u64.into());
        assert!(lock.conviction > U64F64::from_num(0));
        assert!(!LockingColdkeys::<Test>::contains_key((
            netuid, old_hotkey, coldkey
        )));
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, new_hotkey, coldkey
        )));

        // Hotkey lock data also updated, conviction is not reset
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, new_hotkey).unwrap();
        assert_eq!(hotkey_lock.locked_mass, 5000u64.into());
        assert!(hotkey_lock.conviction > U64F64::from_num(0));

        // Trying to top up to new_hotkey works
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &new_hotkey,
            100u64.into()
        ));

        // Trying to top up to old_hotkey fails (old_hotkey is no longer associated with coldkey)
        assert_noop!(
            SubtensorModule::do_lock_stake(&coldkey, netuid, &old_hotkey, 100u64.into()),
            Error::<Test>::HotKeyAccountNotExists
        );
    });
}

// =========================================================================
// GROUP 13: Lock extrinsic via dispatch
// =========================================================================

#[test]
fn test_lock_stake_extrinsic() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let lock_amount: u64 = 5000;
        assert_ok!(SubtensorModule::lock_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            lock_amount.into(),
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).expect("Lock should exist");
        assert_eq!(lock.locked_mass, lock_amount.into());
        assert_eq!(lock.conviction, U64F64::from_num(0));

        // Hotkey lock should also be updated
        let hotkey_lock =
            HotkeyLock::<Test>::get(netuid, hotkey).expect("Hotkey lock should exist");
        assert_eq!(hotkey_lock.locked_mass, lock_amount.into());
        assert_eq!(hotkey_lock.conviction, U64F64::from_num(0));
    });
}

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

        // No stale lock from old subnet remains
        let stale_lock = Lock::<Test>::get((coldkey, netuid, hotkey_old));
        assert!(stale_lock.is_none());

        // No stale hotkey lock remains
        let stale_hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey_old);
        assert!(stale_hotkey_lock.is_none());
    });
}

// =========================================================================
// GROUP 16: Clear small nomination checks lock
// =========================================================================

#[test]
fn test_clear_small_nomination_checks_lock() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(100);
        let owner_hotkey = U256::from(101);
        let netuid = setup_subnet_with_stake(owner_coldkey, owner_hotkey, 100_000_000_000);

        // Set up a nominator (different coldkey, does NOT own the hotkey)
        let nominator = U256::from(200);
        add_balance_to_coldkey_account(&nominator, 100_000_000_000u64.into());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &nominator,
            &owner_hotkey
        ));
        SubtensorModule::stake_into_subnet(
            &owner_hotkey,
            &nominator,
            netuid,
            50_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        let nominator_alpha = get_alpha(&owner_hotkey, &nominator, netuid);
        assert!(nominator_alpha > AlphaBalance::ZERO);

        // Nominator locks their full stake
        let nominator_total = SubtensorModule::total_coldkey_alpha_on_subnet(&nominator, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &nominator,
            netuid,
            &owner_hotkey,
            nominator_total,
        ));

        // Set a high nominator min stake so the current stake is "small"
        SubtensorModule::set_nominator_min_required_stake(u64::MAX);

        // clear_small_nomination removes the lock and unstakes alpha
        SubtensorModule::clear_small_nomination_if_required(&owner_hotkey, &nominator, netuid);

        // Nominator alpha has been removed despite lock
        let nominator_alpha_after = get_alpha(&owner_hotkey, &nominator, netuid);
        assert_eq!(nominator_alpha_after, AlphaBalance::ZERO);

        // Lock entry doesn't exist anymore
        assert!(
            Lock::<Test>::iter_prefix((nominator, netuid))
                .next()
                .is_none()
        );

        // Hotkey lock should also be removed
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, owner_hotkey);
        assert!(hotkey_lock.is_none());
    });
}

#[test]
// If one coldkey has a large nomination on one hotkey and a tiny nomination on another,
// clearing the tiny nomination should reduce the lock state only by that tiny alpha amount.
fn test_clear_small_nomination_reduces_only_tiny_amount_from_lock_state() {
    new_test_ext(1).execute_with(|| {
        // Large stake, subnet owner, and large lock receiver
        let coldkey_large = U256::from(100);
        let hotkey_large = U256::from(101);
        let netuid = setup_subnet_with_stake(coldkey_large, hotkey_large, 100_000_000_000);

        let coldkey_tiny = U256::from(102);
        let hotkey_tiny = U256::from(103);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey_tiny,
            &hotkey_tiny
        ));

        // Coldkey that is going to stake and lock
        let nominator = U256::from(200);
        let large_tao = TaoBalance::from(50_000_000_000u64);
        let tiny_tao = TaoBalance::from(1_000_000u64);
        add_balance_to_coldkey_account(&nominator, large_tao + tiny_tao);

        // Create one large nomination and one tiny nomination on the same subnet.
        SubtensorModule::stake_into_subnet(
            &hotkey_large,
            &nominator,
            netuid,
            large_tao,
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        SubtensorModule::stake_into_subnet(
            &hotkey_tiny,
            &nominator,
            netuid,
            tiny_tao,
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        DecayingLock::<Test>::insert(nominator, netuid, false);

        let large_alpha_before = get_alpha(&hotkey_large, &nominator, netuid);
        let tiny_alpha_before = get_alpha(&hotkey_tiny, &nominator, netuid);
        assert!(large_alpha_before > tiny_alpha_before);

        // Lock against the large nomination hotkey and seed non-zero unlocked_mass + conviction
        // so we can verify each field is reduced only by the tiny nomination's alpha amount.
        let total_before = SubtensorModule::total_coldkey_alpha_on_subnet(&nominator, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &nominator,
            netuid,
            &hotkey_large,
            total_before,
        ));

        let conviction_before = U64F64::from_num(tiny_alpha_before.to_u64() + 2_000);
        let last_update = SubtensorModule::get_current_block_as_u64();
        Lock::<Test>::insert(
            (nominator, netuid, hotkey_large),
            LockState {
                locked_mass: total_before,
                conviction: conviction_before,
                last_update,
            },
        );
        HotkeyLock::<Test>::insert(
            netuid,
            hotkey_large,
            LockState {
                locked_mass: total_before,
                conviction: conviction_before,
                last_update,
            },
        );

        // Force the tiny nomination to qualify as "small" and clear only that nomination.
        SubtensorModule::set_nominator_min_required_stake(u64::MAX);
        SubtensorModule::clear_small_nomination_if_required(&hotkey_tiny, &nominator, netuid);

        // The large nomination stays, the tiny one is removed.
        let large_alpha_after = get_alpha(&hotkey_large, &nominator, netuid);
        let tiny_alpha_after = get_alpha(&hotkey_tiny, &nominator, netuid);
        assert_eq!(large_alpha_after, large_alpha_before);
        assert!(!large_alpha_after.is_zero());
        assert_eq!(tiny_alpha_after, AlphaBalance::ZERO);

        // Only the tiny alpha amount should be shaved off the coldkey lock state.
        // Conviction is reduced proportionally
        let lock_after = Lock::<Test>::get((nominator, netuid, hotkey_large)).unwrap();
        assert!(!lock_after.locked_mass.is_zero());
        assert_eq!(lock_after.locked_mass, total_before - tiny_alpha_before);
        assert!(lock_after.conviction != U64F64::from_num(0));
        let expected_conviction = conviction_before.to_num::<f64>()
            * (1. - u64::from(tiny_alpha_before) as f64 / u64::from(total_before) as f64);
        assert_abs_diff_eq!(
            lock_after.conviction.to_num::<f64>(),
            expected_conviction,
            epsilon = expected_conviction / 1000000.
        );

        // The aggregate hotkey lock on the locked hotkey should also only shrink by the tiny amount.
        let hotkey_lock_after = HotkeyLock::<Test>::get(netuid, hotkey_large).unwrap();
        assert_eq!(
            hotkey_lock_after.locked_mass,
            total_before - tiny_alpha_before
        );
        assert_abs_diff_eq!(
            hotkey_lock_after.conviction.to_num::<f64>(),
            expected_conviction,
            epsilon = expected_conviction / 1000000.
        );
    });
}

// =========================================================================
// GROUP 17: Emission interaction
// =========================================================================

#[test]
fn test_emissions_do_not_break_lock_invariant() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total_alpha_before = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            total_alpha_before
        ));

        // Simulate emission: directly increase alpha for the hotkey on subnet
        // This increases the pool value for all share holders (including our coldkey)
        let emission_amount: AlphaBalance = 10_000_000u64.into();
        SubtensorModule::increase_stake_for_hotkey_on_subnet(&hotkey, netuid, emission_amount);

        // After emission, total alpha should increase by emission_amount
        let total_alpha_after = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        assert_eq!(total_alpha_after, total_alpha_before + emission_amount);

        // Lock invariant still holds: total_alpha >= locked_mass
        let locked = SubtensorModule::get_current_locked(&coldkey, netuid);
        assert!(total_alpha_after >= locked);

        // Available becomes emission_amount
        let available = SubtensorModule::available_to_unstake(&coldkey, netuid);
        assert_eq!(available, emission_amount);
    });
}

#[test]
fn test_epoch_distribution_auto_locks_owner_cut() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let miner_coldkey = U256::from(5);
        let miner_hotkey = U256::from(6);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let subnet_tempo = 10;
        let stake = 100_000_000_000u64;

        SubtensorModule::set_tempo_unchecked(netuid, subnet_tempo);
        SubtensorModule::set_ck_burn(0);
        setup_reserves(netuid, (stake * 10_000).into(), (stake * 10_000).into());

        register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
        register_ok_neuron(netuid, miner_hotkey, miner_coldkey, 1);

        add_balance_to_coldkey_account(
            &validator_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            netuid,
            stake.into()
        ));

        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_max_allowed_validators(netuid, 1);
        step_block(subnet_tempo);
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        OwnerCutAutoLockEnabled::<Test>::insert(netuid, true);

        let owner_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &subnet_owner_hotkey).unwrap();
        let validator_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &validator_hotkey).unwrap();
        let miner_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &miner_hotkey).unwrap();
        let uid_count = [
            owner_uid as usize,
            validator_uid as usize,
            miner_uid as usize,
        ]
        .into_iter()
        .max()
        .unwrap()
            + 1;

        // Setup YUMA so that the next epoch produces non-zero subnet emissions.
        Weights::<Test>::insert(
            NetUidStorageIndex::from(netuid),
            validator_uid,
            vec![(miner_uid, 0xFFFF)],
        );
        BlockAtRegistration::<Test>::set(netuid, owner_uid, 1);
        BlockAtRegistration::<Test>::set(netuid, validator_uid, 1);
        BlockAtRegistration::<Test>::set(netuid, miner_uid, 1);
        LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2; uid_count]);
        Kappa::<Test>::set(netuid, u16::MAX / 5);
        ActivityCutoff::<Test>::set(netuid, u16::MAX);
        let mut validator_permit = vec![false; uid_count];
        validator_permit[validator_uid as usize] = true;
        ValidatorPermit::<Test>::insert(netuid, validator_permit);

        let owner_stake_before = get_alpha(&subnet_owner_hotkey, &subnet_owner_coldkey, netuid);
        assert!(
            Lock::<Test>::iter_prefix((subnet_owner_coldkey, netuid))
                .next()
                .is_none()
        );

        // Advance to the next epoch so owner cut is distributed and auto-locked.
        step_epochs(1, netuid);

        let owner_stake_after = get_alpha(&subnet_owner_hotkey, &subnet_owner_coldkey, netuid);
        let owner_cut_locked = owner_stake_after - owner_stake_before;
        assert!(owner_cut_locked > AlphaBalance::ZERO);

        let owner_lock = Lock::<Test>::get((subnet_owner_coldkey, netuid, subnet_owner_hotkey))
            .expect("owner cut should be auto-locked to the subnet owner's hotkey");
        assert_eq!(owner_lock.locked_mass, owner_cut_locked);
    });
}

#[test]
fn test_auto_lock_owner_cut_is_disabled_by_default_and_can_be_enabled() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid =
            setup_subnet_with_stake(subnet_owner_coldkey, subnet_owner_hotkey, 100_000_000_000);
        let owner_cut: AlphaBalance = 10_000_000u64.into();

        assert!(!SubtensorModule::get_owner_cut_auto_lock_enabled(netuid));
        SubtensorModule::auto_lock_owner_cut(netuid, owner_cut);

        assert!(
            Lock::<Test>::iter_prefix((subnet_owner_coldkey, netuid))
                .next()
                .is_none()
        );

        OwnerCutAutoLockEnabled::<Test>::insert(netuid, true);
        assert!(SubtensorModule::get_owner_cut_auto_lock_enabled(netuid));
        SubtensorModule::auto_lock_owner_cut(netuid, owner_cut);

        let owner_lock = Lock::<Test>::get((subnet_owner_coldkey, netuid, subnet_owner_hotkey))
            .expect("owner cut should be auto-locked when enabled");
        assert_eq!(owner_lock.locked_mass, owner_cut);
    });
}

// =========================================================================
// GROUP 18: Neuron replacement
// =========================================================================

#[test]
fn test_neuron_replacement_does_not_affect_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        // Register the hotkey as a neuron
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        let lock_amount = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount
        ));
        assert_ok!(SubtensorModule::do_set_perpetual_lock(
            &coldkey, netuid, false,
        ));

        let total_before = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let locked_before = SubtensorModule::get_current_locked(&coldkey, netuid);

        // Replace the neuron with a different hotkey
        let new_hotkey = U256::from(99);
        let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
        SubtensorModule::replace_neuron(
            netuid,
            uid,
            &new_hotkey,
            SubtensorModule::get_current_block_as_u64(),
        );

        // Alpha and lock should be unaffected by neuron replacement
        let total_after = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let locked_after = SubtensorModule::get_current_locked(&coldkey, netuid);

        assert_eq!(total_after, total_before);
        assert_eq!(locked_after, locked_before);

        // Lock still references original hotkey
        assert!(Lock::<Test>::get((coldkey, netuid, hotkey)).is_some());

        // Aggregate lock still references original hotkey
        assert!(DecayingHotkeyLock::<Test>::get(netuid, hotkey).is_some());
    });
}

// =========================================================================
// GROUP 19: Moving lock
// =========================================================================

#[test]
fn test_moving_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_origin = U256::from(2);
        let hotkey_destination = U256::from(3);
        let netuid = setup_subnet_with_stake(coldkey, hotkey_origin, 100_000_000_000);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey,
            &hotkey_destination
        ));

        let lock_amount = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey_origin,
            lock_amount
        ));

        // Mock a non-zero conviction
        let mut lock = Lock::<Test>::get((coldkey, netuid, hotkey_origin)).unwrap();
        lock.conviction = U64F64::from_num(1234);
        Lock::<Test>::insert((coldkey, netuid, hotkey_origin), lock);
        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey_origin).unwrap();
        hotkey_lock.conviction = U64F64::from_num(1234);
        HotkeyLock::<Test>::insert(netuid, hotkey_origin, hotkey_lock);

        assert_ok!(SubtensorModule::move_lock(
            RuntimeOrigin::signed(coldkey),
            hotkey_destination,
            netuid,
        ));
        let lock = Lock::<Test>::get((coldkey, netuid, hotkey_destination)).unwrap();
        assert_eq!(lock.locked_mass, lock_amount);
        assert_eq!(lock.conviction, U64F64::from_num(1234));

        // Hotkey lock is removed on origin and added on destination
        assert!(HotkeyLock::<Test>::get(netuid, hotkey_origin).is_none());
        let hotkey_lock_destination_after =
            HotkeyLock::<Test>::get(netuid, hotkey_destination).unwrap();
        assert_eq!(hotkey_lock_destination_after.locked_mass, lock_amount);

        // Conviction is not reset because owner is the same for origin and destination
        // hotkeys
        assert_eq!(
            hotkey_lock_destination_after.conviction,
            U64F64::from_num(1234)
        );
    });
}

#[test]
fn test_moving_lock_to_subnet_owner_hotkey_gets_owner_conviction_for_non_owner_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_origin = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey_origin, 100_000_000_000);
        let owner_hotkey = SubnetOwnerHotkey::<Test>::get(netuid);

        let lock_amount = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey_origin,
            lock_amount
        ));

        assert_ok!(SubtensorModule::move_lock(
            RuntimeOrigin::signed(coldkey),
            owner_hotkey,
            netuid,
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, owner_hotkey)).unwrap();
        assert_eq!(lock.locked_mass, lock_amount);
        assert_eq!(lock.conviction, U64F64::from_num(5000));

        assert!(
            HotkeyLock::<Test>::get(netuid, owner_hotkey).is_none(),
            "lock moved to owner hotkey should use OwnerLock"
        );
        let owner_lock = OwnerLock::<Test>::get(netuid).unwrap();
        assert_eq!(owner_lock.locked_mass, lock_amount);
        assert_eq!(owner_lock.conviction, U64F64::from_num(5000));
    });
}

#[test]
fn test_moving_partial_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey1 = U256::from(1);
        let coldkey2 = U256::from(2);
        let hotkey_origin = U256::from(3);
        let hotkey_destination = U256::from(4);
        let netuid = setup_subnet_with_stake(coldkey1, hotkey_origin, 100_000_000_000);

        // Make hotkey_origin and hotkey_destination owned by different coldkeys
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey1,
            &hotkey_origin
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey2,
            &hotkey_destination
        ));

        // Add coldkey2 stake
        add_balance_to_coldkey_account(&coldkey2, 100_000_000_000u64.into());
        SubtensorModule::stake_into_subnet(
            &hotkey_origin,
            &coldkey2,
            netuid,
            50_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        DecayingLock::<Test>::insert(coldkey2, netuid, false);

        let lock_amount = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey1,
            netuid,
            &hotkey_origin,
            lock_amount
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey2,
            netuid,
            &hotkey_origin,
            lock_amount
        ));

        // Mock a non-zero conviction
        let mut lock1 = Lock::<Test>::get((coldkey1, netuid, hotkey_origin)).unwrap();
        lock1.conviction = U64F64::from_num(1000);
        Lock::<Test>::insert((coldkey1, netuid, hotkey_origin), lock1);
        let mut lock2 = Lock::<Test>::get((coldkey2, netuid, hotkey_origin)).unwrap();
        lock2.conviction = U64F64::from_num(1000);
        Lock::<Test>::insert((coldkey2, netuid, hotkey_origin), lock2);
        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey_origin).unwrap();
        hotkey_lock.conviction = U64F64::from_num(2000);
        HotkeyLock::<Test>::insert(netuid, hotkey_origin, hotkey_lock);

        // Move lock for coldkey1 to hotkey_destination, coldkey2's lock should be unaffected
        assert_ok!(SubtensorModule::move_lock(
            RuntimeOrigin::signed(coldkey1),
            hotkey_destination,
            netuid,
        ));
        let lock1_after = Lock::<Test>::get((coldkey1, netuid, hotkey_destination)).unwrap();
        let lock2_after = Lock::<Test>::get((coldkey2, netuid, hotkey_origin)).unwrap();
        assert_eq!(lock1_after.locked_mass, lock_amount);
        assert_eq!(lock1_after.conviction, U64F64::from_num(0));
        assert_eq!(lock2_after.locked_mass, lock_amount);
        assert_eq!(lock2_after.conviction, U64F64::from_num(1000));

        // Hotkey lock is removed on origin and added on destination
        let hotkey_lock_origin_after = HotkeyLock::<Test>::get(netuid, hotkey_origin).unwrap();
        let hotkey_lock_destination_after =
            HotkeyLock::<Test>::get(netuid, hotkey_destination).unwrap();
        assert_eq!(hotkey_lock_origin_after.locked_mass, lock_amount);
        assert_eq!(hotkey_lock_origin_after.conviction, U64F64::from_num(1000));
        assert_eq!(hotkey_lock_destination_after.locked_mass, lock_amount);
        assert_eq!(
            hotkey_lock_destination_after.conviction,
            U64F64::from_num(0)
        );
    });
}

#[test]
fn test_moving_partial_lock_same_owners() {
    new_test_ext(1).execute_with(|| {
        let coldkey1 = U256::from(1);
        let coldkey2 = U256::from(2);
        let hotkey_origin = U256::from(3);
        let hotkey_destination = U256::from(4);
        let netuid = setup_subnet_with_stake(coldkey1, hotkey_origin, 100_000_000_000);

        // Add coldkey2 stake
        add_balance_to_coldkey_account(&coldkey2, 100_000_000_000u64.into());

        // Make hotkey_origin and hotkey_destination both owned by coldkey1
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey1,
            &hotkey_origin
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey1,
            &hotkey_destination
        ));
        SubtensorModule::stake_into_subnet(
            &hotkey_origin,
            &coldkey2,
            netuid,
            50_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        DecayingLock::<Test>::insert(coldkey2, netuid, false);

        let lock_amount = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey1,
            netuid,
            &hotkey_origin,
            lock_amount
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey2,
            netuid,
            &hotkey_origin,
            lock_amount
        ));

        // Mock a non-zero conviction
        let mut lock1 = Lock::<Test>::get((coldkey1, netuid, hotkey_origin)).unwrap();
        lock1.conviction = U64F64::from_num(1000);
        Lock::<Test>::insert((coldkey1, netuid, hotkey_origin), lock1);
        let mut lock2 = Lock::<Test>::get((coldkey2, netuid, hotkey_origin)).unwrap();
        lock2.conviction = U64F64::from_num(1000);
        Lock::<Test>::insert((coldkey2, netuid, hotkey_origin), lock2);
        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey_origin).unwrap();
        hotkey_lock.conviction = U64F64::from_num(2000);
        HotkeyLock::<Test>::insert(netuid, hotkey_origin, hotkey_lock);

        // Move lock for coldkey1 to hotkey_destination, coldkey2's lock should be unaffected
        assert_ok!(SubtensorModule::move_lock(
            RuntimeOrigin::signed(coldkey1),
            hotkey_destination,
            netuid,
        ));
        let lock1_after = Lock::<Test>::get((coldkey1, netuid, hotkey_destination)).unwrap();
        let lock2_after = Lock::<Test>::get((coldkey2, netuid, hotkey_origin)).unwrap();
        assert_eq!(lock1_after.locked_mass, lock_amount);
        assert_eq!(lock1_after.conviction, U64F64::from_num(1000));
        assert_eq!(lock2_after.locked_mass, lock_amount);
        assert_eq!(lock2_after.conviction, U64F64::from_num(1000));

        // Hotkey lock is moved to destination with conviction
        let hotkey_lock_origin_after = HotkeyLock::<Test>::get(netuid, hotkey_origin).unwrap();
        let hotkey_lock_destination_after =
            HotkeyLock::<Test>::get(netuid, hotkey_destination).unwrap();
        assert_eq!(hotkey_lock_origin_after.locked_mass, lock_amount);
        assert_eq!(hotkey_lock_origin_after.conviction, U64F64::from_num(1000));
        assert_eq!(hotkey_lock_destination_after.locked_mass, lock_amount);
        assert_eq!(
            hotkey_lock_destination_after.conviction,
            U64F64::from_num(1000)
        );
    });
}

#[test]
fn test_hotkey_swap_moves_lock_and_conviction_to_new_hotkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(3);
        let netuid = setup_subnet_with_stake(coldkey, old_hotkey, 100_000_000_000);
        let lock_amount: AlphaBalance = 5000u64.into();
        let conviction = U64F64::from_num(1000);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &old_hotkey,
            lock_amount,
        ));

        let mut lock = Lock::<Test>::get((coldkey, netuid, old_hotkey)).unwrap();
        lock.conviction = conviction;
        Lock::<Test>::insert((coldkey, netuid, old_hotkey), lock);

        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, old_hotkey).unwrap();
        hotkey_lock.conviction = conviction;
        HotkeyLock::<Test>::insert(netuid, old_hotkey, hotkey_lock);

        add_balance_to_coldkey_account(
            &coldkey,
            (SubtensorModule::get_key_swap_cost() + 1000.into()).into(),
        );
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            None,
            false,
        ));

        assert!(Lock::<Test>::get((coldkey, netuid, old_hotkey)).is_none());
        assert!(HotkeyLock::<Test>::get(netuid, old_hotkey).is_none());

        let moved_lock = Lock::<Test>::get((coldkey, netuid, new_hotkey)).unwrap();
        assert_eq!(moved_lock.locked_mass, lock_amount);
        assert_eq!(moved_lock.conviction, conviction);

        let moved_hotkey_lock = HotkeyLock::<Test>::get(netuid, new_hotkey).unwrap();
        assert_eq!(moved_hotkey_lock.locked_mass, lock_amount);
        assert_eq!(moved_hotkey_lock.conviction, conviction);
        assert_eq!(
            SubtensorModule::hotkey_conviction(&new_hotkey, netuid),
            conviction
        );
    });
}

#[test]
fn test_swap_hotkey_v2_on_subnet_moves_lock_and_conviction_to_new_hotkey() {
    new_test_ext(100).execute_with(|| {
        let coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(3);
        let netuid = setup_subnet_with_stake(coldkey, old_hotkey, 100_000_000_000);
        let lock_amount: AlphaBalance = 5000u64.into();
        let conviction = U64F64::from_num(1000);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &old_hotkey,
            lock_amount,
        ));

        let mut lock = Lock::<Test>::get((coldkey, netuid, old_hotkey)).unwrap();
        lock.conviction = conviction;
        Lock::<Test>::insert((coldkey, netuid, old_hotkey), lock);

        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, old_hotkey).unwrap();
        hotkey_lock.conviction = conviction;
        HotkeyLock::<Test>::insert(netuid, old_hotkey, hotkey_lock);

        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000u64.into());
        assert_ok!(SubtensorModule::swap_hotkey_v2(
            RuntimeOrigin::signed(coldkey),
            old_hotkey,
            new_hotkey,
            Some(netuid),
            false,
        ));

        assert!(Lock::<Test>::get((coldkey, netuid, old_hotkey)).is_none());
        assert!(HotkeyLock::<Test>::get(netuid, old_hotkey).is_none());

        let moved_lock = Lock::<Test>::get((coldkey, netuid, new_hotkey)).unwrap();
        assert_eq!(moved_lock.locked_mass, lock_amount);
        assert_eq!(moved_lock.conviction, conviction);

        let moved_hotkey_lock = HotkeyLock::<Test>::get(netuid, new_hotkey).unwrap();
        assert_eq!(moved_hotkey_lock.locked_mass, lock_amount);
        assert_eq!(moved_hotkey_lock.conviction, conviction);
        assert_eq!(
            SubtensorModule::hotkey_conviction(&new_hotkey, netuid),
            conviction
        );
    });
}

#[test]
fn test_swap_hotkey_v2_on_subnet_does_not_move_locks_on_other_subnets() {
    new_test_ext(100).execute_with(|| {
        let coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(3);
        let swapped_netuid = setup_subnet_with_stake(coldkey, old_hotkey, 100_000_000_000);
        let untouched_netuid = setup_subnet_with_stake(coldkey, old_hotkey, 100_000_000_000);
        let lock_amount: AlphaBalance = 5000u64.into();
        let conviction = U64F64::from_num(1000);

        for netuid in [swapped_netuid, untouched_netuid] {
            assert_ok!(SubtensorModule::do_lock_stake(
                &coldkey,
                netuid,
                &old_hotkey,
                lock_amount,
            ));

            let mut lock = Lock::<Test>::get((coldkey, netuid, old_hotkey)).unwrap();
            lock.conviction = conviction;
            Lock::<Test>::insert((coldkey, netuid, old_hotkey), lock);

            let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, old_hotkey).unwrap();
            hotkey_lock.conviction = conviction;
            HotkeyLock::<Test>::insert(netuid, old_hotkey, hotkey_lock);
        }

        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000u64.into());
        assert_ok!(SubtensorModule::swap_hotkey_v2(
            RuntimeOrigin::signed(coldkey),
            old_hotkey,
            new_hotkey,
            Some(swapped_netuid),
            false,
        ));

        assert!(Lock::<Test>::get((coldkey, swapped_netuid, old_hotkey)).is_none());
        assert!(HotkeyLock::<Test>::get(swapped_netuid, old_hotkey).is_none());
        assert_eq!(
            Lock::<Test>::get((coldkey, swapped_netuid, new_hotkey))
                .unwrap()
                .conviction,
            conviction
        );
        assert_eq!(
            HotkeyLock::<Test>::get(swapped_netuid, new_hotkey)
                .unwrap()
                .conviction,
            conviction
        );

        let untouched_lock = Lock::<Test>::get((coldkey, untouched_netuid, old_hotkey)).unwrap();
        assert_eq!(untouched_lock.locked_mass, lock_amount);
        assert_eq!(untouched_lock.conviction, conviction);
        assert!(Lock::<Test>::get((coldkey, untouched_netuid, new_hotkey)).is_none());

        let untouched_hotkey_lock = HotkeyLock::<Test>::get(untouched_netuid, old_hotkey).unwrap();
        assert_eq!(untouched_hotkey_lock.locked_mass, lock_amount);
        assert_eq!(untouched_hotkey_lock.conviction, conviction);
        assert!(HotkeyLock::<Test>::get(untouched_netuid, new_hotkey).is_none());
    });
}
