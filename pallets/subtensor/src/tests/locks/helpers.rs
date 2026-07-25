#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Setup and roll-forward fixtures for stake-lock unit tests.

use frame_support::assert_ok;
use sp_core::U256;
use subtensor_runtime_common::{AlphaBalance, TaoBalance};
use subtensor_swap_interface::SwapHandler;

use super::super::mock::*;
use crate::staking::lock::{ConvictionModel, LockState};
use crate::*;

pub(super) fn setup_subnet_with_stake(
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

pub(super) fn get_alpha(
    hotkey: &U256,
    coldkey: &U256,
    netuid: subtensor_runtime_common::NetUid,
) -> AlphaBalance {
    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid)
}

pub(super) fn roll_forward_lock(
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

pub(super) fn roll_forward_individual_lock(
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

pub(super) fn roll_forward_hotkey_lock(lock: LockState, now: u64) -> LockState {
    roll_forward_lock(lock, now, false, true)
}

pub(super) fn roll_forward_decaying_hotkey_lock(lock: LockState, now: u64) -> LockState {
    roll_forward_lock(lock, now, false, false)
}
