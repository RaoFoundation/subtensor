#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Tests for `set_weights` / dispatch info / stake & permit guards.

use frame_support::{
    assert_err, assert_ok,
    dispatch::{DispatchClass, GetDispatchInfo, Pays},
};
use sp_core::{H256, U256};
use sp_runtime::{
    DispatchError,
    traits::{BlakeTwo256, Hash},
};
use substrate_fixed::types::I32F32;

use super::helpers::commit_reveal_set_weights;
use crate::tests::mock::*;
use crate::*;

/***************************
  pub fn set_weights() tests
*****************************/

// Test the call passes through the subtensor module.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_set_weights_dispatch_info_ok --exact --show-output --nocapture
#[test]
fn test_set_weights_dispatch_info_ok() {
    new_test_ext(0).execute_with(|| {
        let dests = vec![1, 1];
        let weights = vec![1, 1];
        let netuid = NetUid::from(1);
        let version_key: u64 = 0;
        let call = RuntimeCall::SubtensorModule(SubtensorCall::set_weights {
            netuid,
            dests,
            weights,
            version_key,
        });
        let dispatch_info = call.get_dispatch_info();

        assert_eq!(dispatch_info.class, DispatchClass::Normal);
        assert_eq!(dispatch_info.pays_fee, Pays::No);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_commit_weights_dispatch_info_ok --exact --show-output --nocapture
#[test]
fn test_commit_weights_dispatch_info_ok() {
    new_test_ext(0).execute_with(|| {
        let dests = vec![1, 1];
        let weights = vec![1, 1];
        let netuid = NetUid::from(1);
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let version_key: u64 = 0;
        let hotkey: U256 = U256::from(1);

        let commit_hash: H256 =
            BlakeTwo256::hash_of(&(hotkey, netuid, dests, weights, salt, version_key));

        let call = RuntimeCall::SubtensorModule(SubtensorCall::commit_weights {
            netuid,
            commit_hash,
        });
        let dispatch_info = call.get_dispatch_info();

        assert_eq!(dispatch_info.class, DispatchClass::Normal);
        assert_eq!(dispatch_info.pays_fee, Pays::No);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_weights_dispatch_info_ok --exact --show-output --nocapture
#[test]
fn test_reveal_weights_dispatch_info_ok() {
    new_test_ext(0).execute_with(|| {
        let dests = vec![1, 1];
        let weights = vec![1, 1];
        let netuid = NetUid::from(1);
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let version_key: u64 = 0;

        let call = RuntimeCall::SubtensorModule(SubtensorCall::reveal_weights {
            netuid,
            uids: dests,
            values: weights,
            salt,
            version_key,
        });
        let dispatch_info = call.get_dispatch_info();

        assert_eq!(dispatch_info.class, DispatchClass::Normal);
        assert_eq!(dispatch_info.pays_fee, Pays::No);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_set_weights_is_root_error --exact --show-output --nocapture
#[test]
fn test_set_weights_is_root_error() {
    new_test_ext(0).execute_with(|| {
        let uids = vec![0];
        let weights = vec![1];
        let version_key: u64 = 0;
        let hotkey = U256::from(1);
        SubtensorModule::set_commit_reveal_weights_enabled(NetUid::ROOT, false);

        assert_err!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(hotkey),
                NetUid::ROOT,
                uids.clone(),
                weights.clone(),
                version_key,
            ),
            Error::<Test>::CanNotSetRootNetworkWeights
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_weights_err_no_validator_permit --exact --show-output --nocapture
// Test ensures that uid has validator permit to set non-self weights.
#[test]
fn test_weights_err_no_validator_permit() {
    new_test_ext(0).execute_with(|| {
        let hotkey_account_id = U256::from(55);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        add_network_disable_commit_reveal(netuid, tempo, 0);
        SubtensorModule::set_min_allowed_weights(netuid, 0);
        SubtensorModule::set_max_allowed_uids(netuid, 3);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        register_ok_neuron(netuid, U256::from(1), U256::from(1), 65555);
        register_ok_neuron(netuid, U256::from(2), U256::from(2), 75555);

        let weights_keys: Vec<u16> = vec![1, 2];
        let weight_values: Vec<u16> = vec![1, 2];

        let result = SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey_account_id),
            netuid,
            weights_keys,
            weight_values,
            0,
        );
        assert_eq!(result, Err(Error::<Test>::NeuronNoValidatorPermit.into()));

        let weights_keys: Vec<u16> = vec![1, 2];
        let weight_values: Vec<u16> = vec![1, 2];
        let neuron_uid: u16 =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id)
                .expect("Not registered.");
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid, true);
        let result = SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey_account_id),
            netuid,
            weights_keys,
            weight_values,
            0,
        );
        assert_ok!(result);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_set_stake_threshold_failed --exact --show-output --nocapture
#[test]
fn test_set_stake_threshold_failed() {
    new_test_ext(0).execute_with(|| {
        let dests = vec![0];
        let weights = vec![1];
        let netuid = NetUid::from(1);
        let version_key: u64 = 0;
        let hotkey = U256::from(0);
        let coldkey = U256::from(0);

        add_network_disable_commit_reveal(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 2143124);
        SubtensorModule::set_stake_threshold(20_000_000_000_000);
        add_balance_to_coldkey_account(&hotkey, 20_000_000_000_000_000_u64.into());

        // Check the signed extension function.
        assert_eq!(SubtensorModule::get_stake_threshold(), 20_000_000_000_000);
        assert!(!SubtensorModule::check_weights_min_stake(&hotkey, netuid));
        assert_ok!(SubtensorModule::do_add_stake(
            RuntimeOrigin::signed(hotkey),
            hotkey,
            netuid,
            19_000_000_000_000_u64.into()
        ));
        assert!(!SubtensorModule::check_weights_min_stake(&hotkey, netuid));
        assert_ok!(SubtensorModule::do_add_stake(
            RuntimeOrigin::signed(hotkey),
            hotkey,
            netuid,
            20_000_000_000_000_u64.into()
        ));
        assert!(SubtensorModule::check_weights_min_stake(&hotkey, netuid));

        // Check that it fails at the pallet level.
        SubtensorModule::set_stake_threshold(100_000_000_000_000);
        assert_eq!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                dests.clone(),
                weights.clone(),
                version_key,
            ),
            Err(Error::<Test>::NotEnoughStakeToSetWeights.into())
        );
        // Now passes
        assert_ok!(SubtensorModule::do_add_stake(
            RuntimeOrigin::signed(hotkey),
            hotkey,
            netuid,
            100_000_000_000_000_u64.into()
        ));
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            dests.clone(),
            weights.clone(),
            version_key
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_weights_version_key --exact --show-output --nocapture
// Test ensures that a uid can only set weights if it has the valid weights set version key.
#[test]
fn test_weights_version_key() {
    new_test_ext(0).execute_with(|| {
        let hotkey = U256::from(55);
        let coldkey = U256::from(66);
        let netuid0 = NetUid::from(1);
        let netuid1 = NetUid::from(2);

        add_network_disable_commit_reveal(netuid0, 1, 0);
        add_network_disable_commit_reveal(netuid1, 1, 0);
        register_ok_neuron(netuid0, hotkey, coldkey, 2143124);
        register_ok_neuron(netuid1, hotkey, coldkey, 3124124);

        let weights_keys: Vec<u16> = vec![0];
        let weight_values: Vec<u16> = vec![1];
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid0,
            weights_keys.clone(),
            weight_values.clone(),
            0
        ));
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid1,
            weights_keys.clone(),
            weight_values.clone(),
            0
        ));

        // Set version keys.
        let key0: u64 = 12312;
        let key1: u64 = 20313;
        SubtensorModule::set_weights_version_key(netuid0, key0);
        SubtensorModule::set_weights_version_key(netuid1, key1);

        // Setting works with version key.
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid0,
            weights_keys.clone(),
            weight_values.clone(),
            key0
        ));
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid1,
            weights_keys.clone(),
            weight_values.clone(),
            key1
        ));

        // validator:20313 >= network:12312 (accepted: validator newer)
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid0,
            weights_keys.clone(),
            weight_values.clone(),
            key1
        ));

        // Setting fails with incorrect keys.
        // validator:12312 < network:20313 (rejected: validator not updated)
        assert_eq!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(hotkey),
                netuid1,
                weights_keys.clone(),
                weight_values.clone(),
                key0
            ),
            Err(Error::<Test>::IncorrectWeightVersionKey.into())
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_weights_err_setting_weights_too_fast --exact --show-output --nocapture
// Test ensures that uid has validator permit to set non-self weights.
#[test]
fn test_weights_err_setting_weights_too_fast() {
    new_test_ext(0).execute_with(|| {
        let hotkey_account_id = U256::from(55);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        add_network_disable_commit_reveal(netuid, tempo, 0);
        SubtensorModule::set_min_allowed_weights(netuid, 0);
        SubtensorModule::set_max_allowed_uids(netuid, 3);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        register_ok_neuron(netuid, U256::from(1), U256::from(1), 65555);
        register_ok_neuron(netuid, U256::from(2), U256::from(2), 75555);

        let neuron_uid: u16 =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id)
                .expect("Not registered.");
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid, true);
        add_balance_to_coldkey_account(&U256::from(66), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &(U256::from(66)),
            netuid,
            1.into(),
        );
        SubtensorModule::set_weights_set_rate_limit(netuid, 10);
        assert_eq!(SubtensorModule::get_weights_set_rate_limit(netuid), 10);

        let weights_keys: Vec<u16> = vec![1, 2];
        let weight_values: Vec<u16> = vec![1, 2];

        // Note that LastUpdate has default 0 for new uids, but if they have actually set weights on block 0
        // then they are allowed to set weights again once more without a wait restriction, to accommodate the default.
        let result = SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey_account_id),
            netuid,
            weights_keys.clone(),
            weight_values.clone(),
            0,
        );
        assert_ok!(result);
        run_to_block(1);

        for i in 1..100 {
            let result = SubtensorModule::set_weights(
                RuntimeOrigin::signed(hotkey_account_id),
                netuid,
                weights_keys.clone(),
                weight_values.clone(),
                0,
            );
            if i % 10 == 1 {
                assert_ok!(result);
            } else {
                assert_eq!(result, Err(Error::<Test>::SettingWeightsTooFast.into()));
            }
            run_to_block(i + 1);
        }
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_weights_err_weights_vec_not_equal_size --exact --show-output --nocapture
// Test ensures that uids -- weights must have the same size.
#[test]
fn test_weights_err_weights_vec_not_equal_size() {
    new_test_ext(0).execute_with(|| {
        let hotkey_account_id = U256::from(55);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        let neuron_uid: u16 =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id)
                .expect("Not registered.");
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid, true);
        let weights_keys: Vec<u16> = vec![1, 2, 3, 4, 5, 6];
        let weight_values: Vec<u16> = vec![1, 2, 3, 4, 5]; // Uneven sizes
        let result = commit_reveal_set_weights(
            hotkey_account_id,
            1.into(),
            weights_keys.clone(),
            weight_values.clone(),
            salt.clone(),
            0,
        );
        assert_eq!(result, Err(Error::<Test>::WeightVecNotEqualSize.into()));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_weights_err_has_duplicate_ids --exact --show-output --nocapture
// Test ensures that uids can have not duplicates
#[test]
fn test_weights_err_has_duplicate_ids() {
    new_test_ext(0).execute_with(|| {
        let hotkey_account_id = U256::from(666);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        add_network(netuid, tempo, 0);

        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_max_allowed_uids(netuid, 100); // Allow many registrations per block.
        SubtensorModule::set_max_registrations_per_block(netuid, 100); // Allow many registrations per block.
        SubtensorModule::set_target_registrations_per_interval(netuid, 100); // Allow many registrations per block.
        // uid 0
        register_ok_neuron(netuid, hotkey_account_id, U256::from(77), 0);
        let neuron_uid: u16 =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id)
                .expect("Not registered.");
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid, true);
        add_balance_to_coldkey_account(&U256::from(77), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &(U256::from(77)),
            netuid,
            1.into(),
        );

        // uid 1
        register_ok_neuron(netuid, U256::from(1), U256::from(1), 100_000);
        SubtensorModule::get_uid_for_net_and_hotkey(netuid, &U256::from(1))
            .expect("Not registered.");

        // uid 2
        register_ok_neuron(netuid, U256::from(2), U256::from(1), 200_000);
        SubtensorModule::get_uid_for_net_and_hotkey(netuid, &U256::from(2))
            .expect("Not registered.");

        // uid 3
        register_ok_neuron(netuid, U256::from(3), U256::from(1), 300_000);
        SubtensorModule::get_uid_for_net_and_hotkey(netuid, &U256::from(3))
            .expect("Not registered.");

        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 4);

        let weights_keys: Vec<u16> = vec![1, 1, 1]; // Contains duplicates
        let weight_values: Vec<u16> = vec![1, 2, 3];
        let result = commit_reveal_set_weights(
            hotkey_account_id,
            netuid,
            weights_keys.clone(),
            weight_values.clone(),
            salt.clone(),
            0,
        );
        assert_eq!(result, Err(Error::<Test>::DuplicateUids.into()));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_weights_err_max_weight_limit --exact --show-output --nocapture
// Test ensures weights cannot exceed max weight limit.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_no_signature --exact --show-output --nocapture
// Tests the call requires a valid origin.
#[test]
fn test_no_signature() {
    new_test_ext(0).execute_with(|| {
        let uids: Vec<u16> = vec![];
        let values: Vec<u16> = vec![];
        SubtensorModule::set_commit_reveal_weights_enabled(1.into(), false);
        let result = SubtensorModule::set_weights(RuntimeOrigin::none(), 1.into(), uids, values, 0);
        assert_eq!(result, Err(DispatchError::BadOrigin));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_set_weights_err_not_active --exact --show-output --nocapture
// Tests that weights cannot be set BY non-registered hotkeys.
#[test]
fn test_set_weights_err_not_active() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        add_network(netuid, tempo, 0);

        // Register one neuron. Should have uid 0
        register_ok_neuron(netuid, U256::from(666), U256::from(2), 100000);
        SubtensorModule::get_uid_for_net_and_hotkey(netuid, &U256::from(666))
            .expect("Not registered.");

        let weights_keys: Vec<u16> = vec![0]; // Uid 0 is valid.
        let weight_values: Vec<u16> = vec![1];
        // This hotkey is NOT registered.
        let result = commit_reveal_set_weights(
            U256::from(1),
            1.into(),
            weights_keys,
            weight_values,
            salt,
            0,
        );
        assert_eq!(
            result,
            Err(Error::<Test>::HotKeyNotRegisteredInSubNet.into())
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_set_weights_err_invalid_uid --exact --show-output --nocapture
// Tests that set weights fails if you pass invalid uids.
#[test]
fn test_set_weights_err_invalid_uid() {
    new_test_ext(0).execute_with(|| {
        let hotkey_account_id = U256::from(55);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        let neuron_uid: u16 =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_account_id)
                .expect("Not registered.");
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid, true);
        add_balance_to_coldkey_account(&U256::from(66), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &(U256::from(66)),
            netuid,
            1.into(),
        );
        let weight_keys: Vec<u16> = vec![9999]; // Does not exist
        let weight_values: Vec<u16> = vec![88]; // random value
        let result = commit_reveal_set_weights(
            hotkey_account_id,
            netuid,
            weight_keys,
            weight_values,
            salt,
            0,
        );
        assert_eq!(result, Err(Error::<Test>::UidVecContainInvalidOne.into()));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_set_weight_not_enough_values --exact --show-output --nocapture
// Tests that set weights fails if you don't pass enough values.
#[test]
fn test_set_weight_not_enough_values() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let account_id = U256::from(1);
        add_network_disable_commit_reveal(netuid, tempo, 0);

        register_ok_neuron(netuid, account_id, U256::from(2), 100000);
        let neuron_uid: u16 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &U256::from(1))
            .expect("Not registered.");
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid, true);
        add_balance_to_coldkey_account(&U256::from(2), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &account_id,
            &(U256::from(2)),
            netuid,
            1.into(),
        );

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300000);
        SubtensorModule::set_min_allowed_weights(netuid, 2);

        // Should fail because we are only setting a single value and its not the self weight.
        let weight_keys: Vec<u16> = vec![1]; // not weight.
        let weight_values: Vec<u16> = vec![88]; // random value.
        let result = SubtensorModule::set_weights(
            RuntimeOrigin::signed(account_id),
            1.into(),
            weight_keys,
            weight_values,
            0,
        );
        assert_eq!(result, Err(Error::<Test>::WeightVecLengthIsLow.into()));

        // Shouldnt fail because we setting a single value but it is the self weight.
        let weight_keys: Vec<u16> = vec![0]; // self weight.
        let weight_values: Vec<u16> = vec![88]; // random value.
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(account_id),
            1.into(),
            weight_keys,
            weight_values,
            0
        ));

        // Should pass because we are setting enough values.
        let weight_keys: Vec<u16> = vec![0, 1]; // self weight.
        let weight_values: Vec<u16> = vec![10, 10]; // random value.
        SubtensorModule::set_min_allowed_weights(netuid, 1);
        assert_ok!(commit_reveal_set_weights(
            account_id,
            1.into(),
            weight_keys,
            weight_values,
            salt,
            0
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_set_weight_too_many_uids --exact --show-output --nocapture
// Tests that the weights set fails if you pass too many uids for the subnet
#[test]
fn test_set_weight_too_many_uids() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        add_network_disable_commit_reveal(netuid, tempo, 0);

        register_ok_neuron(1.into(), U256::from(1), U256::from(2), 100_000);
        let neuron_uid: u16 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &U256::from(1))
            .expect("Not registered.");
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid, true);

        register_ok_neuron(1.into(), U256::from(3), U256::from(4), 300_000);
        SubtensorModule::set_min_allowed_weights(1.into(), 2);
        // Should fail because we are setting more weights than there are neurons.
        let weight_keys: Vec<u16> = vec![0, 1, 2, 3, 4]; // more uids than neurons in subnet.
        let weight_values: Vec<u16> = vec![88, 102, 303, 1212, 11]; // random value.
        let result = SubtensorModule::set_weights(
            RuntimeOrigin::signed(U256::from(1)),
            1.into(),
            weight_keys,
            weight_values,
            0,
        );
        assert_eq!(
            result,
            Err(Error::<Test>::UidsLengthExceedUidsInSubNet.into())
        );

        // Shouldnt fail because we are setting less weights than there are neurons.
        let weight_keys: Vec<u16> = vec![0, 1]; // Only on neurons that exist.
        let weight_values: Vec<u16> = vec![10, 10]; // random value.
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(U256::from(1)),
            1.into(),
            weight_keys,
            weight_values,
            0
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_set_weights_sum_larger_than_u16_max --exact --show-output --nocapture
// Tests that the weights set doesn't panic if you pass weights that sum to larger than u16 max.
#[test]
fn test_set_weights_sum_larger_than_u16_max() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        add_network(netuid, tempo, 0);

        register_ok_neuron(1.into(), U256::from(1), U256::from(2), 100_000);
        let neuron_uid: u16 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &U256::from(1))
            .expect("Not registered.");
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid, true);
        add_balance_to_coldkey_account(&U256::from(2), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &(U256::from(1)),
            &(U256::from(2)),
            netuid,
            1.into(),
        );

        register_ok_neuron(1.into(), U256::from(3), U256::from(4), 300_000);
        SubtensorModule::set_min_allowed_weights(1.into(), 2);

        // Shouldn't fail because we are setting the right number of weights.
        let weight_keys: Vec<u16> = vec![0, 1];
        let weight_values: Vec<u16> = vec![u16::MAX, u16::MAX];
        // sum of weights is larger than u16 max.
        assert!(weight_values.iter().map(|x| *x as u64).sum::<u64>() > (u16::MAX as u64));

        let result =
            commit_reveal_set_weights(U256::from(1), 1.into(), weight_keys, weight_values, salt, 0);
        assert_ok!(result);

        // Get max-upscaled unnormalized weights.
        let all_weights: Vec<Vec<I32F32>> = SubtensorModule::get_weights(netuid.into());
        let weights_set: &[I32F32] = &all_weights[neuron_uid as usize];
        assert_eq!(weights_set[0], I32F32::from_num(u16::MAX));
        assert_eq!(weights_set[1], I32F32::from_num(u16::MAX));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_do_commit_crv3_weights_disabled --exact --show-output --nocapture
