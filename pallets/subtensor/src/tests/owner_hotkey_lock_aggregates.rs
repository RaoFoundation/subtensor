#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Aggregate lock buckets (`HotkeyLock`, `DecayingHotkeyLock`, `OwnerLock`,
//! `DecayingOwnerLock`) must keep matching the individual `Lock` rows after the
//! subnet owner hotkey is updated.

use frame_support::{assert_ok, weights::Weight};
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::SwapHandler;

use super::mock::*;
use crate::*;

fn setup(coldkey: U256, hotkey: U256, stake_tao: u64, perpetual: bool) -> NetUid {
    let netuid = add_dynamic_network(&U256::from(1002), &U256::from(1001));
    setup_reserves(
        netuid,
        (stake_tao * 1_000_000).into(),
        (stake_tao * 10_000_000).into(),
    );
    stake_on(coldkey, hotkey, netuid, stake_tao);
    if perpetual {
        DecayingLock::<Test>::insert(coldkey, netuid, false);
    }
    MaturityRate::<Test>::put(1_000);
    UnlockRate::<Test>::put(3_000);
    netuid
}

fn stake_on(coldkey: U256, hotkey: U256, netuid: NetUid, stake_tao: u64) {
    assert_ok!(SubtensorModule::create_account_if_non_existent(
        &coldkey, &hotkey
    ));
    let amount: TaoBalance = stake_tao.into();
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
}

fn mass(lock: Option<crate::staking::lock::LockState>) -> u64 {
    lock.map(|l| u64::from(l.locked_mass)).unwrap_or(0)
}

/// Sum of the rolled-forward individual lock rows on `hotkey`.
fn lock_rows_mass(netuid: NetUid, hotkey: &U256) -> u64 {
    let now = SubtensorModule::get_current_block_as_u64();
    LockingColdkeys::<Test>::iter_prefix((netuid, *hotkey))
        .filter_map(|(ck, _)| Lock::<Test>::get((ck, netuid, *hotkey)).map(|l| (ck, l)))
        .map(|(ck, l)| {
            u64::from(
                crate::staking::lock::ConvictionModel::roll_forward_lock(
                    l,
                    now,
                    UnlockRate::<Test>::get(),
                    MaturityRate::<Test>::get(),
                    true,
                    DecayingLock::<Test>::get(ck, netuid) == Some(false),
                )
                .0
                .locked_mass,
            )
        })
        .sum()
}

/// Aggregate mass attributed to the owner hotkey `hotkey`.
fn owner_aggregate_mass(netuid: NetUid, hotkey: &U256) -> u64 {
    mass(HotkeyLock::<Test>::get(netuid, hotkey))
        + mass(DecayingHotkeyLock::<Test>::get(netuid, hotkey))
        + mass(OwnerLock::<Test>::get(netuid))
        + mass(DecayingOwnerLock::<Test>::get(netuid))
}

fn assert_owner_buckets_only(netuid: NetUid, hotkey: &U256) {
    assert!(HotkeyLock::<Test>::get(netuid, hotkey).is_none());
    assert!(DecayingHotkeyLock::<Test>::get(netuid, hotkey).is_none());
}

fn coldkey_swap_case(perpetual: bool) {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let new_coldkey = U256::from(10);
        let netuid = setup(owner_coldkey, hotkey, 100_000_000_000, perpetual);
        SubnetOwner::<Test>::insert(netuid, owner_coldkey);
        let locked: AlphaBalance = 1_000_000_000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &owner_coldkey,
            netuid,
            &hotkey,
            locked
        ));
        step_block(500);
        let coldkey_conviction = SubtensorModule::get_conviction(&owner_coldkey, netuid);

        assert_ok!(SubtensorModule::do_set_sn_owner_hotkey(
            RuntimeOrigin::root(),
            netuid,
            &hotkey
        ));
        assert_owner_buckets_only(netuid, &hotkey);
        assert_eq!(
            owner_aggregate_mass(netuid, &hotkey),
            lock_rows_mass(netuid, &hotkey)
        );
        let before = SubtensorModule::hotkey_conviction(&hotkey, netuid);

        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(new_coldkey),
            false
        ));
        assert_ok!(SubtensorModule::do_swap_coldkey(
            &owner_coldkey,
            &new_coldkey
        ));
        let after = SubtensorModule::hotkey_conviction(&hotkey, netuid);

        assert_eq!(
            owner_aggregate_mass(netuid, &hotkey),
            lock_rows_mass(netuid, &hotkey)
        );
        let locked_f = U64F64::from_num(u64::from(locked)).to_num::<f64>();
        assert!(
            after.to_num::<f64>()
                <= before.to_num::<f64>() + locked_f - coldkey_conviction.to_num::<f64>() + 1.0
        );
    });
}

#[test]
fn owner_hotkey_update_keeps_lock_aggregates_consistent_on_coldkey_swap() {
    coldkey_swap_case(true);
}

#[test]
fn owner_hotkey_update_keeps_decaying_lock_aggregates_consistent_on_coldkey_swap() {
    coldkey_swap_case(false);
}

#[test]
fn owner_hotkey_update_keeps_lock_aggregates_consistent_on_transfer_lock() {
    new_test_ext(1).execute_with(|| {
        let sender = U256::from(1);
        let hotkey = U256::from(2);
        let recipient = U256::from(20);
        let netuid = setup(sender, hotkey, 100_000_000_000, false);
        let locked: AlphaBalance = 1_000_000_000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &sender, netuid, &hotkey, locked
        ));
        step_block(500);
        assert_ok!(SubtensorModule::do_set_sn_owner_hotkey(
            RuntimeOrigin::root(),
            netuid,
            &hotkey
        ));
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(recipient),
            false
        ));
        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&sender, netuid);
        assert_ok!(SubtensorModule::transfer_lock(
            &sender, &recipient, &hotkey, netuid, total
        ));
        assert_eq!(
            owner_aggregate_mass(netuid, &hotkey),
            lock_rows_mass(netuid, &hotkey)
        );
    });
}

#[test]
fn owner_hotkey_update_keeps_lock_aggregates_consistent_on_force_reduce() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup(coldkey, hotkey, 100_000_000_000, true);
        let locked: AlphaBalance = 1_000_000_000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, locked
        ));
        step_block(500);
        assert_ok!(SubtensorModule::do_set_sn_owner_hotkey(
            RuntimeOrigin::root(),
            netuid,
            &hotkey
        ));
        SubtensorModule::force_reduce_lock(&coldkey, netuid, locked);
        assert_eq!(
            owner_aggregate_mass(netuid, &hotkey),
            lock_rows_mass(netuid, &hotkey)
        );
        assert_eq!(
            SubtensorModule::hotkey_conviction(&hotkey, netuid),
            U64F64::from_num(0)
        );
    });
}

#[test]
fn owner_hotkey_swap_moves_new_hotkey_lock_aggregates_to_owner_buckets() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1002);
        let owner_coldkey = U256::from(1001);
        let staker = U256::from(1);
        let new_hotkey = U256::from(2);
        let netuid = setup(staker, new_hotkey, 100_000_000_000, true);
        let locked: AlphaBalance = 1_000_000_000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &staker,
            netuid,
            &new_hotkey,
            locked
        ));
        step_block(500);
        assert!(HotkeyLock::<Test>::get(netuid, new_hotkey).is_some());
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), owner_hotkey);

        let mut weight = Weight::zero();
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &owner_hotkey,
            &new_hotkey,
            &owner_coldkey,
            &mut weight,
            false,
        ));

        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), new_hotkey);
        assert_owner_buckets_only(netuid, &new_hotkey);
        assert_eq!(
            owner_aggregate_mass(netuid, &new_hotkey),
            lock_rows_mass(netuid, &new_hotkey)
        );
    });
}

#[test]
fn coldkey_swap_without_owner_hotkey_update_keeps_lock_aggregates_consistent() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let new_coldkey = U256::from(10);
        let netuid = setup(coldkey, hotkey, 100_000_000_000, true);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            1_000_000_000u64.into()
        ));
        step_block(500);
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(new_coldkey),
            false
        ));
        assert_ok!(SubtensorModule::do_swap_coldkey(&coldkey, &new_coldkey));
        assert_eq!(
            owner_aggregate_mass(netuid, &hotkey),
            lock_rows_mass(netuid, &hotkey)
        );
    });
}
