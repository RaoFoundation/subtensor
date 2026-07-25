#![allow(unused, clippy::indexing_slicing, clippy::panic, clippy::unwrap_used)]

use approx::assert_abs_diff_eq;
use codec::Encode;
use frame_support::weights::Weight;
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::{Config, RawOrigin};
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance, Token};

use super::super::mock::*;
use crate::*;
use share_pool::SafeFloat;
use sp_core::{Get, H160, H256, U256};
use sp_runtime::{PerU16, SaturatedConversion};
use std::collections::BTreeSet;
use substrate_fixed::types::{I96F32, U64F64};

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_total_hotkey_stake --exact --nocapture
#[test]
fn test_swap_total_hotkey_stake() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let fee = (amount as f64 * 0.003) as u64;

        //add network
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        remove_owner_registration_stake(netuid);

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Add stake
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            old_hotkey,
            netuid,
            amount.into()
        ));

        // Check if stake has increased
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&old_hotkey),
            (amount - fee).into(),
            epsilon = TaoBalance::from(amount / 100),
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&new_hotkey),
            TaoBalance::ZERO,
            epsilon = 1.into(),
        );

        // Swap hotkey
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Verify that total hotkey stake swapped
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&old_hotkey),
            TaoBalance::ZERO,
            epsilon = 1.into(),
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&new_hotkey),
            TaoBalance::from(amount - fee),
            epsilon = TaoBalance::from(amount / 100),
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_staking_hotkeys --exact --nocapture
#[test]
fn test_swap_staking_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        StakingHotkeys::<Test>::insert(coldkey, vec![old_hotkey]);
        Alpha::<Test>::insert((old_hotkey, coldkey, netuid), U64F64::from_num(100));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        let staking_hotkeys = StakingHotkeys::<Test>::get(coldkey);
        assert!(staking_hotkeys.contains(&old_hotkey));
        assert!(staking_hotkeys.contains(&new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey::test_swap_hotkey_with_multiple_coldkeys --exact --show-output --nocapture
#[test]
fn test_swap_hotkey_with_multiple_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey1 = U256::from(3);
        let coldkey2 = U256::from(4);

        let stake = 1_000_000_000;

        StakingHotkeys::<Test>::insert(coldkey1, vec![old_hotkey]);
        StakingHotkeys::<Test>::insert(coldkey2, vec![old_hotkey]);
        SubtensorModule::create_account_if_non_existent(&coldkey1, &old_hotkey);
        add_balance_to_coldkey_account(&coldkey1, 1_000_000_000_000_u64.into());
        add_balance_to_coldkey_account(&coldkey2, 1_000_000_000_000_u64.into());

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey1),
            old_hotkey,
            netuid,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey2),
            old_hotkey,
            netuid,
            TaoBalance::from(stake / 2)
        ));
        let stake1_before = SubtensorModule::get_total_stake_for_coldkey(&coldkey1);
        let stake2_before = SubtensorModule::get_total_stake_for_coldkey(&coldkey2);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey1),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey1),
            SubtensorModule::get_total_stake_for_coldkey(&coldkey1),
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey2),
            SubtensorModule::get_total_stake_for_coldkey(&coldkey2),
        );

        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey1),
            stake1_before
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey2),
            stake2_before
        );

        assert!(StakingHotkeys::<Test>::get(coldkey1).contains(&new_hotkey));
        assert!(StakingHotkeys::<Test>::get(coldkey2).contains(&new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_hotkey_with_multiple_subnets --exact --nocapture
#[test]
fn test_swap_hotkey_with_multiple_subnets() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let new_hotkey_2 = U256::from(3);
        let coldkey = U256::from(4);

        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        let netuid1 = add_dynamic_network(&old_hotkey, &coldkey);
        let netuid2 = add_dynamic_network(&old_hotkey, &coldkey);

        IsNetworkMember::<Test>::insert(old_hotkey, netuid1, true);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid2, true);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid1),
            false
        ));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey_2,
            Some(netuid2),
            false
        ));

        assert!(IsNetworkMember::<Test>::get(new_hotkey, netuid1));
        assert!(IsNetworkMember::<Test>::get(new_hotkey_2, netuid2));
        assert!(!IsNetworkMember::<Test>::get(old_hotkey, netuid1));
        assert!(!IsNetworkMember::<Test>::get(old_hotkey, netuid2));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_staking_hotkeys_multiple_coldkeys --exact --nocapture
#[test]
fn test_swap_staking_hotkeys_multiple_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey1 = U256::from(3);
        let coldkey2 = U256::from(4);
        let staker5 = U256::from(5);

        let stake = 1_000_000_000;
        add_balance_to_coldkey_account(&coldkey1, 1_000_000_000_000_u64.into());
        add_balance_to_coldkey_account(&coldkey2, 1_000_000_000_000_u64.into());

        // Set up initial state
        StakingHotkeys::<Test>::insert(coldkey1, vec![old_hotkey]);
        StakingHotkeys::<Test>::insert(coldkey2, vec![old_hotkey, staker5]);

        SubtensorModule::create_account_if_non_existent(&coldkey1, &old_hotkey);

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey1),
            old_hotkey,
            netuid,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey2),
            old_hotkey,
            netuid,
            stake.into()
        ));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey1),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Check if new_hotkey replaced old_hotkey in StakingHotkeys
        assert!(StakingHotkeys::<Test>::get(coldkey1).contains(&new_hotkey));
        assert!(StakingHotkeys::<Test>::get(coldkey1).contains(&old_hotkey));

        // Check if new_hotkey replaced old_hotkey for coldkey2 as well
        assert!(StakingHotkeys::<Test>::get(coldkey2).contains(&new_hotkey));
        assert!(StakingHotkeys::<Test>::get(coldkey2).contains(&old_hotkey));
        assert!(StakingHotkeys::<Test>::get(coldkey2).contains(&staker5));
        // Other hotkeys should remain
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_hotkey_with_no_stake --exact --nocapture
#[test]
fn test_swap_hotkey_with_no_stake() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Set up initial state with no stake
        Owner::<Test>::insert(old_hotkey, coldkey);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Check if ownership transferred
        assert!(Owner::<Test>::contains_key(old_hotkey));
        assert_eq!(Owner::<Test>::get(new_hotkey), coldkey);

        // Ensure no unexpected changes in Stake
        assert!(!Alpha::<Test>::contains_key((old_hotkey, coldkey, netuid)));
        assert!(!Alpha::<Test>::contains_key((new_hotkey, coldkey, netuid)));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey::test_swap_hotkey_with_multiple_coldkeys_and_subnets --exact --show-output
#[test]
fn test_swap_hotkey_with_multiple_coldkeys_and_subnets() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let new_hotkey_2 = U256::from(3);
        let coldkey1 = U256::from(4);
        let coldkey2 = U256::from(5);
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let stake = DefaultMinStake::<Test>::get().to_u64() * 10;

        // Set up initial state
        add_network(netuid1, 1, 1);
        add_network(netuid2, 1, 1);
        register_ok_neuron(netuid1, old_hotkey, coldkey1, 1234);
        register_ok_neuron(netuid2, old_hotkey, coldkey1, 1234);

        // Add balance to both coldkeys
        add_balance_to_coldkey_account(&coldkey1, 1_000_000_000_000_u64.into());
        add_balance_to_coldkey_account(&coldkey2, 1_000_000_000_000_u64.into());

        // Stake with coldkey1
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey1),
            old_hotkey,
            netuid1,
            stake.into()
        ));

        // Stake with coldkey2 also
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey2),
            old_hotkey,
            netuid2,
            stake.into()
        ));

        let ck1_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &coldkey1,
            netuid1,
        );
        let ck2_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &coldkey2,
            netuid2,
        );
        assert!(!ck1_stake.is_zero());
        assert!(!ck2_stake.is_zero());
        let total_hk_stake = SubtensorModule::get_total_stake_for_hotkey(&old_hotkey);
        assert!(!total_hk_stake.is_zero());
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());

        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey1),
            &old_hotkey,
            &new_hotkey,
            Some(netuid1),
            false
        ));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey1),
            &old_hotkey,
            &new_hotkey_2,
            Some(netuid2),
            false
        ));

        // Check ownership transfer
        assert_eq!(
            SubtensorModule::get_owning_coldkey_for_hotkey(&new_hotkey),
            coldkey1
        );
        assert!(!SubtensorModule::get_owned_hotkeys(&coldkey2).contains(&new_hotkey));
        assert_eq!(
            SubtensorModule::get_owning_coldkey_for_hotkey(&new_hotkey_2),
            coldkey1
        );
        assert!(!SubtensorModule::get_owned_hotkeys(&coldkey2).contains(&new_hotkey_2));

        // Check stake transfer
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey1,
                netuid1
            ),
            ck1_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey_2,
                &coldkey2,
                netuid2
            ),
            ck2_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &coldkey1,
                netuid1
            ),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &coldkey2,
                netuid2
            ),
            AlphaBalance::ZERO
        );

        // Check subnet membership transfer
        assert!(SubtensorModule::is_hotkey_registered_on_network(
            netuid1,
            &new_hotkey
        ));
        assert!(SubtensorModule::is_hotkey_registered_on_network(
            netuid2,
            &new_hotkey_2
        ));
        assert!(!SubtensorModule::is_hotkey_registered_on_network(
            netuid1,
            &old_hotkey
        ));
        assert!(!SubtensorModule::is_hotkey_registered_on_network(
            netuid2,
            &old_hotkey
        ));

        // Check total stake transfer
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&new_hotkey)
                + SubtensorModule::get_total_stake_for_hotkey(&new_hotkey_2),
            total_hk_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&old_hotkey),
            TaoBalance::ZERO
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_stake_success --exact --nocapture
#[test]
fn test_swap_stake_success() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        remove_owner_registration_stake(netuid);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        let amount = 10_000;
        let shares = U64F64::from_num(10_000);

        // Initialize staking variables for old_hotkey
        TotalHotkeyAlpha::<Test>::insert(old_hotkey, netuid, AlphaBalance::from(amount));
        TotalHotkeyAlphaLastEpoch::<Test>::insert(
            old_hotkey,
            netuid,
            AlphaBalance::from(amount * 2),
        );
        TotalHotkeyShares::<Test>::insert(old_hotkey, netuid, U64F64::from_num(shares));
        Alpha::<Test>::insert((old_hotkey, coldkey, netuid), U64F64::from_num(amount));
        AlphaDividendsPerSubnet::<Test>::insert(netuid, old_hotkey, AlphaBalance::from(amount));

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Verify the swap
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid),
            AlphaBalance::from(amount)
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(old_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(new_hotkey, netuid),
            AlphaBalance::from(amount * 2)
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(old_hotkey, netuid),
            U64F64::from_num(0)
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(new_hotkey, netuid),
            U64F64::from_num(0)
        );
        assert_abs_diff_eq!(
            f64::from(TotalHotkeySharesV2::<Test>::get(new_hotkey, netuid)),
            shares.to_num::<f64>(),
            epsilon = 0.0000000001
        );
        assert_eq!(
            Alpha::<Test>::get((old_hotkey, coldkey, netuid)),
            U64F64::from_num(0)
        );
        assert_eq!(
            Alpha::<Test>::get((new_hotkey, coldkey, netuid)),
            U64F64::from_num(0)
        );
        assert_eq!(
            f64::from(AlphaV2::<Test>::get((new_hotkey, coldkey, netuid))),
            amount as f64
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, old_hotkey),
            AlphaBalance::ZERO
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, new_hotkey),
            AlphaBalance::from(amount)
        );
    });
}

#[test]
fn test_swap_stake_v2_success() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        let amount = 10_000;
        let shares = U64F64::from_num(10_000);

        // Initialize staking variables for old_hotkey
        TotalHotkeyAlpha::<Test>::insert(old_hotkey, netuid, AlphaBalance::from(amount));
        TotalHotkeyAlphaLastEpoch::<Test>::insert(
            old_hotkey,
            netuid,
            AlphaBalance::from(amount * 2),
        );
        TotalHotkeySharesV2::<Test>::insert(old_hotkey, netuid, SafeFloat::from(shares));
        AlphaV2::<Test>::insert(
            (old_hotkey, coldkey, netuid),
            SafeFloat::from(U64F64::from_num(amount)),
        );
        AlphaDividendsPerSubnet::<Test>::insert(netuid, old_hotkey, AlphaBalance::from(amount));

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false,
        ),);

        // Verify the swap
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid),
            AlphaBalance::from(amount)
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(old_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(new_hotkey, netuid),
            AlphaBalance::from(amount * 2)
        );
        assert_eq!(
            f64::from(TotalHotkeySharesV2::<Test>::get(old_hotkey, netuid)),
            0_f64
        );
        assert_abs_diff_eq!(
            f64::from(TotalHotkeySharesV2::<Test>::get(new_hotkey, netuid)),
            shares.to_num::<f64>(),
            epsilon = 0.0000000001
        );
        assert_eq!(
            f64::from(AlphaV2::<Test>::get((old_hotkey, coldkey, netuid))),
            0_f64
        );
        assert_eq!(
            f64::from(AlphaV2::<Test>::get((new_hotkey, coldkey, netuid))),
            amount as f64
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, old_hotkey),
            AlphaBalance::ZERO
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, new_hotkey),
            AlphaBalance::from(amount)
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_hotkey_error_cases --exact --nocapture
#[test]
fn test_swap_hotkey_error_cases() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let wrong_coldkey = U256::from(4);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);

        // Set up initial state
        Owner::<Test>::insert(old_hotkey, coldkey);
        TotalNetworks::<Test>::put(1);
        SubtensorModule::set_last_tx_block(&coldkey, 0);

        // Test not enough balance
        let swap_cost = SubtensorModule::get_key_swap_cost();
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_err!(
            SubtensorModule::perform_hotkey_swap(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NotEnoughBalanceToPaySwapHotKey
        );

        let initial_balance = SubtensorModule::get_key_swap_cost() + 1000.into();
        add_balance_to_coldkey_account(&coldkey, initial_balance);

        // Test new hotkey same as old
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_noop!(
            SubtensorModule::perform_hotkey_swap(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &old_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NewHotKeyIsSameWithOld
        );

        // Test new hotkey already registered
        IsNetworkMember::<Test>::insert(new_hotkey, netuid, true);
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_noop!(
            SubtensorModule::perform_hotkey_swap(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::HotKeyAlreadyRegisteredInSubNet
        );
        IsNetworkMember::<Test>::remove(new_hotkey, netuid);

        // Test non-associated coldkey
        assert_noop!(
            SubtensorModule::perform_hotkey_swap(
                RuntimeOrigin::signed(wrong_coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NonAssociatedColdKey
        );

        // Run the successful swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);
    });
}

// Check swap hotkey with keep_stake doesn't affect stake and related storage maps
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_hotkey_swap_keep_stake --exact --nocapture
#[test]
fn test_hotkey_swap_keep_stake() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let child_key = U256::from(4);
        let coldkey = U256::from(3);
        let swap_cost = 1_000_000_000u64 * 2;
        let stake_amount = 1_000_000_000u64;
        let voting_power_value = 5_000_000_000_000_u64;

        // Setup
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, old_hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, swap_cost.into());

        VotingPower::<Test>::insert(netuid, old_hotkey, voting_power_value);
        assert_eq!(
            SubtensorModule::get_voting_power(netuid, &old_hotkey),
            voting_power_value
        );

        ChildKeys::<Test>::insert(old_hotkey, netuid, vec![(u64::MAX, child_key)]);
        ParentKeys::<Test>::insert(child_key, netuid, vec![(u64::MAX, old_hotkey)]);

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &coldkey,
            netuid,
            stake_amount.into(),
        );

        assert!(SubtensorModule::is_hotkey_registered_on_network(
            netuid,
            &old_hotkey
        ));

        step_block(20);

        let old_hotkey_stake_before_swap =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &coldkey,
                netuid,
            );

        assert_ok!(SubtensorModule::perform_hotkey_swap(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            true
        ));

        let old_hotkey_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &coldkey,
            netuid,
        );
        assert_eq!(
            old_hotkey_stake_after, old_hotkey_stake_before_swap,
            "old_hotkey stake must NOT change during keep_stake swap"
        );

        let new_hotkey_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &new_hotkey,
            &coldkey,
            netuid,
        );
        assert_eq!(
            new_hotkey_stake_after,
            0.into(),
            "new_hotkey should have no stake"
        );

        assert!(
            SubtensorModule::is_hotkey_registered_on_network(netuid, &new_hotkey),
            "new_hotkey should be registered on netuid"
        );

        assert!(
            !SubtensorModule::is_hotkey_registered_on_network(netuid, &old_hotkey),
            "old_hotkey should NOT be registered on netuid after swap"
        );

        let root_total_alpha = TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid);
        let child_total_alpha = TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid);
        assert!(
            root_total_alpha > 0.into(),
            "old_hotkey should retain TotalHotkeyAlpha"
        );
        assert_eq!(
            child_total_alpha,
            0.into(),
            "new_hotkey should have zero TotalHotkeyAlpha"
        );

        let root_voting_power = VotingPower::<Test>::get(netuid, old_hotkey);
        let child_voting_power = VotingPower::<Test>::get(netuid, new_hotkey);
        assert!(
            root_voting_power > 0,
            "old_hotkey should retain VotingPower"
        );
        assert_eq!(
            child_voting_power, 0,
            "new_hotkey should have zero VotingPower"
        );

        let old_hotkey_children = ChildKeys::<Test>::get(old_hotkey, netuid);
        assert!(
            !old_hotkey_children.iter().any(|(_, c)| *c == child_key),
            "old_hotkey should NOT retain ChildKeys after swap"
        );
        let new_hotkey_children = ChildKeys::<Test>::get(new_hotkey, netuid);
        assert!(
            new_hotkey_children.iter().any(|(_, c)| *c == child_key),
            "new_hotkey should inherit ChildKeys from old_hotkey"
        );

        let child_key_parents = ParentKeys::<Test>::get(child_key, netuid);
        assert!(
            child_key_parents.iter().any(|(_, p)| *p == new_hotkey),
            "child_key should have new_hotkey as parent after swap"
        );
        assert!(
            !child_key_parents.iter().any(|(_, p)| *p == old_hotkey),
            "child_key should NOT have old_hotkey as parent after swap"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_with_existing_stake --exact --show-output
#[test]
fn test_swap_hotkey_with_existing_stake() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(4);
        let staker1 = U256::from(5);
        let staker2 = U256::from(6);
        let subnet_owner_coldkey = U256::from(1000);
        let subnet_owner_hotkey = U256::from(1001);
        let staked_tao_1 = 100_000_000;
        let staked_tao_2 = 200_000_000;
        let staked_tao_3 = 300_000_000;
        let staked_tao_4 = 500_000_000;

        // Set up initial state
        let netuid = add_dynamic_network(&subnet_owner_coldkey, &subnet_owner_hotkey);
        register_ok_neuron(netuid, old_hotkey, coldkey, 1234);
        register_ok_neuron(netuid, new_hotkey, coldkey, 1234);

        // Add balance to coldkeys
        add_balance_to_coldkey_account(&coldkey, 10_000_000_000_u64.into());
        add_balance_to_coldkey_account(&staker1, 10_000_000_000_u64.into());
        add_balance_to_coldkey_account(&staker2, 10_000_000_000_u64.into());

        // Stake with staker1 coldkey on old_hotkey
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(staker1),
            old_hotkey,
            netuid,
            staked_tao_1.into()
        ));

        // Stake with staker2 coldkey on old_hotkey
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(staker2),
            old_hotkey,
            netuid,
            staked_tao_2.into()
        ));

        // Stake with staker1 coldkey on new_hotkey
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(staker1),
            new_hotkey,
            netuid,
            staked_tao_3.into()
        ));

        // Stake with staker2 coldkey on new_hotkey
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(staker2),
            new_hotkey,
            netuid,
            staked_tao_4.into()
        ));

        // Emulate effect of emission into alpha pool - makes numerators and denominators not equal to alpha
        let emission = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_on_subnet(&old_hotkey, netuid, emission);
        SubtensorModule::increase_stake_for_hotkey_on_subnet(&new_hotkey, netuid, emission);

        // Hotkey new_hotkey gets deregistered, stake stays
        IsNetworkMember::<Test>::remove(new_hotkey, netuid);

        let hk1_stake_1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &staker1,
            netuid,
        );
        let hk2_stake_1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &new_hotkey,
            &staker1,
            netuid,
        );
        let hk1_stake_2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &staker2,
            netuid,
        );
        let hk2_stake_2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &new_hotkey,
            &staker2,
            netuid,
        );

        assert!(!hk1_stake_1.is_zero());
        assert!(!hk2_stake_1.is_zero());
        assert!(!hk1_stake_2.is_zero());
        assert!(!hk2_stake_2.is_zero());

        let total_hk1_stake = SubtensorModule::get_total_stake_for_hotkey(&old_hotkey);
        let total_hk2_stake = SubtensorModule::get_total_stake_for_hotkey(&new_hotkey);
        assert!(!total_hk1_stake.is_zero());
        assert!(!total_hk2_stake.is_zero());
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());

        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Check correctness of stake transfer
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &staker1,
                netuid
            ),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &staker2,
                netuid
            ),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &staker1,
                netuid
            ),
            hk2_stake_1 + hk1_stake_1
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &staker2,
                netuid
            ),
            hk2_stake_2 + hk1_stake_2
        );

        // Check total stake transfer
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&old_hotkey),
            0.into(),
            epsilon = 1.into()
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&new_hotkey),
            total_hk1_stake + total_hk2_stake,
            epsilon = 1.into()
        );
    });
}
