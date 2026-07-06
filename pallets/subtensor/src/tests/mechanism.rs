#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

// Run all tests
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::mechanism --show-output

// Test plan:
//   - [x] Netuid index math (with MechanismCountCurrent limiting)
//   - [x] Sub-subnet validity tests
//   - [x] do_set_desired tests
//   - [x] Emissions are split proportionally
//   - [x] Sum of split emissions is equal to rao_emission passed to epoch
//   - [x] Only subnet owner or root can set desired mechanism count (pallet admin test)
//   - [x] Weights can be set by mechanism
//   - [x] Weights can be commited/revealed by mechanism
//   - [x] Weights can be commited/revealed in crv3 by mechanism
//   - [x] Prevent weight setting/commitment/revealing above mechanism_limit_in_force
//   - [x] Prevent weight commitment/revealing above mechanism_limit_in_force
//   - [x] Prevent weight commitment/revealing in crv3 above mechanism_limit_in_force
//   - [x] When a miner is deregistered, their weights are cleaned across all mechanisms
//   - [x] Weight setting rate limiting is enforced by mechanism
//   - [x] Bonds are applied per mechanism
//   - [x] Incentives are per mechanism
//   - [x] Per-mechanism incentives are distributed proportionally to miner weights
//   - [x] Mechanism limit can be set up to 8 (with admin pallet)
//   - [x] When reduction of mechanism limit occurs, Weights, Incentive, LastUpdate, Bonds, and WeightCommits are cleared
//   - [x] Epoch terms of subnet are weighted sum (or logical OR) of all mechanism epoch terms
//   - [x] Subnet epoch terms persist in state
//   - [x] Mechanism epoch terms persist in state
//   - [x] "Yuma Emergency Mode" (consensus sum is 0 for a mechanism), emission distributed by stake
//   - [x] Miner with no weights on any mechanism receives no reward
//   - [x] MechanismEmissionSplit is reset on mechanism count increase
//   - [x] MechanismEmissionSplit is reset on mechanism count decrease

use super::mock::*;
use crate::coinbase::reveal_commits::WeightsTlockPayload;
use crate::subnets::mechanism::{GLOBAL_MAX_SUBNET_COUNT, MAX_MECHANISM_COUNT_PER_SUBNET};
use crate::*;
use alloc::collections::BTreeMap;
use approx::assert_abs_diff_eq;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use codec::Encode;
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use pallet_drand::types::Pulse;
use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
use sha2::Digest;
use sp_core::{H256, U256};
use sp_runtime::traits::{BlakeTwo256, Hash};
use sp_std::collections::vec_deque::VecDeque;
use substrate_fixed::types::{I32F32, U64F64};
use subtensor_runtime_common::{MechId, NetUid, NetUidStorageIndex};
use tle::{
    curves::drand::TinyBLS381, ibe::fullident::Identity,
    stream_ciphers::AESGCMStreamCipherProvider, tlock::tle,
};
use w3f_bls::EngineBLS;

#[test]
fn test_index_from_netuid_and_subnet() {
    new_test_ext(1).execute_with(|| {
        [
            (0_u16, 0_u8),
            (GLOBAL_MAX_SUBNET_COUNT / 2, 1),
            (GLOBAL_MAX_SUBNET_COUNT / 2, 7),
            (GLOBAL_MAX_SUBNET_COUNT / 2, 14),
            (GLOBAL_MAX_SUBNET_COUNT / 2, 15),
            (GLOBAL_MAX_SUBNET_COUNT - 1, 1),
            (GLOBAL_MAX_SUBNET_COUNT - 1, 7),
            (GLOBAL_MAX_SUBNET_COUNT - 1, 14),
            (GLOBAL_MAX_SUBNET_COUNT - 1, 15),
        ]
        .iter()
        .for_each(|(netuid, sub_id)| {
            let idx = SubtensorModule::get_mechanism_storage_index(
                NetUid::from(*netuid),
                MechId::from(*sub_id),
            );
            let expected = *sub_id as u64 * GLOBAL_MAX_SUBNET_COUNT as u64 + *netuid as u64;
            assert_eq!(idx, NetUidStorageIndex::from(expected as u16));
        });
    });
}

#[test]
fn test_netuid_and_subnet_from_index() {
    new_test_ext(1).execute_with(|| {
        [
            0_u16,
            1,
            14,
            15,
            16,
            17,
            GLOBAL_MAX_SUBNET_COUNT - 1,
            GLOBAL_MAX_SUBNET_COUNT,
            GLOBAL_MAX_SUBNET_COUNT + 1,
            0xFFFE / 2,
            0xFFFE,
            0xFFFF,
        ]
        .iter()
        .for_each(|netuid_index| {
            let expected_netuid = (*netuid_index as u64 % GLOBAL_MAX_SUBNET_COUNT as u64) as u16;
            let expected_subid = (*netuid_index as u64 / GLOBAL_MAX_SUBNET_COUNT as u64) as u8;

            // Allow subnet ID
            NetworksAdded::<Test>::insert(NetUid::from(expected_netuid), true);
            MechanismCountCurrent::<Test>::insert(
                NetUid::from(expected_netuid),
                MechId::from(expected_subid + 1),
            );

            let (netuid, mecid) =
                SubtensorModule::get_netuid_and_subid(NetUidStorageIndex::from(*netuid_index))
                    .unwrap();
            assert_eq!(netuid, NetUid::from(expected_netuid));
            assert_eq!(mecid, MechId::from(expected_subid));
        });
    });
}

#[test]
fn test_netuid_index_math_constants() {
    assert_eq!(
        GLOBAL_MAX_SUBNET_COUNT as u64 * MAX_MECHANISM_COUNT_PER_SUBNET as u64,
        0x10000
    );
}

#[test]
fn ensure_mechanism_exists_ok() {
    new_test_ext(1).execute_with(|| {
        let netuid: NetUid = 3u16.into();
        let sub_id = MechId::from(1u8);

        // ensure base subnet exists
        NetworksAdded::<Test>::insert(NetUid::from(netuid), true);

        // Allow at least 2 sub-subnets (so sub_id = 1 is valid)
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));
        assert_ok!(SubtensorModule::ensure_mechanism_exists(netuid, sub_id));
    });
}

#[test]
fn ensure_mechanism_fails_when_base_subnet_missing() {
    new_test_ext(1).execute_with(|| {
        let netuid: NetUid = 7u16.into();
        let sub_id = MechId::from(0u8);

        // Intentionally DO NOT create the base subnet

        assert_noop!(
            SubtensorModule::ensure_mechanism_exists(netuid, sub_id),
            Error::<Test>::MechanismDoesNotExist
        );
    });
}

#[test]
fn ensure_mechanism_fails_when_subid_out_of_range() {
    new_test_ext(1).execute_with(|| {
        let netuid: NetUid = 9u16.into();
        NetworksAdded::<Test>::insert(NetUid::from(netuid), true);

        // Current allowed sub-subnet count is 2 => valid sub_ids: {0, 1}
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));

        // sub_id == 2 is out of range (must be < 2)
        let sub_id_eq = MechId::from(2u8);
        assert_noop!(
            SubtensorModule::ensure_mechanism_exists(netuid, sub_id_eq),
            Error::<Test>::MechanismDoesNotExist
        );

        // sub_id > 2 is also out of range
        let sub_id_gt = MechId::from(3u8);
        assert_noop!(
            SubtensorModule::ensure_mechanism_exists(netuid, sub_id_gt),
            Error::<Test>::MechanismDoesNotExist
        );
    });
}

#[test]
fn do_set_mechanism_count_ok_minimal() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(3u16);
        NetworksAdded::<Test>::insert(NetUid::from(3u16), true); // base subnet exists

        assert_ok!(SubtensorModule::do_set_mechanism_count(
            netuid,
            MechId::from(1u8)
        ));

        assert_eq!(
            MechanismCountCurrent::<Test>::get(netuid),
            MechId::from(1u8)
        );
    });
}

#[test]
fn do_set_mechanism_count_ok_at_effective_cap() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(4u16);
        NetworksAdded::<Test>::insert(NetUid::from(4u16), true); // base subnet exists
        MaxAllowedUids::<Test>::insert(netuid, 128u16);

        // Effective bound is min(runtime cap, compile-time cap)
        let runtime_cap = MaxMechanismCount::<Test>::get(); // e.g., MechId::from(8)
        let compile_cap = MechId::from(MAX_MECHANISM_COUNT_PER_SUBNET);
        let bound = if runtime_cap <= compile_cap {
            runtime_cap
        } else {
            compile_cap
        };

        assert_ok!(SubtensorModule::do_set_mechanism_count(netuid, bound));
        assert_eq!(MechanismCountCurrent::<Test>::get(netuid), bound);
    });
}

#[test]
fn do_set_fails_when_base_subnet_missing() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(7u16);
        // No NetworksAdded insert => base subnet absent

        assert_noop!(
            SubtensorModule::do_set_mechanism_count(netuid, MechId::from(1u8)),
            Error::<Test>::MechanismDoesNotExist
        );
    });
}

#[test]
fn do_set_fails_for_zero() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(9u16);
        NetworksAdded::<Test>::insert(NetUid::from(9u16), true); // base subnet exists

        assert_noop!(
            SubtensorModule::do_set_mechanism_count(netuid, MechId::from(0u8)),
            Error::<Test>::InvalidValue
        );
    });
}

#[test]
fn do_set_fails_when_over_runtime_cap() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(11u16);
        NetworksAdded::<Test>::insert(NetUid::from(11u16), true); // base subnet exists

        // Runtime cap is 8 (per function), so 9 must fail
        assert_noop!(
            SubtensorModule::do_set_mechanism_count(netuid, MechId::from(9u8)),
            Error::<Test>::InvalidValue
        );
    });
}

#[test]
fn do_set_fails_when_over_compile_time_cap() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(12u16);
        NetworksAdded::<Test>::insert(NetUid::from(12u16), true); // base subnet exists

        let too_big = MechId::from(MAX_MECHANISM_COUNT_PER_SUBNET + 1);
        assert_noop!(
            SubtensorModule::do_set_mechanism_count(netuid, too_big),
            Error::<Test>::InvalidValue
        );
    });
}

#[test]
fn update_mechanism_counts_decreases_and_cleans() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1);

        // Base subnet exists
        let netuid = NetUid::from(42u16);
        NetworksAdded::<Test>::insert(NetUid::from(42u16), true);

        // Choose counts so result is deterministic.
        let old = MechId::from(3);
        let desired = MechId::from(2u8);
        MechanismCountCurrent::<Test>::insert(netuid, old);

        // Set non-default subnet emission split
        MechanismEmissionSplit::<Test>::insert(netuid, vec![123u16, 234u16, 345u16]);

        // Seed data at a kept mecid (1) and a removed mecid (2)
        let idx_keep = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(1u8));
        let idx_rm3 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(2u8));

        Weights::<Test>::insert(idx_keep, 0u16, vec![(1u16, 1u16)]);
        Incentive::<Test>::insert(idx_keep, vec![1u16]);
        LastUpdate::<Test>::insert(idx_keep, vec![123u64]);
        Bonds::<Test>::insert(idx_keep, 0u16, vec![(1u16, 2u16)]);
        WeightCommits::<Test>::insert(
            idx_keep,
            hotkey,
            VecDeque::from([(sp_core::H256::zero(), 1u64, 2u64, 3u64)]),
        );
        TimelockedWeightCommits::<Test>::insert(
            idx_keep,
            1u64,
            VecDeque::from([(hotkey, 1u64, Default::default(), Default::default())]),
        );

        Weights::<Test>::insert(idx_rm3, 0u16, vec![(9u16, 9u16)]);
        Incentive::<Test>::insert(idx_rm3, vec![9u16]);
        LastUpdate::<Test>::insert(idx_rm3, vec![999u64]);
        Bonds::<Test>::insert(idx_rm3, 0u16, vec![(9u16, 9u16)]);
        WeightCommits::<Test>::insert(
            idx_rm3,
            hotkey,
            VecDeque::from([(sp_core::H256::zero(), 1u64, 2u64, 3u64)]),
        );
        TimelockedWeightCommits::<Test>::insert(
            idx_rm3,
            1u64,
            VecDeque::from([(hotkey, 1u64, Default::default(), Default::default())]),
        );

        // Act
        SubtensorModule::update_mechanism_counts_if_needed(netuid, desired);

        // New count is as desired
        assert_eq!(MechanismCountCurrent::<Test>::get(netuid), desired);

        // Kept prefix intact
        assert_eq!(Incentive::<Test>::get(idx_keep), vec![1u16]);
        assert!(Weights::<Test>::iter_prefix(idx_keep).next().is_some());
        assert!(LastUpdate::<Test>::contains_key(idx_keep));
        assert!(Bonds::<Test>::iter_prefix(idx_keep).next().is_some());
        assert!(WeightCommits::<Test>::contains_key(idx_keep, hotkey));
        assert!(TimelockedWeightCommits::<Test>::contains_key(
            idx_keep, 1u64
        ));

        // Removed prefix (mecid 3) cleared
        assert!(Weights::<Test>::iter_prefix(idx_rm3).next().is_none());
        assert_eq!(Incentive::<Test>::get(idx_rm3), Vec::<u16>::new());
        assert!(!LastUpdate::<Test>::contains_key(idx_rm3));
        assert!(Bonds::<Test>::iter_prefix(idx_rm3).next().is_none());
        assert!(!WeightCommits::<Test>::contains_key(idx_rm3, hotkey));
        assert!(!TimelockedWeightCommits::<Test>::contains_key(
            idx_rm3, 1u64
        ));

        // MechanismEmissionSplit is reset
        assert!(MechanismEmissionSplit::<Test>::get(netuid).is_none());
    });
}

#[test]
fn update_mechanism_counts_increases() {
    new_test_ext(1).execute_with(|| {
        // Base subnet exists
        let netuid = NetUid::from(42u16);
        NetworksAdded::<Test>::insert(NetUid::from(42u16), true);

        // Choose counts
        let old = MechId::from(1u8);
        let desired = MechId::from(2u8);
        MechanismCountCurrent::<Test>::insert(netuid, old);

        // Set non-default subnet emission split
        MechanismEmissionSplit::<Test>::insert(netuid, vec![123u16, 234u16, 345u16]);

        // Act
        SubtensorModule::update_mechanism_counts_if_needed(netuid, desired);

        // New count is as desired
        assert_eq!(MechanismCountCurrent::<Test>::get(netuid), desired);

        // MechanismEmissionSplit is reset
        assert!(MechanismEmissionSplit::<Test>::get(netuid).is_none());
    });
}

#[test]
fn split_emissions_even_division() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(5u16);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(5u8)); // 5 sub-subnets
        let out = SubtensorModule::split_emissions(netuid, AlphaBalance::from(25u64));
        assert_eq!(out, vec![AlphaBalance::from(5u64); 5]);
    });
}

#[test]
fn split_emissions_rounding_to_first() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(6u16);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(4u8)); // 4 sub-subnets
        let out = SubtensorModule::split_emissions(netuid, AlphaBalance::from(10u64)); // 10 / 4 = 2, rem=2
        assert_eq!(
            out,
            vec![
                AlphaBalance::from(4u64), // 2 + remainder(2)
                AlphaBalance::from(2u64),
                AlphaBalance::from(2u64),
                AlphaBalance::from(2u64),
            ]
        );
    });
}

#[test]
fn split_emissions_fibbonacci() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(5u16);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(5u8)); // 5 sub-subnets
        MechanismEmissionSplit::<Test>::insert(netuid, vec![3450, 6899, 10348, 17247, 27594]);
        let out = SubtensorModule::split_emissions(netuid, AlphaBalance::from(19u64));
        assert_eq!(
            out,
            vec![
                AlphaBalance::from(1u64),
                AlphaBalance::from(2u64),
                AlphaBalance::from(3u64),
                AlphaBalance::from(5u64),
                AlphaBalance::from(8u64),
            ]
        );
    });
}

/// Seeds a 2-neuron and 2-mechanism subnet so `epoch_mechanism` produces non-zero
/// incentives & dividends.
/// Returns the sub-subnet storage index.
pub fn mock_epoch_state(netuid: NetUid, ck0: U256, hk0: U256, ck1: U256, hk1: U256) {
    let idx0 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(0));
    let idx1 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(1));

    // Base subnet exists; 2 neurons.
    NetworksAdded::<Test>::insert(NetUid::from(u16::from(netuid)), true);
    MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));
    SubnetworkN::<Test>::insert(netuid, 2);

    // Register two neurons (UID 0,1) → keys drive `get_subnetwork_n`.
    Keys::<Test>::insert(netuid, 0u16, hk0);
    Keys::<Test>::insert(netuid, 1u16, hk1);

    // Make both ACTIVE: recent updates & old registrations.
    Tempo::<Test>::insert(netuid, 1u16);
    ActivityCutoff::<Test>::insert(netuid, u16::MAX); // large cutoff keeps them active
    LastUpdate::<Test>::insert(idx0, vec![2, 2]);
    LastUpdate::<Test>::insert(idx1, vec![2, 2]);
    BlockAtRegistration::<Test>::insert(netuid, 0, 1u64); // registered long ago
    BlockAtRegistration::<Test>::insert(netuid, 1, 1u64);

    // Add stake
    let stake_amount = AlphaBalance::from(1_000_000_000); // 1 Alpha
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hk0,
        &ck0,
        netuid,
        stake_amount,
    );
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hk1,
        &ck1,
        netuid,
        stake_amount,
    );

    // Non-zero stake above threshold; permit both as validators.
    StakeThreshold::<Test>::put(0u64);
    ValidatorPermit::<Test>::insert(netuid, vec![true, true]);

    // Simple weights, setting for each other on both mechanisms
    Weights::<Test>::insert(idx0, 0, vec![(0u16, 0xFFFF), (1u16, 0xFFFF)]);
    Weights::<Test>::insert(idx0, 1, vec![(0u16, 0xFFFF), (1u16, 0xFFFF)]);
    Weights::<Test>::insert(idx1, 0, vec![(0u16, 0xFFFF), (1u16, 0xFFFF)]);
    Weights::<Test>::insert(idx1, 1, vec![(0u16, 0xFFFF), (1u16, 0xFFFF)]);

    // Keep weight masking off for simplicity.
    CommitRevealWeightsEnabled::<Test>::insert(netuid, false);
    Yuma3On::<Test>::insert(netuid, false);
}

pub fn mock_3_neurons(netuid: NetUid, hk: U256) {
    let idx0 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(0));
    let idx1 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(1));

    SubnetworkN::<Test>::insert(netuid, 3);
    Keys::<Test>::insert(netuid, 2u16, hk);
    LastUpdate::<Test>::insert(idx0, vec![2, 2, 2]);
    LastUpdate::<Test>::insert(idx1, vec![2, 2, 2]);
    BlockAtRegistration::<Test>::insert(netuid, 2, 1u64);
}

#[test]
fn epoch_with_mechanisms_produces_per_mechanism_incentive() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        let idx0 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(0));
        let idx1 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(1));
        let ck0 = U256::from(1);
        let hk0 = U256::from(2);
        let ck1 = U256::from(3);
        let hk1 = U256::from(4);
        let emission = AlphaBalance::from(1_000_000_000);

        mock_epoch_state(netuid, ck0, hk0, ck1, hk1);
        SubtensorModule::epoch_with_mechanisms(netuid, emission);

        let actual_incentive_sub0 = Incentive::<Test>::get(idx0);
        let actual_incentive_sub1 = Incentive::<Test>::get(idx1);
        let expected_incentive = 0xFFFF / 2;
        assert_eq!(actual_incentive_sub0[0], expected_incentive);
        assert_eq!(actual_incentive_sub0[1], expected_incentive);
        assert_eq!(actual_incentive_sub1[0], expected_incentive);
        assert_eq!(actual_incentive_sub1[1], expected_incentive);
    });
}

#[test]
fn epoch_with_mechanisms_updates_bonds() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        let idx0 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(0));
        let idx1 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(1));
        let ck0 = U256::from(1);
        let hk0 = U256::from(2);
        let ck1 = U256::from(3);
        let hk1 = U256::from(4);
        let emission = AlphaBalance::from(1_000_000_000);

        mock_epoch_state(netuid, ck0, hk0, ck1, hk1);

        // Cause bonds to be asymmetric on diff mechanisms
        Weights::<Test>::insert(idx1, 0, vec![(0u16, 0xFFFF), (1u16, 0)]);
        Weights::<Test>::insert(idx1, 1, vec![(0u16, 0xFFFF), (1u16, 0xFFFF)]);

        SubtensorModule::epoch_with_mechanisms(netuid, emission);

        let bonds_uid0_sub0 = Bonds::<Test>::get(idx0, 0);
        let bonds_uid1_sub0 = Bonds::<Test>::get(idx0, 1);
        let bonds_uid0_sub1 = Bonds::<Test>::get(idx1, 0);
        let bonds_uid1_sub1 = Bonds::<Test>::get(idx1, 1);

        // Mechanism 0: UID0 fully bonds to UID1, UID1 fully bonds to UID0
        assert_eq!(bonds_uid0_sub0, vec![(1, 65535)]);
        assert_eq!(bonds_uid1_sub0, vec![(0, 65535)]);

        // Mechanism 1: UID0 no bond to UID1, UID1 fully bonds to UID0
        assert_eq!(bonds_uid0_sub1, vec![]);
        assert_eq!(bonds_uid1_sub1, vec![(0, 65535)]);
    });
}

#[test]
fn epoch_with_mechanisms_incentives_proportional_to_weights() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        let idx0 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(0));
        let idx1 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(1));
        let ck0 = U256::from(1);
        let hk0 = U256::from(2);
        let ck1 = U256::from(3);
        let hk1 = U256::from(4);
        let hk2 = U256::from(6);
        let emission = AlphaBalance::from(1_000_000_000);

        mock_epoch_state(netuid, ck0, hk0, ck1, hk1);
        mock_3_neurons(netuid, hk2);

        // Need 3 neurons for this: One validator that will be setting weights to 2 miners
        ValidatorPermit::<Test>::insert(netuid, vec![true, false, false]);

        // Set greater weight to uid1 on sub-subnet 0 and to uid2 on mechanism 1
        Weights::<Test>::insert(idx0, 0, vec![(1u16, 0xFFFF / 5 * 4), (2u16, 0xFFFF / 5)]);
        Weights::<Test>::insert(idx1, 0, vec![(1u16, 0xFFFF / 5), (2u16, 0xFFFF / 5 * 4)]);

        SubtensorModule::epoch_with_mechanisms(netuid, emission);

        let actual_incentive_sub0 = Incentive::<Test>::get(idx0);
        let actual_incentive_sub1 = Incentive::<Test>::get(idx1);

        let expected_incentive_high = 0xFFFF / 5 * 4;
        let expected_incentive_low = 0xFFFF / 5;
        assert_abs_diff_eq!(
            actual_incentive_sub0[1],
            expected_incentive_high,
            epsilon = 1
        );
        assert_abs_diff_eq!(
            actual_incentive_sub0[2],
            expected_incentive_low,
            epsilon = 1
        );
        assert_abs_diff_eq!(
            actual_incentive_sub1[1],
            expected_incentive_low,
            epsilon = 1
        );
        assert_abs_diff_eq!(
            actual_incentive_sub1[2],
            expected_incentive_high,
            epsilon = 1
        );
    });
}

#[test]
fn epoch_with_mechanisms_persists_and_aggregates_all_terms() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        let idx0 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(0));
        let idx1 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(1));

        // Three neurons: validator (uid=0) + two miners (uid=1,2)
        let ck0 = U256::from(1);
        let hk0 = U256::from(2);
        let ck1 = U256::from(3);
        let hk1 = U256::from(4);
        let hk2 = U256::from(6);
        let emission = AlphaBalance::from(1_000_000_000u64);

        // Healthy minimal state and 3rd neuron
        mock_epoch_state(netuid, ck0, hk0, ck1, hk1);
        mock_3_neurons(netuid, hk2);
        let uid0 = 0_usize;
        let uid1 = 1_usize;
        let uid2 = 2_usize;

        // Two sub-subnets with non-equal split (~25% / 75%)
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));
        let split0 = u16::MAX / 4;
        let split1 = u16::MAX - split0;
        MechanismEmissionSplit::<Test>::insert(netuid, vec![split0, split1]);

        // One validator; skew weights differently per sub-subnet
        ValidatorPermit::<Test>::insert(netuid, vec![true, false, false]);
        // sub 0: uid1 heavy, uid2 light
        Weights::<Test>::insert(
            idx0,
            0,
            vec![(1u16, 0xFFFF / 5 * 3), (2u16, 0xFFFF / 5 * 2)],
        );
        // sub 1: uid1 light, uid2 heavy
        Weights::<Test>::insert(idx1, 0, vec![(1u16, 0xFFFF / 5), (2u16, 0xFFFF / 5 * 4)]);

        // Per-sub emissions (and weights used for aggregation)
        let mechanism_emissions = SubtensorModule::split_emissions(netuid, emission);
        let w0 = U64F64::from_num(u64::from(mechanism_emissions[0]))
            / U64F64::from_num(u64::from(emission));
        let w1 = U64F64::from_num(u64::from(mechanism_emissions[1]))
            / U64F64::from_num(u64::from(emission));
        assert_abs_diff_eq!(w0.to_num::<f64>(), 0.25, epsilon = 0.0001);
        assert_abs_diff_eq!(w1.to_num::<f64>(), 0.75, epsilon = 0.0001);

        // Get per-mechanism epoch outputs to build expectations
        let out0 =
            SubtensorModule::epoch_mechanism(netuid, MechId::from(0), mechanism_emissions[0]);
        let out1 =
            SubtensorModule::epoch_mechanism(netuid, MechId::from(1), mechanism_emissions[1]);

        // Now run the real aggregated path (also persists terms)
        let agg = SubtensorModule::epoch_with_mechanisms(netuid, emission);

        // hotkey -> (server_emission_u64, validator_emission_u64)
        let agg_map: BTreeMap<U256, (u64, u64)> = agg
            .into_iter()
            .map(|(hk, se, ve)| (hk, (u64::from(se), u64::from(ve))))
            .collect();

        // Helper to fetch per-sub terms by hotkey
        let terms0 = |hk: &U256| out0.0.get(hk).unwrap();
        let terms1 = |hk: &U256| out1.0.get(hk).unwrap();

        // Returned aggregated emissions match plain sums of mechanism emissions
        for hk in [&hk1, &hk2] {
            let (got_se, got_ve) = agg_map.get(hk).cloned().expect("present");
            let t0 = terms0(hk);
            let t1 = terms1(hk);
            let exp_se = (U64F64::saturating_from_num(u64::from(t0.server_emission))
                + U64F64::saturating_from_num(u64::from(t1.server_emission)))
            .saturating_to_num::<u64>();
            let exp_ve = (U64F64::saturating_from_num(u64::from(t0.validator_emission))
                + U64F64::saturating_from_num(u64::from(t1.validator_emission)))
            .saturating_to_num::<u64>();
            assert_abs_diff_eq!(u64::from(got_se), exp_se, epsilon = 1);
            assert_abs_diff_eq!(u64::from(got_ve), exp_ve, epsilon = 1);
        }

        // Persisted per-mechanism Incentive vectors match per-sub terms
        let inc0 = Incentive::<Test>::get(idx0);
        let inc1 = Incentive::<Test>::get(idx1);
        let exp_inc0 = {
            let mut v = vec![0u16; 3];
            v[terms0(&hk0).uid] = terms0(&hk0).incentive;
            v[terms0(&hk1).uid] = terms0(&hk1).incentive;
            v[terms0(&hk2).uid] = terms0(&hk2).incentive;
            v
        };
        let exp_inc1 = {
            let mut v = vec![0u16; 3];
            v[terms1(&hk0).uid] = terms1(&hk0).incentive;
            v[terms1(&hk1).uid] = terms1(&hk1).incentive;
            v[terms1(&hk2).uid] = terms1(&hk2).incentive;
            v
        };
        for (a, e) in inc0.iter().zip(exp_inc0.iter()) {
            assert_abs_diff_eq!(*a, *e, epsilon = 1);
        }
        for (a, e) in inc1.iter().zip(exp_inc1.iter()) {
            assert_abs_diff_eq!(*a, *e, epsilon = 1);
        }

        // Persisted Bonds for validator (uid0) exist and mirror per-sub terms
        let b0 = Bonds::<Test>::get(idx0, 0u16);
        let b1 = Bonds::<Test>::get(idx1, 0u16);
        let exp_b0 = &terms0(&hk0).bond;
        let exp_b1 = &terms1(&hk0).bond;

        assert!(!b0.is_empty(), "bonds sub0 empty");
        assert!(!b1.is_empty(), "bonds sub1 empty");
        assert_eq!(b0.len(), exp_b0.len());
        assert_eq!(b1.len(), exp_b1.len());
        for ((u_a, w_a), (u_e, w_e)) in b0.iter().zip(exp_b0.iter()) {
            assert_eq!(u_a, u_e);
            assert_abs_diff_eq!(*w_a, *w_e, epsilon = 1);
        }
        for ((u_a, w_a), (u_e, w_e)) in b1.iter().zip(exp_b1.iter()) {
            assert_eq!(u_a, u_e);
            assert_abs_diff_eq!(*w_a, *w_e, epsilon = 1);
        }

        // Persisted subnet-level terms are weighted/OR aggregates of sub-subnets
        // Fetch persisted vectors
        let active = Active::<Test>::get(netuid);
        let emission_v = Emission::<Test>::get(netuid);
        let cons_v = Consensus::<Test>::get(netuid);
        let div_v = Dividends::<Test>::get(netuid);
        let vtrust_v = ValidatorTrust::<Test>::get(netuid);
        let vperm_v = ValidatorPermit::<Test>::get(netuid);

        // Helpers for weighted u16 / u64
        let wu16 = |a: u16, b: u16| -> u16 {
            (U64F64::saturating_from_num(a) * w0 + U64F64::saturating_from_num(b) * w1)
                .saturating_to_num::<u16>()
        };
        let wu64 = |a: u64, b: u64| -> u64 {
            (U64F64::saturating_from_num(a) * w0 + U64F64::saturating_from_num(b) * w1)
                .saturating_to_num::<u64>()
        };

        // For each UID, compute expected aggregate from out0/out1 terms
        let check_uid = |uid: usize, hk: &U256| {
            let t0 = terms0(hk);
            let t1 = terms1(hk);

            // Active & ValidatorPermit are OR-aggregated
            assert_eq!(active[uid], t0.active || t1.active);
            assert_eq!(
                vperm_v[uid],
                t0.new_validator_permit || t1.new_validator_permit
            );

            // Emission (u64)
            let exp_em = wu64(u64::from(t0.emission), u64::from(t1.emission));
            assert_abs_diff_eq!(u64::from(emission_v[uid]), exp_em, epsilon = 1);

            // u16 terms
            assert_abs_diff_eq!(cons_v[uid], wu16(t0.consensus, t1.consensus), epsilon = 1);
            assert_abs_diff_eq!(div_v[uid], wu16(t0.dividend, t1.dividend), epsilon = 1);
            assert_abs_diff_eq!(
                vtrust_v[uid],
                wu16(t0.validator_trust, t1.validator_trust),
                epsilon = 1
            );
        };

        check_uid(uid0, &hk0);
        check_uid(uid1, &hk1);
        check_uid(uid2, &hk2);
    });
}

#[test]
fn epoch_with_mechanisms_no_weight_no_incentive() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        let idx0 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(0));
        let idx1 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(1));
        let ck0 = U256::from(1);
        let hk0 = U256::from(2);
        let ck1 = U256::from(3);
        let hk1 = U256::from(4);
        let hk2 = U256::from(5); // No weight miner
        let emission = AlphaBalance::from(1_000_000_000);

        mock_epoch_state(netuid, ck0, hk0, ck1, hk1);
        mock_3_neurons(netuid, hk2);

        // Need 3 neurons for this: One validator that will be setting weights to 2 miners
        ValidatorPermit::<Test>::insert(netuid, vec![true, false, false]);

        // Set no weight to uid2 on sub-subnet 0 and 1
        Weights::<Test>::insert(idx0, 0, vec![(1u16, 1), (2u16, 0)]);
        Weights::<Test>::insert(idx1, 0, vec![(1u16, 1), (2u16, 0)]);

        SubtensorModule::epoch_with_mechanisms(netuid, emission);

        let actual_incentive_sub0 = Incentive::<Test>::get(idx0);
        let actual_incentive_sub1 = Incentive::<Test>::get(idx1);
        let expected_incentive = 0xFFFF;
        assert_eq!(actual_incentive_sub0[0], 0);
        assert_eq!(actual_incentive_sub0[1], expected_incentive);
        assert_eq!(actual_incentive_sub0[2], 0);
        assert_eq!(actual_incentive_sub1[0], 0);
        assert_eq!(actual_incentive_sub1[1], expected_incentive);
        assert_eq!(actual_incentive_sub1[2], 0);
        assert_eq!(actual_incentive_sub0.len(), 3);
        assert_eq!(actual_incentive_sub1.len(), 3);
    });
}

#[test]
fn neuron_dereg_cleans_weights_across_subids() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(77u16);
        let neuron_uid: u16 = 1; // we'll deregister UID=1
        // two sub-subnets
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));

        // Setup initial map values
        Emission::<Test>::insert(
            netuid,
            vec![
                AlphaBalance::from(1u64),
                AlphaBalance::from(9u64),
                AlphaBalance::from(3u64),
            ],
        );
        Consensus::<Test>::insert(netuid, vec![21u16, 88u16, 44u16]);
        Dividends::<Test>::insert(netuid, vec![7u16, 77u16, 17u16]);

        // Clearing per-mecid maps
        for sub in [0u8, 1u8] {
            let idx = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(sub));

            // Incentive vector: position 1 should become 0
            Incentive::<Test>::insert(idx, vec![10u16, 20u16, 30u16]);

            // Row set BY neuron_uid (to be removed)
            Weights::<Test>::insert(idx, neuron_uid, vec![(0u16, 5u16)]);
            Bonds::<Test>::insert(idx, neuron_uid, vec![(0u16, 6u16)]);

            // Rows FOR neuron_uid inside other validators' vecs => value should be set to 0 (not removed)
            Weights::<Test>::insert(idx, 0u16, vec![(neuron_uid, 7u16), (42u16, 3u16)]);
            Bonds::<Test>::insert(idx, 0u16, vec![(neuron_uid, 8u16), (42u16, 4u16)]);
        }

        // Act
        SubtensorModule::clear_neuron(netuid, neuron_uid);

        // Top-level zeroed at index 1, others intact
        let e = Emission::<Test>::get(netuid);
        assert_eq!(e[0], 1u64.into());
        assert_eq!(e[1], 0u64.into());
        assert_eq!(e[2], 3u64.into());

        let c = Consensus::<Test>::get(netuid);
        assert_eq!(c, vec![21, 0, 44]);

        let d = Dividends::<Test>::get(netuid);
        assert_eq!(d, vec![7, 0, 17]);

        // Per-mecid cleanup
        for sub in [0u8, 1u8] {
            let idx = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(sub));

            // Incentive element at index 1 set to 0
            let inc = Incentive::<Test>::get(idx);
            assert_eq!(inc, vec![10, 0, 30]);

            // Rows BY neuron_uid removed
            assert!(!Weights::<Test>::contains_key(idx, neuron_uid));
            assert!(!Bonds::<Test>::contains_key(idx, neuron_uid));

            // In other rows, entries FOR neuron_uid are zeroed, others unchanged
            let w0 = Weights::<Test>::get(idx, 0u16);
            assert!(w0.iter().any(|&(u, w)| u == neuron_uid && w == 0));
            assert!(w0.iter().any(|&(u, w)| u == 42 && w == 3));
        }
    });
}

#[test]
fn clear_neuron_handles_absent_rows_gracefully() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(55u16);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(1u8)); // single sub-subnet

        // Minimal vectors with non-zero at index 0 (we will clear UID=0)
        Emission::<Test>::insert(netuid, vec![AlphaBalance::from(5u64)]);
        Consensus::<Test>::insert(netuid, vec![6u16]);
        Dividends::<Test>::insert(netuid, vec![7u16]);

        // No Weights/Bonds rows at all → function should not panic
        let neuron_uid: u16 = 0;
        SubtensorModule::clear_neuron(netuid, neuron_uid);

        // Emission/Consensus/Dividends zeroed at index 0
        assert_eq!(
            Emission::<Test>::get(netuid),
            vec![AlphaBalance::from(0u64)]
        );

        assert_eq!(Consensus::<Test>::get(netuid), vec![0u16]);
        assert_eq!(Dividends::<Test>::get(netuid), vec![0u16]);
    });
}

#[test]
fn test_set_mechanism_weights_happy_path_sets_row_under_subid() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        add_network_disable_commit_reveal(netuid, tempo, 0);

        // Register validator (caller) and a destination neuron
        let hk1 = U256::from(55);
        let ck1 = U256::from(66);
        let hk2 = U256::from(77);
        let ck2 = U256::from(88);
        let hk3 = U256::from(99);
        let ck3 = U256::from(111);
        register_ok_neuron(netuid, hk1, ck1, 0);
        register_ok_neuron(netuid, hk2, ck2, 0);
        register_ok_neuron(netuid, hk3, ck3, 0);

        let uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk1).expect("caller uid");
        let uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk2).expect("dest uid 1");
        let uid3 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk3).expect("dest uid 2");

        // Make caller a permitted validator with stake
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_validator_permit_for_uid(netuid, uid1, true);
        add_balance_to_coldkey_account(&ck1, 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &ck1,
            netuid,
            1.into(),
        );

        // Have at least two sub-subnets; write under mecid = 1
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));
        let mecid = MechId::from(1u8);

        // Call extrinsic
        let dests = vec![uid2, uid3];
        let weights = vec![88u16, 0xFFFF];
        assert_ok!(SubtensorModule::set_mechanism_weights(
            RawOrigin::Signed(hk1).into(),
            netuid,
            mecid,
            dests.clone(),
            weights.clone(),
            0, // version_key
        ));

        // Verify row exists under the chosen mecid and not under a different mecid
        let idx1 = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        assert_eq!(
            Weights::<Test>::get(idx1, uid1),
            vec![(uid2, 88u16), (uid3, 0xFFFF)]
        );

        let idx0 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(0u8));
        assert!(Weights::<Test>::get(idx0, uid1).is_empty());
    });
}

#[test]
fn test_set_mechanism_weights_above_mechanism_count_fails() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        add_network_disable_commit_reveal(netuid, tempo, 0);

        // Register validator (caller) and a destination neuron
        let hk1 = U256::from(55);
        let ck1 = U256::from(66);
        let hk2 = U256::from(77);
        let ck2 = U256::from(88);
        register_ok_neuron(netuid, hk1, ck1, 0);
        register_ok_neuron(netuid, hk2, ck2, 0);

        let uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk1).expect("caller uid");
        let uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk2).expect("dest uid 1");

        // Make caller a permitted validator with stake
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_validator_permit_for_uid(netuid, uid1, true);
        add_balance_to_coldkey_account(&ck1, 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &ck1,
            netuid,
            1.into(),
        );

        // Have exactly two sub-subnets; write under mecid = 1
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));
        let subid_above = MechId::from(2u8);

        // Call extrinsic
        let dests = vec![uid2];
        let weights = vec![88u16];
        assert_noop!(
            SubtensorModule::set_mechanism_weights(
                RawOrigin::Signed(hk1).into(),
                netuid,
                subid_above,
                dests.clone(),
                weights.clone(),
                0, // version_key
            ),
            Error::<Test>::MechanismDoesNotExist
        );
    });
}

#[test]
fn test_commit_reveal_mechanism_weights_ok() {
    new_test_ext(1).execute_with(|| {
        System::set_block_number(0);

        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        add_network(netuid, tempo, 0);

        // Three neurons: validator (caller) + two destinations
        let hk1 = U256::from(55);
        let ck1 = U256::from(66);
        let hk2 = U256::from(77);
        let ck2 = U256::from(88);
        let hk3 = U256::from(99);
        let ck3 = U256::from(111);
        register_ok_neuron(netuid, hk1, ck1, 0);
        register_ok_neuron(netuid, hk2, ck2, 0);
        register_ok_neuron(netuid, hk3, ck3, 0);

        let uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk1).unwrap(); // caller
        let uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk2).unwrap();
        let uid3 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk3).unwrap();

        // Enable commit-reveal path and make caller a validator with stake
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
        SubtensorModule::set_validator_permit_for_uid(netuid, uid1, true);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        add_balance_to_coldkey_account(&ck1, 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &ck1,
            netuid,
            1.into(),
        );

        // Ensure sub-subnet exists; write under mecid = 1
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));
        let mecid = MechId::from(1u8);
        let idx0 = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(0u8));
        let idx1 = SubtensorModule::get_mechanism_storage_index(netuid, mecid);

        // Prepare payload and commit hash (include mecid!)
        let dests = vec![uid2, uid3];
        let weights = vec![88u16, 0xFFFFu16];
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let version_key: u64 = 0;
        let commit_hash: H256 = BlakeTwo256::hash_of(&(
            hk1,
            idx1,
            dests.clone(),
            weights.clone(),
            salt.clone(),
            version_key,
        ));

        // Commit in epoch 0
        assert_ok!(SubtensorModule::commit_mechanism_weights(
            RuntimeOrigin::signed(hk1),
            netuid,
            mecid,
            commit_hash
        ));

        // Advance one epoch, then reveal
        step_epochs(1, netuid);
        assert_ok!(SubtensorModule::reveal_mechanism_weights(
            RuntimeOrigin::signed(hk1),
            netuid,
            mecid,
            dests.clone(),
            weights.clone(),
            salt,
            version_key
        ));

        // Verify weights stored under the chosen mecid (normalized keeps max=0xFFFF here)
        assert_eq!(
            Weights::<Test>::get(idx1, uid1),
            vec![(uid2, 88u16), (uid3, 0xFFFFu16)]
        );

        // And not under a different mecid
        assert!(Weights::<Test>::get(idx0, uid1).is_empty());
    });
}

#[test]
fn test_commit_reveal_above_mechanism_count_fails() {
    new_test_ext(1).execute_with(|| {
        System::set_block_number(0);

        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        add_network(netuid, tempo, 0);

        // Two neurons: validator (caller) + miner
        let hk1 = U256::from(55);
        let ck1 = U256::from(66);
        let hk2 = U256::from(77);
        let ck2 = U256::from(88);
        register_ok_neuron(netuid, hk1, ck1, 0);
        register_ok_neuron(netuid, hk2, ck2, 0);

        let uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk1).unwrap(); // caller
        let uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk2).unwrap();

        // Enable commit-reveal path and make caller a validator with stake
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
        SubtensorModule::set_validator_permit_for_uid(netuid, uid1, true);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        add_balance_to_coldkey_account(&ck1, 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &ck1,
            netuid,
            1.into(),
        );

        // Ensure there are two mechanisms: 0 and 1
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));
        let subid_above = MechId::from(2u8); // non-existing sub-subnet
        let idx2 = SubtensorModule::get_mechanism_storage_index(netuid, subid_above);

        // Prepare payload and commit hash
        let dests = vec![uid2];
        let weights = vec![88u16];
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let version_key: u64 = 0;
        let commit_hash: H256 = BlakeTwo256::hash_of(&(
            hk1,
            idx2,
            dests.clone(),
            weights.clone(),
            salt.clone(),
            version_key,
        ));

        // Commit in epoch 0
        assert_noop!(
            SubtensorModule::commit_mechanism_weights(
                RuntimeOrigin::signed(hk1),
                netuid,
                subid_above,
                commit_hash
            ),
            Error::<Test>::MechanismDoesNotExist
        );

        // Advance one epoch, then attempt to reveal
        step_epochs(1, netuid);
        assert_noop!(
            SubtensorModule::reveal_mechanism_weights(
                RuntimeOrigin::signed(hk1),
                netuid,
                subid_above,
                dests.clone(),
                weights.clone(),
                salt,
                version_key
            ),
            Error::<Test>::NoWeightsCommitFound
        );

        // Verify that weights didn't update
        assert!(Weights::<Test>::get(idx2, uid1).is_empty());
        assert!(Weights::<Test>::get(idx2, uid2).is_empty());
    });
}

#[test]
fn test_reveal_crv3_commits_sub_success() {
    new_test_ext(100).execute_with(|| {
        System::set_block_number(0);

        let netuid = NetUid::from(1);
        let mecid  = MechId::from(1u8); // write under sub-subnet #1
        let hotkey1: AccountId = U256::from(1);
        let hotkey2: AccountId = U256::from(2);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        // ensure we actually have mecid=1 available
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));

        // Register neurons and set up configs
        register_ok_neuron(netuid, hotkey1, U256::from(3), 100_000);
        register_ok_neuron(netuid, hotkey2, U256::from(4), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 3));

        let uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey1).expect("uid1");
        let uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey2).expect("uid2");

        SubtensorModule::set_validator_permit_for_uid(netuid, uid1, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, uid2, true);
        add_balance_to_coldkey_account(&U256::from(3), 1.into());
        add_balance_to_coldkey_account(&U256::from(4), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &U256::from(3), netuid, 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(&hotkey2, &U256::from(4), netuid, 1.into());

        let version_key = SubtensorModule::get_weights_version_key(netuid);

        // Payload (same as legacy; mecid is provided to the extrinsic)
        let payload = WeightsTlockPayload {
            hotkey: hotkey1.encode(),
            values: vec![10, 20],
            uids: vec![uid1, uid2],
            version_key,
        };
        let serialized_payload = payload.encode();

        // Public key + encrypt
        let esk = [2; 32];
        let rng = ChaCha20Rng::seed_from_u64(0);
        let pk_bytes = hex::decode("83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a").unwrap();
        let pub_key = <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes).unwrap();

        let message = {
            let mut hasher = sha2::Sha256::new();
            hasher.update(reveal_round.to_be_bytes());
            hasher.finalize().to_vec()
        };
        let identity = Identity::new(b"", vec![message]);

        let ct = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(pub_key, esk, &serialized_payload, identity, rng).expect("encrypt");
        let mut commit_bytes = Vec::new();
        ct.serialize_compressed(&mut commit_bytes).expect("serialize");

        // Commit (sub variant)
        assert_ok!(SubtensorModule::commit_timelocked_mechanism_weights(
            RuntimeOrigin::signed(hotkey1),
            netuid,
            mecid,
            commit_bytes.clone().try_into().expect("bounded"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        // Inject drand pulse for the reveal round
        let sig_bytes = hex::decode("b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39").unwrap();
        pallet_drand::Pulses::<Test>::insert(
            reveal_round,
            Pulse {
                round: reveal_round,
                randomness: vec![0; 32].try_into().unwrap(),
                signature: sig_bytes.try_into().unwrap(),
            },
        );

        // Run epochs so the commit is processed
        step_epochs(3, netuid);

        // Verify weights applied under the selected mecid index
        let idx = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        let weights_sparse = SubtensorModule::get_weights_sparse(idx);
        let row = weights_sparse.get(uid1 as usize).cloned().unwrap_or_default();
        assert!(!row.is_empty(), "expected weights set for validator uid1 under mecid");

        // Compare rounded normalized weights to expected proportions (like legacy test)
        let expected: Vec<(u16, I32F32)> = payload.uids.iter().zip(payload.values.iter()).map(|(&u,&v)|(u, I32F32::from_num(v))).collect();
        let total: I32F32 = row.iter().map(|(_, w)| *w).sum();
        let normalized: Vec<(u16, I32F32)> = row.iter().map(|&(u,w)| (u, w * I32F32::from_num(30) / total)).collect();

        for ((ua, wa), (ub, wb)) in normalized.iter().zip(expected.iter()) {
            assert_eq!(ua, ub);
            let actual = wa.to_num::<f64>().round() as i64;
            let expect = wb.to_num::<i64>();
            assert_ne!(actual, 0, "actual weight for uid {ua} is zero");
            assert_eq!(actual, expect, "weight mismatch for uid {ua}");
        }
    });
}

#[test]
fn test_crv3_above_mechanism_count_fails() {
    new_test_ext(100).execute_with(|| {
        System::set_block_number(0);

        let netuid = NetUid::from(1);
        let subid_above  = MechId::from(2u8); // non-existing sub-subnet
        let hotkey1: AccountId = U256::from(1);
        let hotkey2: AccountId = U256::from(2);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        // ensure we actually have mecid=1 available
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));

        // Register neurons and set up configs
        register_ok_neuron(netuid, hotkey1, U256::from(3), 100_000);
        register_ok_neuron(netuid, hotkey2, U256::from(4), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 3));

        let uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey1).expect("uid1");
        let uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey2).expect("uid2");

        SubtensorModule::set_validator_permit_for_uid(netuid, uid1, true);
        add_balance_to_coldkey_account(&U256::from(3), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &U256::from(3), netuid, 1.into());

        let version_key = SubtensorModule::get_weights_version_key(netuid);

        // Payload (same as legacy; mecid is provided to the extrinsic)
        let payload = WeightsTlockPayload {
            hotkey: hotkey1.encode(),
            values: vec![10, 20],
            uids: vec![uid1, uid2],
            version_key,
        };
        let serialized_payload = payload.encode();

        // Public key + encrypt
        let esk = [2; 32];
        let rng = ChaCha20Rng::seed_from_u64(0);
        let pk_bytes = hex::decode("83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a").unwrap();
        let pub_key = <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes).unwrap();

        let message = {
            let mut hasher = sha2::Sha256::new();
            hasher.update(reveal_round.to_be_bytes());
            hasher.finalize().to_vec()
        };
        let identity = Identity::new(b"", vec![message]);

        let ct = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(pub_key, esk, &serialized_payload, identity, rng).expect("encrypt");
        let mut commit_bytes = Vec::new();
        ct.serialize_compressed(&mut commit_bytes).expect("serialize");

        // Commit (sub variant)
        assert_noop!(
            SubtensorModule::commit_timelocked_mechanism_weights(
                RuntimeOrigin::signed(hotkey1),
                netuid,
                subid_above,
                commit_bytes.clone().try_into().expect("bounded"),
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::MechanismDoesNotExist
        );
    });
}

#[test]
fn test_do_commit_crv3_mechanism_weights_committing_too_fast() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let mecid = MechId::from(1u8);
        let hotkey: AccountId = U256::from(1);
        let commit_data_1: Vec<u8> = vec![1, 2, 3];
        let commit_data_2: Vec<u8> = vec![4, 5, 6];
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8)); // allow subids {0,1}

        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).expect("uid");
        let idx1 = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        SubtensorModule::set_last_update_for_uid(idx1, uid, 0);

        // make validator with stake
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_validator_permit_for_uid(netuid, uid, true);
        add_balance_to_coldkey_account(&U256::from(2), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &U256::from(2),
            netuid,
            1.into(),
        );

        // first commit OK on mecid=1
        assert_ok!(SubtensorModule::commit_timelocked_mechanism_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            mecid,
            commit_data_1.clone().try_into().expect("bounded"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        // immediate second commit on SAME mecid blocked
        assert_noop!(
            SubtensorModule::commit_timelocked_mechanism_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                mecid,
                commit_data_2.clone().try_into().expect("bounded"),
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::CommittingWeightsTooFast
        );

        // BUT committing too soon on a DIFFERENT mecid is allowed
        let other_subid = MechId::from(0u8);
        let idx0 = SubtensorModule::get_mechanism_storage_index(netuid, other_subid);
        SubtensorModule::set_last_update_for_uid(idx0, uid, 0); // baseline like above
        assert_ok!(SubtensorModule::commit_timelocked_mechanism_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            other_subid,
            commit_data_2.clone().try_into().expect("bounded"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        // still too fast on original mecid after 2 blocks
        step_block(2);
        assert_noop!(
            SubtensorModule::commit_timelocked_mechanism_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                mecid,
                commit_data_2.clone().try_into().expect("bounded"),
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::CommittingWeightsTooFast
        );

        // after enough blocks, OK again on original mecid
        step_block(3);
        assert_ok!(SubtensorModule::commit_timelocked_mechanism_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            mecid,
            commit_data_2.try_into().expect("bounded"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));
    });
}

#[test]
fn epoch_mechanism_emergency_mode_distributes_by_stake() {
    new_test_ext(1).execute_with(|| {
        // setup a single sub-subnet where consensus sum becomes 0
        let netuid = NetUid::from(1u16);
        let mecid = MechId::from(1u8);
        let idx = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        let tempo: u16 = 5;
        add_network(netuid, tempo, 0);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8)); // allow subids {0,1}
        SubtensorModule::set_max_registrations_per_block(netuid, 4);
        SubtensorModule::set_target_registrations_per_interval(netuid, 4);

        // three neurons: make ALL permitted validators so active_stake is non-zero
        let hk0 = U256::from(10);
        let ck0 = U256::from(11);
        let hk1 = U256::from(20);
        let ck1 = U256::from(21);
        let hk2 = U256::from(30);
        let ck2 = U256::from(31);
        let hk3 = U256::from(40); // miner
        let ck3 = U256::from(41);
        register_ok_neuron(netuid, hk0, ck0, 0);
        register_ok_neuron(netuid, hk1, ck1, 0);
        register_ok_neuron(netuid, hk2, ck2, 0);
        register_ok_neuron(netuid, hk3, ck3, 0);

        // active + recent updates so they're all active
        let now = SubtensorModule::get_current_block_as_u64();
        ActivityCutoff::<Test>::insert(netuid, 1_000u16);
        LastUpdate::<Test>::insert(idx, vec![now, now, now, now]);

        // All staking validators permitted => active_stake = stake
        ValidatorPermit::<Test>::insert(netuid, vec![true, true, true, false]);
        SubtensorModule::set_stake_threshold(0);

        // force ZERO consensus/incentive path: no weights/bonds
        // (leave Weights/Bonds empty for all rows on this sub-subnet)

        // stake proportions: uid0:uid1:uid2 = 10:30:60
        add_balance_to_coldkey_account(&ck0, 10.into());
        add_balance_to_coldkey_account(&ck1, 30.into());
        add_balance_to_coldkey_account(&ck2, 60.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk0,
            &ck0,
            netuid,
            AlphaBalance::from(10),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &ck1,
            netuid,
            AlphaBalance::from(30),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk2,
            &ck2,
            netuid,
            AlphaBalance::from(60),
        );

        let emission = AlphaBalance::from(1_000_000u64);

        // --- act: run epoch on this sub-subnet only ---
        let out = SubtensorModule::epoch_mechanism(netuid, mecid, emission);

        // collect validator emissions per hotkey
        let t0 = out.0.get(&hk0).unwrap();
        let t1 = out.0.get(&hk1).unwrap();
        let t2 = out.0.get(&hk2).unwrap();
        let t3 = out.0.get(&hk3).unwrap();

        // In emergency mode (consensus sum == 0):
        //  - validator_emission is distributed by (active) stake proportions
        //  - server_emission remains zero (incentive path is zero)
        assert_eq!(u64::from(t0.server_emission), 0);
        assert_eq!(u64::from(t1.server_emission), 0);
        assert_eq!(u64::from(t2.server_emission), 0);
        assert_eq!(u64::from(t3.server_emission), 0);

        // expected splits by stake: 10%, 30%, 60% of total emission
        let e = u64::from(emission);
        let exp0 = e / 10; // 10%
        let exp1 = e * 3 / 10; // 30%
        let exp2 = e * 6 / 10; // 60%

        // allow tiny rounding drift from fixed-point conversions
        assert_abs_diff_eq!(u64::from(t0.validator_emission), exp0, epsilon = 2);
        assert_abs_diff_eq!(u64::from(t1.validator_emission), exp1, epsilon = 2);
        assert_abs_diff_eq!(u64::from(t2.validator_emission), exp2, epsilon = 2);
        assert_eq!(u64::from(t3.validator_emission), 0);

        // all emission goes to validators
        assert_abs_diff_eq!(
            u64::from(t0.validator_emission)
                + u64::from(t1.validator_emission)
                + u64::from(t2.validator_emission),
            e,
            epsilon = 2
        );
    });
}
