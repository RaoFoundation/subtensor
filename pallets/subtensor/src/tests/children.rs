#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
use super::mock;
use super::mock::*;
use approx::assert_abs_diff_eq;
use frame_support::{assert_err, assert_noop, assert_ok};
use substrate_fixed::types::{I64F64, I96F32};
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance};
use subtensor_swap_interface::SwapHandler;

use crate::{utils::rate_limiting::TransactionType, *};
use sp_core::U256;

fn close(value: u64, target: u64, eps: u64, msg: &str) {
    assert!(
        (value as i64 - target as i64).abs() <= eps as i64,
        "{msg}: value = {value}, target = {target}, eps = {eps}"
    )
}

// 1: Successful setting of a single child
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_singular_success --exact --show-output --nocapture
#[test]
fn test_do_set_child_singular_success() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set child
        mock_set_children(&coldkey, &hotkey, netuid, &[(proportion, child)]);

        // Verify child assignment
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(proportion, child)]);
    });
}

// 2: Attempt to set child in non-existent network
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_singular_network_does_not_exist --exact --show-output --nocapture
#[test]
fn test_do_set_child_singular_network_does_not_exist() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(999); // Non-existent network
        let proportion: u64 = 1000;

        // Attempt to set child
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(proportion, child)]
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

// 3: Attempt to set invalid child (same as hotkey)
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_singular_invalid_child --exact --show-output --nocapture
#[test]
fn test_do_set_child_singular_invalid_child() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Attempt to set child as the same hotkey
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![
                    (proportion, hotkey) // Invalid child
                ]
            ),
            Error::<Test>::InvalidChild
        );
    });
}

// 4: Attempt to set child with non-associated coldkey
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_singular_non_associated_coldkey --exact --show-output --nocapture
#[test]
fn test_do_set_child_singular_non_associated_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey with a different coldkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, U256::from(999), 0);

        // Attempt to set child
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(proportion, child)]
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

// 5: Attempt to set child in root network
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_singular_root_network --exact --show-output --nocapture
#[test]
fn test_do_set_child_singular_root_network() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::ROOT; // Root network
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);

        // Attempt to set child
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(proportion, child)]
            ),
            Error::<Test>::RegistrationNotPermittedOnRootSubnet
        );
    });
}

// 6: Cleanup of old children when setting new ones
// This test verifies that when new children are set, the old ones are properly removed.
// It checks:
// - Setting an initial child
// - Replacing it with a new child
// - Ensuring the old child is no longer associated
// - Confirming the new child is correctly assigned
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_singular_old_children_cleanup --exact --show-output --nocapture
#[test]
fn test_do_set_child_singular_old_children_cleanup() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let old_child = U256::from(3);
        let new_child = U256::from(4);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set old child
        mock_set_children(&coldkey, &hotkey, netuid, &[(proportion, old_child)]);

        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Set new child
        mock_set_children(&coldkey, &hotkey, netuid, &[(proportion, new_child)]);

        // Verify old child is removed
        let old_child_parents = SubtensorModule::get_parents(&old_child, netuid);
        assert!(old_child_parents.is_empty());

        // Verify new child assignment
        let new_child_parents = SubtensorModule::get_parents(&new_child, netuid);
        assert_eq!(new_child_parents, vec![(proportion, hotkey)]);
    });
}

// 7: Verify new children assignment
// This test checks if new children are correctly assigned to a parent.
// It verifies:
// - Setting a child for a parent
// - Confirming the child is correctly listed under the parent
// - Ensuring the parent is correctly listed for the child
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_singular_new_children_assignment --exact --show-output --nocapture
#[test]
fn test_do_set_child_singular_new_children_assignment() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set child
        mock_set_children(&coldkey, &hotkey, netuid, &[(proportion, child)]);

        // Verify child assignment
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(proportion, child)]);

        // Verify parent assignment
        let parents = SubtensorModule::get_parents(&child, netuid);
        assert_eq!(parents, vec![(proportion, hotkey)]);
    });
}

// 8: Test edge cases for proportion values
// This test verifies that the system correctly handles minimum and maximum proportion values.
// It checks:
// - Setting a child with the minimum possible proportion (0)
// - Setting a child with the maximum possible proportion (u64::MAX)
// - Confirming both assignments are processed correctly
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_singular_proportion_edge_cases --exact --show-output --nocapture
#[test]
fn test_do_set_child_singular_proportion_edge_cases() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set child with minimum proportion
        let min_proportion: u64 = 0;
        mock_set_children(&coldkey, &hotkey, netuid, &[(min_proportion, child)]);

        // Verify child assignment with minimum proportion
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(min_proportion, child)]);

        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Set child with maximum proportion
        let max_proportion: u64 = u64::MAX;
        mock_set_children(&coldkey, &hotkey, netuid, &[(max_proportion, child)]);

        // Verify child assignment with maximum proportion
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(max_proportion, child)]);
    });
}

// 9: Test setting multiple children
// This test verifies that when multiple children are set, only the last one remains.
// It checks:
// - Setting an initial child
// - Setting a second child
// - Confirming only the second child remains associated
// - Verifying the first child is no longer associated
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_singular_multiple_children --exact --show-output --nocapture
#[test]
fn test_do_set_child_singular_multiple_children() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(1);
        let proportion1: u64 = 500;
        let proportion2: u64 = 500;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set first child
        mock_set_children(&coldkey, &hotkey, netuid, &[(proportion1, child1)]);

        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Set second child
        mock_set_children(&coldkey, &hotkey, netuid, &[(proportion1, child2)]);

        // Verify children assignment
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(proportion2, child2)]);

        // Verify parent assignment for both children
        let parents1 = SubtensorModule::get_parents(&child1, netuid);
        assert!(parents1.is_empty()); // Old child should be removed

        let parents2 = SubtensorModule::get_parents(&child2, netuid);
        assert_eq!(parents2, vec![(proportion2, hotkey)]);
    });
}

// 10: Test adding a singular child with various error conditions
// This test checks different scenarios when adding a child, including:
// - Attempting to set a child in a non-existent network
// - Trying to set a child with an unassociated coldkey
// - Setting an invalid child
// - Successfully setting a valid child
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_add_singular_child --exact --show-output --nocapture
#[test]
fn test_add_singular_child() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let child = U256::from(1);
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        assert_eq!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(u64::MAX, child)]
            ),
            Err(Error::<Test>::SubnetNotExists.into())
        );
        add_network(netuid, 1, 0);
        step_rate_limit(&TransactionType::SetChildren, netuid);
        assert_eq!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(u64::MAX, child)]
            ),
            Err(Error::<Test>::NonAssociatedColdKey.into())
        );
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        step_rate_limit(&TransactionType::SetChildren, netuid);
        assert_eq!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(u64::MAX, child)]
            ),
            Err(Error::<Test>::InvalidChild.into())
        );
        let child = U256::from(3);
        step_rate_limit(&TransactionType::SetChildren, netuid);

        mock_set_children(&coldkey, &hotkey, netuid, &[(u64::MAX, child)]);
    })
}

// 11: Test getting stake for a hotkey on a subnet
// This test verifies the correct calculation of stake for a parent and child neuron:
// - Sets up a network with a parent and child neuron
// - Stakes tokens to both parent and child from different coldkeys
// - Establishes a parent-child relationship with 100% stake allocation
// - Checks that the parent's stake is correctly transferred to the child
// - Ensures the total stake is preserved in the system
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_get_stake_for_hotkey_on_subnet --exact --show-output --nocapture
#[test]
fn test_get_stake_for_hotkey_on_subnet() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let parent = U256::from(1);
        let child = U256::from(2);
        let coldkey1 = U256::from(3);
        let coldkey2 = U256::from(4);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, parent, coldkey1, 0);
        register_ok_neuron(netuid, child, coldkey2, 0);
        // Set parent-child relationship with 100% stake allocation
        mock_set_children(&coldkey1, &parent, netuid, &[(u64::MAX, child)]);
        // Stake 1000 to parent from coldkey1
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey1,
            netuid,
            1000.into(),
        );
        // Stake 1000 to parent from coldkey2
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey2,
            netuid,
            1000.into(),
        );
        // Stake 1000 to child from coldkey1
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &child,
            &coldkey1,
            netuid,
            1000.into(),
        );
        // Stake 1000 to child from coldkey2
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &child,
            &coldkey2,
            netuid,
            1000.into(),
        );
        let parent_stake = SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent, netuid);
        let child_stake = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child, netuid);
        // The parent should have 0 stake as it's all allocated to the child
        assert_eq!(parent_stake, 0.into());
        // The child should have its original stake (2000) plus the parent's stake (2000)
        assert_eq!(child_stake, 4000.into());

        // Ensure total stake is preserved
        assert_eq!(parent_stake + child_stake, 4000.into());
    });
}

// 12: Test revoking a singular child successfully
// This test checks the process of revoking a child neuron:
// - Sets up a network with a parent and child neuron
// - Establishes a parent-child relationship
// - Revokes the child relationship
// - Verifies that the child is removed from the parent's children list
// - Ensures the parent is removed from the child's parents list
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_revoke_child_singular_success --exact --show-output --nocapture
#[test]
fn test_do_revoke_child_singular_success() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;
        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Set child
        mock_set_children(&coldkey, &hotkey, netuid, &[(proportion, child)]);
        // Verify child assignment
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(proportion, child)]);
        step_rate_limit(&TransactionType::SetChildren, netuid);
        // Revoke child
        mock_set_children(&coldkey, &hotkey, netuid, &[]);
        // Verify child removal
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert!(children.is_empty());
        // Verify parent removal
        let parents = SubtensorModule::get_parents(&child, netuid);
        assert!(parents.is_empty());
    });
}

// 13: Test setting empty child vector on a non-existing subnet
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_empty_children_network_does_not_exist --exact --show-output --nocapture
#[test]
fn test_do_set_empty_children_network_does_not_exist() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(999); // Non-existent network
        // Attempt to revoke child
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![]
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

// 14: Test revoking a child with a non-associated coldkey
// This test ensures that attempting to revoke a child using an unassociated coldkey results in an error:
// - Sets up a network with a hotkey registered to a different coldkey
// - Attempts to revoke a child using an unassociated coldkey
// - Verifies that the appropriate error is returned
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_revoke_child_singular_non_associated_coldkey --exact --show-output --nocapture
#[test]
fn test_do_revoke_child_singular_non_associated_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);

        // Add network and register hotkey with a different coldkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, U256::from(999), 0);

        // Attempt to revoke child
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![]
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

// 15: Test revoking a non-associated child
// This test verifies that attempting to revoke a child that is not associated with the parent results in an error:
// - Sets up a network and registers a hotkey
// - Attempts to revoke a child that was never associated with the parent
// - Checks that the appropriate error is returned
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_revoke_child_singular_child_not_associated --exact --show-output --nocapture
#[test]
fn test_do_revoke_child_singular_child_not_associated() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        // Attempt to revoke child that is not associated
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(u64::MAX, child)]
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

// 16: Test setting multiple children successfully
// This test verifies that multiple children can be set for a parent successfully:
// - Sets up a network and registers a hotkey
// - Sets multiple children with different proportions
// - Verifies that the children are correctly assigned to the parent
// - Checks that the parent is correctly assigned to each child
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_schedule_children_multiple_success --exact --show-output --nocapture
#[test]
fn test_do_schedule_children_multiple_success() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(1);
        let proportion1: u64 = 1000;
        let proportion2: u64 = 2000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set multiple children
        mock_set_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(proportion1, child1), (proportion2, child2)],
        );

        // Verify children assignment
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(proportion1, child1), (proportion2, child2)]);

        // Verify parent assignment for both children
        let parents1 = SubtensorModule::get_parents(&child1, netuid);
        assert_eq!(parents1, vec![(proportion1, hotkey)]);

        let parents2 = SubtensorModule::get_parents(&child2, netuid);
        assert_eq!(parents2, vec![(proportion2, hotkey)]);
    });
}

// 17: Test setting multiple children in a non-existent network
// This test ensures that attempting to set multiple children in a non-existent network results in an error:
// - Attempts to set children in a network that doesn't exist
// - Verifies that the appropriate error is returned
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_schedule_children_multiple_network_does_not_exist --exact --show-output --nocapture
#[test]
fn test_do_schedule_children_multiple_network_does_not_exist() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let netuid = NetUid::from(999); // Non-existent network
        let proportion: u64 = 1000;

        // Attempt to set children
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(proportion, child1)]
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

// 18: Test setting multiple children with an invalid child
// This test verifies that attempting to set multiple children with an invalid child (same as parent) results in an error:
// - Sets up a network and registers a hotkey
// - Attempts to set a child that is the same as the parent hotkey
// - Checks that the appropriate error is returned
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_schedule_children_multiple_invalid_child --exact --show-output --nocapture
#[test]
fn test_do_schedule_children_multiple_invalid_child() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Attempt to set child as the same hotkey
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(proportion, hotkey)]
            ),
            Error::<Test>::InvalidChild
        );
    });
}

// 19: Test setting multiple children with a non-associated coldkey
// This test ensures that attempting to set multiple children using an unassociated coldkey results in an error:
// - Sets up a network with a hotkey registered to a different coldkey
// - Attempts to set children using an unassociated coldkey
// - Verifies that the appropriate error is returned
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_schedule_children_multiple_non_associated_coldkey --exact --show-output --nocapture
#[test]
fn test_do_schedule_children_multiple_non_associated_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey with a different coldkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, U256::from(999), 0);

        // Attempt to set children
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(proportion, child)]
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

// 20: Test setting multiple children in root network
// This test verifies that attempting to set children in the root network results in an error:
// - Sets up the root network
// - Attempts to set children in the root network
// - Checks that the appropriate error is returned
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_schedule_children_multiple_root_network --exact --show-output --nocapture
#[test]
fn test_do_schedule_children_multiple_root_network() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::ROOT; // Root network
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);

        // Attempt to set children
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(proportion, child)]
            ),
            Error::<Test>::RegistrationNotPermittedOnRootSubnet
        );
    });
}

// 21: Test cleanup of old children when setting multiple new ones
// This test ensures that when new children are set, the old ones are properly removed:
// - Sets up a network and registers a hotkey
// - Sets an initial child
// - Replaces it with multiple new children
// - Verifies that the old child is no longer associated
// - Confirms the new children are correctly assigned
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_schedule_children_multiple_old_children_cleanup --exact --show-output --nocapture
#[test]
fn test_do_schedule_children_multiple_old_children_cleanup() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let old_child = U256::from(3);
        let new_child1 = U256::from(4);
        let new_child2 = U256::from(5);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set old child
        mock_set_children(&coldkey, &hotkey, netuid, &[(proportion, old_child)]);

        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Set new children
        mock_set_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(proportion, new_child1), (proportion, new_child2)],
        );

        // Verify old child is removed
        let old_child_parents = SubtensorModule::get_parents(&old_child, netuid);
        assert!(old_child_parents.is_empty());

        // Verify new children assignment
        let new_child1_parents = SubtensorModule::get_parents(&new_child1, netuid);
        assert_eq!(new_child1_parents, vec![(proportion, hotkey)]);

        let new_child2_parents = SubtensorModule::get_parents(&new_child2, netuid);
        assert_eq!(new_child2_parents, vec![(proportion, hotkey)]);
    });
}

// 22: Test setting multiple children with edge case proportions
// This test verifies the behavior when setting multiple children with minimum and maximum proportions:
// - Sets up a network and registers a hotkey
// - Sets two children with minimum and maximum proportions respectively
// - Verifies that the children are correctly assigned with their respective proportions
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_schedule_children_multiple_proportion_edge_cases --exact --show-output --nocapture
#[test]
fn test_do_schedule_children_multiple_proportion_edge_cases() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(1);

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set children with minimum and maximum proportions
        let min_proportion: u64 = 0;
        let max_proportion: u64 = u64::MAX;
        mock_set_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(min_proportion, child1), (max_proportion, child2)],
        );

        // Verify children assignment
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(
            children,
            vec![(min_proportion, child1), (max_proportion, child2)]
        );
    });
}

// 23: Test overwriting existing children with new ones
// This test ensures that when new children are set, they correctly overwrite the existing ones:
// - Sets up a network and registers a hotkey
// - Sets initial children
// - Overwrites with new children
// - Verifies that the final children assignment is correct
// - Checks that old children are properly removed and new ones are correctly assigned
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_schedule_children_multiple_overwrite_existing --exact --show-output --nocapture
#[test]
fn test_do_schedule_children_multiple_overwrite_existing() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let child3 = U256::from(5);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set initial children
        mock_set_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(proportion, child1), (proportion, child2)],
        );

        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Overwrite with new children
        mock_set_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(proportion * 2, child2), (proportion * 3, child3)],
        );

        // Verify final children assignment
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(
            children,
            vec![(proportion * 2, child2), (proportion * 3, child3)]
        );

        // Verify parent assignment for all children
        let parents1 = SubtensorModule::get_parents(&child1, netuid);
        assert!(parents1.is_empty());

        let parents2 = SubtensorModule::get_parents(&child2, netuid);
        assert_eq!(parents2, vec![(proportion * 2, hotkey)]);

        let parents3 = SubtensorModule::get_parents(&child3, netuid);
        assert_eq!(parents3, vec![(proportion * 3, hotkey)]);
    });
}

// 24: Test childkey take functionality
// This test verifies the functionality of setting and getting childkey take:
// - Sets up a network and registers a hotkey
// - Checks default and maximum childkey take values
// - Sets a new childkey take value
// - Verifies the new take value is stored correctly
// - Attempts to set an invalid take value and checks for appropriate error
// - Tries to set take with a non-associated coldkey and verifies the error
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_childkey_take_functionality --exact --show-output --nocapture
#[test]
fn test_childkey_take_functionality() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Test default and max childkey take
        let default_take = SubtensorModule::get_default_childkey_take();
        let min_take = SubtensorModule::get_min_childkey_take();
        log::info!("Default take: {default_take}, Max take: {min_take}");

        // Check if default take and max take are the same
        assert_eq!(
            default_take, min_take,
            "Default take should be equal to max take"
        );

        // Log the actual value of MaxChildkeyTake
        log::info!(
            "MaxChildkeyTake value: {:?}",
            MaxChildkeyTake::<Test>::get()
        );

        // Test setting childkey take
        let new_take: u16 = SubtensorModule::get_max_childkey_take() / 2; // 50% of max_take
        assert_ok!(SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            new_take
        ));

        // Verify childkey take was set correctly
        let stored_take = SubtensorModule::get_childkey_take(&hotkey, netuid);
        log::info!("Stored take: {stored_take}");
        assert_eq!(stored_take, new_take);

        // Test setting childkey take outside of allowed range
        let invalid_take: u16 = SubtensorModule::get_max_childkey_take() + 1;
        assert_noop!(
            SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                invalid_take
            ),
            Error::<Test>::InvalidChildkeyTake
        );

        // Test setting childkey take with non-associated coldkey
        let non_associated_coldkey = U256::from(999);
        assert_noop!(
            SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(non_associated_coldkey),
                hotkey,
                netuid,
                new_take
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

#[test]
fn test_childkey_take_respects_effective_subnet_minimum() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);
        let subnet_min = SubtensorModule::get_max_childkey_take() / 2;

        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        SubtensorModule::set_min_childkey_take_for_subnet(netuid, subnet_min);

        assert_eq!(
            SubtensorModule::get_effective_min_childkey_take(netuid),
            subnet_min
        );
        assert_eq!(
            SubtensorModule::get_childkey_take(&hotkey, netuid),
            subnet_min
        );

        assert_noop!(
            SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                subnet_min - 1
            ),
            Error::<Test>::InvalidChildkeyTake
        );

        assert_ok!(SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            subnet_min
        ));

        ChildkeyTake::<Test>::insert(hotkey, netuid, subnet_min - 1);
        assert_eq!(
            SubtensorModule::get_childkey_take(&hotkey, netuid),
            subnet_min
        );
    });
}

// 25: Test childkey take rate limiting
// This test verifies the rate limiting functionality for setting childkey take:
// - Sets up a network and registers a hotkey
// - Sets a rate limit for childkey take changes
// - Performs multiple attempts to set childkey take
// - Verifies that rate limiting prevents frequent changes
// - Advances blocks to bypass rate limit and confirms successful change
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_childkey_take_rate_limiting --exact --show-output --nocapture
#[test]
fn test_childkey_take_rate_limiting() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set a rate limit for childkey take changes
        let rate_limit: u64 = 100;
        SubtensorModule::set_tx_childkey_take_rate_limit(rate_limit);

        log::info!(
            "Set TxChildkeyTakeRateLimit: {:?}",
            TxChildkeyTakeRateLimit::<Test>::get()
        );

        // Helper function to log rate limit information
        let log_rate_limit_info = || {
            let current_block = SubtensorModule::get_current_block_as_u64();
            let last_block = TransactionType::SetChildkeyTake.last_block_on_subnet::<Test>(
                &hotkey,
                netuid,
            );
            let passes = TransactionType::SetChildkeyTake.passes_rate_limit_on_subnet::<Test>(
                &hotkey,
                netuid,
            );
            let limit = TransactionType::SetChildkeyTake.rate_limit_on_subnet::<Test>(netuid);
            log::info!(
                "Rate limit info: current_block: {}, last_block: {}, limit: {}, passes: {}, diff: {}",
                current_block,
                last_block,
                limit,
                passes,
                current_block - last_block
            );
        };

        // First transaction (should succeed)
        log_rate_limit_info();
        assert_ok!(SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            500
        ));
        log_rate_limit_info();

        // Second transaction (should fail due to rate limit)
        log_rate_limit_info();
        assert_noop!(
            SubtensorModule::set_childkey_take(RuntimeOrigin::signed(coldkey), hotkey, netuid, 600),
            Error::<Test>::TxChildkeyTakeRateLimitExceeded
        );
        log_rate_limit_info();

        // Advance the block number to just before the rate limit
        run_to_block(rate_limit - 1);

        // Third transaction (should still fail)
        log_rate_limit_info();
        assert_noop!(
            SubtensorModule::set_childkey_take(RuntimeOrigin::signed(coldkey), hotkey, netuid, 650),
            Error::<Test>::TxChildkeyTakeRateLimitExceeded
        );
        log_rate_limit_info();

        // Advance the block number to just after the rate limit
        run_to_block(rate_limit + 1);

        // Fourth transaction (should succeed)
        log_rate_limit_info();
        assert_ok!(SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            700
        ));
        log_rate_limit_info();

        // Verify the final take was set
        let stored_take = SubtensorModule::get_childkey_take(&hotkey, netuid);
        assert_eq!(stored_take, 700);
    });
}

// 26: Test childkey take functionality across multiple networks
// This test verifies the childkey take functionality across multiple networks:
// - Creates multiple networks and sets up neurons
// - Sets unique childkey take values for each network
// - Verifies that each network has a different childkey take value
// - Attempts to set childkey take again (should fail due to rate limit)
// - Advances blocks to bypass rate limit and successfully updates take value
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_multiple_networks_childkey_take --exact --show-output --nocapture
#[test]
fn test_multiple_networks_childkey_take() {
    new_test_ext(1).execute_with(|| {
        const NUM_NETWORKS: u16 = 10;
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        // Create 10 networks and set up neurons (skip network 0)
        for netuid in 1..NUM_NETWORKS {
            let netuid = NetUid::from(netuid);
            // Add network
            add_network(netuid, 13, 0);

            // Register neuron
            register_ok_neuron(netuid, hotkey, coldkey, 0);

            // Set a unique childkey take value for each network
            let take_value = u16::from(netuid.next()) * 100; // Values will be 200, 300, ..., 1000
            assert_ok!(SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                take_value
            ));

            // Verify the childkey take was set correctly
            let stored_take = SubtensorModule::get_childkey_take(&hotkey, netuid);
            assert_eq!(
                stored_take, take_value,
                "Childkey take not set correctly for network {netuid}"
            );

            // Log the set value
            log::info!("Network {netuid}: Childkey take set to {take_value}");
        }

        // Verify all networks have different childkey take values
        for i in 1..NUM_NETWORKS {
            for j in (i + 1)..NUM_NETWORKS {
                let take_i = SubtensorModule::get_childkey_take(&hotkey, i.into());
                let take_j = SubtensorModule::get_childkey_take(&hotkey, j.into());
                assert_ne!(
                    take_i, take_j,
                    "Childkey take values should be different for networks {i} and {j}"
                );
            }
        }

        // Attempt to set childkey take again (should fail due to rate limit)
        let result = SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            1.into(),
            1100,
        );
        assert_noop!(result, Error::<Test>::TxChildkeyTakeRateLimitExceeded);

        // Advance blocks to bypass rate limit
        run_to_block(SubtensorModule::get_tx_childkey_take_rate_limit() + 1);

        // Now setting childkey take should succeed
        assert_ok!(SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            1.into(),
            1100
        ));

        // Verify the new take value
        let new_take = SubtensorModule::get_childkey_take(&hotkey, 1.into());
        assert_eq!(new_take, 1100, "Childkey take not updated after rate limit");
    });
}

// 27: Test setting children with an empty list
// This test verifies the behavior of setting an empty children list:
// - Adds a network and registers a hotkey
// - Sets an empty children list for the hotkey
// - Verifies that the children assignment is empty
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_schedule_children_multiple_empty_list --exact --show-output --nocapture
#[test]
fn test_do_schedule_children_multiple_empty_list() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set empty children list
        mock_set_children(&coldkey, &hotkey, netuid, &[]);

        // Verify children assignment is empty
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert!(children.is_empty());
    });
}

// 28: Test revoking multiple children successfully
// This test verifies the successful revocation of multiple children:
// - Adds a network and registers a hotkey
// - Sets multiple children for the hotkey
// - Revokes all children by setting an empty list
// - Verifies that the children list is empty
// - Verifies that the parent-child relationships are removed for both children
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_revoke_children_multiple_success --exact --show-output --nocapture
#[test]
fn test_do_revoke_children_multiple_success() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(1);
        let proportion1: u64 = 1000;
        let proportion2: u64 = 2000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set multiple children
        mock_set_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(proportion1, child1), (proportion2, child2)],
        );

        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Revoke multiple children
        mock_set_children(&coldkey, &hotkey, netuid, &[]);

        // Verify children removal
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert!(children.is_empty());

        // Verify parent removal for both children
        let parents1 = SubtensorModule::get_parents(&child1, netuid);
        assert!(parents1.is_empty());

        let parents2 = SubtensorModule::get_parents(&child2, netuid);
        assert!(parents2.is_empty());
    });
}

// 29: Test revoking children when network does not exist
// This test verifies the behavior when attempting to revoke children on a non-existent network:
// - Attempts to revoke children on a network that doesn't exist
// - Verifies that the operation fails with the correct error
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_revoke_children_multiple_network_does_not_exist --exact --show-output --nocapture
#[test]
fn test_do_revoke_children_multiple_network_does_not_exist() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(999); // Non-existent network
        // Attempt to revoke children
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(u64::MAX / 2, child1), (u64::MAX / 2, child2)]
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

// 30: Test revoking children with non-associated coldkey
// This test verifies the behavior when attempting to revoke children using a non-associated coldkey:
// - Adds a network and registers a hotkey with a different coldkey
// - Attempts to revoke children using an unassociated coldkey
// - Verifies that the operation fails with the correct error
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_revoke_children_multiple_non_associated_coldkey --exact --show-output --nocapture
#[test]
fn test_do_revoke_children_multiple_non_associated_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(1);

        // Add network and register hotkey with a different coldkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, U256::from(999), 0);

        // Attempt to revoke children
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(u64::MAX / 2, child1), (u64::MAX / 2, child2)]
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

// 31: Test partial revocation of children
// This test verifies the behavior when partially revoking children:
// - Adds a network and registers a hotkey
// - Sets multiple children for the hotkey
// - Revokes one of the children
// - Verifies that the correct children remain and the revoked child is removed
// - Checks the parent-child relationships after partial revocation
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_revoke_children_multiple_partial_revocation --exact --show-output --nocapture
#[test]
fn test_do_revoke_children_multiple_partial_revocation() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let child3 = U256::from(5);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set multiple children
        mock_set_children(
            &coldkey,
            &hotkey,
            netuid,
            &[
                (proportion, child1),
                (proportion, child2),
                (proportion, child3),
            ],
        );

        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Revoke only child3
        mock_set_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(proportion, child1), (proportion, child2)],
        );

        // Verify children removal
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(proportion, child1), (proportion, child2)]);

        // Verify parents.
        let parents1 = SubtensorModule::get_parents(&child3, netuid);
        assert!(parents1.is_empty());
        let parents1 = SubtensorModule::get_parents(&child1, netuid);
        assert_eq!(parents1, vec![(proportion, hotkey)]);
        let parents2 = SubtensorModule::get_parents(&child2, netuid);
        assert_eq!(parents2, vec![(proportion, hotkey)]);
    });
}

// 32: Test revoking non-existent children
// This test verifies the behavior when attempting to revoke non-existent children:
// - Adds a network and registers a hotkey
// - Sets one child for the hotkey
// - Attempts to revoke all children (including non-existent ones)
// - Verifies that all children are removed, including the existing one
// - Checks that the parent-child relationship is properly updated
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_revoke_children_multiple_non_existent_children --exact --show-output --nocapture
#[test]
fn test_do_revoke_children_multiple_non_existent_children() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set one child
        mock_set_children(&coldkey, &hotkey, netuid, &[(proportion, child1)]);

        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Attempt to revoke existing and non-existent children
        mock_set_children(&coldkey, &hotkey, netuid, &[]);

        // Verify all children are removed
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert!(children.is_empty());

        // Verify parent removal for the existing child
        let parents1 = SubtensorModule::get_parents(&child1, netuid);
        assert!(parents1.is_empty());
    });
}

// 33: Test revoking children with an empty list
// This test verifies the behavior when attempting to revoke children using an empty list:
// - Adds a network and registers a hotkey
// - Attempts to revoke children with an empty list
// - Verifies that no changes occur in the children list
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_revoke_children_multiple_empty_list --exact --show-output --nocapture
#[test]
fn test_do_revoke_children_multiple_empty_list() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Attempt to revoke with an empty list
        mock_set_children(&coldkey, &hotkey, netuid, &[]);

        // Verify no changes in children
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert!(children.is_empty());
    });
}

// 34: Test complex scenario for revoking multiple children
// This test verifies a complex scenario involving setting and revoking multiple children:
// - Adds a network and registers a hotkey
// - Sets multiple children with different proportions
// - Revokes one child and verifies the remaining children
// - Revokes all remaining children
// - Verifies that all parent-child relationships are properly updated
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_revoke_children_multiple_complex_scenario --exact --show-output --nocapture
#[test]
fn test_do_revoke_children_multiple_complex_scenario() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let child3 = U256::from(5);
        let netuid = NetUid::from(1);
        let proportion1: u64 = 1000;
        let proportion2: u64 = 2000;
        let proportion3: u64 = 3000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set multiple children
        mock_set_children(
            &coldkey,
            &hotkey,
            netuid,
            &[
                (proportion1, child1),
                (proportion2, child2),
                (proportion3, child3),
            ],
        );

        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Revoke child2
        mock_set_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(proportion1, child1), (proportion3, child3)],
        );

        // Verify remaining children
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(proportion1, child1), (proportion3, child3)]);

        // Verify parent removal for child2
        let parents2 = SubtensorModule::get_parents(&child2, netuid);
        assert!(parents2.is_empty());

        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Revoke remaining children
        mock_set_children(&coldkey, &hotkey, netuid, &[]);

        // Verify all children are removed
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert!(children.is_empty());

        // Verify parent removal for all children
        let parents1 = SubtensorModule::get_parents(&child1, netuid);
        assert!(parents1.is_empty());
        let parents3 = SubtensorModule::get_parents(&child3, netuid);
        assert!(parents3.is_empty());
    });
}

// 39: Test children stake values
// This test verifies the correct distribution of stake among parent and child neurons:
// - Sets up a network with a parent neuron and multiple child neurons
// - Assigns stake to the parent neuron
// - Sets child neurons with specific proportions
// - Verifies that the stake is correctly distributed among parent and child neurons
// - Checks that the total stake remains constant across all neurons
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_children_stake_values --exact --show-output --nocapture
#[test]
fn test_children_stake_values() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let child3 = U256::from(5);
        let proportion1: u64 = u64::MAX / 4;
        let proportion2: u64 = u64::MAX / 4;
        let proportion3: u64 = u64::MAX / 4;

        // Add network and register hotkey
        SubtensorModule::set_max_registrations_per_block(netuid, 4);
        SubtensorModule::set_target_registrations_per_interval(netuid, 4);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        register_ok_neuron(netuid, child1, coldkey, 0);
        register_ok_neuron(netuid, child2, coldkey, 0);
        register_ok_neuron(netuid, child3, coldkey, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            100_000_000_000_000_u64.into(),
        );

        // Set multiple children with proportions.
        mock_set_children_no_epochs(
            netuid,
            &hotkey,
            &[
                (proportion1, child1),
                (proportion2, child2),
                (proportion3, child3),
            ],
        );

        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&hotkey, netuid),
            25_000_000_069_849_u64.into()
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&child1, netuid),
            24_999_999_976_716_u64.into()
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&child2, netuid),
            24_999_999_976_716_u64.into()
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&child3, netuid),
            24_999_999_976_716_u64.into()
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&child3, netuid)
                + SubtensorModule::get_inherited_for_hotkey_on_subnet(&child2, netuid)
                + SubtensorModule::get_inherited_for_hotkey_on_subnet(&child1, netuid)
                + SubtensorModule::get_inherited_for_hotkey_on_subnet(&hotkey, netuid),
            99999999999997_u64.into()
        );
    });
}

// 40: Test getting parents chain
// This test verifies the correct implementation of parent-child relationships and the get_parents function:
// - Sets up a network with multiple neurons in a chain of parent-child relationships
// - Verifies that each neuron has the correct parent
// - Tests the root neuron has no parents
// - Tests a neuron with multiple parents
// - Verifies correct behavior when adding a new parent to an existing child
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_get_parents_chain --exact --show-output --nocapture
#[test]
fn test_get_parents_chain() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let coldkey = U256::from(1);
        let num_keys: usize = 5;
        let proportion = u64::MAX / 2; // 50% stake allocation

        log::info!(
            "Test setup: netuid={netuid}, coldkey={coldkey}, num_keys={num_keys}, proportion={proportion}"
        );

        // Create a vector of hotkeys
        let hotkeys: Vec<U256> = (0..num_keys).map(|i| U256::from(i as u64 + 2)).collect();
        log::info!("Created hotkeys: {hotkeys:?}");

        // Add network
        add_network(netuid, 13, 0);
        SubtensorModule::set_max_registrations_per_block(netuid, 1000);
        SubtensorModule::set_target_registrations_per_interval(netuid, 1000);
        log::info!("Network added and parameters set: netuid={netuid}");

        // Register all neurons
        for hotkey in &hotkeys {
            register_ok_neuron(netuid, *hotkey, coldkey, 0);
            log::info!(
                "Registered neuron: hotkey={hotkey}, coldkey={coldkey}, netuid={netuid}"
            );
        }

        // Set up parent-child relationships
        for i in 0..num_keys - 1 {
            mock_schedule_children(
                &coldkey,
                &hotkeys[i],
                netuid,
                &[(proportion, hotkeys[i + 1])],
            );
            log::info!(
                "Set parent-child relationship: parent={}, child={}, proportion={}",
                hotkeys[i],
                hotkeys[i + 1],
                proportion
            );
        }
        // Wait for children to be set
        wait_and_set_pending_children(netuid);

        // Test get_parents for each hotkey
        for i in 1..num_keys {
            let parents = SubtensorModule::get_parents(&hotkeys[i], netuid);
            log::info!(
                "Testing get_parents for hotkey {}: {:?}",
                hotkeys[i],
                parents
            );
            assert_eq!(
                parents.len(),
                1,
                "Hotkey {i} should have exactly one parent"
            );
            assert_eq!(
                parents[0],
                (proportion, hotkeys[i - 1]),
                "Incorrect parent for hotkey {i}"
            );
        }

        // Test get_parents for the root (should be empty)
        let root_parents = SubtensorModule::get_parents(&hotkeys[0], netuid);
        log::info!(
            "Testing get_parents for root hotkey {}: {:?}",
            hotkeys[0],
            root_parents
        );
        assert!(
            root_parents.is_empty(),
            "Root hotkey should have no parents"
        );

        // Test multiple parents
        let last_hotkey = hotkeys[num_keys - 1];
        let new_parent = U256::from(num_keys as u64 + 2);
        // Set reg diff back down (adjusted from last block steps)
        SubtensorModule::set_difficulty(netuid, 1);
        register_ok_neuron(netuid, new_parent, coldkey, 99 * 2);
        log::info!(
            "Registered new parent neuron: new_parent={new_parent}, coldkey={coldkey}, netuid={netuid}"
        );

        mock_set_children(
            &coldkey,
            &new_parent,
            netuid,
            &[(proportion / 2, last_hotkey)],
        );

        log::info!(
            "Set additional parent-child relationship: parent={}, child={}, proportion={}",
            new_parent,
            last_hotkey,
            proportion / 2
        );

        let last_hotkey_parents = SubtensorModule::get_parents(&last_hotkey, netuid);
        log::info!(
            "Testing get_parents for last hotkey {last_hotkey} with multiple parents: {last_hotkey_parents:?}"
        );
        assert_eq!(
            last_hotkey_parents.len(),
            2,
            "Last hotkey should have two parents"
        );
        assert!(
            last_hotkey_parents.contains(&(proportion, hotkeys[num_keys - 2])),
            "Last hotkey should still have its original parent"
        );
        assert!(
            last_hotkey_parents.contains(&(proportion / 2, new_parent)),
            "Last hotkey should have the new parent"
        );
    });
}

// 47: Test basic stake retrieval for a single hotkey on a subnet
/// This test verifies the basic functionality of retrieving stake for a single hotkey on a subnet:
/// - Sets up a network with one neuron
/// - Increases stake for the neuron
/// - Checks if the retrieved stake matches the increased amount
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_get_stake_for_hotkey_on_subnet_basic --exact --show-output --nocapture
#[test]
fn test_get_stake_for_hotkey_on_subnet_basic() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);

        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1000.into(),
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&hotkey, netuid),
            1000.into()
        );
    });
}

// 48: Test stake retrieval for a hotkey with multiple coldkeys on a subnet
/// This test verifies the functionality of retrieving stake for a hotkey with multiple coldkeys on a subnet:
/// - Sets up a network with one neuron and two coldkeys
/// - Increases stake from both coldkeys
/// - Checks if the retrieved stake matches the total increased amount
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_get_stake_for_hotkey_on_subnet_multiple_coldkeys --exact --show-output --nocapture
#[test]
fn test_get_stake_for_hotkey_on_subnet_multiple_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(1);
        let coldkey1 = U256::from(2);
        let coldkey2 = U256::from(3);

        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey, coldkey1, 0);

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey1,
            netuid,
            1000.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey2,
            netuid,
            2000.into(),
        );

        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&hotkey, netuid),
            3000.into()
        );
    });
}

// 49: Test stake retrieval for a single parent-child relationship on a subnet
/// This test verifies the functionality of retrieving stake for a single parent-child relationship on a subnet:
/// - Sets up a network with a parent and child neuron
/// - Increases stake for the parent
/// - Sets the child as the parent's only child with 100% stake allocation
/// - Checks if the retrieved stake for both parent and child is correct
///
/// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_get_stake_for_hotkey_on_subnet_single_parent_child --exact --show-output --nocapture
#[test]
fn test_get_stake_for_hotkey_on_subnet_single_parent_child() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let parent = U256::from(1);
        let child = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, parent, coldkey, 0);
        register_ok_neuron(netuid, child, coldkey, 0);

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            1_000_000_000.into(),
        );

        mock_set_children_no_epochs(netuid, &parent, &[(u64::MAX, child)]);

        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent, netuid),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&child, netuid),
            1_000_000_000.into()
        );
    });
}

// 50: Test stake retrieval for multiple parents and a single child on a subnet
/// This test verifies the functionality of retrieving stake for multiple parents and a single child on a subnet:
/// - Sets up a network with two parents and one child neuron
/// - Increases stake for both parents
/// - Sets the child as a 50% stake recipient for both parents
/// - Checks if the retrieved stake for parents and child is correct
///
/// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_get_stake_for_hotkey_on_subnet_multiple_parents_single_child --exact --show-output --nocapture
#[test]
fn test_get_stake_for_hotkey_on_subnet_multiple_parents_single_child() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let parent1 = U256::from(1);
        let parent2 = U256::from(2);
        let child = U256::from(3);
        let coldkey = U256::from(4);

        register_ok_neuron(netuid, parent1, coldkey, 0);
        register_ok_neuron(netuid, parent2, coldkey, 0);
        register_ok_neuron(netuid, child, coldkey, 0);

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent1,
            &coldkey,
            netuid,
            1000.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent2,
            &coldkey,
            netuid,
            2000.into(),
        );

        mock_set_children_no_epochs(netuid, &parent1, &[(u64::MAX / 2, child)]);
        mock_set_children_no_epochs(netuid, &parent2, &[(u64::MAX / 2, child)]);

        close(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent1, netuid).into(),
            500,
            10,
            "Incorrect inherited stake for parent1",
        );
        close(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent2, netuid).into(),
            1000,
            10,
            "Incorrect inherited stake for parent2",
        );
        close(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&child, netuid).into(),
            1499,
            10,
            "Incorrect inherited stake for child",
        );
    });
}

// 51: Test stake retrieval for a single parent with multiple children on a subnet
/// This test verifies the functionality of retrieving stake for a single parent with multiple children on a subnet:
/// - Sets up a network with one parent and two child neurons
/// - Increases stake for the parent
/// - Sets both children as 1/3 stake recipients of the parent
/// - Checks if the retrieved stake for parent and children is correct and preserves total stake
///
/// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_get_stake_for_hotkey_on_subnet_single_parent_multiple_children --exact --show-output --nocapture
#[test]
fn test_get_stake_for_hotkey_on_subnet_single_parent_multiple_children() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let parent = U256::from(1);
        let child1 = U256::from(2);
        let child2 = U256::from(3);
        let coldkey = U256::from(4);

        register_ok_neuron(netuid, parent, coldkey, 0);
        register_ok_neuron(netuid, child1, coldkey, 0);
        register_ok_neuron(netuid, child2, coldkey, 0);

        let total_stake = 3000.into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            total_stake,
        );

        mock_set_children_no_epochs(
            netuid,
            &parent,
            &[(u64::MAX / 3, child1), (u64::MAX / 3, child2)],
        );

        let parent_stake = SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent, netuid);
        let child1_stake = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child1, netuid);
        let child2_stake = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child2, netuid);

        // Check that the total stake is preserved
        close(
            (parent_stake + child1_stake + child2_stake).into(),
            total_stake.into(),
            10,
            "Total stake not preserved",
        );

        // Check that the parent stake is slightly higher due to rounding
        close(parent_stake.into(), 1000, 10, "Parent stake incorrect");

        // Check that each child gets an equal share of the remaining stake
        close(child1_stake.into(), 1000, 10, "Child1 stake incorrect");
        close(child2_stake.into(), 1000, 10, "Child2 stake incorrect");

        // Log the actual stake values
        log::info!("Parent stake: {parent_stake}");
        log::info!("Child1 stake: {child1_stake}");
        log::info!("Child2 stake: {child2_stake}");
    });
}

// 52: Test stake retrieval for edge cases on a subnet
/// This test verifies the functionality of retrieving stake for edge cases on a subnet:
/// - Sets up a network with one parent and two child neurons
/// - Increases stake to the network maximum
/// - Sets children with 0% and 100% stake allocation
/// - Checks if the retrieved stake for parent and children is correct and preserves total stake
///
/// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_get_stake_for_hotkey_on_subnet_edge_cases --exact --show-output --nocapture
#[test]
fn test_get_stake_for_hotkey_on_subnet_edge_cases() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let parent = U256::from(1);
        let child1 = U256::from(2);
        let child2 = U256::from(3);
        let coldkey = U256::from(4);

        register_ok_neuron(netuid, parent, coldkey, 0);
        register_ok_neuron(netuid, child1, coldkey, 0);
        register_ok_neuron(netuid, child2, coldkey, 0);

        // Set above old value of network max stake
        let network_max_stake = 600_000_000_000_000_u64.into();

        // Increase stake to the network max
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            network_max_stake,
        );

        // Test with 0% and 100% stake allocation
        mock_set_children_no_epochs(netuid, &parent, &[(0, child1), (u64::MAX, child2)]);

        let parent_stake = SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent, netuid);
        let child1_stake = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child1, netuid);
        let child2_stake = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child2, netuid);

        log::info!("Parent stake: {parent_stake}");
        log::info!("Child1 stake: {child1_stake}");
        log::info!("Child2 stake: {child2_stake}");

        assert_eq!(parent_stake, 0.into(), "Parent should have 0 stake");
        assert_eq!(child1_stake, 0.into(), "Child1 should have 0 stake");
        assert_eq!(
            child2_stake, network_max_stake,
            "Child2 should have all the stake"
        );

        // Check that the total stake is preserved and equal to the network max stake
        close(
            (parent_stake + child1_stake + child2_stake).into(),
            network_max_stake.into(),
            10,
            "Total stake should equal network max stake",
        );
    });
}

// 53: Test stake distribution in a complex hierarchy of parent-child relationships
// This test verifies the correct distribution of stake in a multi-level parent-child hierarchy:
// - Sets up a network with four neurons: parent, child1, child2, and grandchild
// - Establishes parent-child relationships between parent and its children, and child1 and grandchild
// - Adds initial stake to the parent
// - Checks stake distribution after setting up the first level of relationships
// - Checks stake distribution after setting up the second level of relationships
// - Verifies correct stake calculations, parent-child relationships, and preservation of total stake
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_get_stake_for_hotkey_on_subnet_complex_hierarchy --exact --show-output --nocapture
#[test]
fn test_get_stake_for_hotkey_on_subnet_complex_hierarchy() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let parent = U256::from(1);
        let child1 = U256::from(2);
        let child2 = U256::from(3);
        let grandchild = U256::from(4);
        let coldkey_parent = U256::from(5);
        let coldkey_child1 = U256::from(6);
        let coldkey_child2 = U256::from(7);
        let coldkey_grandchild = U256::from(8);

        SubtensorModule::set_max_registrations_per_block(netuid, 1000);
        SubtensorModule::set_target_registrations_per_interval(netuid, 1000);
        register_ok_neuron(netuid, parent, coldkey_parent, 0);
        register_ok_neuron(netuid, child1, coldkey_child1, 0);
        register_ok_neuron(netuid, child2, coldkey_child2, 0);
        register_ok_neuron(netuid, grandchild, coldkey_grandchild, 0);

        let total_stake = 1000.into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey_parent,
            netuid,
            total_stake,
        );

        log::info!("Initial stakes:");
        log::info!(
            "Parent stake: {}",
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent, netuid)
        );
        log::info!(
            "Child1 stake: {}",
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&child1, netuid)
        );
        log::info!(
            "Child2 stake: {}",
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&child2, netuid)
        );
        log::info!(
            "Grandchild stake: {}",
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&grandchild, netuid)
        );

        // Step 1: Set children for parent
        mock_set_children_no_epochs(
            netuid,
            &parent,
            &[(u64::MAX / 2, child1), (u64::MAX / 2, child2)],
        );

        log::info!("After setting parent's children:");
        log::info!(
            "Parent's children: {:?}",
            SubtensorModule::get_children(&parent, netuid)
        );
        log::info!(
            "Child1's parents: {:?}",
            SubtensorModule::get_parents(&child1, netuid)
        );
        log::info!(
            "Child2's parents: {:?}",
            SubtensorModule::get_parents(&child2, netuid)
        );

        let parent_stake_1 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent, netuid);
        let child1_stake_1 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child1, netuid);
        let child2_stake_1 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child2, netuid);

        log::info!("Parent stake: {parent_stake_1}");
        log::info!("Child1 stake: {child1_stake_1}");
        log::info!("Child2 stake: {child2_stake_1}");

        assert_eq!(
            parent_stake_1,
            0.into(),
            "Parent should have 0 stake after distributing all stake to children"
        );
        close(
            child1_stake_1.into(),
            499,
            10,
            "Child1 should have 499 stake",
        );
        close(
            child2_stake_1.into(),
            499,
            10,
            "Child2 should have 499 stake",
        );

        // Step 2: Set children for child1
        mock_set_children_no_epochs(netuid, &child1, &[(u64::MAX, grandchild)]);

        log::info!("After setting child1's children:");
        log::info!(
            "Child1's children: {:?}",
            SubtensorModule::get_children(&child1, netuid)
        );
        log::info!(
            "Grandchild's parents: {:?}",
            SubtensorModule::get_parents(&grandchild, netuid)
        );

        let parent_stake_2 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent, netuid);
        let child1_stake_2 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child1, netuid);
        let child2_stake_2 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child2, netuid);
        let grandchild_stake =
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&grandchild, netuid);

        log::info!("Parent stake: {parent_stake_2}");
        log::info!("Child1 stake: {child1_stake_2}");
        log::info!("Child2 stake: {child2_stake_2}");
        log::info!("Grandchild stake: {grandchild_stake}");

        close(parent_stake_2.into(), 0, 10, "Parent stake should remain 2");
        close(
            child1_stake_2.into(),
            499,
            10,
            "Child1 should still have 499 stake",
        );
        close(
            child2_stake_2.into(),
            499,
            10,
            "Child2 should still have 499 stake",
        );
        close(
            grandchild_stake.into(),
            0,
            10,
            "Grandchild should have 0 stake, as child1 doesn't have any owned stake",
        );

        // Check that the total stake is preserved
        close(
            (parent_stake_2 + child1_stake_2 + child2_stake_2 + grandchild_stake).into(),
            total_stake.into(),
            10,
            "Total stake should equal the initial stake",
        );

        // Additional checks
        log::info!("Final parent-child relationships:");
        log::info!(
            "Parent's children: {:?}",
            SubtensorModule::get_children(&parent, netuid)
        );
        log::info!(
            "Child1's parents: {:?}",
            SubtensorModule::get_parents(&child1, netuid)
        );
        log::info!(
            "Child2's parents: {:?}",
            SubtensorModule::get_parents(&child2, netuid)
        );
        log::info!(
            "Child1's children: {:?}",
            SubtensorModule::get_children(&child1, netuid)
        );
        log::info!(
            "Grandchild's parents: {:?}",
            SubtensorModule::get_parents(&grandchild, netuid)
        );

        // Check if the parent-child relationships are correct
        assert_eq!(
            SubtensorModule::get_children(&parent, netuid),
            vec![(u64::MAX / 2, child1), (u64::MAX / 2, child2)],
            "Parent should have both children"
        );
        assert_eq!(
            SubtensorModule::get_parents(&child1, netuid),
            vec![(u64::MAX / 2, parent)],
            "Child1 should have parent as its parent"
        );
        assert_eq!(
            SubtensorModule::get_parents(&child2, netuid),
            vec![(u64::MAX / 2, parent)],
            "Child2 should have parent as its parent"
        );
        assert_eq!(
            SubtensorModule::get_children(&child1, netuid),
            vec![(u64::MAX, grandchild)],
            "Child1 should have grandchild as its child"
        );
        assert_eq!(
            SubtensorModule::get_parents(&grandchild, netuid),
            vec![(u64::MAX, child1)],
            "Grandchild should have child1 as its parent"
        );
    });
}

// 54: Test stake distribution across multiple networks
// This test verifies the correct distribution of stake for a single neuron across multiple networks:
// - Sets up two networks with a single neuron registered on both
// - Adds initial stake to the neuron
// - Checks that the stake is correctly reflected on both networks
// - Verifies that changes in stake are consistently applied across all networks
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_get_stake_for_hotkey_on_subnet_multiple_networks --exact --show-output --nocapture
#[test]
fn test_get_stake_for_hotkey_on_subnet_multiple_networks() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);

        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);
        register_ok_neuron(netuid1, hotkey, coldkey, 0);
        register_ok_neuron(netuid2, hotkey, coldkey, 0);

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid1,
            1000.into(),
        );

        close(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&hotkey, netuid1).into(),
            1000,
            10,
            "Stake on network 1 incorrect",
        );
        close(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&hotkey, netuid2).into(),
            0,
            10,
            "Stake on network 2 incorrect",
        );
    });
}

// Test that min stake is enforced for setting children
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_below_min_stake --exact --show-output --nocapture
#[test]
fn test_do_set_child_below_min_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        StakeThreshold::<Test>::set(1_000_000_000_000);

        // Attempt to set child
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(proportion, child)]
            ),
            Error::<Test>::NotEnoughStakeToSetChildkeys
        );
    });
}

/// --- test_do_remove_stake_clears_pending_childkeys ---
///
/// Test Description: Ensures that removing stake clears any pending childkeys.
///
/// Expected Behavior:
/// - Pending childkeys should be cleared when stake is removed
/// - Cooldown block should be reset to 0
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_remove_stake_clears_pending_childkeys --exact --show-output --nocapture
#[test]
fn test_do_remove_stake_clears_pending_childkeys() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, 10_000_000_000_000_u64.into());
        SubtokenEnabled::<Test>::insert(netuid, true);

        let reserve = 1_000_000_000_000_000_u64;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Set non-default value for childkey stake threshold
        StakeThreshold::<Test>::set(1_000_000_000_000);

        assert_ok!(SubtensorModule::do_add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            (StakeThreshold::<Test>::get() * 2).into()
        ));

        let alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        println!(
            "StakeThreshold::<Test>::get() = {:?}",
            StakeThreshold::<Test>::get()
        );
        println!("alpha                         = {alpha:?}");

        // Attempt to set child
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            vec![(proportion, child)]
        ));

        // Check that pending child exists
        let pending_before = PendingChildKeys::<Test>::get(netuid, hotkey);
        assert!(!pending_before.0.is_empty());
        assert!(pending_before.1 > 0);

        // Remove stake
        assert_ok!(SubtensorModule::do_remove_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            alpha,
        ));

        // Assert that pending child is removed
        let pending_after = PendingChildKeys::<Test>::get(netuid, hotkey);
        close(
            pending_after.0.len() as u64,
            0,
            0,
            "Pending children vector should be empty",
        );
        close(pending_after.1, 0, 0, "Cooldown block should be zero");
    });
}

// Test that pending childkeys do not apply immediately and apply after cooldown period
//
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_cooldown_period --exact --show-output --nocapture
#[cfg(test)]
#[test]
fn test_do_set_child_cooldown_period() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let parent = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, parent, coldkey, 0);

        // Set minimum stake for setting children
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            StakeThreshold::<Test>::get().into(),
        );

        // Schedule parent-child relationship
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            parent,
            netuid,
            vec![(proportion, child)],
        ));

        // Ensure the childkeys are not yet applied
        let children_before = SubtensorModule::get_children(&parent, netuid);
        close(
            children_before.len() as u64,
            0,
            0,
            "Children vector should be empty before cooldown",
        );

        wait_and_set_pending_children(netuid);
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            StakeThreshold::<Test>::get().into(),
        );

        // Verify child assignment
        let children_after = SubtensorModule::get_children(&parent, netuid);
        close(
            children_after.len() as u64,
            1,
            0,
            "Children vector should have one entry after cooldown",
        );
        close(
            children_after[0].0,
            proportion,
            0,
            "Child proportion should match",
        );
        close(
            children_after[0].1.try_into().unwrap(),
            child.try_into().unwrap(),
            0,
            "Child key should match",
        );
    });
}

// Test that pending childkeys get set during the epoch after the cooldown period.
//
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_pending_children_runs_in_epoch --exact --show-output --nocapture
#[cfg(test)]
#[test]
fn test_do_set_pending_children_runs_in_epoch() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let parent = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, parent, coldkey, 0);

        // Set minimum stake for setting children
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            StakeThreshold::<Test>::get().into(),
        );

        // Schedule parent-child relationship
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            parent,
            netuid,
            vec![(proportion, child)],
        ));

        // Ensure the childkeys are not yet applied
        let children_before = SubtensorModule::get_children(&parent, netuid);
        close(
            children_before.len() as u64,
            0,
            0,
            "Children vector should be empty before cooldown",
        );

        wait_set_pending_children_cooldown(netuid);

        // Verify child assignment
        let children_after = SubtensorModule::get_children(&parent, netuid);
        close(
            children_after.len() as u64,
            1,
            0,
            "Children vector should have one entry after cooldown",
        );
        close(
            children_after[0].0,
            proportion,
            0,
            "Child proportion should match",
        );
        close(
            children_after[0].1.try_into().unwrap(),
            child.try_into().unwrap(),
            0,
            "Child key should match",
        );
    });
}

// Test that revoking childkeys does not require minimum stake
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_revoke_child_no_min_stake_check --exact --show-output --nocapture
#[test]
fn test_revoke_child_no_min_stake_check() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let parent = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(NetUid::ROOT, 13, 0);
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, parent, coldkey, 0);

        let reserve = 1_000_000_000_000_000_u64;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());
        mock::setup_reserves(NetUid::ROOT, reserve.into(), reserve.into());

        // Set minimum stake for setting children
        StakeThreshold::<Test>::put(1_000_000_000_000);

        let (_, fee) = mock::swap_tao_to_alpha(NetUid::ROOT, StakeThreshold::<Test>::get().into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            NetUid::ROOT,
            (StakeThreshold::<Test>::get() + fee).into(),
        );

        // Schedule parent-child relationship
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            parent,
            netuid,
            vec![(proportion, child)],
        ));

        // Ensure the childkeys are not yet applied
        let children_before = SubtensorModule::get_children(&parent, netuid);
        assert_eq!(children_before, vec![]);

        wait_and_set_pending_children(netuid);
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            NetUid::ROOT,
            (StakeThreshold::<Test>::get() + fee).into(),
        );

        // Ensure the childkeys are applied
        let children_after = SubtensorModule::get_children(&parent, netuid);
        assert_eq!(children_after, vec![(proportion, child)]);

        // Bypass tx rate limit
        TransactionType::SetChildren.set_last_block_on_subnet::<Test>(&parent, netuid, 0);

        // Schedule parent-child relationship revokation
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            parent,
            netuid,
            vec![],
        ));

        wait_and_set_pending_children(netuid);

        // Ensure the childkeys are revoked
        let children_after = SubtensorModule::get_children(&parent, netuid);
        assert_eq!(children_after, vec![]);
    });
}

// Test that setting childkeys works even if subnet registration is disabled
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_do_set_child_registration_disabled --exact --show-output --nocapture
#[test]
fn test_do_set_child_registration_disabled() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let parent = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, parent, coldkey, 0);

        let reserve = 1_000_000_000_000_000_u64;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Set minimum stake for setting children
        StakeThreshold::<Test>::put(1_000_000_000_000);
        let (_, fee) = mock::swap_tao_to_alpha(netuid, StakeThreshold::<Test>::get().into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            (StakeThreshold::<Test>::get() + fee).into(),
        );

        // Disable subnet registrations
        NetworkRegistrationAllowed::<Test>::insert(netuid, false);

        // Schedule parent-child relationship
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            parent,
            netuid,
            vec![(proportion, child)],
        ));

        wait_and_set_pending_children(netuid);
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            (StakeThreshold::<Test>::get() + fee).into(),
        );

        // Ensure the childkeys are applied
        let children_after = SubtensorModule::get_children(&parent, netuid);
        assert_eq!(children_after, vec![(proportion, child)]);
    });
}

// 60: Test set_children rate limiting - Fail then succeed
// This test ensures that an immediate second `set_children` transaction fails due to rate limiting:
// - Sets up a network and registers a hotkey
// - Performs a `set_children` transaction
// - Attempts a second `set_children` transaction immediately
// - Verifies that the second transaction fails with `TxRateLimitExceeded`
// Then the rate limit period passes and the second transaction succeeds
// - Steps blocks for the rate limit period
// - Attempts the second transaction again and verifies it succeeds
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_set_children_rate_limit_fail_then_succeed --exact --show-output --nocapture
#[test]
fn test_set_children_rate_limit_fail_then_succeed() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(1);
        let tempo = 13;

        // Add network and register hotkey
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // First set_children transaction
        mock_set_children(&coldkey, &hotkey, netuid, &[(100, child)]);

        // Immediate second transaction should fail due to rate limit
        assert_noop!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(100, child2)]
            ),
            Error::<Test>::TxRateLimitExceeded
        );

        // Verify first children assignment remains
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(100, child)]);

        // Try again after rate limit period has passed
        // Check rate limit
        let limit = TransactionType::SetChildren.rate_limit_on_subnet::<Test>(netuid);

        // Step that many blocks
        step_block(limit as u16);

        // Verify rate limit passes
        assert!(TransactionType::SetChildren.passes_rate_limit_on_subnet::<Test>(&hotkey, netuid));

        // Try again
        mock_set_children(&coldkey, &hotkey, netuid, &[(100, child2)]);

        // Verify children assignment has changed
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(100, child2)]);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_childkey_set_weights_single_parent --exact --show-output --nocapture
#[test]
fn test_childkey_set_weights_single_parent() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid =
            add_dynamic_network_disable_commit_reveal(&subnet_owner_hotkey, &subnet_owner_coldkey);
        Tempo::<Test>::insert(netuid, 1);

        // Define hotkeys
        let parent: U256 = U256::from(1);
        let child: U256 = U256::from(2);
        let weight_setter: U256 = U256::from(3);

        // Define coldkeys with more readable names
        let coldkey_parent: U256 = U256::from(100);
        let coldkey_child: U256 = U256::from(101);
        let coldkey_weight_setter: U256 = U256::from(102);

        let balance_to_give_child = TaoBalance::from(109_999);
        let stake_to_give_child = AlphaBalance::from(109_999);

        // Register parent with minimal stake and child with high stake
        add_balance_to_coldkey_account(&coldkey_parent, 1.into());
        add_balance_to_coldkey_account(&coldkey_child, balance_to_give_child + 10.into());
        add_balance_to_coldkey_account(&coldkey_weight_setter, 1_000_000.into());

        // Add neurons for parent, child and weight_setter
        register_ok_neuron(netuid, parent, coldkey_parent, 1);
        register_ok_neuron(netuid, child, coldkey_child, 1);
        register_ok_neuron(netuid, weight_setter, coldkey_weight_setter, 1);

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey_parent,
            netuid,
            stake_to_give_child,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &weight_setter,
            &coldkey_weight_setter,
            netuid,
            1_000_000.into(),
        );

        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        // Set parent-child relationship
        mock_set_children_no_epochs(netuid, &parent, &[(u64::MAX, child)]);

        // Set weights on the child using the weight_setter account
        let origin = RuntimeOrigin::signed(weight_setter);
        let uids: Vec<u16> = vec![1]; // Only set weight for the child (UID 1)
        let values: Vec<u16> = vec![u16::MAX]; // Use maximum value for u16
        let version_key = SubtensorModule::get_weights_version_key(netuid);
        ValidatorPermit::<Test>::insert(netuid, vec![true, true, true, true]);
        assert_ok!(SubtensorModule::set_weights(
            origin,
            netuid,
            uids.clone(),
            values.clone(),
            version_key
        ));

        // Set the min stake very high
        SubtensorModule::set_stake_threshold(u64::from(stake_to_give_child) * 5);

        // Check the child has less stake than required
        assert!(
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&child, netuid).0
                < SubtensorModule::get_stake_threshold()
        );

        // Check the child cannot set weights
        assert_noop!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(child),
                netuid,
                uids.clone(),
                values.clone(),
                version_key
            ),
            Error::<Test>::NotEnoughStakeToSetWeights
        );

        assert!(!SubtensorModule::check_weights_min_stake(&child, netuid));

        // Set a minimum stake to set weights
        SubtensorModule::set_stake_threshold(u64::from(stake_to_give_child) - 5);

        // Check if the stake for the child is above
        assert!(
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&child, netuid).0
                >= SubtensorModule::get_stake_threshold()
        );

        // Check the child can set weights
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(child),
            netuid,
            uids,
            values,
            version_key
        ));

        assert!(SubtensorModule::check_weights_min_stake(&child, netuid));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --test children -- test_set_weights_no_parent --exact --nocapture
#[test]
fn test_set_weights_no_parent() {
    // Verify that a regular key without a parent delegation is effected by the minimum stake requirements
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid =
            add_dynamic_network_disable_commit_reveal(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let hotkey: U256 = U256::from(2);
        let spare_hk: U256 = U256::from(3);

        let coldkey: U256 = U256::from(101);
        let spare_ck = U256::from(102);

        let balance_to_give_child = TaoBalance::from(109_999);
        let stake_to_give_child = AlphaBalance::from(109_999);

        add_balance_to_coldkey_account(&coldkey, balance_to_give_child + 10.into());

        // Is registered
        register_ok_neuron(netuid, hotkey, coldkey, 1);
        // Register a spare key
        register_ok_neuron(netuid, spare_hk, spare_ck, 1);

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_to_give_child,
        );

        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        // Has stake and no parent
        step_block(7200 + 1);

        let uids: Vec<u16> = vec![1]; // Set weights on the other hotkey
        let values: Vec<u16> = vec![u16::MAX]; // Use maximum value for u16
        let version_key = SubtensorModule::get_weights_version_key(netuid);

        // Check the stake weight
        let curr_stake_weight =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&hotkey, netuid).0;

        // Set the min stake very high, above the stake weight of the key
        SubtensorModule::set_stake_threshold(
            curr_stake_weight
                .saturating_mul(I64F64::saturating_from_num(5))
                .saturating_to_num::<u64>(),
        );

        let curr_stake_threshold = SubtensorModule::get_stake_threshold();
        assert!(
            curr_stake_weight < curr_stake_threshold,
            "{curr_stake_weight:?} is not less than {curr_stake_threshold:?} "
        );

        // Check the hotkey cannot set weights
        assert_noop!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                values.clone(),
                version_key
            ),
            Error::<Test>::NotEnoughStakeToSetWeights
        );

        assert!(!SubtensorModule::check_weights_min_stake(&hotkey, netuid));

        // Set a minimum stake to set weights
        SubtensorModule::set_stake_threshold(
            (curr_stake_weight - I64F64::from_num(5)).to_num::<u64>(),
        );

        // Check if the stake for the hotkey is above
        let new_stake_weight =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&hotkey, netuid).0;
        let new_stake_threshold = SubtensorModule::get_stake_threshold();
        assert!(
            new_stake_weight >= new_stake_threshold,
            "{new_stake_weight:?} is not greater than or equal to {new_stake_threshold:?} "
        );

        // Check the hotkey can set weights
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids,
            values,
            version_key
        ));

        assert!(SubtensorModule::check_weights_min_stake(&hotkey, netuid));
    });
}

/// Test that distribute_emission sends childkey take fully to the nominators if childkey
/// doesn't have its own stake, independently of parent hotkey take.
/// cargo test --package pallet-subtensor --lib -- tests::children::test_childkey_take_drain --exact --show-output
#[allow(clippy::assertions_on_constants)]
#[test]
fn test_childkey_take_drain() {
    // Test cases: parent_hotkey_take
    [0_u16, u16::MAX / 5].iter().for_each(|parent_hotkey_take| {
        new_test_ext(1).execute_with(|| {
            let parent_coldkey = U256::from(1);
            let parent_hotkey = U256::from(3);
            let child_coldkey = U256::from(2);
            let child_hotkey = U256::from(4);
            let miner_coldkey = U256::from(5);
            let miner_hotkey = U256::from(6);
            let nominator = U256::from(7);
            let netuid = NetUid::from(1);
            let subnet_tempo = 10;
            let stake = 100_000_000_000_u64;
            let proportion: u64 = u64::MAX / 2;

            // Add network, register hotkeys, and setup network parameters
            add_network(netuid, subnet_tempo, 0);
            SubtensorModule::set_ck_burn(0);
            mock::setup_reserves(netuid, (stake * 10_000).into(), (stake * 10_000).into());
            register_ok_neuron(netuid, child_hotkey, child_coldkey, 0);
            register_ok_neuron(netuid, parent_hotkey, parent_coldkey, 1);
            register_ok_neuron(netuid, miner_hotkey, miner_coldkey, 1);
            add_balance_to_coldkey_account(
                &parent_coldkey,
                TaoBalance::from(stake) + ExistentialDeposit::get(),
            );
            add_balance_to_coldkey_account(
                &nominator,
                TaoBalance::from(stake) + ExistentialDeposit::get(),
            );
            SubtensorModule::set_weights_set_rate_limit(netuid, 0);
            SubtensorModule::set_max_allowed_validators(netuid, 2);
            step_block(subnet_tempo);
            SubnetOwnerCut::<Test>::set(0);

            // Set children
            mock_set_children_no_epochs(netuid, &parent_hotkey, &[(proportion, child_hotkey)]);

            // Set 20% childkey take
            let max_take: u16 = 0xFFFF / 5;
            SubtensorModule::set_max_childkey_take(max_take);
            assert_ok!(SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(child_coldkey),
                child_hotkey,
                netuid,
                max_take
            ));

            // Set hotkey take for parent
            SubtensorModule::set_max_delegate_take(*parent_hotkey_take);
            Delegates::<Test>::insert(parent_hotkey, *parent_hotkey_take);

            // Set 0% for childkey-as-a-delegate take
            Delegates::<Test>::insert(child_hotkey, 0);

            // Setup stakes:
            //   Stake from parent
            //   Stake from nominator to childkey
            //   Parent gives 50% of stake to childkey
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(parent_coldkey),
                parent_hotkey,
                netuid,
                stake.into()
            ));
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(nominator),
                child_hotkey,
                netuid,
                stake.into()
            ));

            // Setup YUMA so that it creates emissions
            Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 0, vec![(2, 0xFFFF)]);
            Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 1, vec![(2, 0xFFFF)]);
            BlockAtRegistration::<Test>::set(netuid, 0, 1);
            BlockAtRegistration::<Test>::set(netuid, 1, 1);
            BlockAtRegistration::<Test>::set(netuid, 2, 1);
            LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2, 2, 2]);
            Kappa::<Test>::set(netuid, u16::MAX / 5);
            ActivityCutoff::<Test>::set(netuid, u16::MAX); // makes all stake active
            ValidatorPermit::<Test>::insert(netuid, vec![true, true, false]);

            // Run run_coinbase to hit subnet epoch
            let child_stake_before = SubtensorModule::get_total_stake_for_coldkey(&child_coldkey);
            let parent_stake_before = SubtensorModule::get_total_stake_for_coldkey(&parent_coldkey);
            let nominator_stake_before = SubtensorModule::get_total_stake_for_coldkey(&nominator);

            step_block(subnet_tempo);

            // Verify how emission is split between keys
            //   - Child stake remains 0
            //   - Childkey take is 20% of its total emission that rewards both inherited from
            //     parent stake and nominated stake, which all goes to nominators. Because child
            //     validator emission is 50% of total emission, 20% of it is 10% of total emission
            //     and it all goes to nominator. If childkey take was 0%, then only 5% would go to
            //     the nominator, so the final solit is:
            //   - Parent stake increases by 45% of total emission
            //   - Nominator stake increases by 55% of total emission
            let child_emission =
                SubtensorModule::get_total_stake_for_coldkey(&child_coldkey) - child_stake_before;
            let parent_emission =
                SubtensorModule::get_total_stake_for_coldkey(&parent_coldkey) - parent_stake_before;
            let nominator_emission =
                SubtensorModule::get_total_stake_for_coldkey(&nominator) - nominator_stake_before;
            let total_emission = child_emission + parent_emission + nominator_emission;

            assert_abs_diff_eq!(child_emission, TaoBalance::ZERO, epsilon = 10.into());
            assert_abs_diff_eq!(
                parent_emission,
                total_emission * 9.into() / 20.into(),
                epsilon = 10.into()
            );
            assert_abs_diff_eq!(
                nominator_emission,
                total_emission * 11.into() / 20.into(),
                epsilon = 10.into()
            );
        });
    });
}

// 44: Test with a chain of parent-child relationships (e.g., A -> B -> C)
// This test verifies the correct distribution of emissions in a chain of parent-child relationships:
// - Sets up a network with three neurons A, B, and C in a chain (A -> B -> C)
// - Establishes parent-child relationships with different stake proportions
// - Sets weights for all neurons
// - Runs an epoch with a hardcoded emission value
// - Checks the emission distribution among A, B, and C
// - Verifies that all parties received emissions and the total stake increased correctly
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_parent_child_chain_emission --exact --show-output
#[test]
fn test_parent_child_chain_emission() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        remove_owner_registration_stake(netuid);
        SubtensorModule::set_ck_burn(0);
        Tempo::<Test>::insert(netuid, 1);

        // Setup large LPs to prevent slippage
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000_000_000_000_u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000_000_u64));

        // Set owner cut to 0
        SubtensorModule::set_subnet_owner_cut(0_u16);

        // Define hotkeys and coldkeys
        let hotkey_a: U256 = U256::from(1);
        let hotkey_b: U256 = U256::from(2);
        let hotkey_c: U256 = U256::from(3);
        let coldkey_a: U256 = U256::from(100);
        let coldkey_b: U256 = U256::from(101);
        let coldkey_c: U256 = U256::from(102);

        // Register neurons with decreasing stakes
        register_ok_neuron(netuid, hotkey_a, coldkey_a, 0);
        register_ok_neuron(netuid, hotkey_b, coldkey_b, 0);
        register_ok_neuron(netuid, hotkey_c, coldkey_c, 0);

        // Add initial stakes
        add_balance_to_coldkey_account(&coldkey_a, 1_000.into());
        add_balance_to_coldkey_account(&coldkey_b, 1_000.into());
        add_balance_to_coldkey_account(&coldkey_c, 1_000.into());

        // Swap to alpha
        let stake_a = 300_000_000_000_u64;
        let stake_b = 100_000_000_000_u64;
        let stake_c = 50_000_000_000_u64;
        let total_tao: I96F32 = I96F32::from_num(stake_a + stake_b + stake_c);
        let total_alpha: I96F32 = I96F32::from_num(
            SubtensorModule::swap_tao_for_alpha(
                netuid,
                total_tao.to_num::<u64>().into(),
                <Test as Config>::SwapInterface::max_price(),
                false,
            )
            .unwrap()
            .amount_paid_out,
        );

        // Set the stakes directly
        // This avoids needing to swap tao to alpha, impacting the initial stake distribution.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            (total_alpha * I96F32::from_num(stake_a) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_b,
            &coldkey_b,
            netuid,
            (total_alpha * I96F32::from_num(stake_b) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_c,
            &coldkey_c,
            netuid,
            (total_alpha * I96F32::from_num(stake_c) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );

        // Get old stakes
        let stake_a = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);
        let stake_c = SubtensorModule::get_total_stake_for_hotkey(&hotkey_c);

        let _total_stake: I96F32 = I96F32::from_num(stake_a + stake_b + stake_c);

        // Assert initial stake is correct
        let rel_stake_a = I96F32::from_num(stake_a) / total_tao;
        let rel_stake_b = I96F32::from_num(stake_b) / total_tao;
        let rel_stake_c = I96F32::from_num(stake_c) / total_tao;

        log::info!("rel_stake_a: {rel_stake_a:?}"); // 0.6666 -> 2/3
        log::info!("rel_stake_b: {rel_stake_b:?}"); // 0.2222 -> 2/9
        log::info!("rel_stake_c: {rel_stake_c:?}"); // 0.1111 -> 1/9
        assert!((rel_stake_a - I96F32::from_num(stake_a) / total_tao).abs() < 0.001);
        assert!((rel_stake_b - I96F32::from_num(stake_b) / total_tao).abs() < 0.001);
        assert!((rel_stake_c - I96F32::from_num(stake_c) / total_tao).abs() < 0.001);

        // Set parent-child relationships
        // A -> B (50% of A's stake)
        mock_set_children_no_epochs(netuid, &hotkey_a, &[(u64::MAX / 2, hotkey_b)]);

        // B -> C (50% of B's stake)
        mock_set_children_no_epochs(netuid, &hotkey_b, &[(u64::MAX / 2, hotkey_c)]);

        // Get old stakes after children are scheduled
        let stake_a_old = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b_old = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);
        let stake_c_old = SubtensorModule::get_total_stake_for_hotkey(&hotkey_c);

        let total_stake_old: I96F32 =
            I96F32::from_num((stake_a_old + stake_b_old + stake_c_old).to_u64());
        log::info!("Old stake for hotkey A: {stake_a_old:?}");
        log::info!("Old stake for hotkey B: {stake_b_old:?}");
        log::info!("Old stake for hotkey C: {stake_c_old:?}");
        log::info!("Total old stake: {total_stake_old:?}");

        // Set CHK take rate to 1/9
        let chk_take: I96F32 = I96F32::from_num(1_f64 / 9_f64);
        let chk_take_u16: u16 = (chk_take * I96F32::from_num(u16::MAX)).saturating_to_num::<u16>();
        ChildkeyTake::<Test>::insert(hotkey_b, netuid, chk_take_u16);
        ChildkeyTake::<Test>::insert(hotkey_c, netuid, chk_take_u16);

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        let emission = SubtensorModule::get_block_emission();

        // Set pending emission to 0
        PendingValidatorEmission::<Test>::insert(netuid, AlphaBalance::ZERO);
        PendingServerEmission::<Test>::insert(netuid, AlphaBalance::ZERO);

        // To trigger the epoch, block should be > tempo. So we advance it before
        System::set_block_number(2);

        // Run epoch with emission value
        let emission_value = u64::from(emission.peek());
        SubtensorModule::run_coinbase(emission);

        // Log new stake
        let stake_a_new = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b_new = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);
        let stake_c_new = SubtensorModule::get_total_stake_for_hotkey(&hotkey_c);
        let total_stake_new = I96F32::from_num((stake_a_new + stake_b_new + stake_c_new).to_u64());
        log::info!("Stake for hotkey A: {stake_a_new:?}");
        log::info!("Stake for hotkey B: {stake_b_new:?}");
        log::info!("Stake for hotkey C: {stake_c_new:?}");

        let stake_inc_a = stake_a_new - stake_a_old;
        let stake_inc_b = stake_b_new - stake_b_old;
        let stake_inc_c = stake_c_new - stake_c_old;
        let total_stake_inc: I96F32 = total_stake_new - total_stake_old;
        log::info!("Stake increase for hotkey A: {stake_inc_a:?}");
        log::info!("Stake increase for hotkey B: {stake_inc_b:?}");
        log::info!("Stake increase for hotkey C: {stake_inc_c:?}");
        log::info!("Total stake increase: {total_stake_inc:?}");
        let rel_stake_inc_a = I96F32::from_num(stake_inc_a) / total_stake_inc;
        let rel_stake_inc_b = I96F32::from_num(stake_inc_b) / total_stake_inc;
        let rel_stake_inc_c = I96F32::from_num(stake_inc_c) / total_stake_inc;
        log::info!("rel_stake_inc_a: {rel_stake_inc_a:?}");
        log::info!("rel_stake_inc_b: {rel_stake_inc_b:?}");
        log::info!("rel_stake_inc_c: {rel_stake_inc_c:?}");

        // Verify the final stake distribution
        let stake_inc_eps = I96F32::from_num(1e-4); // 4 decimal places

        // Each child has chk_take take
        let expected_a = I96F32::from_num(2_f64 / 3_f64)
            * (I96F32::from_num(1_f64) - (I96F32::from_num(1_f64 / 2_f64) * chk_take));
        assert!(
            (rel_stake_inc_a - expected_a).abs() // B's take on 50% CHK
            <= stake_inc_eps,
            "A should have {expected_a:?} of total stake increase; {rel_stake_inc_a:?}"
        );
        let expected_b = I96F32::from_num(2_f64 / 9_f64)
            * (I96F32::from_num(1_f64) - (I96F32::from_num(1_f64 / 2_f64) * chk_take))
            + I96F32::from_num(2_f64 / 3_f64) * (I96F32::from_num(1_f64 / 2_f64) * chk_take);
        assert!(
            (rel_stake_inc_b - expected_b).abs() // C's take on 50% CHK + take from A
            <= stake_inc_eps,
            "B should have {expected_b:?} of total stake increase; {rel_stake_inc_b:?}"
        );
        let expected_c = I96F32::from_num(1_f64 / 9_f64)
            + (I96F32::from_num(2_f64 / 9_f64) * I96F32::from_num(1_f64 / 2_f64) * chk_take);
        assert!(
            (rel_stake_inc_c - expected_c).abs() // B's take on 50% CHK
            <= stake_inc_eps,
            "C should have {expected_c:?} of total stake increase; {rel_stake_inc_c:?}"
        );

        let hotkeys = [hotkey_a, hotkey_b, hotkey_c];
        let mut total_stake_now = AlphaBalance::ZERO;
        for (hotkey, netuid, stake) in TotalHotkeyAlpha::<Test>::iter() {
            if hotkeys.contains(&hotkey) {
                total_stake_now += stake;
            } else {
                log::info!("hotkey: {hotkey:?}, netuid: {netuid:?}, stake: {stake:?}");
            }
        }
        log::info!("total_stake_now: {total_stake_now:?}, total_stake_new: {total_stake_new:?}");

        assert_abs_diff_eq!(
            total_stake_inc.to_num::<u64>(),
            emission_value,
            epsilon = emission_value / 1000,
        );
    });
}

// 45: Test *epoch* with a chain of parent-child relationships (e.g., A -> B -> C)
// This test verifies the correct distribution of emissions in a chain of parent-child relationships:
// - Sets up a network with three neurons A, B, and C in a chain (A -> B -> C)
// - Establishes parent-child relationships with different stake proportions
// - Sets weights for all neurons
// - Runs an epoch with a hardcoded emission value
// - Checks the emission distribution among A, B, and C
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_parent_child_chain_epoch --exact --show-output
#[test]
fn test_parent_child_chain_epoch() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        SubtensorModule::set_ck_burn(0);
        // Set owner cut to 0
        SubtensorModule::set_subnet_owner_cut(0_u16);

        // Define hotkeys and coldkeys
        let hotkey_a: U256 = U256::from(1);
        let hotkey_b: U256 = U256::from(2);
        let hotkey_c: U256 = U256::from(3);
        let coldkey_a: U256 = U256::from(100);
        let coldkey_b: U256 = U256::from(101);
        let coldkey_c: U256 = U256::from(102);

        // Register neurons with decreasing stakes
        register_ok_neuron(netuid, hotkey_a, coldkey_a, 0);
        register_ok_neuron(netuid, hotkey_b, coldkey_b, 0);
        register_ok_neuron(netuid, hotkey_c, coldkey_c, 0);

        // Add initial stakes
        add_balance_to_coldkey_account(&coldkey_a, 1_000.into());
        add_balance_to_coldkey_account(&coldkey_b, 1_000.into());
        add_balance_to_coldkey_account(&coldkey_c, 1_000.into());

        mock::setup_reserves(
            netuid,
            1_000_000_000_000_u64.into(),
            1_000_000_000_000_u64.into(),
        );

        // Swap to alpha
        let total_tao = I96F32::from_num(300_000 + 100_000 + 50_000);
        let (total_alpha, _) = mock::swap_tao_to_alpha(netuid, total_tao.to_num::<u64>().into());
        let total_alpha = I96F32::from_num(total_alpha);

        // Set the stakes directly
        // This avoids needing to swap tao to alpha, impacting the initial stake distribution.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            (total_alpha * I96F32::from_num(300_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_b,
            &coldkey_b,
            netuid,
            (total_alpha * I96F32::from_num(100_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_c,
            &coldkey_c,
            netuid,
            (total_alpha * I96F32::from_num(50_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );

        // Get old stakes
        let stake_a = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);
        let stake_c = SubtensorModule::get_total_stake_for_hotkey(&hotkey_c);

        // Assert initial stake is correct
        let rel_stake_a = I96F32::from_num(stake_a) / total_alpha;
        let rel_stake_b = I96F32::from_num(stake_b) / total_alpha;
        let rel_stake_c = I96F32::from_num(stake_c) / total_alpha;

        log::info!("rel_stake_a: {rel_stake_a:?}"); // 0.6666 -> 2/3
        log::info!("rel_stake_b: {rel_stake_b:?}"); // 0.2222 -> 2/9
        log::info!("rel_stake_c: {rel_stake_c:?}"); // 0.1111 -> 1/9

        assert!(rel_stake_a > I96F32::from_num(0));
        assert!(rel_stake_b > I96F32::from_num(0));
        assert!(rel_stake_c > I96F32::from_num(0));

        // because of the fee we allow slightly higher range
        let epsilon = I96F32::from_num(0.00001);
        assert!((rel_stake_a - (I96F32::from_num(300_000) / total_tao)).abs() <= epsilon);
        assert!((rel_stake_b - (I96F32::from_num(100_000) / total_tao)).abs() <= epsilon);
        assert!((rel_stake_c - (I96F32::from_num(50_000) / total_tao)).abs() <= epsilon);

        // Set parent-child relationships
        // A -> B (50% of A's stake)
        mock_set_children(&coldkey_a, &hotkey_a, netuid, &[(u64::MAX / 2, hotkey_b)]);

        // B -> C (50% of B's stake)
        mock_set_children(&coldkey_b, &hotkey_b, netuid, &[(u64::MAX / 2, hotkey_c)]);

        // Set CHK take rate to 1/9
        let chk_take = I96F32::from_num(1_f64 / 9_f64);
        let chk_take_u16: u16 = (chk_take * I96F32::from_num(u16::MAX)).saturating_to_num::<u16>();
        ChildkeyTake::<Test>::insert(hotkey_b, netuid, chk_take_u16);
        ChildkeyTake::<Test>::insert(hotkey_c, netuid, chk_take_u16);

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        let hardcoded_emission = I96F32::from_num(1_000_000); // 1 million (adjust as needed)

        let hotkey_emission =
            SubtensorModule::epoch(netuid, hardcoded_emission.saturating_to_num::<u64>().into());
        log::info!("hotkey_emission: {hotkey_emission:?}");
        let total_emission: I96F32 = hotkey_emission
            .iter()
            .map(|(_, _, emission)| I96F32::from_num(*emission))
            .sum();

        // Verify emissions match expected from CHK arrangements
        let em_eps = I96F32::from_num(1e-4); // 4 decimal places
        // A's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[0].2) / total_emission) -
            I96F32::from_num(2_f64 / 3_f64 * 1_f64 / 2_f64)).abs() // 2/3 * 1/2 = 1/3; 50% -> B
			<= em_eps,
            "A should have pending emission of 1/3 of total emission"
        );
        // B's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[1].2) / total_emission) -
            (I96F32::from_num(2_f64 / 9_f64 * 1_f64 / 2_f64 + 2_f64 / 3_f64 * 1_f64 / 2_f64))).abs() // 2/9 * 1/2 + 2/3 * 1/2; 50% -> C + 50% from A
            <= em_eps,
            "B should have pending emission of 4/9 of total emission"
        );
        // C's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[2].2) / total_emission) -
            (I96F32::from_num(1_f64 / 9_f64 + 1_f64 / 2_f64 * 2_f64 / 9_f64))).abs() // 1/9 + 2/9 * 1/2; 50% from B
            <= em_eps,
            "C should have pending emission of 1/9 of total emission"
        );
    });
}

// 46: Test dividend distribution with children
// This test verifies the correct distribution of emissions in a chain of parent-child relationships:
// - Sets up a network with three neurons A, B, and C in a chain (A -> B -> C)
// - Establishes parent-child relationships with different stake proportions
// - Adds a childkey take for both B and C
// - Distributes emission across each hotkey using a the helper
// - Checks the emission distribution among A, B, and C
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_dividend_distribution_with_children --exact --show-output
#[test]
fn test_dividend_distribution_with_children() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        SubtensorModule::set_ck_burn(0);
        mock::setup_reserves(
            netuid,
            1_000_000_000_000_000_u64.into(),
            1_000_000_000_000_000_u64.into(),
        );
        // Set owner cut to 0
        SubtensorModule::set_subnet_owner_cut(0_u16);

        // Define hotkeys and coldkeys
        let hotkey_a: U256 = U256::from(1);
        let hotkey_b: U256 = U256::from(2);
        let hotkey_c: U256 = U256::from(3);
        let coldkey_a: U256 = U256::from(100);
        let coldkey_b: U256 = U256::from(101);
        let coldkey_c: U256 = U256::from(102);

        // Register neurons with decreasing stakes
        register_ok_neuron(netuid, hotkey_a, coldkey_a, 0);
        register_ok_neuron(netuid, hotkey_b, coldkey_b, 0);
        register_ok_neuron(netuid, hotkey_c, coldkey_c, 0);

        // Add initial stakes
        add_balance_to_coldkey_account(&coldkey_a, 1_000.into());
        add_balance_to_coldkey_account(&coldkey_b, 1_000.into());
        add_balance_to_coldkey_account(&coldkey_c, 1_000.into());

        // Swap to alpha
        let total_tao = I96F32::from_num(300_000 + 100_000 + 50_000);
        let (total_alpha, _) = mock::swap_tao_to_alpha(netuid, total_tao.to_num::<u64>().into());
        let total_alpha = I96F32::from_num(total_alpha);

        // Set the stakes directly
        // This avoids needing to swap tao to alpha, impacting the initial stake distribution.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            (total_alpha * I96F32::from_num(300_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_b,
            &coldkey_b,
            netuid,
            (total_alpha * I96F32::from_num(100_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_c,
            &coldkey_c,
            netuid,
            (total_alpha * I96F32::from_num(50_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );

        // Get old stakes
        let stake_a = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);
        let stake_c = SubtensorModule::get_total_stake_for_hotkey(&hotkey_c);

        // Assert initial stake is correct
        let rel_stake_a = I96F32::from_num(stake_a) / total_alpha;
        let rel_stake_b = I96F32::from_num(stake_b) / total_alpha;
        let rel_stake_c = I96F32::from_num(stake_c) / total_alpha;

        log::info!("rel_stake_a: {rel_stake_a:?}"); // 0.6666 -> 2/3
        log::info!("rel_stake_b: {rel_stake_b:?}"); // 0.2222 -> 2/9
        log::info!("rel_stake_c: {rel_stake_c:?}"); // 0.1111 -> 1/9
        let epsilon = I96F32::from_num(0.00001);
        assert!((rel_stake_a - I96F32::from_num(300_000) / total_tao).abs() <= epsilon);
        assert!((rel_stake_b - I96F32::from_num(100_000) / total_tao).abs() <= epsilon);
        assert!((rel_stake_c - I96F32::from_num(50_000) / total_tao).abs() <= epsilon);

        // Set parent-child relationships
        // A -> B (50% of A's stake)
        mock_set_children(&coldkey_a, &hotkey_a, netuid, &[(u64::MAX / 2, hotkey_b)]);

        // B -> C (50% of B's stake)
        mock_set_children(&coldkey_b, &hotkey_b, netuid, &[(u64::MAX / 2, hotkey_c)]);

        // Set CHK take rate to 1/9
        let chk_take: I96F32 = I96F32::from_num(1_f64 / 9_f64);
        let chk_take_u16: u16 = (chk_take * I96F32::from_num(u16::MAX)).saturating_to_num::<u16>();
        ChildkeyTake::<Test>::insert(hotkey_b, netuid, chk_take_u16);
        ChildkeyTake::<Test>::insert(hotkey_c, netuid, chk_take_u16);

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        let hardcoded_emission: I96F32 = I96F32::from_num(1_000_000); // 1 million (adjust as needed)

        let hotkey_emission =
            SubtensorModule::epoch(netuid, hardcoded_emission.saturating_to_num::<u64>().into());
        log::info!("hotkey_emission: {hotkey_emission:?}");
        let total_emission: I96F32 = hotkey_emission
            .iter()
            .map(|(_, _, emission)| I96F32::from_num(*emission))
            .sum();

        // Verify emissions match expected from CHK arrangements
        let em_eps: I96F32 = I96F32::from_num(1e-4); // 4 decimal places
        // A's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[0].2) / total_emission) -
            I96F32::from_num(2_f64 / 3_f64 * 1_f64 / 2_f64)).abs() // 2/3 * 1/2 = 1/3; 50% -> B
			<= em_eps,
            "A should have pending emission of 1/3 of total emission"
        );
        // B's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[1].2) / total_emission) -
            (I96F32::from_num(2_f64 / 9_f64 * 1_f64 / 2_f64 + 2_f64 / 3_f64 * 1_f64 / 2_f64))).abs() // 2/9 * 1/2 + 2/3 * 1/2; 50% -> C + 50% from A
            <= em_eps,
            "B should have pending emission of 4/9 of total emission"
        );
        // C's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[2].2) / total_emission) -
            (I96F32::from_num(1_f64 / 9_f64 + 1_f64 / 2_f64 * 2_f64 / 9_f64))).abs() // 1/9 + 2/9 * 1/2; 50% from B
            <= em_eps,
            "C should have pending emission of 1/9 of total emission"
        );

        let dividends_a = SubtensorModule::get_parent_child_dividends_distribution(
            &hotkey_a,
            netuid,
            hardcoded_emission.saturating_to_num::<u64>().into(),
        );
        let dividends_b = SubtensorModule::get_parent_child_dividends_distribution(
            &hotkey_b,
            netuid,
            hardcoded_emission.saturating_to_num::<u64>().into(),
        );
        let dividends_c = SubtensorModule::get_parent_child_dividends_distribution(
            &hotkey_c,
            netuid,
            hardcoded_emission.saturating_to_num::<u64>().into(),
        );
        log::info!("dividends_a: {dividends_a:?}");
        log::info!("dividends_b: {dividends_b:?}");
        log::info!("dividends_c: {dividends_c:?}");

        // We expect A to get all of its own emission, as it has no parents.
        assert_eq!(dividends_a.len(), 1);
        assert_eq!(dividends_a[0].0, hotkey_a);
        assert_eq!(
            dividends_a[0].1,
            hardcoded_emission.saturating_to_num::<u64>().into()
        );
        assert_abs_diff_eq!(
            dividends_a
                .iter()
                .map(|(_, emission)| u64::from(*emission))
                .sum::<u64>(),
            hardcoded_emission.saturating_to_num::<u64>(),
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );

        // We expect B to get a portion of its own emission, and some comission from A, where A gets the rest.
        // B re-delegates 0.5 of its stake to C; And A re-delegates 0.5 of its stake to B.
        let total_stake_b = rel_stake_b * 1 / 2 + rel_stake_a * 1 / 2;
        let expected_b_b: u64 = ((rel_stake_b * 1 / 2) / total_stake_b * hardcoded_emission
            + (rel_stake_a * 1 / 2) / total_stake_b * hardcoded_emission * chk_take)
            .saturating_to_num::<u64>();
        assert_eq!(dividends_b.len(), 2); // A and B
        assert_eq!(dividends_b[1].0, hotkey_b);
        assert_abs_diff_eq!(
            u64::from(dividends_b[1].1),
            expected_b_b,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
        let expected_b_a: u64 = hardcoded_emission.saturating_to_num::<u64>() - expected_b_b;
        assert_eq!(dividends_b[0].0, hotkey_a);
        assert_abs_diff_eq!(
            u64::from(dividends_b[0].1),
            expected_b_a,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
        assert_abs_diff_eq!(
            dividends_b
                .iter()
                .map(|(_, emission)| u64::from(*emission))
                .sum::<u64>(),
            hardcoded_emission.saturating_to_num::<u64>(),
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );

        // We expect C to get a portion of its own emission, and some comission from B, where B gets the rest.
        let total_stake_c = rel_stake_c + rel_stake_b * 1 / 2;
        let expected_c_c: u64 = (rel_stake_c / total_stake_c * hardcoded_emission
            + (rel_stake_b * 1 / 2) / total_stake_c * hardcoded_emission * chk_take)
            .saturating_to_num::<u64>();
        assert_eq!(dividends_c.len(), 2); // B and C
        assert_eq!(dividends_c[1].0, hotkey_c);
        assert_abs_diff_eq!(
            u64::from(dividends_c[1].1),
            expected_c_c,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
        let expected_c_b: u64 = hardcoded_emission.saturating_to_num::<u64>() - expected_c_c;
        assert_eq!(dividends_c[0].0, hotkey_b);
        assert_abs_diff_eq!(
            u64::from(dividends_c[0].1),
            expected_c_b,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
        assert_abs_diff_eq!(
            dividends_c
                .iter()
                .map(|(_, emission)| u64::from(*emission))
                .sum::<u64>(),
            hardcoded_emission.saturating_to_num::<u64>(),
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
    });
}

// 47: Test emission distribution when adding/removing parent-child relationships mid-epoch
// This test verifies the correct distribution of emissions when parent-child relationships change:
// - Sets up a network with three neurons: parent, child1, and child2
// - Establishes initial parent-child relationship between parent and child1
// - Runs first epoch and distributes emissions
// - Changes parent-child relationships to include both child1 and child2
// - Runs second epoch and distributes emissions
// - Checks final emission distribution and stake updates
// - Verifies correct parent-child relationships and stake proportions
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_dynamic_parent_child_relationships --exact --show-output
#[test]
fn test_dynamic_parent_child_relationships() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        SubtensorModule::set_ck_burn(0);
        add_network_disable_commit_reveal(netuid, 1, 0);

        // Define hotkeys and coldkeys
        let parent = U256::from(1);
        let child1 = U256::from(2);
        let child2 = U256::from(3);
        let coldkey_parent = U256::from(100);
        let coldkey_child1 = U256::from(101);
        let coldkey_child2 = U256::from(102);

        // Register neurons with varying stakes
        register_ok_neuron(netuid, parent, coldkey_parent, 0);
        register_ok_neuron(netuid, child1, coldkey_child1, 0);
        register_ok_neuron(netuid, child2, coldkey_child2, 0);

        let chk_take_1 = SubtensorModule::get_childkey_take(&child1, netuid);
        let chk_take_2 = SubtensorModule::get_childkey_take(&child2, netuid);
        log::info!("child take 1: {chk_take_1:?}");
        log::info!("child take 2: {chk_take_2:?}");

        // Add initial stakes
        add_balance_to_coldkey_account(&coldkey_parent, (500_000 + 1_000).into());
        add_balance_to_coldkey_account(&coldkey_child1, (50_000 + 1_000).into());
        add_balance_to_coldkey_account(&coldkey_child2, (30_000 + 1_000).into());

        let reserve = 1_000_000_000_000_u64;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Swap to alpha
        let total_tao = I96F32::from_num(500_000 + 50_000 + 30_000);
        let (total_alpha, _) = mock::swap_tao_to_alpha(netuid, total_tao.to_num::<u64>().into());
        let total_alpha = I96F32::from_num(total_alpha);
        log::info!("total_alpha: {total_alpha:?}");

        // Set the stakes directly
        // This avoids needing to swap tao to alpha, impacting the initial stake distribution.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey_parent,
            netuid,
            (total_alpha * I96F32::from_num(500_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &child1,
            &coldkey_child1,
            netuid,
            (total_alpha * I96F32::from_num(50_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &child2,
            &coldkey_child2,
            netuid,
            (total_alpha * I96F32::from_num(30_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );

        // Get old stakes
        let stake_parent_0 = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid);
        let stake_child1_0 = SubtensorModule::get_stake_for_hotkey_on_subnet(&child1, netuid);
        let stake_child2_0 = SubtensorModule::get_stake_for_hotkey_on_subnet(&child2, netuid);
        log::info!("stake_parent_0: {stake_parent_0:?}");
        log::info!("stake_child1_0: {stake_child1_0:?}");
        log::info!("stake_child2_0: {stake_child2_0:?}");

        let total_stake_0 = stake_parent_0 + stake_child1_0 + stake_child2_0;

        // Assert initial stake is correct
        let rel_stake_parent_0 = I96F32::from_num(stake_parent_0) / total_alpha;
        let rel_stake_child1_0 = I96F32::from_num(stake_child1_0) / total_alpha;
        let rel_stake_child2_0 = I96F32::from_num(stake_child2_0) / total_alpha;

        log::info!("rel_stake_parent_0: {rel_stake_parent_0:?}");
        log::info!("rel_stake_child1_0: {rel_stake_child1_0:?}");
        log::info!("rel_stake_child2_0: {rel_stake_child2_0:?}");
        let epsilon = I96F32::from_num(0.00001);
        assert!((rel_stake_parent_0 - I96F32::from_num(500_000) / total_tao).abs() <= epsilon);
        assert!((rel_stake_child1_0 - I96F32::from_num(50_000) / total_tao).abs() <= epsilon);
        assert!((rel_stake_child2_0 - I96F32::from_num(30_000) / total_tao).abs() <= epsilon);

        mock_set_children(&coldkey_parent, &parent, netuid, &[(u64::MAX / 2, child1)]);

        step_block(2);

        // Set weights
        let origin = RuntimeOrigin::signed(parent);
        let uids: Vec<u16> = vec![0, 1, 2]; // UIDs for parent, child1, child2
        let values: Vec<u16> = vec![65535, 65535, 65535]; // Set equal weights for all hotkeys
        let version_key = SubtensorModule::get_weights_version_key(netuid);

        // Ensure we can set weights without rate limiting
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        assert_ok!(SubtensorModule::set_weights(
            origin,
            netuid,
            uids,
            values,
            version_key
        ));

        // Step blocks to allow for emission distribution
        step_block(11);
        step_rate_limit(&TransactionType::SetChildren, netuid);

        // Get total stake after first payout
        let total_stake_1 = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid)
            + SubtensorModule::get_stake_for_hotkey_on_subnet(&child1, netuid)
            + SubtensorModule::get_stake_for_hotkey_on_subnet(&child2, netuid);
        log::info!("total_stake_1: {total_stake_1:?}");

        // Change parent-child relationships
        mock_set_children(
            &coldkey_parent,
            &parent,
            netuid,
            &[(u64::MAX / 4, child1), (u64::MAX / 3, child2)],
        );

        // Step blocks again to allow for emission distribution
        step_block(11);

        // Get total stake after second payout
        let total_stake_2 = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid)
            + SubtensorModule::get_stake_for_hotkey_on_subnet(&child1, netuid)
            + SubtensorModule::get_stake_for_hotkey_on_subnet(&child2, netuid);
        log::info!("total_stake_2: {total_stake_2:?}");

        // Check final emission distribution
        let stake_parent_2 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent, netuid);
        let stake_child1_2 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child1, netuid);
        let stake_child2_2 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child2, netuid);
        let total_parent_stake = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid);
        let _total_child1_stake = SubtensorModule::get_stake_for_hotkey_on_subnet(&child1, netuid);
        let _total_child2_stake = SubtensorModule::get_stake_for_hotkey_on_subnet(&child2, netuid);

        log::info!("Final stakes:");
        log::info!("Parent stake: {stake_parent_2}");
        log::info!("Child1 stake: {stake_child1_2}");
        log::info!("Child2 stake: {stake_child2_2}");

        // Payout 1
        let payout_1 = total_stake_1 - total_stake_0;
        log::info!("payout_1: {payout_1:?}");

        // Payout 2
        let payout_2 = total_stake_2 - total_stake_1;
        log::info!("payout_2: {payout_2:?}");

        let total_emission = I96F32::from_num(payout_1 + payout_2);

        #[allow(non_snake_case)]
        let TOLERANCE = I96F32::from_num(0.001); // Allow for a small discrepancy due to potential rounding

        // Precise assertions with tolerance
        log::info!("total_emission: {total_emission:?}");
        let expected_parent_stake = ((I96F32::from_num(u64::from(stake_parent_0))
            + total_emission * rel_stake_parent_0)
            * I96F32::from_num(5))
            / I96F32::from_num(12);
        assert!(
            (I96F32::from_num(stake_parent_2) - expected_parent_stake).abs()
                / expected_parent_stake
                <= TOLERANCE,
            "Parent stake should be close to {expected_parent_stake:?}, but was {stake_parent_2}"
        );
        // Parent stake calculation:
        // Initial stake: 500,000
        // First epoch: 1/2 parent_stake
        // Second epoch: 5/12 parent_stake

        let expected_child1_stake = total_emission * rel_stake_child1_0
            + I96F32::from_num(stake_child1_0 + total_parent_stake / 4.into());
        assert!(
            (I96F32::from_num(stake_child1_2) - expected_child1_stake).abs()
                / expected_child1_stake
                <= TOLERANCE,
            "Child1 stake should be close to {expected_child1_stake:?}, but was {stake_child1_2}"
        );
        // Child1 stake calculation:
        // Initial stake: 50,000
        // First epoch: 1/2 parent_stake + child1_stake
        // Second epoch: 1/4 parent_stake + child1_stake

        let expected_child2_stake = total_emission * rel_stake_child2_0
            + I96F32::from_num(u64::from(stake_child2_0 + total_parent_stake / 3.into()));
        assert!(
            (I96F32::from_num(stake_child2_2) - expected_child2_stake).abs()
                / expected_child2_stake
                <= TOLERANCE,
            "Child2 stake should be close to {expected_child2_stake:?}, but was {stake_child2_2}"
        );
        // Child2 stake calculation:
        // Initial stake: 30,000
        // First epoch: child2_stake
        // Second epoch: 1/3 parent_stake + child2_stake

        // Additional checks for parent-child relationships
        let parent_children: Vec<(u64, U256)> = SubtensorModule::get_children(&parent, netuid);
        assert_eq!(
            parent_children,
            vec![(u64::MAX / 4, child1), (u64::MAX / 3, child2)],
            "Parent should have both children with correct proportions"
        );
        // Parent-child relationship:
        // child1: 1/4 of parent's stake
        // child2: 1/3 of parent's stake

        let child1_parents: Vec<(u64, U256)> = SubtensorModule::get_parents(&child1, netuid);
        assert_eq!(
            child1_parents,
            vec![(u64::MAX / 4, parent)],
            "Child1 should have parent as its parent with correct proportion"
        );
        // Child1-parent relationship:
        // parent: 1/4 of child1's stake

        let child2_parents: Vec<(u64, U256)> = SubtensorModule::get_parents(&child2, netuid);
        assert_eq!(
            child2_parents,
            vec![(u64::MAX / 3, parent)],
            "Child2 should have parent as its parent with correct proportion"
        );
        // Child2-parent relationship:
        // parent: 1/3 of child2's stake

        // Check that child2 has received more stake than child1
        assert!(
            stake_child2_2 > stake_child1_2,
            "Child2 should have received more emission than Child1 due to higher proportion"
        );
        // Child2 stake (874,826) > Child1 stake (778,446)
    });
}

#[test]
fn test_do_set_child_as_sn_owner_not_enough_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let sn_owner_hotkey = U256::from(4);

        let child_coldkey = U256::from(2);
        let child_hotkey = U256::from(5);

        let threshold = 10_000;
        SubtensorModule::set_stake_threshold(threshold);

        let proportion: u64 = 1000;

        let netuid = add_dynamic_network(&sn_owner_hotkey, &coldkey);
        remove_owner_registration_stake(netuid);
        register_ok_neuron(netuid, child_hotkey, child_coldkey, 0);

        // Verify stake of sn_owner_hotkey is NOT enough
        assert!(
            SubtensorModule::get_total_stake_for_hotkey(&sn_owner_hotkey)
                < StakeThreshold::<Test>::get().into()
        );

        // Verify that we can set child as sn owner, even though sn_owner_hotkey has insufficient stake
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            sn_owner_hotkey,
            netuid,
            vec![(proportion, child_hotkey)]
        ));

        // Make new hotkey from owner coldkey
        let other_sn_owner_hotkey = U256::from(6);
        register_ok_neuron(netuid, other_sn_owner_hotkey, coldkey, 1234);

        // Verify stake of other_sn_owner_hotkey is NOT enough
        assert!(
            SubtensorModule::get_total_stake_for_hotkey(&other_sn_owner_hotkey)
                < StakeThreshold::<Test>::get().into()
        );

        // Can't set child as sn owner, because it is not in SubnetOwnerHotkey map
        assert_noop!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                other_sn_owner_hotkey,
                netuid,
                vec![(proportion, child_hotkey)]
            ),
            Error::<Test>::NotEnoughStakeToSetChildkeys
        );
    });
}

// Test dividend distribution for children with same coldkey Owner
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_dividend_distribution_with_children_same_coldkey_owner --exact --show-output
#[test]
fn test_dividend_distribution_with_children_same_coldkey_owner() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        // Set SN owner cut to 0
        SubtensorModule::set_subnet_owner_cut(0_u16);
        mock::setup_reserves(
            netuid,
            1_000_000_000_000_u64.into(),
            1_000_000_000_000_u64.into(),
        );

        // Define hotkeys and coldkeys
        let hotkey_a: U256 = U256::from(1);
        let hotkey_b: U256 = U256::from(2);
        let coldkey_a: U256 = U256::from(100); // Only one coldkey

        // Register neurons with decreasing stakes
        register_ok_neuron(netuid, hotkey_a, coldkey_a, 0);
        register_ok_neuron(netuid, hotkey_b, coldkey_a, 0);

        // Add initial stakes
        add_balance_to_coldkey_account(&coldkey_a, 1_000.into());
        add_balance_to_coldkey_account(&coldkey_a, 1_000.into());

        // Swap to alpha
        let total_tao = 300_000 + 100_000;
        let total_alpha = I96F32::from_num(mock::swap_tao_to_alpha(netuid, total_tao.into()).0);
        let total_tao = I96F32::from_num(total_tao);

        // Set the stakes directly
        // This avoids needing to swap tao to alpha, impacting the initial stake distribution.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            (total_alpha * I96F32::from_num(300_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_b,
            &coldkey_a,
            netuid,
            (total_alpha * I96F32::from_num(100_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );

        // Get old stakes
        let stake_a = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);

        // Assert initial stake is correct
        let rel_stake_a = I96F32::from_num(stake_a) / total_alpha;
        let rel_stake_b = I96F32::from_num(stake_b) / total_alpha;

        log::info!("rel_stake_a: {rel_stake_a:?}"); // 0.75 -> 3/4
        log::info!("rel_stake_b: {rel_stake_b:?}"); // 0.25 -> 1/4
        let epsilon = I96F32::from_num(0.0001);
        assert!((rel_stake_a - I96F32::from_num(300_000) / total_tao).abs() <= epsilon);
        assert!((rel_stake_b - I96F32::from_num(100_000) / total_tao).abs() <= epsilon);

        // Set parent-child relationships
        // A -> B (50% of A's stake)
        mock_set_children(&coldkey_a, &hotkey_a, netuid, &[(u64::MAX / 2, hotkey_b)]);

        // Set CHK take rate to 1/9
        let chk_take: I96F32 = I96F32::from_num(1_f64 / 9_f64);
        let chk_take_u16: u16 = (chk_take * I96F32::from_num(u16::MAX)).saturating_to_num::<u16>();
        ChildkeyTake::<Test>::insert(hotkey_b, netuid, chk_take_u16);

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        let hardcoded_emission: I96F32 = I96F32::from_num(1_000_000); // 1 million (adjust as needed)

        let hotkey_emission =
            SubtensorModule::epoch(netuid, hardcoded_emission.saturating_to_num::<u64>().into());
        log::info!("hotkey_emission: {hotkey_emission:?}");
        let total_emission: I96F32 = hotkey_emission
            .iter()
            .map(|(_, _, emission)| I96F32::from_num(*emission))
            .sum();

        // Verify emissions match expected from CHK arrangements
        let em_eps: I96F32 = I96F32::from_num(1e-4); // 4 decimal places
        // A's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[0].2) / total_emission) -
            I96F32::from_num(3_f64 / 4_f64 * 1_f64 / 2_f64)).abs() // 3/4 * 1/2 = 3/8; 50% -> B
			<= em_eps,
            "A should have pending emission of 3/8 of total emission"
        );
        // B's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[1].2) / total_emission) -
            (I96F32::from_num(1_f64 / 4_f64 + 3_f64 / 4_f64 * 1_f64 / 2_f64))).abs() // 1/4 + 3/4 * 1/2 = 5/8; 50% from A
            <= em_eps,
            "B should have pending emission of 5/8 of total emission: {:?}",
            I96F32::from_num(hotkey_emission[1].2) / total_emission
        );

        // Get the distribution of dividends including the Parent/Child relationship.
        let dividends_a = SubtensorModule::get_parent_child_dividends_distribution(
            &hotkey_a,
            netuid,
            hardcoded_emission.saturating_to_num::<u64>().into(),
        );
        let dividends_b = SubtensorModule::get_parent_child_dividends_distribution(
            &hotkey_b,
            netuid,
            hardcoded_emission.saturating_to_num::<u64>().into(),
        );
        log::info!("dividends_a: {dividends_a:?}");
        log::info!("dividends_b: {dividends_b:?}");

        // We expect A should have no impact from B, as they have the same owner.
        assert_eq!(dividends_a.len(), 1);
        assert_eq!(dividends_a[0].0, hotkey_a);
        assert_eq!(
            dividends_a[0].1,
            hardcoded_emission.saturating_to_num::<u64>().into()
        );
        assert_abs_diff_eq!(
            dividends_a
                .iter()
                .map(|(_, emission)| u64::from(*emission))
                .sum::<u64>(),
            hardcoded_emission.saturating_to_num::<u64>(),
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );

        // Expect only 2 dividends. Parent key A and child key B.
        assert_eq!(dividends_b.len(), 2); // A and B
        assert_eq!(dividends_b[0].0, hotkey_a);
        assert_eq!(dividends_b[1].0, hotkey_b);

        // We expect B's coldkey to have no increase in dividends from A, as they have the same owner.
        // And therefore, B should get no CHK_TAKE.

        // A should also have no decrease because there is no CHK_TAKE.
        let total_stake_b = rel_stake_b + rel_stake_a * 1 / 2;
        let expected_b_b: u64 =
            (rel_stake_b / total_stake_b * hardcoded_emission).saturating_to_num::<u64>();

        assert_abs_diff_eq!(
            u64::from(dividends_b[1].1),
            expected_b_b,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>(),
        );

        let expected_b_a: u64 =
            ((rel_stake_a * 1 / 2) / total_stake_b * hardcoded_emission).saturating_to_num::<u64>();
        assert_eq!(dividends_b[0].0, hotkey_a);
        assert_abs_diff_eq!(
            u64::from(dividends_b[0].1),
            expected_b_a,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
        assert_abs_diff_eq!(
            dividends_b
                .iter()
                .map(|(_, emission)| u64::from(*emission))
                .sum::<u64>(),
            hardcoded_emission.saturating_to_num::<u64>(),
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
    });
}

#[test]
fn test_pending_cooldown_as_expected() {
    let curr_block = 1;
    // TODO: Fix when CHK splitting patched
    // let expected_cooldown = prod_or_fast!(7200, 15);

    new_test_ext(curr_block).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(1);
        let proportion1: u64 = 1000;
        let proportion2: u64 = 2000;
        let expected_cooldown = PendingChildKeyCooldown::<Test>::get();

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set multiple children
        mock_schedule_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(proportion1, child1), (proportion2, child2)],
        );

        // Verify pending map
        let pending_children = PendingChildKeys::<Test>::get(netuid, hotkey);
        assert_eq!(
            pending_children.0,
            vec![(proportion1, child1), (proportion2, child2)]
        );
        assert_eq!(pending_children.1, curr_block + expected_cooldown);
    });
}

#[test]
fn test_do_set_childkey_take_success() {
    new_test_ext(1).execute_with(|| {
        // Setup
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);
        let take = 5000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set childkey take
        assert_ok!(SubtensorModule::do_set_childkey_take(
            coldkey, hotkey, netuid, take
        ));

        // Verify the take was set correctly
        assert_eq!(SubtensorModule::get_childkey_take(&hotkey, netuid), take);
        let tx_type: u16 = TransactionType::SetChildkeyTake.into();
        assert_eq!(
            TransactionKeyLastBlock::<Test>::get((hotkey, netuid, tx_type,)),
            System::block_number()
        );
    });
}

#[test]
fn test_do_set_childkey_take_non_associated_coldkey() {
    new_test_ext(1).execute_with(|| {
        // Setup
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let hotkey2 = U256::from(3);
        let netuid = NetUid::from(1);
        let take = 5000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set childkey take
        assert_noop!(
            SubtensorModule::do_set_childkey_take(coldkey, hotkey2, netuid, take),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

#[test]
fn test_do_set_childkey_take_invalid_take_value() {
    new_test_ext(1).execute_with(|| {
        // Setup
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);
        let take = SubtensorModule::get_max_childkey_take() + 1;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set childkey take
        assert_noop!(
            SubtensorModule::do_set_childkey_take(coldkey, hotkey, netuid, take),
            Error::<Test>::InvalidChildkeyTake
        );
    });
}

#[test]
fn test_do_set_childkey_take_rate_limit_exceeded() {
    new_test_ext(1).execute_with(|| {
        // Setup
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);
        let initial_take = 3000;
        let higher_take = 5000;
        let lower_take = 1000;

        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set initial childkey take
        assert_ok!(SubtensorModule::do_set_childkey_take(
            coldkey,
            hotkey,
            netuid,
            initial_take
        ));

        // Try to increase the take value, should hit rate limit
        assert_noop!(
            SubtensorModule::do_set_childkey_take(coldkey, hotkey, netuid, higher_take),
            Error::<Test>::TxChildkeyTakeRateLimitExceeded
        );

        // lower take value should be ok
        assert_ok!(SubtensorModule::do_set_childkey_take(
            coldkey, hotkey, netuid, lower_take
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_set_child_keys_empty_vector_clears_storage --exact --show-output
#[test]
fn test_set_child_keys_empty_vector_clears_storage() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hotkey = U256::from(1001);
        let sn_owner_coldkey = U256::from(1002);
        let parent = U256::from(1);
        let child = U256::from(2);
        let netuid = add_dynamic_network(&sn_owner_hotkey, &sn_owner_coldkey);

        // Initialize ChildKeys for `parent` with a non-empty vector
        ChildKeys::<Test>::insert(parent, netuid, vec![(u64::MAX, child)]);
        ParentKeys::<Test>::insert(child, netuid, vec![(u64::MAX, parent)]);

        // Sanity: entry exists right now because we explicitly inserted it
        assert!(ChildKeys::<Test>::contains_key(parent, netuid));
        assert!(ParentKeys::<Test>::contains_key(child, netuid));

        // Set children to empty
        let empty_children: Vec<(u64, U256)> = Vec::new();
        mock_set_children_no_epochs(netuid, &parent, &empty_children);

        // When the child vector is empty, we should NOT keep an empty vec in storage.
        // The key must be fully removed (no entry), not just zero-length value.
        assert!(!ChildKeys::<Test>::contains_key(parent, netuid));
        assert!(!ParentKeys::<Test>::contains_key(child, netuid));

        // `get` returns empty due to ValueQuery default, but presence is false.
        assert!(ChildKeys::<Test>::get(parent, netuid).is_empty());
        assert!(ParentKeys::<Test>::get(child, netuid).is_empty());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_set_child_keys_no_start_call_sets_immediately --exact --show-output
#[test]
fn test_set_child_keys_no_start_call_sets_immediately() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(1);
        let proportion1: u64 = 1000;
        let proportion2: u64 = 2000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Clear SubtokenEnabled
        SubtokenEnabled::<Test>::remove(netuid);

        // Set multiple children
        mock_schedule_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(proportion1, child1), (proportion2, child2)],
        );

        // Normally happens on epoch
        SubtensorModule::do_set_pending_children(netuid);

        // Verify pending map is empty
        assert!(!PendingChildKeys::<Test>::contains_key(netuid, hotkey));

        // Verify that childkey is set
        assert_eq!(
            ChildKeys::<Test>::get(hotkey, netuid),
            vec![(proportion1, child1), (proportion2, child2)]
        );
    });
}

// Test that the subnet owner can always set weights (owner bypass in check_weights_min_stake)
// and that do_set_root_validators_for_subnet correctly creates parent-child relationships.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_root_children_enable_subnet_owner_set_weights --exact --show-output --nocapture
#[test]
fn test_root_children_enable_subnet_owner_set_weights() {
    new_test_ext(1).execute_with(|| {
        // --- Setup accounts ---
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);

        let root_val_coldkey_1 = U256::from(100);
        let root_val_hotkey_1 = U256::from(101);
        let root_val_coldkey_2 = U256::from(200);
        let root_val_hotkey_2 = U256::from(201);

        // --- Create root network and subnet ---
        add_network(NetUid::ROOT, 1, 0);
        let netuid =
            add_dynamic_network_disable_commit_reveal(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // --- Register root validators on a subnet first (required before root_register) ---
        register_ok_neuron(netuid, root_val_hotkey_1, root_val_coldkey_1, 0);
        register_ok_neuron(netuid, root_val_hotkey_2, root_val_coldkey_2, 0);

        // --- Register root validators on root network ---
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(root_val_coldkey_1),
            root_val_hotkey_1,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(root_val_coldkey_2),
            root_val_hotkey_2,
        ));

        // --- Add stake for root validators on root and the subnet ---
        let root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_1,
            &root_val_coldkey_1,
            NetUid::ROOT,
            root_stake,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_1,
            &root_val_coldkey_1,
            netuid,
            root_stake,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_2,
            &root_val_coldkey_2,
            NetUid::ROOT,
            root_stake,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_2,
            &root_val_coldkey_2,
            netuid,
            root_stake,
        );

        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        let version_key = SubtensorModule::get_weights_version_key(netuid);
        let uids: Vec<u16> = vec![0];
        let values: Vec<u16> = vec![u16::MAX];

        // Subnet owner can set weights with default (zero) stake threshold.
        assert!(
            SubtensorModule::check_weights_min_stake(&subnet_owner_hotkey, netuid),
            "Subnet owner should pass the min stake check with default threshold"
        );
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(subnet_owner_hotkey),
            netuid,
            uids.clone(),
            values.clone(),
            version_key
        ));

        // Subnet owner can still set weights after raising the stake threshold (owner bypass).
        SubtensorModule::set_stake_threshold(500_000_000u64);
        assert!(
            SubtensorModule::check_weights_min_stake(&subnet_owner_hotkey, netuid),
            "Subnet owner should pass the min stake check even with high threshold"
        );
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(subnet_owner_hotkey),
            netuid,
            uids.clone(),
            values.clone(),
            version_key
        ));

        // --- Verify do_set_root_validators_for_subnet creates parent-child relationships ---
        assert_ok!(SubtensorModule::set_pending_childkey_cooldown(
            RuntimeOrigin::root(),
            0,
        ));

        assert_ok!(SubtensorModule::do_set_root_validators_for_subnet(netuid));

        // Activate pending children (cooldown is 0, advance 1 block)
        step_block(1);
        SubtensorModule::do_set_pending_children(netuid);

        // Each root validator should have the subnet owner hotkey as a child on netuid
        let children_1 = SubtensorModule::get_children(&root_val_hotkey_1, netuid);
        assert_eq!(
            children_1,
            vec![(u64::MAX, subnet_owner_hotkey)],
            "Root validator 1 should have subnet owner as child"
        );
        let children_2 = SubtensorModule::get_children(&root_val_hotkey_2, netuid);
        assert_eq!(
            children_2,
            vec![(u64::MAX, subnet_owner_hotkey)],
            "Root validator 2 should have subnet owner as child"
        );

        // Subnet owner should have both root validators as parents
        let parents = SubtensorModule::get_parents(&subnet_owner_hotkey, netuid);
        assert_eq!(parents.len(), 2, "Subnet owner should have 2 parents");
    });
}

// Test that register_network automatically sets root validators as parents of the
// subnet owner, enabling the owner to set weights. Since SubtokenEnabled is false
// for a new subnet (start_call hasn't executed yet), child keys are applied immediately.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_register_network_schedules_root_validators --exact --show-output --nocapture
#[test]
fn test_register_network_schedules_root_validators() {
    new_test_ext(1).execute_with(|| {
        // --- Setup root network and root validators ---
        let root_val_coldkey_1 = U256::from(100);
        let root_val_hotkey_1 = U256::from(101);
        let root_val_coldkey_2 = U256::from(200);
        let root_val_hotkey_2 = U256::from(201);

        add_network(NetUid::ROOT, 1, 0);

        // Root validators need to be registered on some subnet before root_register.
        // Create a bootstrap subnet for that purpose.
        let bootstrap_netuid = NetUid::from(1);
        add_network(bootstrap_netuid, 1, 0);
        register_ok_neuron(bootstrap_netuid, root_val_hotkey_1, root_val_coldkey_1, 0);
        register_ok_neuron(bootstrap_netuid, root_val_hotkey_2, root_val_coldkey_2, 0);

        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(root_val_coldkey_1),
            root_val_hotkey_1,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(root_val_coldkey_2),
            root_val_hotkey_2,
        ));

        // Give root validators significant stake on root and bootstrap subnet
        let root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_1,
            &root_val_coldkey_1,
            NetUid::ROOT,
            root_stake,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_2,
            &root_val_coldkey_2,
            NetUid::ROOT,
            root_stake,
        );

        // --- Minimize cooldown so pending children activate quickly ---
        assert_ok!(SubtensorModule::set_pending_childkey_cooldown(
            RuntimeOrigin::root(),
            0,
        ));

        // --- Set a high stake threshold ---
        let high_threshold = 500_000_000u64;
        SubtensorModule::set_stake_threshold(high_threshold);

        // --- Register a new subnet (this should automatically call do_set_root_validators_for_subnet) ---
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let lock_cost = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&subnet_owner_coldkey, lock_cost.into());
        TotalIssuance::<Test>::mutate(|total| {
            *total = total.saturating_add(lock_cost);
        });
        assert_ok!(SubtensorModule::register_network(
            RuntimeOrigin::signed(subnet_owner_coldkey),
            subnet_owner_hotkey,
        ));

        // Determine the netuid that was just created
        let netuid: NetUid = (TotalNetworks::<Test>::get().saturating_sub(1)).into();
        assert_eq!(
            SubnetOwnerHotkey::<Test>::get(netuid),
            subnet_owner_hotkey,
            "Subnet owner hotkey should be set"
        );

        // Root validators need stake on the new subnet for child stake inheritance to work
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_1,
            &root_val_coldkey_1,
            netuid,
            root_stake,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_2,
            &root_val_coldkey_2,
            netuid,
            root_stake,
        );

        // --- Verify child keys were applied immediately (SubtokenEnabled is false for new subnets) ---
        let children_1 = SubtensorModule::get_children(&root_val_hotkey_1, netuid);
        assert_eq!(
            children_1,
            vec![(u64::MAX, subnet_owner_hotkey)],
            "Root validator 1 should have subnet owner as child"
        );
        let children_2 = SubtensorModule::get_children(&root_val_hotkey_2, netuid);
        assert_eq!(
            children_2,
            vec![(u64::MAX, subnet_owner_hotkey)],
            "Root validator 2 should have subnet owner as child"
        );

        // --- Verify subnet owner can now set weights ---
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        let version_key = SubtensorModule::get_weights_version_key(netuid);

        assert!(
            SubtensorModule::check_weights_min_stake(&subnet_owner_hotkey, netuid),
            "Subnet owner should have enough inherited stake to set weights"
        );
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(subnet_owner_hotkey),
            netuid,
            vec![0],
            vec![u16::MAX],
            version_key
        ));
    });
}

// Test that register_network automatically sets root validators as parents of the
// subnet owner, only if AutoParentDelegationEnabled is enabled (default).
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::test_register_network_schedules_root_validators_auto_parent_delegation_flag --exact --show-output --nocapture
#[test]
fn test_register_network_schedules_root_validators_auto_parent_delegation_flag() {
    new_test_ext(1).execute_with(|| {
        // --- Setup root network and root validators ---
        let root_val_coldkey_1 = U256::from(100);
        let root_val_hotkey_1 = U256::from(101);
        let root_val_coldkey_2 = U256::from(200);
        let root_val_hotkey_2 = U256::from(201);

        add_network(NetUid::ROOT, 1, 0);

        // Root validators need to be registered on some subnet before root_register.
        // Create a bootstrap subnet for that purpose.
        let bootstrap_netuid = NetUid::from(1);
        add_network(bootstrap_netuid, 1, 0);
        register_ok_neuron(bootstrap_netuid, root_val_hotkey_1, root_val_coldkey_1, 0);
        register_ok_neuron(bootstrap_netuid, root_val_hotkey_2, root_val_coldkey_2, 0);

        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(root_val_coldkey_1),
            root_val_hotkey_1,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(root_val_coldkey_2),
            root_val_hotkey_2,
        ));

        // Give root validators significant stake on root and bootstrap subnet
        let root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_1,
            &root_val_coldkey_1,
            NetUid::ROOT,
            root_stake,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_2,
            &root_val_coldkey_2,
            NetUid::ROOT,
            root_stake,
        );

        // --- Minimize cooldown so pending children activate quickly ---
        assert_ok!(SubtensorModule::set_pending_childkey_cooldown(
            RuntimeOrigin::root(),
            0,
        ));

        // --- Set a high stake threshold ---
        let high_threshold = 500_000_000u64;
        SubtensorModule::set_stake_threshold(high_threshold);

        // --- Register a new subnet (this should automatically call do_set_root_validators_for_subnet) ---
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let lock_cost = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&subnet_owner_coldkey, lock_cost.into());
        TotalIssuance::<Test>::mutate(|total| {
            *total = total.saturating_add(lock_cost);
        });

        assert_ok!(SubtensorModule::set_auto_parent_delegation_enabled(
            RuntimeOrigin::signed(root_val_coldkey_1),
            root_val_hotkey_1,
            false,
        ));

        assert_ok!(SubtensorModule::register_network(
            RuntimeOrigin::signed(subnet_owner_coldkey),
            subnet_owner_hotkey,
        ));

        // Determine the netuid that was just created
        let netuid: NetUid = (TotalNetworks::<Test>::get().saturating_sub(1)).into();
        assert_eq!(
            SubnetOwnerHotkey::<Test>::get(netuid),
            subnet_owner_hotkey,
            "Subnet owner hotkey should be set"
        );

        // Root validators need stake on the new subnet for child stake inheritance to work
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_1,
            &root_val_coldkey_1,
            netuid,
            root_stake,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_val_hotkey_2,
            &root_val_coldkey_2,
            netuid,
            root_stake,
        );

        // --- Verify child keys were applied immediately (SubtokenEnabled is false for new subnets) ---
        let children_1 = SubtensorModule::get_children(&root_val_hotkey_1, netuid);
        assert_eq!(
            children_1,
            vec![],
            "Root validator 1 not have subnet owner as a child because AutoParentDelegationEnabled is false"
        );
        let children_2 = SubtensorModule::get_children(&root_val_hotkey_2, netuid);
        assert_eq!(
            children_2,
            vec![(u64::MAX, subnet_owner_hotkey)],
            "Root validator 2 should have subnet owner as child"
        );

        // --- Verify subnet owner can now set weights ---
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        let version_key = SubtensorModule::get_weights_version_key(netuid);

        assert!(
            SubtensorModule::check_weights_min_stake(&subnet_owner_hotkey, netuid),
            "Subnet owner should have enough inherited stake to set weights"
        );
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(subnet_owner_hotkey),
            netuid,
            vec![0],
            vec![u16::MAX],
            version_key
        ));
    });
}
