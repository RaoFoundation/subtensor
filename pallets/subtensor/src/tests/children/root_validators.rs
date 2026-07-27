#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
use super::super::mock::*;
use frame_support::assert_ok;
use subtensor_runtime_common::AlphaBalance;

use crate::*;
use sp_core::U256;

// Test that register_network automatically sets root validators as parents of the
// subnet owner, enabling the owner to set weights. Since SubtokenEnabled is false
// for a new subnet (start_call hasn't executed yet), child keys are applied immediately.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::root_validators::test_register_network_schedules_root_validators --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::root_validators::test_register_network_schedules_root_validators_auto_parent_delegation_flag --exact --show-output --nocapture
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
