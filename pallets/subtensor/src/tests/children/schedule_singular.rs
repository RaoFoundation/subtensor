#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
use super::super::mock::*;
use frame_support::assert_err;

use crate::{utils::rate_limiting::TransactionType, *};
use sp_core::U256;

// 1: Successful setting of a single child
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_set_child_singular_success --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_set_child_singular_network_does_not_exist --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_set_child_singular_invalid_child --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_set_child_singular_non_associated_coldkey --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_set_child_singular_root_network --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_set_child_singular_old_children_cleanup --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_set_child_singular_new_children_assignment --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_set_child_singular_proportion_edge_cases --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_set_child_singular_multiple_children --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_add_singular_child --exact --show-output --nocapture
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

// 12: Test revoking a singular child successfully
// This test checks the process of revoking a child neuron:
// - Sets up a network with a parent and child neuron
// - Establishes a parent-child relationship
// - Revokes the child relationship
// - Verifies that the child is removed from the parent's children list
// - Ensures the parent is removed from the child's parents list
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_revoke_child_singular_success --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_set_empty_children_network_does_not_exist --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_revoke_child_singular_non_associated_coldkey --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_singular::test_do_revoke_child_singular_child_not_associated --exact --show-output --nocapture
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
