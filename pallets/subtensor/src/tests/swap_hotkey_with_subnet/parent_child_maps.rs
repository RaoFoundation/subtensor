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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_child_keys --exact --nocapture
#[test]
fn test_swap_child_keys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        let children = vec![(100u64, U256::from(4)), (200u64, U256::from(5))];

        // Initialize ChildKeys for old_hotkey
        ChildKeys::<Test>::insert(old_hotkey, netuid, children.clone());

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Verify the swap
        assert_eq!(ChildKeys::<Test>::get(new_hotkey, netuid), children);
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid).is_empty());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_child_keys_self_loop --exact --show-output
#[test]
#[allow(deprecated)]
fn test_swap_child_keys_self_loop() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        let amount = AlphaBalance::from(12345);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Only for checking
        TotalHotkeyAlpha::<Test>::insert(old_hotkey, netuid, AlphaBalance::from(amount));

        let children = vec![(200u64, new_hotkey)];

        // Initialize ChildKeys for old_hotkey
        ChildKeys::<Test>::insert(old_hotkey, netuid, children.clone());

        // Perform the swap extrinsic
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_err!(
            SubtensorModule::swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                old_hotkey,
                new_hotkey,
                Some(netuid),
            ),
            Error::<Test>::InvalidChild
        );

        // Verify the swap didn't happen
        assert_eq!(ChildKeys::<Test>::get(old_hotkey, netuid), children);
        assert!(ChildKeys::<Test>::get(new_hotkey, netuid).is_empty());
        assert_eq!(TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid), amount);
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid),
            AlphaBalance::from(0)
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_parent_keys --exact --nocapture
#[test]
fn test_swap_parent_keys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        let parents = vec![(100u64, U256::from(4)), (200u64, U256::from(5))];

        // Initialize ParentKeys for old_hotkey
        ParentKeys::<Test>::insert(old_hotkey, netuid, parents.clone());

        // Initialize ChildKeys for parent
        ChildKeys::<Test>::insert(U256::from(4), netuid, vec![(100u64, old_hotkey)]);
        ChildKeys::<Test>::insert(U256::from(5), netuid, vec![(200u64, old_hotkey)]);

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Verify ParentKeys swap
        assert_eq!(ParentKeys::<Test>::get(new_hotkey, netuid), parents);
        assert!(ParentKeys::<Test>::get(old_hotkey, netuid).is_empty());

        // Verify ChildKeys update for parents
        assert_eq!(
            ChildKeys::<Test>::get(U256::from(4), netuid),
            vec![(100u64, new_hotkey)]
        );
        assert_eq!(
            ChildKeys::<Test>::get(U256::from(5), netuid),
            vec![(200u64, new_hotkey)]
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_multiple_subnets --exact --nocapture
#[test]
fn test_swap_multiple_subnets() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let new_hotkey_2 = U256::from(3);
        let coldkey = U256::from(4);
        let netuid1 = add_dynamic_network(&old_hotkey, &coldkey);
        let netuid2 = add_dynamic_network(&old_hotkey, &coldkey);

        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        let children1 = vec![(100u64, U256::from(4)), (200u64, U256::from(5))];
        let children2 = vec![(300u64, U256::from(6))];

        // Initialize ChildKeys for old_hotkey in multiple subnets
        ChildKeys::<Test>::insert(old_hotkey, netuid1, children1.clone());
        ChildKeys::<Test>::insert(old_hotkey, netuid2, children2.clone());

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid1),
            false
        ),);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey_2,
            Some(netuid2),
            false
        ),);

        // Verify the swap for both subnets
        assert_eq!(ChildKeys::<Test>::get(new_hotkey, netuid1), children1);
        assert_eq!(ChildKeys::<Test>::get(new_hotkey_2, netuid2), children2);
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid1).is_empty());
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid2).is_empty());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_complex_parent_child_structure --exact --nocapture
#[test]
fn test_swap_complex_parent_child_structure() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        let parent1 = U256::from(4);
        let parent2 = U256::from(5);
        let child1 = U256::from(6);
        let child2 = U256::from(7);

        // Set up complex parent-child structure
        ParentKeys::<Test>::insert(
            old_hotkey,
            netuid,
            vec![(100u64, parent1), (200u64, parent2)],
        );
        ChildKeys::<Test>::insert(old_hotkey, netuid, vec![(300u64, child1), (400u64, child2)]);
        ChildKeys::<Test>::insert(
            parent1,
            netuid,
            vec![(100u64, old_hotkey), (500u64, U256::from(8))],
        );
        ChildKeys::<Test>::insert(
            parent2,
            netuid,
            vec![(200u64, old_hotkey), (600u64, U256::from(9))],
        );

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Verify ParentKeys swap
        assert_eq!(
            ParentKeys::<Test>::get(new_hotkey, netuid),
            vec![(100u64, parent1), (200u64, parent2)]
        );
        assert!(ParentKeys::<Test>::get(old_hotkey, netuid).is_empty());

        // Verify ChildKeys swap
        assert_eq!(
            ChildKeys::<Test>::get(new_hotkey, netuid),
            vec![(300u64, child1), (400u64, child2)]
        );
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid).is_empty());

        // Verify parent's ChildKeys update
        assert!(ChildKeys::<Test>::get(parent1, netuid).contains(&(500u64, U256::from(8))),);
        assert!(ChildKeys::<Test>::get(parent1, netuid).contains(&(100u64, new_hotkey)),);
        assert!(ChildKeys::<Test>::get(parent2, netuid).contains(&(600u64, U256::from(9))),);
        assert!(ChildKeys::<Test>::get(parent2, netuid).contains(&(200u64, new_hotkey)),);
    });
}

#[test]
fn test_swap_parent_hotkey_childkey_maps() {
    new_test_ext(1).execute_with(|| {
        let parent_old = U256::from(1);
        let coldkey = U256::from(2);
        let child = U256::from(3);
        let child_other = U256::from(4);
        let parent_new = U256::from(5);

        let netuid = add_dynamic_network(&parent_old, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        SubtensorModule::create_account_if_non_existent(&coldkey, &parent_old);

        // Set child and verify state maps
        mock_set_children(&coldkey, &parent_old, netuid, &[(u64::MAX, child)]);
        // Wait rate limit
        step_rate_limit(&TransactionType::SetChildren, netuid);
        // Schedule some pending child keys.
        mock_schedule_children(&coldkey, &parent_old, netuid, &[(u64::MAX, child_other)]);

        assert_eq!(
            ParentKeys::<Test>::get(child, netuid),
            vec![(u64::MAX, parent_old)]
        );
        assert_eq!(
            ChildKeys::<Test>::get(parent_old, netuid),
            vec![(u64::MAX, child)]
        );
        let existing_pending_child_keys = PendingChildKeys::<Test>::get(netuid, parent_old);
        assert_eq!(existing_pending_child_keys.0, vec![(u64::MAX, child_other)]);

        // Swap

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &parent_old,
            &parent_new,
            Some(netuid),
            false
        ),);

        // Verify parent and child keys updates
        assert_eq!(
            ParentKeys::<Test>::get(child, netuid),
            vec![(u64::MAX, parent_new)]
        );
        assert_eq!(
            ChildKeys::<Test>::get(parent_new, netuid),
            vec![(u64::MAX, child)]
        );
        assert_eq!(
            PendingChildKeys::<Test>::get(netuid, parent_new),
            existing_pending_child_keys // Entry under new hotkey.
        );
    })
}

#[test]
fn test_swap_child_hotkey_childkey_maps() {
    new_test_ext(1).execute_with(|| {
        let parent = U256::from(1);
        let coldkey = U256::from(2);
        let child_old = U256::from(3);
        let child_new = U256::from(4);
        let netuid = add_dynamic_network(&child_old, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        SubtensorModule::create_account_if_non_existent(&coldkey, &child_old);
        SubtensorModule::create_account_if_non_existent(&coldkey, &parent);

        // Set child and verify state maps
        mock_set_children(&coldkey, &parent, netuid, &[(u64::MAX, child_old)]);
        // Wait rate limit
        step_rate_limit(&TransactionType::SetChildren, netuid);
        // Schedule some pending child keys.
        mock_schedule_children(&coldkey, &parent, netuid, &[(u64::MAX, child_old)]);

        assert_eq!(
            ParentKeys::<Test>::get(child_old, netuid),
            vec![(u64::MAX, parent)]
        );
        assert_eq!(
            ChildKeys::<Test>::get(parent, netuid),
            vec![(u64::MAX, child_old)]
        );
        let existing_pending_child_keys = PendingChildKeys::<Test>::get(netuid, parent);
        assert_eq!(existing_pending_child_keys.0, vec![(u64::MAX, child_old)]);

        // Swap

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &child_old,
            &child_new,
            Some(netuid),
            false
        ),);

        // Verify parent and child keys updates
        assert_eq!(
            ParentKeys::<Test>::get(child_new, netuid),
            vec![(u64::MAX, parent)]
        );
        assert_eq!(
            ChildKeys::<Test>::get(parent, netuid),
            vec![(u64::MAX, child_new)]
        );
        assert_eq!(
            PendingChildKeys::<Test>::get(netuid, parent),
            (vec![(u64::MAX, child_new)], existing_pending_child_keys.1) // Same cooldown block.
        );
    })
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_auto_parent_delegation_transferred_on_root --exact --nocapture
#[test]
fn test_swap_hotkey_auto_parent_delegation_transferred_on_root() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let old_hotkey = U256::from(1004);
        let new_hotkey = U256::from(1005);

        let _ = add_dynamic_network(&old_hotkey, &owner_coldkey);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);
        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());

        // Opt out of auto parent delegation on the old hotkey.
        AutoParentDelegationEnabled::<Test>::insert(old_hotkey, false);
        assert!(AutoParentDelegationEnabled::<Test>::contains_key(
            old_hotkey
        ));
        assert!(!AutoParentDelegationEnabled::<Test>::get(old_hotkey));

        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(NetUid::ROOT),
            false
        ));

        // Flag is moved to the new hotkey, cleared from the old one.
        assert!(!AutoParentDelegationEnabled::<Test>::contains_key(
            old_hotkey
        ));
        assert!(AutoParentDelegationEnabled::<Test>::contains_key(
            new_hotkey
        ));
        assert!(!AutoParentDelegationEnabled::<Test>::get(new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_auto_parent_delegation_transferred_on_all_subnets --exact --nocapture
#[test]
fn test_swap_hotkey_auto_parent_delegation_transferred_on_all_subnets() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let old_hotkey = U256::from(1004);
        let new_hotkey = U256::from(1005);

        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        let _ = add_dynamic_network(&old_hotkey, &owner_coldkey);
        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());

        AutoParentDelegationEnabled::<Test>::insert(old_hotkey, false);

        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &old_hotkey,
            &new_hotkey,
            None,
            false
        ));

        assert!(!AutoParentDelegationEnabled::<Test>::contains_key(
            old_hotkey
        ));
        assert!(AutoParentDelegationEnabled::<Test>::contains_key(
            new_hotkey
        ));
        assert!(!AutoParentDelegationEnabled::<Test>::get(new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_auto_parent_delegation_not_transferred_on_non_root --exact --nocapture
#[test]
fn test_swap_hotkey_auto_parent_delegation_not_transferred_on_non_root() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let old_hotkey = U256::from(1004);
        let new_hotkey = U256::from(1005);

        let netuid = add_dynamic_network(&old_hotkey, &owner_coldkey);
        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());

        AutoParentDelegationEnabled::<Test>::insert(old_hotkey, false);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Non-root subnet swap must not move the flag.
        assert!(AutoParentDelegationEnabled::<Test>::contains_key(
            old_hotkey
        ));
        assert!(!AutoParentDelegationEnabled::<Test>::get(old_hotkey));
        assert!(!AutoParentDelegationEnabled::<Test>::contains_key(
            new_hotkey
        ));
    });
}
