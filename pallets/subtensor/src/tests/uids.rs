#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use super::mock::*;
use crate::*;
use frame_support::{assert_err, assert_ok};
use sp_core::{H160, U256};
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex};

/********************************************
    tests for uids.rs file
*********************************************/

/********************************************
    tests uids::replace_neuron()
*********************************************/

#[test]
fn test_replace_neuron() {
    new_test_ext(1).execute_with(|| {
        let block_number: u64 = 0;
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let hotkey_account_id = U256::from(1);
        let coldkey_account_id = U256::from(1234);

        let new_hotkey_account_id = U256::from(2);
        let _new_colkey_account_id = U256::from(12345);
        let evm_address = H160::from_slice(&[1_u8; 20]);

        System::set_block_number(block_number);

        // Add network.
        add_network(netuid, tempo, 0);

        // Register a neuron.
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 0);

        // Get UID.
        let neuron_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id);
        assert_ok!(neuron_uid);
        let neuron_uid = neuron_uid.unwrap();

        // set non-default values
        Emission::<Test>::mutate(netuid, |v| {
            SubtensorModule::set_element_at(v, neuron_uid as usize, 5.into())
        });
        Consensus::<Test>::mutate(netuid, |v| {
            SubtensorModule::set_element_at(v, neuron_uid as usize, 5u16)
        });
        Incentive::<Test>::mutate(NetUidStorageIndex::from(netuid), |v| {
            SubtensorModule::set_element_at(v, neuron_uid as usize, 5u16)
        });
        Dividends::<Test>::mutate(netuid, |v| {
            SubtensorModule::set_element_at(v, neuron_uid as usize, 5u16)
        });
        Bonds::<Test>::insert(NetUidStorageIndex::from(netuid), neuron_uid, vec![(0, 1)]);

        Axons::<Test>::insert(netuid, hotkey_account_id, AxonInfoOf::default());
        NeuronCertificates::<Test>::insert(
            netuid,
            hotkey_account_id,
            NeuronCertificateOf::default(),
        );
        Prometheus::<Test>::insert(netuid, hotkey_account_id, PrometheusInfoOf::default());
        AssociatedEvmAddress::<Test>::insert(netuid, neuron_uid, (evm_address, 1));

        // Replace the neuron.
        SubtensorModule::replace_neuron(netuid, neuron_uid, &new_hotkey_account_id, block_number);

        // Check old hotkey is not registered on any network.
        assert!(SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id).is_err());
        assert!(!SubtensorModule::is_hotkey_registered_on_any_network(
            &hotkey_account_id
        ));

        let curr_hotkey = SubtensorModule::get_hotkey_for_net_and_uid(netuid, neuron_uid);
        assert_ok!(curr_hotkey);
        assert_ne!(curr_hotkey.unwrap(), hotkey_account_id);

        // Check new hotkey is registered on the network.
        assert!(
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &new_hotkey_account_id).is_ok()
        );
        assert!(SubtensorModule::is_hotkey_registered_on_any_network(
            &new_hotkey_account_id
        ));
        assert_eq!(curr_hotkey.unwrap(), new_hotkey_account_id);

        // Check neuron certificate was reset.
        let certificate = NeuronCertificates::<Test>::get(netuid, hotkey_account_id);
        assert_eq!(certificate, None);

        // Check trust, emission, consensus, incentive, dividends have been reset to 0.
        assert_eq!(SubtensorModule::get_trust_for_uid(netuid, neuron_uid), 0);
        assert_eq!(
            SubtensorModule::get_emission_for_uid(netuid, neuron_uid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_consensus_for_uid(netuid, neuron_uid),
            0
        );
        assert_eq!(
            SubtensorModule::get_incentive_for_uid(netuid.into(), neuron_uid),
            0
        );
        assert_eq!(
            SubtensorModule::get_dividends_for_uid(netuid, neuron_uid),
            0
        );

        // Check axon info is reset.
        assert!(!Axons::<Test>::contains_key(netuid, curr_hotkey.unwrap()));
        assert!(!NeuronCertificates::<Test>::contains_key(
            netuid,
            curr_hotkey.unwrap()
        ));
        assert!(!Prometheus::<Test>::contains_key(
            netuid,
            curr_hotkey.unwrap()
        ));

        // Check bonds are cleared.
        assert_eq!(
            Bonds::<Test>::get(NetUidStorageIndex::from(netuid), neuron_uid),
            vec![]
        );
        assert_eq!(AssociatedEvmAddress::<Test>::get(netuid, neuron_uid), None);
    });
}

#[test]
fn test_bonds_cleared_on_replace() {
    new_test_ext(1).execute_with(|| {
        let block_number: u64 = 0;
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let hotkey_account_id = U256::from(1);
        let coldkey_account_id = U256::from(1234);

        let new_hotkey_account_id = U256::from(2);
        let _new_colkey_account_id = U256::from(12345);
        let evm_address = H160::from_slice(&[1_u8; 20]);

        System::set_block_number(block_number);

        // Add network.
        add_network(netuid, tempo, 0);

        // Register a neuron.
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 0);

        // Get UID.
        let neuron_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id);
        assert_ok!(neuron_uid);
        let neuron_uid = neuron_uid.unwrap();
        AssociatedEvmAddress::<Test>::insert(netuid, neuron_uid, (evm_address, 1));

        // Set non-default bonds.
        Bonds::<Test>::insert(NetUidStorageIndex::from(netuid), neuron_uid, vec![(0, 1)]);

        // Replace the neuron.
        SubtensorModule::replace_neuron(netuid, neuron_uid, &new_hotkey_account_id, block_number);

        // Check old hotkey is not registered on any network.
        assert!(SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id).is_err());
        assert!(!SubtensorModule::is_hotkey_registered_on_any_network(
            &hotkey_account_id
        ));

        let curr_hotkey = SubtensorModule::get_hotkey_for_net_and_uid(netuid, neuron_uid);
        assert_ok!(curr_hotkey);
        assert_ne!(curr_hotkey.unwrap(), hotkey_account_id);

        // Check new hotkey is registered on the network.
        assert!(
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &new_hotkey_account_id).is_ok()
        );
        assert!(SubtensorModule::is_hotkey_registered_on_any_network(
            &new_hotkey_account_id
        ));
        assert_eq!(curr_hotkey.unwrap(), new_hotkey_account_id);

        // Check bonds are cleared.
        assert_eq!(
            Bonds::<Test>::get(NetUidStorageIndex::from(netuid), neuron_uid),
            vec![]
        );
        assert_eq!(AssociatedEvmAddress::<Test>::get(netuid, neuron_uid), None);
    });
}

#[test]
fn test_replace_neuron_resets_last_update() {
    new_test_ext(1).execute_with(|| {
        let registration_block: u64 = 0;
        let replacement_block: u64 = 123;
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let hotkey_account_id = U256::from(1);
        let coldkey_account_id = U256::from(1234);
        let new_hotkey_account_id = U256::from(2);

        System::set_block_number(registration_block);
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 0);

        let neuron_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id).unwrap();
        let netuid_index = NetUidStorageIndex::from(netuid);

        LastUpdate::<Test>::insert(netuid_index, vec![7]);

        SubtensorModule::replace_neuron(
            netuid,
            neuron_uid,
            &new_hotkey_account_id,
            replacement_block,
        );

        assert_eq!(
            LastUpdate::<Test>::get(netuid_index)[neuron_uid as usize],
            replacement_block
        );
    });
}

#[test]
fn test_replace_neuron_clears_validator_trust_and_permit() {
    new_test_ext(1).execute_with(|| {
        let registration_block: u64 = 0;
        let replacement_block: u64 = 123;
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let hotkey_account_id = U256::from(1);
        let coldkey_account_id = U256::from(1234);
        let new_hotkey_account_id = U256::from(2);

        System::set_block_number(registration_block);
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 0);

        let neuron_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id).unwrap();
        let idx = neuron_uid as usize;

        // Simulate the previous occupant having earned a validator_permit and trust score.
        ValidatorTrust::<Test>::mutate(netuid, |v| {
            if v.len() <= idx {
                v.resize(idx + 1, 0);
            }
            v[idx] = 42;
        });
        ValidatorPermit::<Test>::mutate(netuid, |v| {
            if v.len() <= idx {
                v.resize(idx + 1, false);
            }
            v[idx] = true;
        });

        SubtensorModule::replace_neuron(
            netuid,
            neuron_uid,
            &new_hotkey_account_id,
            replacement_block,
        );

        // The replaced neuron must not inherit the previous occupant's validator state.
        assert_eq!(ValidatorTrust::<Test>::get(netuid)[idx], 0);
        assert!(!ValidatorPermit::<Test>::get(netuid)[idx]);
    });
}

#[test]
fn test_replace_neuron_multiple_subnets() {
    new_test_ext(1).execute_with(|| {
        let block_number: u64 = 0;
        let netuid = NetUid::from(1);
        let netuid1 = NetUid::from(2);
        let tempo: u16 = 13;
        let hotkey_account_id = U256::from(1);
        let new_hotkey_account_id = U256::from(2);

        let coldkey_account_id = U256::from(1234);

        let _new_colkey_account_id = U256::from(12345);
        let evm_address = H160::from_slice(&[1_u8; 20]);

        // Make sure the environment is at the expected block.
        System::set_block_number(block_number);

        // Add networks.
        add_network(netuid, tempo, 0);
        add_network(netuid1, tempo, 0);

        // Register a neuron on both networks.
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 0);
        register_ok_neuron(netuid1, hotkey_account_id, coldkey_account_id, 0);

        // Get UID.
        let neuron_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id);
        assert_ok!(neuron_uid);

        // Verify neuron is registered on both networks.
        assert!(SubtensorModule::is_hotkey_registered_on_network(
            netuid,
            &hotkey_account_id
        ));
        assert!(SubtensorModule::is_hotkey_registered_on_network(
            netuid1,
            &hotkey_account_id
        ));
        assert!(SubtensorModule::is_hotkey_registered_on_any_network(
            &hotkey_account_id
        ));

        AssociatedEvmAddress::<Test>::insert(netuid, neuron_uid.unwrap(), (evm_address, 1));

        // Replace the neuron (ONLY on ONE network).
        SubtensorModule::replace_neuron(
            netuid,
            neuron_uid.unwrap(),
            &new_hotkey_account_id,
            block_number,
        );

        // Check old hotkey is not registered on netuid network.
        assert!(SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id).is_err());

        // Verify still registered on netuid1 network.
        assert!(SubtensorModule::is_hotkey_registered_on_any_network(
            &hotkey_account_id
        ));
        assert!(SubtensorModule::is_hotkey_registered_on_network(
            netuid1,
            &hotkey_account_id
        ));

        assert_eq!(
            AssociatedEvmAddress::<Test>::get(netuid, neuron_uid.unwrap()),
            None
        );
    });
}

#[test]
fn test_neuron_certificate() {
    new_test_ext(1).execute_with(|| {
        // 512 bits key
        let mut data = [0; 65].to_vec();
        assert_ok!(NeuronCertificate::try_from(data));

        // 256 bits key
        data = [1; 33].to_vec();
        assert_ok!(NeuronCertificate::try_from(data));

        // too much data
        data = [8; 88].to_vec();
        assert_err!(NeuronCertificate::try_from(data), ());

        // no data
        data = vec![];
        assert_err!(NeuronCertificate::try_from(data), ());
    });
}

#[test]
fn test_replace_neuron_subnet_owner_not_replaced() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(100);
        let owner_coldkey = U256::from(999);
        let new_hotkey_account_id = U256::from(2);
        let evm_address = H160::from_slice(&[1_u8; 20]);

        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        let neuron_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &owner_hotkey)
            .expect("Owner neuron should be registered by add_dynamic_network");
        AssociatedEvmAddress::<Test>::insert(netuid, neuron_uid, (evm_address, 1));
        let current_block = SubtensorModule::get_current_block_as_u64();
        SubtensorModule::replace_neuron(netuid, neuron_uid, &new_hotkey_account_id, current_block);

        let still_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &owner_hotkey);
        assert_ok!(still_uid);
        assert_eq!(
            still_uid.unwrap(),
            neuron_uid,
            "UID should remain unchanged for subnet owner"
        );

        let new_key_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &new_hotkey_account_id);
        assert_err!(new_key_uid, Error::<Test>::HotKeyNotRegisteredInSubNet,);
        assert!(AssociatedEvmAddress::<Test>::get(netuid, neuron_uid).is_some());
    });
}

#[test]
fn test_replace_neuron_subnet_owner_not_replaced_if_in_sn_owner_hotkey_map() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(123);
        let owner_coldkey = U256::from(999);
        let other_owner_hotkey = U256::from(456);
        let evm_address = H160::from_slice(&[1_u8; 20]);

        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        SubtensorModule::set_max_registrations_per_block(netuid, 100);
        SubtensorModule::set_target_registrations_per_interval(netuid, 100);
        SubnetOwner::<Test>::insert(netuid, owner_coldkey);

        let owner_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &owner_hotkey)
            .expect("Owner neuron should already be registered by add_dynamic_network");

        AssociatedEvmAddress::<Test>::insert(netuid, owner_uid, (evm_address, 1));

        // Register another hotkey for the owner
        register_ok_neuron(netuid, other_owner_hotkey, owner_coldkey, 0);
        let other_owner_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &other_owner_hotkey)
                .expect("Should be registered");

        let additional_hotkey_1 = U256::from(1000);
        let additional_hotkey_2 = U256::from(1001);

        let current_block = SubtensorModule::get_current_block_as_u64();
        SubtensorModule::replace_neuron(netuid, owner_uid, &additional_hotkey_1, current_block);

        let still_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &owner_hotkey);
        assert_ok!(still_uid);
        assert_eq!(
            still_uid.unwrap(),
            owner_uid,
            "Owner's first hotkey should remain registered"
        );
        assert!(AssociatedEvmAddress::<Test>::get(netuid, owner_uid).is_some());

        let new_key_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &additional_hotkey_1);
        assert_err!(new_key_uid, Error::<Test>::HotKeyNotRegisteredInSubNet,);
        AssociatedEvmAddress::<Test>::insert(netuid, other_owner_uid, (evm_address, 1));

        // Try to replace the other owner hotkey
        SubtensorModule::replace_neuron(
            netuid,
            other_owner_uid,
            &additional_hotkey_1,
            current_block,
        );
        let still_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &other_owner_hotkey);
        assert_err!(still_uid, Error::<Test>::HotKeyNotRegisteredInSubNet,); // Was replaced
        assert!(AssociatedEvmAddress::<Test>::get(netuid, other_owner_uid).is_none());

        // Re-register this hotkey
        register_ok_neuron(netuid, other_owner_hotkey, owner_coldkey, 0);
        let _other_owner_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &other_owner_hotkey)
                .expect("Should be registered");

        // Set this hotkey as the SubnetOwnerHotkey
        SubnetOwnerHotkey::<Test>::insert(netuid, other_owner_hotkey);

        SubtensorModule::replace_neuron(netuid, owner_uid, &additional_hotkey_2, current_block);

        // The owner's first hotkey should be replaceable; it's not the top-stake owner hotkey
        let still_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &owner_hotkey);
        assert_err!(still_uid, Error::<Test>::HotKeyNotRegisteredInSubNet,);

        let new_key_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &additional_hotkey_2);
        assert_ok!(new_key_uid);
        assert!(AssociatedEvmAddress::<Test>::get(netuid, owner_uid).is_none());
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn test_get_neuron_to_prune_owner_not_pruned() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(123);
        let owner_coldkey = U256::from(999);

        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        SubtensorModule::set_max_registrations_per_block(netuid, 100);
        SubtensorModule::set_target_registrations_per_interval(netuid, 100);
        SubnetOwner::<Test>::insert(netuid, owner_coldkey);

        // Ensure owner's hotkey is counted as immortal in this test
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 1);

        // Neutralize safety floor for this test
        SubtensorModule::set_min_non_immune_uids(netuid, 0);

        let owner_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &owner_hotkey)
            .expect("Owner neuron should already be registered by add_dynamic_network");

        let additional_hotkey_1 = U256::from(1000);
        let additional_coldkey_1 = U256::from(2000);

        let additional_hotkey_2 = U256::from(1001);
        let additional_coldkey_2 = U256::from(2001);

        register_ok_neuron(netuid, additional_hotkey_1, additional_coldkey_1, 0);
        let uid_1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &additional_hotkey_1)
            .expect("Should be registered");

        register_ok_neuron(netuid, additional_hotkey_2, additional_coldkey_2, 1);
        let uid_2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &additional_hotkey_2)
            .expect("Should be registered");

        // Set emissions; owner has the lowest but is immortal, so choose uid_1
        Emission::<Test>::mutate(netuid, |v| {
            v[owner_uid as usize] = 0u64.into();
            v[uid_1 as usize] = 1u64.into();
            v[uid_2 as usize] = 2u64.into();
        });

        let pruned_uid = SubtensorModule::get_neuron_to_prune(netuid);

        // Expect to prune uid_1; owner's UID is skipped as immortal.
        assert_eq!(
            pruned_uid,
            Some(uid_1),
            "Should prune the neuron with the lowest emission among non-immortal candidates."
        );
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn test_get_neuron_to_prune_owner_pruned_if_not_in_sn_owner_hotkey_map() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(123);
        let owner_coldkey = U256::from(999);
        let other_owner_hotkey = U256::from(456);

        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        SubtensorModule::set_max_registrations_per_block(netuid, 100);
        SubtensorModule::set_target_registrations_per_interval(netuid, 100);
        SubnetOwner::<Test>::insert(netuid, owner_coldkey);

        // Make only one owner hotkey immortal at a time
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 1);

        // Neutralize safety floor for this test
        SubtensorModule::set_min_non_immune_uids(netuid, 0);

        let owner_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &owner_hotkey)
            .expect("Owner neuron should already be registered by add_dynamic_network");

        // Register another hotkey for the owner (same coldkey)
        register_ok_neuron(netuid, other_owner_hotkey, owner_coldkey, 0);
        let other_owner_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &other_owner_hotkey)
                .expect("Should be registered");

        let additional_hotkey_1 = U256::from(1000);
        let additional_coldkey_1 = U256::from(2000);

        let additional_hotkey_2 = U256::from(1001);
        let additional_coldkey_2 = U256::from(2001);

        register_ok_neuron(netuid, additional_hotkey_1, additional_coldkey_1, 1);
        let uid_2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &additional_hotkey_1)
            .expect("Should be registered");

        register_ok_neuron(netuid, additional_hotkey_2, additional_coldkey_2, 2);
        let uid_3 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &additional_hotkey_2)
            .expect("Should be registered");

        // Case 1: With ImmuneOwnerUidsLimit = 1, the default SubnetOwnerHotkey is `owner_hotkey`.
        //         That makes `owner_uid` immortal and leaves `other_owner_uid` pruneable.
        // Emissions: owner=0 (immortal), other_owner=1, uid_2=2, uid_3=3 -> expect other_owner_uid
        Emission::<Test>::mutate(netuid, |v| {
            v[owner_uid as usize] = 0u64.into();
            v[other_owner_uid as usize] = 1u64.into();
            v[uid_2 as usize] = 2u64.into();
            v[uid_3 as usize] = 3u64.into();
        });

        let pruned_uid = SubtensorModule::get_neuron_to_prune(netuid);
        assert_eq!(pruned_uid, Some(other_owner_uid), "Should prune the owner");

        // Case 2: Make the other owner's hotkey the SubnetOwnerHotkey, so it becomes the prioritized
        //         immortal one; now `owner_uid` is not immortal and has the lowest emission -> prune it.
        SubnetOwnerHotkey::<Test>::insert(netuid, other_owner_hotkey);

        // Emissions remain the same; `owner_uid` now becomes the lowest non-immortal candidate.
        let pruned_uid = SubtensorModule::get_neuron_to_prune(netuid);
        assert_eq!(
            pruned_uid,
            Some(owner_uid),
            "Should prune the owner, not the top-stake owner hotkey and not the additional hotkeys"
        );
    });
}

#[test]
fn test_prune_respects_min_non_immune_floor_prefers_immune() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(2);
        add_network(netuid, 10_000, 0);
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 0);

        MaxRegistrationsPerBlock::<Test>::insert(netuid, 1024);
        SubtensorModule::set_target_registrations_per_interval(netuid, u16::MAX);

        let immunity_period: u64 = 1000;
        SubtensorModule::set_immunity_period(netuid, immunity_period as u16);

        // Register three neurons, each in its own block so the per‑block counter resets.
        for i in 0..3 {
            register_ok_neuron(netuid, U256::from(10_000 + i), U256::from(20_000 + i), 0);
            step_block(1);
        }

        // Jump block height forward past immunity for the first 3, without iterating 1000 blocks.
        let target = frame_system::Pallet::<Test>::block_number() + immunity_period + 5;
        frame_system::Pallet::<Test>::set_block_number(target - 1);
        step_block(1);

        // Register a 4th neuron now — it will be immune.
        register_ok_neuron(netuid, U256::from(99_999), U256::from(88_888), 0);

        SubtensorModule::set_min_non_immune_uids(netuid, 3);

        // With floor in place (3 non‑immune + 1 immune), we must prune the immune candidate (uid = 3).
        assert_eq!(SubtensorModule::get_neuron_to_prune(netuid), Some(3));
    });
}

#[test]
fn test_prune_tie_breakers_non_immune_emission_block_uid() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(3);
        add_network(netuid, 1, 0);

        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 0);
        SubtensorModule::set_immunity_period(netuid, 0);

        // Register 3 neurons; registration blocks ascend (0,1,2).
        for i in 0..3 {
            register_ok_neuron(netuid, U256::from(30_000 + i), U256::from(40_000 + i), 0);
            step_block(1);
        }

        // Allow pruning of non-immune.
        SubtensorModule::set_min_non_immune_uids(netuid, 0);

        // Equalize emissions across all 3.
        Emission::<Test>::mutate(netuid, |v| {
            for e in v.iter_mut() {
                *e = 10u64.into();
            }
        });

        // Since emission ties, the earliest registration (uid=0) should be pruned.
        assert_eq!(SubtensorModule::get_neuron_to_prune(netuid), Some(0));
    });
}

#[test]
fn test_prune_all_owner_immortal_returns_none() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(4);

        let owner_ck = U256::from(7777);
        let owner_hk_0 = U256::from(9001);
        let owner_hk_1 = U256::from(9002);
        let owner_hk_2 = U256::from(9003);

        add_network(netuid, 1, 0);
        SubnetOwner::<Test>::insert(netuid, owner_ck);

        register_ok_neuron(netuid, owner_hk_0, owner_ck, 0);
        register_ok_neuron(netuid, owner_hk_1, owner_ck, 0);
        register_ok_neuron(netuid, owner_hk_2, owner_ck, 0);

        Owner::<Test>::insert(owner_hk_0, owner_ck);
        Owner::<Test>::insert(owner_hk_1, owner_ck);
        Owner::<Test>::insert(owner_hk_2, owner_ck);
        OwnedHotkeys::<Test>::insert(owner_ck, vec![owner_hk_0, owner_hk_1, owner_hk_2]);

        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 10);

        assert_eq!(SubtensorModule::get_neuron_to_prune(netuid), None);
    });
}
