#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
use super::super::mock::*;
use frame_support::{assert_noop, assert_ok};
use substrate_fixed::types::I64F64;
use subtensor_runtime_common::{AlphaBalance, TaoBalance};

use crate::*;
use sp_core::U256;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::child_weights::test_childkey_set_weights_single_parent --exact --show-output --nocapture
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

// Test that the subnet owner can always set weights (owner bypass in check_weights_min_stake)
// and that do_set_root_validators_for_subnet correctly creates parent-child relationships.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::child_weights::test_root_children_enable_subnet_owner_set_weights --exact --show-output --nocapture
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
