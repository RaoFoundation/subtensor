#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
use super::super::mock::*;
use frame_support::assert_err;

use crate::{utils::rate_limiting::TransactionType, *};
use sp_core::U256;

// 16: Test setting multiple children successfully
// This test verifies that multiple children can be set for a parent successfully:
// - Sets up a network and registers a hotkey
// - Sets multiple children with different proportions
// - Verifies that the children are correctly assigned to the parent
// - Checks that the parent is correctly assigned to each child
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_schedule_children_multiple_success --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_schedule_children_multiple_network_does_not_exist --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_schedule_children_multiple_invalid_child --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_schedule_children_multiple_non_associated_coldkey --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_schedule_children_multiple_root_network --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_schedule_children_multiple_old_children_cleanup --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_schedule_children_multiple_proportion_edge_cases --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_schedule_children_multiple_overwrite_existing --exact --show-output --nocapture
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

// 27: Test setting children with an empty list
// This test verifies the behavior of setting an empty children list:
// - Adds a network and registers a hotkey
// - Sets an empty children list for the hotkey
// - Verifies that the children assignment is empty
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_schedule_children_multiple_empty_list --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_revoke_children_multiple_success --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_revoke_children_multiple_network_does_not_exist --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_revoke_children_multiple_non_associated_coldkey --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_revoke_children_multiple_partial_revocation --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_revoke_children_multiple_non_existent_children --exact --show-output --nocapture
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
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_revoke_children_multiple_empty_list --exact --show-output --nocapture
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
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_do_revoke_children_multiple_complex_scenario --exact --show-output --nocapture
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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_set_child_keys_empty_vector_clears_storage --exact --show-output
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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::schedule_multiple::test_set_child_keys_no_start_call_sets_immediately --exact --show-output
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
