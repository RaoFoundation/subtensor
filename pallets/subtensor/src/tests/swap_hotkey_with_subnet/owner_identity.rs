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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_owner --exact --nocapture
#[test]
fn test_swap_owner() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        Owner::<Test>::insert(old_hotkey, coldkey);
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false,
        ));

        assert_eq!(Owner::<Test>::get(old_hotkey), coldkey);
        assert_eq!(Owner::<Test>::get(new_hotkey), coldkey);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_owned_hotkeys --exact --nocapture
#[test]
fn test_swap_owned_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        OwnedHotkeys::<Test>::insert(coldkey, vec![old_hotkey]);
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        let hotkeys = OwnedHotkeys::<Test>::get(coldkey);
        assert!(hotkeys.contains(&old_hotkey));
        assert!(hotkeys.contains(&new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_delegates --exact --nocapture
#[test]
fn test_swap_delegates() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        Delegates::<Test>::insert(old_hotkey, PerU16::from_parts(100));
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert!(Delegates::<Test>::contains_key(old_hotkey));
        assert!(!Delegates::<Test>::contains_key(new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_do_swap_hotkey_err_not_owner --exact --nocapture
#[test]
fn test_do_swap_hotkey_err_not_owner() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let not_owner_coldkey = U256::from(4);
        let swap_cost = TaoBalance::from(1_000_000_000u64);

        // Setup initial state
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, old_hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&not_owner_coldkey, swap_cost);

        // Attempt the swap with a non-owner coldkey
        assert_err!(
            SubtensorModule::perform_hotkey_swap(
                RuntimeOrigin::signed(not_owner_coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_owner_old_hotkey_not_exist --exact --nocapture
#[test]
fn test_swap_owner_old_hotkey_not_exist() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&new_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Ensure old_hotkey does not exist
        assert!(!Owner::<Test>::contains_key(old_hotkey));

        // Perform the swap
        assert_err!(
            SubtensorModule::perform_hotkey_swap(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NonAssociatedColdKey
        );

        // Verify the swap
        assert_eq!(Owner::<Test>::get(new_hotkey), coldkey);
        assert!(!Owner::<Test>::contains_key(old_hotkey));
    });
}

// SKIP_WASM_BUILD=1 cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_owner_new_hotkey_owned_by_another_coldkey --exact --nocapture
#[test]
fn test_swap_owner_new_hotkey_owned_by_another_coldkey() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let another_coldkey = U256::from(4);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // new_hotkey already exists globally and is owned by a foreign coldkey,
        // so the new-hotkey ownership check must reject the swap.
        Owner::<Test>::insert(new_hotkey, another_coldkey);

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_err!(
            SubtensorModule::perform_hotkey_swap(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NonAssociatedColdKey
        );

        // Verify the swap
        assert_eq!(Owner::<Test>::get(old_hotkey), coldkey);
        assert!(Owner::<Test>::contains_key(old_hotkey));
    });
}

// SKIP_WASM_BUILD=1 cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_owner_new_hotkey_already_exists --exact --nocapture
#[test]
fn test_swap_owner_new_hotkey_already_exists() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&new_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // old_hotkey is owned by coldkey; new_hotkey was already registered on `netuid`
        // by add_dynamic_network (the condition under test). Do NOT reassign new_hotkey to
        // a foreign coldkey — the new_hotkey-ownership check (NonAssociatedColdKey) would
        // then fire before the already-registered-in-subnet check this test targets.
        Owner::<Test>::insert(old_hotkey, coldkey);

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_err!(
            SubtensorModule::perform_hotkey_swap(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::HotKeyAlreadyRegisteredInSubNet
        );

        // Verify the swap
        assert_eq!(Owner::<Test>::get(old_hotkey), coldkey);
        assert!(Owner::<Test>::contains_key(old_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_hotkey_is_sn_owner_hotkey --exact --nocapture
#[test]
fn test_swap_hotkey_is_sn_owner_hotkey() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        // Create dynamic network
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Check for SubnetOwnerHotkey
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), old_hotkey);

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Check for SubnetOwnerHotkey
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), new_hotkey);
    });
}
