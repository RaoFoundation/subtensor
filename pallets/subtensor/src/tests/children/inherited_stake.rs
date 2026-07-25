#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
use super::super::mock::*;

use crate::*;
use sp_core::U256;

use super::helpers::close;

// 11: Test getting stake for a hotkey on a subnet
// This test verifies the correct calculation of stake for a parent and child neuron:
// - Sets up a network with a parent and child neuron
// - Stakes tokens to both parent and child from different coldkeys
// - Establishes a parent-child relationship with 100% stake allocation
// - Checks that the parent's stake is correctly transferred to the child
// - Ensures the total stake is preserved in the system
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_get_stake_for_hotkey_on_subnet --exact --show-output --nocapture
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

// 39: Test children stake values
// This test verifies the correct distribution of stake among parent and child neurons:
// - Sets up a network with a parent neuron and multiple child neurons
// - Assigns stake to the parent neuron
// - Sets child neurons with specific proportions
// - Verifies that the stake is correctly distributed among parent and child neurons
// - Checks that the total stake remains constant across all neurons
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_children_stake_values --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_get_parents_chain --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_get_stake_for_hotkey_on_subnet_basic --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_get_stake_for_hotkey_on_subnet_multiple_coldkeys --exact --show-output --nocapture
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
/// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_get_stake_for_hotkey_on_subnet_single_parent_child --exact --show-output --nocapture
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
/// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_get_stake_for_hotkey_on_subnet_multiple_parents_single_child --exact --show-output --nocapture
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
/// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_get_stake_for_hotkey_on_subnet_single_parent_multiple_children --exact --show-output --nocapture
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
/// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_get_stake_for_hotkey_on_subnet_edge_cases --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_get_stake_for_hotkey_on_subnet_complex_hierarchy --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::inherited_stake::test_get_stake_for_hotkey_on_subnet_multiple_networks --exact --show-output --nocapture
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
