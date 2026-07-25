//! Max/min allowed UIDs, validators, trim-to-max, and min non-immune UID settings.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    unused_imports
)]

use super::prelude::*;

#[test]
fn test_sudo_set_max_allowed_uids() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u16 = 12;
        add_network(netuid, 10);
        MaxRegistrationsPerBlock::<Test>::insert(netuid, 256);
        TargetRegistrationsPerInterval::<Test>::insert(netuid, 256);

        for i in 0..=8 {
            let hotkey = U256::from(i * 1000);
            let coldkey = U256::from(i * 1000 + i);

            let funds: u64 = 1_000_000_000_000_000; // 1,000,000 TAO (in RAO)
            let _ = Balances::deposit_creating(&coldkey, Balance::from(funds));
            let _ = Balances::deposit_creating(&hotkey, Balance::from(funds)); // defensive

            register_ok_neuron(netuid, hotkey, coldkey, 0);
            step_block(1);
        }

        // Bad origin that is not root or subnet owner
        assert_noop!(
            AdminUtils::sudo_set_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(42)),
                netuid,
                to_be_set
            ),
            DispatchError::BadOrigin
        );

        // Random netuid that doesn't exist
        assert_noop!(
            AdminUtils::sudo_set_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                NetUid::from(42),
                to_be_set
            ),
            Error::<Test>::SubnetDoesNotExist
        );

        // Trying to set max allowed uids less than min allowed uids
        assert_noop!(
            AdminUtils::sudo_set_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                SubtensorModule::get_min_allowed_uids(netuid) - 1
            ),
            Error::<Test>::MaxAllowedUidsLessThanMinAllowedUids
        );

        // Trying to set max allowed uids less than current uids
        assert_noop!(
            AdminUtils::sudo_set_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                SubtensorModule::get_subnetwork_n(netuid) - 1
            ),
            Error::<Test>::MaxAllowedUIdsLessThanCurrentUIds
        );

        // Trying to set max allowed uids greater than default max allowed uids
        assert_noop!(
            AdminUtils::sudo_set_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                DefaultMaxAllowedUids::<Test>::get() + 1
            ),
            Error::<Test>::MaxAllowedUidsGreaterThanDefaultMaxAllowedUids
        );

        // Trying to set max allowed uids that would cause max_allowed_uids * mechanism_count > 256
        MaxAllowedUids::<Test>::insert(netuid, 8);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(32));
        let large_max_uids = 16;
        assert_noop!(
            AdminUtils::sudo_set_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                large_max_uids
            ),
            SubtensorError::<Test>::TooManyUIDsPerMechanism
        );
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(1));

        // Normal case
        assert_ok!(AdminUtils::sudo_set_max_allowed_uids(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_max_allowed_uids(netuid), to_be_set);

        // Exact current case
        assert_ok!(AdminUtils::sudo_set_max_allowed_uids(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            SubtensorModule::get_subnetwork_n(netuid)
        ));
        assert_eq!(
            SubtensorModule::get_max_allowed_uids(netuid),
            SubtensorModule::get_subnetwork_n(netuid)
        );

        // Lower bound case
        SubtensorModule::set_min_allowed_uids(netuid, SubtensorModule::get_subnetwork_n(netuid));
        assert_ok!(AdminUtils::sudo_set_max_allowed_uids(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            SubtensorModule::get_min_allowed_uids(netuid)
        ));
        assert_eq!(
            SubtensorModule::get_max_allowed_uids(netuid),
            SubtensorModule::get_min_allowed_uids(netuid)
        );

        // Upper bound case
        assert_ok!(AdminUtils::sudo_set_max_allowed_uids(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            DefaultMaxAllowedUids::<Test>::get(),
        ));
        assert_eq!(
            SubtensorModule::get_max_allowed_uids(netuid),
            DefaultMaxAllowedUids::<Test>::get()
        );
    });
}

#[test]
fn test_sudo_set_max_allowed_validators() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u16 = 10;
        add_network(netuid, 10);
        let init_value: u16 = SubtensorModule::get_max_allowed_validators(netuid);
        assert_eq!(
            AdminUtils::sudo_set_max_allowed_validators(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_max_allowed_validators(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );
        assert_eq!(
            SubtensorModule::get_max_allowed_validators(netuid),
            init_value
        );
        assert_ok!(AdminUtils::sudo_set_max_allowed_validators(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(
            SubtensorModule::get_max_allowed_validators(netuid),
            to_be_set
        );
    });
}

#[test]
fn test_trim_to_max_allowed_uids() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let sn_owner = U256::from(1);
        let sn_owner_hotkey1 = U256::from(2);

        add_network(netuid, 10);
        SubnetOwner::<Test>::insert(netuid, sn_owner);
        SubnetOwnerHotkey::<Test>::insert(netuid, sn_owner_hotkey1);

        MaxRegistrationsPerBlock::<Test>::insert(netuid, 256);
        TargetRegistrationsPerInterval::<Test>::insert(netuid, 256);
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 2);
        // We set a low value here to make testing easier
        MinAllowedUids::<Test>::set(netuid, 4);
        // We define 4 mechanisms
        let mechanism_count = MechId::from(4);
        MechanismCountCurrent::<Test>::insert(netuid, mechanism_count);

        // Add some neurons (fund accounts + step blocks between regs).
        let max_n: u16 = 16;
        for i in 1..=max_n {
            let n: u64 = (i as u64) * 1000;
            let hotkey = U256::from(n);
            let coldkey = U256::from(n + i as u64);

            let funds: u64 = 1_000_000_000_000_000; // 1,000,000 TAO (in RAO)
            let _ = Balances::deposit_creating(&coldkey, Balance::from(funds));
            let _ = Balances::deposit_creating(&hotkey, Balance::from(funds)); // defensive

            register_ok_neuron(netuid, hotkey, coldkey, 0);
            step_block(1);
        }

        // Run some blocks to ensure stake weights are set and that we are past the immunity period
        // for all neurons
        let immunity_period: u64 = ImmunityPeriod::<Test>::get(netuid).into();
        let current_block: u64 = frame_system::Pallet::<Test>::block_number().into();
        run_to_block(current_block + immunity_period + 1);

        // Set some randomized values that we can keep track of
        let values = vec![
            17u16, 42u16, 8u16, 56u16, 23u16, 91u16,
            34u16, // uid 6 (34) will be forced-immune below
            77u16, 12u16, 65u16, 3u16, 88u16, 29u16, 51u16, 74u16, 39u16,
        ];
        let bool_values = vec![
            false, false, false, true, false, true, true, true, false, true, false, true, false,
            true, true, false,
        ];
        let alpha_values = values.iter().map(|&v| (v as u64).into()).collect();
        let u64_values: Vec<u64> = values.iter().map(|&v| v as u64).collect();
        let per_values: Vec<PerU16> = values.iter().map(|&v| PerU16::from_parts(v)).collect();

        Emission::<Test>::set(netuid, alpha_values);
        Consensus::<Test>::insert(netuid, per_values.clone());
        Dividends::<Test>::insert(netuid, per_values.clone());
        ValidatorTrust::<Test>::insert(netuid, per_values.clone());
        StakeWeight::<Test>::insert(netuid, values.clone());
        ValidatorPermit::<Test>::insert(netuid, bool_values.clone());
        Active::<Test>::insert(netuid, bool_values);

        for mecid in 0..mechanism_count.into() {
            let netuid_index =
                SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(mecid));
            Incentive::<Test>::insert(netuid_index, per_values.clone());
            LastUpdate::<Test>::insert(netuid_index, u64_values.clone());
        }

        // Make UID 6 temporally immune so it cannot be trimmed even though it's not a top-8 emitter.
        let now = frame_system::Pallet::<Test>::block_number();
        BlockAtRegistration::<Test>::set(netuid, 6, now);

        // Set some evm addresses (include both kept + trimmed uids). Go through the normal
        // setter so both the forward map and the reverse index are populated, exactly as the
        // association extrinsic does in production.
        let evm_addr_uid6 = sp_core::H160::from_slice(b"12345678901234567891");
        let evm_addr_uid10 = sp_core::H160::from_slice(b"12345678901234567892");
        let evm_addr_uid12 = sp_core::H160::from_slice(b"12345678901234567893");
        let evm_addr_uid14 = sp_core::H160::from_slice(b"12345678901234567894");
        SubtensorModule::set_associated_evm_address(netuid, 6, evm_addr_uid6, now);
        SubtensorModule::set_associated_evm_address(netuid, 10, evm_addr_uid10, now);
        SubtensorModule::set_associated_evm_address(netuid, 12, evm_addr_uid12, now);
        SubtensorModule::set_associated_evm_address(netuid, 14, evm_addr_uid14, now);

        // Populate Weights and Bonds storage items to test trimming
        for uid in 0..max_n {
            let mut weights = Vec::new();
            let mut bonds = Vec::new();

            // Add connections to all other uids, including those that will be trimmed
            for target_uid in 0..max_n {
                if target_uid != uid {
                    let weight_value = (uid + target_uid) % 1000;
                    let bond_value = (uid * target_uid) % 1000;
                    weights.push((target_uid, weight_value));
                    bonds.push((target_uid, bond_value));
                }
            }

            for mecid in 0..mechanism_count.into() {
                let netuid_index =
                    SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(mecid));
                Weights::<Test>::insert(netuid_index, uid, weights.clone());
                Bonds::<Test>::insert(netuid_index, uid, bonds.clone());
            }
        }

        // Normal case
        let new_max_n = 8;
        assert_ok!(AdminUtils::sudo_trim_to_max_allowed_uids(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            new_max_n
        ));

        // Ensure the max allowed uids has been set correctly
        assert_eq!(MaxAllowedUids::<Test>::get(netuid), new_max_n);

        // Ensure the emission has been trimmed correctly and compressed to the left
        assert_eq!(
            Emission::<Test>::get(netuid),
            vec![
                56.into(),
                91.into(),
                34.into(),
                77.into(),
                65.into(),
                88.into(),
                51.into(),
                74.into()
            ]
        );

        // Ensure rest of (active) storage has been trimmed correctly
        let expected_values: Vec<u16> = vec![56, 91, 34, 77, 65, 88, 51, 74];
        let expected_per_values: Vec<PerU16> = expected_values
            .iter()
            .map(|&v| PerU16::from_parts(v))
            .collect();
        let expected_bools = vec![true, true, true, true, true, true, true, true];
        let expected_u64_values = vec![56, 91, 34, 77, 65, 88, 51, 74];

        assert_eq!(Active::<Test>::get(netuid), expected_bools);
        assert_eq!(Consensus::<Test>::get(netuid), expected_per_values);
        assert_eq!(Dividends::<Test>::get(netuid), expected_per_values);
        assert_eq!(ValidatorTrust::<Test>::get(netuid), expected_per_values);
        assert_eq!(ValidatorPermit::<Test>::get(netuid), expected_bools);
        assert_eq!(StakeWeight::<Test>::get(netuid), expected_values);

        for mecid in 0..mechanism_count.into() {
            let netuid_index =
                SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(mecid));
            assert_eq!(Incentive::<Test>::get(netuid_index), expected_per_values);
            assert_eq!(LastUpdate::<Test>::get(netuid_index), expected_u64_values);
        }

        // Ensure trimmed uids related storage has been cleared
        for uid in new_max_n..max_n {
            assert!(!Keys::<Test>::contains_key(netuid, uid));
            assert!(!BlockAtRegistration::<Test>::contains_key(netuid, uid));
            assert!(!AssociatedEvmAddress::<Test>::contains_key(netuid, uid));
            for mecid in 0..mechanism_count.into() {
                let netuid_index =
                    SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(mecid));
                assert!(!Weights::<Test>::contains_key(netuid_index, uid));
                assert!(!Bonds::<Test>::contains_key(netuid_index, uid));
            }
        }

        // Ensure trimmed uids hotkey related storage has been cleared
        let trimmed_hotkeys = vec![
            U256::from(1000),
            U256::from(2000),
            U256::from(3000),
            U256::from(5000),
            U256::from(9000),
            U256::from(11000),
            U256::from(13000),
            U256::from(16000),
        ];
        for hotkey in trimmed_hotkeys {
            assert!(!Uids::<Test>::contains_key(netuid, hotkey));
            assert!(!IsNetworkMember::<Test>::contains_key(hotkey, netuid));
            assert!(!LastHotkeyEmissionOnNetuid::<Test>::contains_key(
                hotkey, netuid
            ));
            assert!(!AlphaDividendsPerSubnet::<Test>::contains_key(
                netuid, hotkey
            ));
            assert!(!Axons::<Test>::contains_key(netuid, hotkey));
            assert!(!NeuronCertificates::<Test>::contains_key(netuid, hotkey));
            assert!(!Prometheus::<Test>::contains_key(netuid, hotkey));
        }

        // Ensure trimmed uids weights and bonds connections have been trimmed correctly
        for uid in 0..new_max_n {
            for mecid in 0..mechanism_count.into() {
                let netuid_index =
                    SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(mecid));
                assert!(
                    Weights::<Test>::get(netuid_index, uid)
                        .iter()
                        .all(|(target_uid, _)| *target_uid < new_max_n),
                    "Found a weight with target_uid >= new_max_n"
                );
                assert!(
                    Bonds::<Test>::get(netuid_index, uid)
                        .iter()
                        .all(|(target_uid, _)| *target_uid < new_max_n),
                    "Found a bond with target_uid >= new_max_n"
                );
            }
        }

        // Actual number of neurons on the network updated after trimming
        assert_eq!(SubnetworkN::<Test>::get(netuid), new_max_n);

        // Uids match enumeration order
        for i in 0..new_max_n.into() {
            let hotkey = Keys::<Test>::get(netuid, i);
            let uid = Uids::<Test>::get(netuid, hotkey);
            assert_eq!(uid, Some(i));
        }

        // EVM association have been remapped correctly (uids: 6 -> 2, 14 -> 7)
        assert_eq!(
            AssociatedEvmAddress::<Test>::get(netuid, 2),
            Some((evm_addr_uid6, now))
        );
        assert_eq!(
            AssociatedEvmAddress::<Test>::get(netuid, 7),
            Some((evm_addr_uid14, now))
        );

        // The reverse index has been remapped in place to the new UIDs (6 -> 2, 14 -> 7),
        // without rebuilding it from scratch.
        assert_eq!(
            AssociatedUidsByEvmAddress::<Test>::get(netuid, evm_addr_uid6).into_inner(),
            vec![(2u16, now)]
        );
        assert_eq!(
            AssociatedUidsByEvmAddress::<Test>::get(netuid, evm_addr_uid14).into_inner(),
            vec![(7u16, now)]
        );
        // Trimmed UIDs (10, 12) were dropped from the reverse index entirely.
        assert!(AssociatedUidsByEvmAddress::<Test>::get(netuid, evm_addr_uid10).is_empty());
        assert!(AssociatedUidsByEvmAddress::<Test>::get(netuid, evm_addr_uid12).is_empty());
        // uid_lookup resolves the remapped UID.
        assert_eq!(
            SubtensorModule::uid_lookup(netuid, evm_addr_uid6, u16::MAX),
            vec![(2u16, now)]
        );

        // Non existent subnet
        assert_err!(
            AdminUtils::sudo_trim_to_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                NetUid::from(42),
                new_max_n
            ),
            pallet_subtensor::Error::<Test>::SubnetNotExists
        );

        // New max n less than lower bound
        assert_err!(
            AdminUtils::sudo_trim_to_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                2
            ),
            pallet_subtensor::Error::<Test>::InvalidValue
        );

        // New max n greater than upper bound
        assert_err!(
            AdminUtils::sudo_trim_to_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                SubtensorModule::get_max_allowed_uids(netuid) + 1
            ),
            pallet_subtensor::Error::<Test>::InvalidValue
        );
    });
}

#[test]
fn test_trim_to_max_allowed_uids_too_many_immune() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let sn_owner = U256::from(1);
        add_network(netuid, 10);
        SubnetOwner::<Test>::insert(netuid, sn_owner);
        MaxRegistrationsPerBlock::<Test>::insert(netuid, 256);
        TargetRegistrationsPerInterval::<Test>::insert(netuid, 256);
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 2);
        MinAllowedUids::<Test>::set(netuid, 2);

        // Add 5 neurons (fund + step blocks between regs)
        let max_n = 5;
        for i in 1..=max_n {
            let n = i * 1000;
            let hotkey = U256::from(n);
            let coldkey = U256::from(n + i);

            let funds: u64 = 1_000_000_000_000_000; // 1,000,000 TAO (in RAO)
            let _ = Balances::deposit_creating(&coldkey, Balance::from(funds));
            let _ = Balances::deposit_creating(&hotkey, Balance::from(funds)); // defensive

            register_ok_neuron(netuid, hotkey, coldkey, 0);
            step_block(1);
        }

        // Run some blocks to ensure stake weights are set
        run_to_block((ImmunityPeriod::<Test>::get(netuid) + 1).into());

        // Set owner immune uids (2 UIDs) by adding them to OwnedHotkeys
        let owner_hotkey1 = U256::from(1000);
        let owner_hotkey2 = U256::from(2000);
        OwnedHotkeys::<Test>::insert(sn_owner, vec![owner_hotkey1, owner_hotkey2]);
        Keys::<Test>::insert(netuid, 0, owner_hotkey1);
        Uids::<Test>::insert(netuid, owner_hotkey1, 0);
        Keys::<Test>::insert(netuid, 1, owner_hotkey2);
        Uids::<Test>::insert(netuid, owner_hotkey2, 1);

        // Set temporally immune uids (2 UIDs) to make total immune count 4 out of 5 (80%)
        // Set their registration block to current block to make them temporally immune
        let current_block = frame_system::Pallet::<Test>::block_number();
        for uid in 2..4 {
            let hotkey = U256::from(uid * 1000 + 1000);
            Keys::<Test>::insert(netuid, uid, hotkey);
            Uids::<Test>::insert(netuid, hotkey, uid);
            BlockAtRegistration::<Test>::insert(netuid, uid, current_block);
        }

        // Try to trim to 4 UIDs - this should fail because 4/4 = 100% immune (>= 80%)
        assert_err!(
            AdminUtils::sudo_trim_to_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                4
            ),
            pallet_subtensor::Error::<Test>::TrimmingWouldExceedMaxImmunePercentage
        );

        // Try to trim to 3 UIDs - this should also fail because 4/3 > 80% immune (>= 80%)
        assert_err!(
            AdminUtils::sudo_trim_to_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                3
            ),
            pallet_subtensor::Error::<Test>::TrimmingWouldExceedMaxImmunePercentage
        );

        // Now test a scenario where trimming should succeed
        // Remove one immune UID to make it 3 immune out of 4 total
        let uid_to_remove = 3;
        let hotkey_to_remove = U256::from(uid_to_remove * 1000 + 1000);
        #[allow(unknown_lints)]
        Keys::<Test>::remove(netuid, uid_to_remove);
        Uids::<Test>::remove(netuid, hotkey_to_remove);
        BlockAtRegistration::<Test>::remove(netuid, uid_to_remove);

        // Remove another immune UID to make it 2 immune out of 3 total
        let uid_to_remove2 = 2;
        let hotkey_to_remove2 = U256::from(uid_to_remove2 * 1000 + 1000);
        #[allow(unknown_lints)]
        Keys::<Test>::remove(netuid, uid_to_remove2);
        Uids::<Test>::remove(netuid, hotkey_to_remove2);
        BlockAtRegistration::<Test>::remove(netuid, uid_to_remove2);

        // Now we have 2 immune out of 2 total UIDs
        // Try to trim to 1 UID - this should fail because 2/1 is impossible, but the check prevents it
        assert_err!(
            AdminUtils::sudo_trim_to_max_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                1
            ),
            pallet_subtensor::Error::<Test>::InvalidValue
        );
    });
}

#[test]
fn test_sudo_set_min_allowed_uids() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u16 = 8;
        add_network(netuid, 10);
        MaxRegistrationsPerBlock::<Test>::insert(netuid, 256);
        TargetRegistrationsPerInterval::<Test>::insert(netuid, 256);

        for i in 0..=16 {
            let hotkey = U256::from(i * 1000);
            let coldkey = U256::from(i * 1000 + i);

            let funds: u64 = 1_000_000_000_000_000; // 1,000,000 TAO (in RAO)
            let _ = Balances::deposit_creating(&coldkey, Balance::from(funds));
            let _ = Balances::deposit_creating(&hotkey, Balance::from(funds)); // defensive

            register_ok_neuron(netuid, hotkey, coldkey, 0);
            step_block(1);
        }

        // Normal case
        assert_ok!(AdminUtils::sudo_set_min_allowed_uids(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_min_allowed_uids(netuid), to_be_set);

        // Non root
        assert_err!(
            AdminUtils::sudo_set_min_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(0)),
                netuid,
                to_be_set
            ),
            DispatchError::BadOrigin
        );

        // Non existent subnet
        assert_err!(
            AdminUtils::sudo_set_min_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                NetUid::from(42),
                to_be_set
            ),
            Error::<Test>::SubnetDoesNotExist
        );

        // Min allowed uids greater than max allowed uids
        assert_err!(
            AdminUtils::sudo_set_min_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                SubtensorModule::get_max_allowed_uids(netuid) + 1
            ),
            Error::<Test>::MinAllowedUidsGreaterThanMaxAllowedUids
        );

        // Min allowed uids greater than current uids
        assert_err!(
            AdminUtils::sudo_set_min_allowed_uids(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                SubtensorModule::get_subnetwork_n(netuid) + 1
            ),
            Error::<Test>::MinAllowedUidsGreaterThanCurrentUids
        );
    });
}

#[test]
fn test_sudo_set_min_non_immune_uids() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 10);

        let to_be_set: u16 = 12;
        let init_value: u16 = SubtensorModule::get_min_non_immune_uids(netuid);

        assert_ok!(AdminUtils::sudo_set_min_non_immune_uids(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));

        assert!(init_value != to_be_set);
        assert_eq!(SubtensorModule::get_min_non_immune_uids(netuid), to_be_set);
    });
}
