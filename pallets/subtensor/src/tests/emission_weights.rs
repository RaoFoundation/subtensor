#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use super::mock::*;
use crate::migrations::migrate_remove_subnet_tao_weight::{
    MIGRATION_NAME as REMOVE_TAO_WEIGHT_MIGRATION_NAME, migrate_remove_subnet_tao_weight,
    subnet_tao_weight_entries, subnet_tao_weight_prefix,
};
use crate::migrations::migrate_seed_subnet_emission_weights::{
    MIGRATION_NAME, migrate_seed_subnet_emission_weights,
};
use crate::*;
use alloc::collections::BTreeMap;
use approx::assert_abs_diff_eq;
use codec::Encode;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::SwapHandler;

const ONE: u64 = EMISSION_WEIGHT_ONE;
const BURN: u16 = EmissionIndex::Burn as u16;
const RECYCLE: u16 = EmissionIndex::Recycle as u16;
const OWNER: u16 = EmissionIndex::Owner as u16;
const VALIDATOR: u16 = EmissionIndex::Validator as u16;
const MINER: u16 = EmissionIndex::Miner as u16;

/// Funded budgets remain authoritative through Yuma scoring and actual stake credits.
#[test]
fn test_yuma_credits_funded_miner_budget() {
    let cases = [
        (
            "SN35/36",
            133_199_999_640u64,
            108_000_000_000u64,
            118_800_000_000u64,
        ),
        ("SN41", 180_000_000_000, 144_000_000_000, 36_000_000_000),
        ("SN70", 122_399_999_640, 118_800_000_000, 118_800_000_000),
        ("SN122", 230_399_999_640, 64_800_000_000, 64_800_000_000),
        ("equal-control", 180_000_000_000, 180_000_000_000, 0),
        (
            "validator-heavy",
            118_800_000_000,
            180_000_000_000,
            61_200_000_000,
        ),
        ("one-unit", 1, 1, 1),
        ("zero-miner", 0, 1_000_000, 0),
        ("zero", 0, 0, 0),
    ];
    for yuma3 in [false, true] {
        for has_weights in [false, true] {
            for (label, miner_budget, validator_budget, owner_budget) in cases {
                new_test_ext(1).execute_with(|| {
                    let owner_hot = U256::from(100);
                    let owner_cold = U256::from(101);
                    let validator_hot = U256::from(200);
                    let validator_cold = U256::from(201);
                    let miner_hot = U256::from(300);
                    let miner_cold = U256::from(301);
                    let netuid = add_dynamic_network_disable_commit_reveal(&owner_hot, &owner_cold);
                    remove_owner_registration_stake(netuid);
                    SubtensorModule::set_burn(netuid, TaoBalance::ZERO);
                    register_ok_neuron(netuid, validator_hot, validator_cold, 0);
                    register_ok_neuron(netuid, miner_hot, miner_cold, 0);
                    SubtensorModule::set_yuma3_enabled(netuid, yuma3);
                    SubtensorModule::set_tao_weight(0);
                    SubtensorModule::set_max_allowed_validators(netuid, 1);
                    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                        &validator_hot, &validator_cold, netuid, 1_000_000_000_000u64.into(),
                    );
                    let validator_uid = Uids::<Test>::get(netuid, validator_hot).unwrap();
                    let miner_uid = Uids::<Test>::get(netuid, miner_hot).unwrap();
                    SubtensorModule::set_validator_permit_for_uid(netuid, validator_uid, true);
                    let index = subtensor_runtime_common::NetUidStorageIndex::from(netuid);
                    if has_weights {
                        Weights::<Test>::insert(index, validator_uid, vec![(miner_uid, u16::MAX)]);
                        SubtensorModule::set_last_update_for_uid(index, validator_uid, 99);
                    }
                    LastMechansimStepBlock::<Test>::insert(netuid, 50);
                    System::set_block_number(100);
                    System::reset_events();
                    let stake = |hot, cold| SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &hot, &cold, netuid,
                    ).to_u64();
                    let miner_before = stake(miner_hot, miner_cold);
                    let validator_before = stake(validator_hot, validator_cold);
                    let owner_before = stake(owner_hot, owner_cold);
                    SubtensorModule::distribute_emission(
                        netuid, miner_budget.into(), validator_budget.into(),
                        AlphaBalance::ZERO, owner_budget.into(),
                    );
                    let miner_credit = stake(miner_hot, miner_cold) - miner_before;
                    let validator_credit = stake(validator_hot, validator_cold) - validator_before;
                    let owner_credit = stake(owner_hot, owner_cold) - owner_before;
                    let event_total: u64 = System::events().iter().filter_map(|record| {
                        if let RuntimeEvent::SubtensorModule(Event::IncentiveAlphaEmittedToMiners { emissions, .. }) = &record.event {
                            Some(emissions.iter().map(|amount| amount.to_u64()).sum::<u64>())
                        } else { None }
                    }).sum();
                    let expected_miner = if has_weights { miner_budget } else { 0 };
                    let expected_validator = if has_weights {
                        validator_budget
                    } else {
                        validator_budget + miner_budget
                    };
                    assert_eq!(miner_credit, expected_miner,
                        "{label}, yuma3={yuma3}, weights={has_weights}");
                    assert_eq!(event_total, miner_credit);
                    assert!(validator_credit.abs_diff(expected_validator) <= 2,
                        "{label}, yuma3={yuma3}, weights={has_weights}: validator {validator_credit} vs {expected_validator}");
                    assert_eq!(owner_credit, owner_budget);
                    assert!((miner_credit + validator_credit + owner_credit)
                        .abs_diff(miner_budget + validator_budget + owner_budget) <= 2);
                    assert_total_alpha_staked_invariant(netuid);
                });
            }
        }
    }
}

/// `pct` percent of emission in u64 parts.
fn pct(p: u64) -> u64 {
    emission_fraction(p, 100)
}

fn sum(weights: &EmissionWeights) -> u128 {
    weights.iter().map(|w| *w as u128).sum()
}

fn full_vector(owner: u64, validator: u64) -> (Vec<u16>, Vec<u64>) {
    full_vector_with_burn_recycle(0, 0, owner, validator)
}

fn full_vector_with_burn_recycle(
    burn: u64,
    recycle: u64,
    owner: u64,
    validator: u64,
) -> (Vec<u16>, Vec<u64>) {
    let miner = ONE - burn - recycle - owner - validator;
    (
        vec![BURN, RECYCLE, OWNER, VALIDATOR, MINER],
        vec![burn, recycle, owner, validator, miner],
    )
}

fn new_subnet() -> NetUid {
    add_dynamic_network(&U256::from(1), &U256::from(2))
}

// ---------------------------------------------------------------------------
// Defaults and encoding
// ---------------------------------------------------------------------------

#[test]
fn test_constants_are_consistent() {
    assert_eq!(MAX_OWNER_EMISSION, pct(33));
    assert_eq!(MAX_BURN_RECYCLE_EMISSION, pct(33));
    assert_eq!(MAX_VALIDATOR_EMISSION, pct(50));
    assert_eq!(MIN_MINER_EMISSION, pct(33));
    assert_eq!(emission_fraction(1, 1), ONE);
    assert_eq!(emission_fraction(0, 1), 0);
    assert_eq!(EmissionIndex::ALL.len(), EMISSION_WEIGHT_SLOTS as usize);
    for (position, index) in EmissionIndex::ALL.iter().enumerate() {
        assert_eq!(index.position(), position);
        assert_eq!(EmissionIndex::from_u16(position as u16), Some(*index));
    }
    assert_eq!(EmissionIndex::from_u16(EMISSION_WEIGHT_SLOTS as u16), None);
}

#[test]
fn test_new_subnet_is_seeded_with_default_vector() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        let weights = SubtensorModule::get_subnet_emission_weights(netuid);
        let default = SubtensorModule::default_emission_weights();
        assert_eq!(weights, default);
        assert_eq!(sum(&weights), ONE as u128);
        assert_eq!(weights[0], 0);
        assert_eq!(weights[1], 0);
        // Mock owner cut is 0%: the remainder is split 50/50.
        assert_eq!(weights[2], 0);
        assert_eq!(weights[3], ONE / 2);
        assert_eq!(weights[4], ONE - ONE / 2);
        assert!(!SubtensorModule::get_owner_cut_enabled(netuid));
    });
}

#[test]
fn test_default_vector_tracks_initial_owner_cut() {
    new_test_ext(1).execute_with(|| {
        InitialSubnetOwnerCut::set(11_796); // mainnet 18%
        let owner = SubtensorModule::default_owner_emission();
        assert_eq!(
            owner,
            SubtensorModule::scale_u16_fraction_to_emission_weight(11_796)
        );
        let weights = SubtensorModule::default_emission_weights();
        assert_eq!(weights[2], owner);
        assert_eq!(weights[3], (ONE - owner) / 2);
        assert_eq!(sum(&weights), ONE as u128);
        assert_eq!(SubtensorModule::leased_subnet_min_owner_emission(), owner);

        let netuid = new_subnet();
        assert_eq!(
            SubtensorModule::get_subnet_emission_weights(netuid),
            weights
        );
        assert!(SubtensorModule::get_owner_cut_enabled(netuid));
        InitialSubnetOwnerCut::set(0);
    });
}

#[test]
fn test_named_getters_mirror_slots() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        let weights = [0, 0, pct(10), pct(45), ONE - pct(10) - pct(45)];
        SubtensorModule::set_subnet_emission_weights_unchecked(netuid, weights);
        assert_eq!(SubtensorModule::get_subnet_burn_emission(netuid), 0);
        assert_eq!(SubtensorModule::get_subnet_recycle_emission(netuid), 0);
        assert_eq!(
            SubtensorModule::get_subnet_owner_emission_cut(netuid),
            pct(10)
        );
        assert_eq!(
            SubtensorModule::get_subnet_validator_emission_cut(netuid),
            pct(45)
        );
        assert_eq!(
            SubtensorModule::get_subnet_miner_emission_cut(netuid),
            weights[4]
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_float_subnet_owner_emission_cut(netuid).to_num::<f64>(),
            0.10,
            epsilon = 1e-9
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_float_subnet_validator_emission_cut(netuid).to_num::<f64>(),
            0.45,
            epsilon = 1e-9
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_float_subnet_miner_emission_cut(netuid).to_num::<f64>(),
            0.45,
            epsilon = 1e-9
        );
        assert!(SubtensorModule::get_owner_cut_enabled(netuid));
    });
}

// ---------------------------------------------------------------------------
// Sparse merge semantics
// ---------------------------------------------------------------------------

#[test]
fn test_merge_keeps_omitted_slots_including_burn_and_recycle() {
    let current = [7, 9, pct(10), pct(45), pct(45)];
    let merged =
        SubtensorModule::merge_emission_weight_updates(current, &[VALIDATOR], &[pct(40)]).unwrap();
    assert_eq!(merged, [7, 9, pct(10), pct(40), pct(45)]);
}

#[test]
fn test_merge_rejects_malformed_updates() {
    let current = SubtensorModule::default_emission_weights();
    assert_eq!(
        SubtensorModule::merge_emission_weight_updates(current, &[OWNER], &[]),
        Err(Error::<Test>::EmissionWeightsLengthMismatch)
    );
    assert_eq!(
        SubtensorModule::merge_emission_weight_updates(current, &[5], &[1]),
        Err(Error::<Test>::EmissionWeightIndexOutOfRange)
    );
    assert_eq!(
        SubtensorModule::merge_emission_weight_updates(current, &[OWNER, OWNER], &[1, 2]),
        Err(Error::<Test>::EmissionWeightIndexDuplicated)
    );
}

// ---------------------------------------------------------------------------
// Rules: one error per constraint
// ---------------------------------------------------------------------------

#[test]
fn test_set_rejects_unknown_or_root_subnet() {
    new_test_ext(1).execute_with(|| {
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(NetUid::from(42), vec![OWNER], vec![0]),
            Error::<Test>::SubnetNotExists
        );
        add_network(NetUid::ROOT, 1, 0);
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(NetUid::ROOT, vec![OWNER], vec![0]),
            Error::<Test>::EmissionWeightsNotSettableOnRoot
        );
    });
}

#[test]
fn test_set_accepts_nonzero_burn_and_recycle_within_combined_cap() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        // Burn alone at the 33% combined cap, with room for miner >= 33%.
        let (uids, values) =
            full_vector_with_burn_recycle(MAX_BURN_RECYCLE_EMISSION, 0, 0, pct(34));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        assert_eq!(
            SubtensorModule::get_subnet_burn_emission(netuid),
            MAX_BURN_RECYCLE_EMISSION
        );
        assert_eq!(SubtensorModule::get_subnet_recycle_emission(netuid), 0);

        // Recycle alone at the same inclusive bound.
        let (uids, values) =
            full_vector_with_burn_recycle(0, MAX_BURN_RECYCLE_EMISSION, 0, pct(34));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        assert_eq!(SubtensorModule::get_subnet_burn_emission(netuid), 0);
        assert_eq!(
            SubtensorModule::get_subnet_recycle_emission(netuid),
            MAX_BURN_RECYCLE_EMISSION
        );

        // Split across both slots: 20% + 13% = 33%.
        let (uids, values) = full_vector_with_burn_recycle(pct(20), pct(13), 0, pct(34));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        assert_eq!(SubtensorModule::get_subnet_burn_emission(netuid), pct(20));
        assert_eq!(
            SubtensorModule::get_subnet_recycle_emission(netuid),
            pct(13)
        );
    });
}

#[test]
fn test_set_rejects_burn_plus_recycle_above_combined_maximum() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        // Exactly one part over the 33% combined cap.
        let (uids, values) =
            full_vector_with_burn_recycle(MAX_BURN_RECYCLE_EMISSION + 1, 0, 0, pct(34));
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values),
            Error::<Test>::BurnRecycleEmissionAboveMaximum
        );
        let (uids, values) =
            full_vector_with_burn_recycle(0, MAX_BURN_RECYCLE_EMISSION + 1, 0, pct(34));
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values),
            Error::<Test>::BurnRecycleEmissionAboveMaximum
        );
        // Split that exceeds: 20% + 14% = 34%.
        let (uids, values) = full_vector_with_burn_recycle(pct(20), pct(14), 0, pct(33));
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values),
            Error::<Test>::BurnRecycleEmissionAboveMaximum
        );
    });
}

#[test]
fn test_sparse_update_preserves_omitted_burn_and_recycle() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        let (uids, values) = full_vector_with_burn_recycle(pct(10), pct(5), pct(10), pct(40));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        // Touch only owner/validator/miner; burn and recycle must stay.
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid,
            vec![OWNER, VALIDATOR, MINER],
            vec![pct(5), pct(40), ONE - pct(10) - pct(5) - pct(5) - pct(40)]
        ));
        let weights = SubtensorModule::get_subnet_emission_weights(netuid);
        assert_eq!(weights[0], pct(10));
        assert_eq!(weights[1], pct(5));
        assert_eq!(weights[2], pct(5));
        assert_eq!(weights[3], pct(40));
        assert_eq!(sum(&weights), ONE as u128);
    });
}

#[test]
fn test_set_rejects_owner_above_maximum() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        let (uids, values) = full_vector(MAX_OWNER_EMISSION + 1, pct(34));
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values),
            Error::<Test>::OwnerEmissionAboveMaximum
        );
        // The bound is inclusive.
        let (uids, values) = full_vector(MAX_OWNER_EMISSION, pct(34));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        assert_eq!(
            SubtensorModule::get_subnet_owner_emission_cut(netuid),
            MAX_OWNER_EMISSION
        );
    });
}

#[test]
fn test_set_rejects_validator_above_maximum() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        let (uids, values) = full_vector(0, MAX_VALIDATOR_EMISSION + 1);
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values),
            Error::<Test>::ValidatorEmissionAboveMaximum
        );
        let (uids, values) = full_vector(0, MAX_VALIDATOR_EMISSION);
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
    });
}

#[test]
fn test_set_rejects_miner_below_minimum() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        // owner 33% + validator 34.1% leaves the miner below 33%.
        let (uids, values) = full_vector(pct(33), pct(34) + pct(1));
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values),
            Error::<Test>::MinerEmissionBelowMinimum
        );
        let (uids, values) = full_vector(pct(33), pct(34));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        assert!(SubtensorModule::get_subnet_miner_emission_cut(netuid) >= MIN_MINER_EMISSION);
    });
}

#[test]
fn test_set_rejects_vector_not_summing_to_one() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        // Each slot individually valid, but the total is one ulp short.
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(
                netuid,
                vec![OWNER, VALIDATOR, MINER],
                vec![pct(10), pct(45), ONE - pct(10) - pct(45) - 1]
            ),
            Error::<Test>::EmissionWeightsDoNotSumToOne
        );
        // ...and one ulp over.
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(
                netuid,
                vec![OWNER, VALIDATOR, MINER],
                vec![pct(10), pct(45), ONE - pct(10) - pct(45) + 1]
            ),
            Error::<Test>::EmissionWeightsDoNotSumToOne
        );
        // A sparse update that only touches the validator slot must also renormalise.
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(netuid, vec![VALIDATOR], vec![pct(40)]),
            Error::<Test>::EmissionWeightsDoNotSumToOne
        );
    });
}

#[test]
fn test_set_enforces_leased_subnet_owner_floor() {
    new_test_ext(1).execute_with(|| {
        InitialSubnetOwnerCut::set(11_796); // mainnet 18%
        let netuid = new_subnet();
        let floor = SubtensorModule::leased_subnet_min_owner_emission();
        assert!(floor > 0);

        // Not lease-created: owner may drop to zero.
        let (uids, values) = full_vector(0, pct(50));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid,
            uids.clone(),
            values.clone()
        ));

        // Lease-created: the owner slot may not fall below the launch default.
        SubnetCreatedViaLease::<Test>::insert(netuid, true);
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values),
            Error::<Test>::LeasedSubnetOwnerCutBelowMinimum
        );
        let (uids, values) = full_vector(floor - 1, pct(45));
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values),
            Error::<Test>::LeasedSubnetOwnerCutBelowMinimum
        );
        let (uids, values) = full_vector(floor, pct(45));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        InitialSubnetOwnerCut::set(0);
    });
}

// ---------------------------------------------------------------------------
// Root off the top: every block root is paid min(V, r) · alpha_out where r is
// its live baseline share (default validator share, global TaoWeight)
// ---------------------------------------------------------------------------

const TAO: u64 = 1_000_000_000;

/// Put `root_tao` rao of TAO on root and make `netuid`'s alpha issuance about
/// `alpha_issuance`; returns the issuance actually read back by the chain.
fn fund_root_and_issuance(netuid: NetUid, root_tao: u64, alpha_issuance: u64) -> f64 {
    SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(root_tao));
    SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::ZERO);
    SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(alpha_issuance));
    SubtensorModule::get_alpha_issuance(netuid).to_u64() as f64
}

/// Root's alpha out of one block's `alpha_out` on `netuid` under the live
/// state: `min(V, r) · alpha_out`, exactly how `emit_to_subnets` carves it
/// out of the validator slot.
fn root_alpha_for(netuid: NetUid, alpha_out: f64) -> f64 {
    SubtensorModule::get_root_emission_share(netuid).to_num::<f64>() * alpha_out
}

/// Root's baseline share from first principles: `V0 · x_g / (x_g + A)` with
/// `x_g = root_tao · TaoWeight`, independent of anything the subnet stores.
fn baseline_share(root_tao: f64, alpha_issuance: f64) -> f64 {
    let v0 = SubtensorModule::default_validator_emission() as f64 / ONE as f64;
    let weighted = root_tao * SubtensorModule::get_tao_weight().to_num::<f64>();
    v0 * weighted / (weighted + alpha_issuance)
}

fn set_validator(netuid: NetUid, owner: u64, validator: u64) -> DispatchResult {
    let (uids, values) = full_vector(owner, validator);
    SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values)
}

/// `fraction` of emission in u64 parts.
fn share(fraction: f64) -> u64 {
    (fraction * ONE as f64) as u64
}

#[test]
fn test_set_emits_full_vector_event() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        let (uids, values) = full_vector(pct(10), pct(40));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid,
            uids,
            values.clone()
        ));
        assert_eq!(
            last_event(),
            RuntimeEvent::SubtensorModule(Event::SubnetEmissionWeightsSet {
                netuid,
                weights: values,
            })
        );
    });
}

/// Greg's scenario: A/T = 0.01 at set time, TaoWeight 18%, owner sets
/// V = 50%; later A/T = 1. Spec 480's frozen weight paid root 1.682% there
/// against a 6.254% baseline. Now root's share *is* the baseline at every
/// block, with no further call.
#[test]
fn test_root_share_tracks_baseline_as_issuance_ratio_drifts_without_further_sets() {
    new_test_ext(1).execute_with(|| {
        InitialSubnetOwnerCut::set(11_796); // mainnet 18% → V0 = 41%
        let netuid = new_subnet();
        SubtensorModule::set_tao_weight(pct(18));
        let root_tao = 100_000_000 * TAO;
        fund_root_and_issuance(netuid, root_tao, root_tao / 100);
        // Owner 10% leaves room for the 33% miner floor next to V = 50%.
        assert_ok!(set_validator(netuid, pct(10), pct(50)));

        // Issuance catches up with root TAO: A/T = 1.
        let alpha_issuance = fund_root_and_issuance(netuid, root_tao, root_tao);
        let baseline = baseline_share(root_tao as f64, alpha_issuance);
        assert_abs_diff_eq!(baseline, 0.06254, epsilon = 1e-5);
        assert_abs_diff_eq!(
            SubtensorModule::get_root_emission_share(netuid).to_num::<f64>(),
            baseline,
            epsilon = 1e-9
        );
        assert_abs_diff_eq!(root_alpha_for(netuid, 1e9), baseline * 1e9, epsilon = 1.0);

        // And back the other way: issuance shrinks relative to root TAO.
        let alpha_issuance = fund_root_and_issuance(netuid, root_tao, root_tao / 100);
        assert_abs_diff_eq!(
            SubtensorModule::get_root_emission_share(netuid).to_num::<f64>(),
            baseline_share(root_tao as f64, alpha_issuance),
            epsilon = 1e-9
        );
        InitialSubnetOwnerCut::set(0);
    });
}

/// Root's share equals the baseline for any valid validator share, as the
/// issuance / root-TAO ratio and the global TaoWeight drift after the set.
#[test]
fn test_root_share_equals_baseline_for_any_validator_share_under_drift() {
    new_test_ext(1).execute_with(|| {
        InitialSubnetOwnerCut::set(11_796);
        let v0 = SubtensorModule::default_validator_emission();
        let root_tao_rao = 1_000_000 * TAO;
        let mut checked = 0u32;
        for validator in [pct(50), pct(45), v0, pct(30), pct(20), pct(10)] {
            let netuid = new_subnet();
            // Set under a state where V clears the baseline (large issuance).
            SubtensorModule::set_tao_weight(pct(18));
            fund_root_and_issuance(netuid, root_tao_rao, root_tao_rao * 10);
            assert_ok!(set_validator(netuid, pct(10), validator));
            let v = validator as f64 / ONE as f64;

            for tao_weight in [pct(5), pct(18), pct(50), ONE] {
                SubtensorModule::set_tao_weight(tao_weight);
                for ratio in [0.01_f64, 0.1, 1.0, 10.0] {
                    let alpha_issuance = fund_root_and_issuance(
                        netuid,
                        root_tao_rao,
                        (root_tao_rao as f64 * ratio) as u64,
                    );
                    let baseline = baseline_share(root_tao_rao as f64, alpha_issuance);
                    let expected = baseline.min(v);
                    assert_abs_diff_eq!(
                        SubtensorModule::get_root_emission_share(netuid).to_num::<f64>(),
                        expected,
                        epsilon = 1e-9
                    );
                    assert_eq!(
                        SubtensorModule::is_root_emission_share_capped(netuid),
                        baseline > v
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked >= 90, "only {checked} cases exercised");
        InitialSubnetOwnerCut::set(0);
    });
}

/// Sets do not move root's share at all: before and after any accepted set
/// root earns the baseline, whatever path the validator share took.
#[test]
fn test_sets_never_move_root_share() {
    new_test_ext(1).execute_with(|| {
        InitialSubnetOwnerCut::set(11_796);
        SubtensorModule::set_tao_weight(pct(18));
        let root_tao = 5_443_000 * TAO;
        let netuid = new_subnet();
        let alpha_issuance = fund_root_and_issuance(netuid, root_tao, 6_457_000 * TAO);
        let baseline = baseline_share(root_tao as f64, alpha_issuance);
        let before = root_alpha_for(netuid, 1e9);
        for validator in [pct(45), pct(30), pct(10), pct(50), pct(20)] {
            assert_ok!(set_validator(netuid, pct(10), validator));
            assert_abs_diff_eq!(root_alpha_for(netuid, 1e9), before, epsilon = 1.0);
            assert_abs_diff_eq!(
                SubtensorModule::get_root_emission_share(netuid).to_num::<f64>(),
                baseline,
                epsilon = 1e-9
            );
        }
        InitialSubnetOwnerCut::set(0);
    });
}

/// When the baseline later grows past `V`, root's cut is capped at the whole
/// validator slot and alpha validators get nothing.
#[test]
fn test_root_share_is_capped_at_validator_slot() {
    new_test_ext(1).execute_with(|| {
        InitialSubnetOwnerCut::set(11_796);
        let owner = SubtensorModule::default_owner_emission();
        let netuid = new_subnet();
        SubtensorModule::set_tao_weight(pct(18));
        let root_tao = 1_000_000 * TAO;
        let alpha_issuance = fund_root_and_issuance(netuid, root_tao, 10_000_000 * TAO);
        let baseline = baseline_share(root_tao as f64, alpha_issuance);
        let validator = share(baseline * 1.05);
        assert_ok!(set_validator(netuid, owner, validator));
        assert!(!SubtensorModule::is_root_emission_share_capped(netuid));

        // Root stake grows tenfold: the baseline overtakes V.
        let alpha_issuance = fund_root_and_issuance(netuid, root_tao * 10, 10_000_000 * TAO);
        let grown = baseline_share(root_tao as f64 * 10.0, alpha_issuance);
        let v = validator as f64 / ONE as f64;
        assert!(grown > v, "{grown} vs {v}");
        assert!(SubtensorModule::is_root_emission_share_capped(netuid));
        assert_abs_diff_eq!(
            SubtensorModule::get_root_emission_share(netuid).to_num::<f64>(),
            v,
            epsilon = 1e-9
        );

        // The owner can raise V (it clears the new baseline) to pay validators again.
        assert_ok!(set_validator(netuid, owner, pct(49)));
        assert!(!SubtensorModule::is_root_emission_share_capped(netuid));
        assert_abs_diff_eq!(
            SubtensorModule::get_root_emission_share(netuid).to_num::<f64>(),
            grown,
            epsilon = 1e-9
        );
        InitialSubnetOwnerCut::set(0);
    });
}

/// Rejection when `V_new ≤ r`; the rejected call leaves the vector untouched.
#[test]
fn test_set_rejects_validator_share_at_or_below_root_baseline() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        SubtensorModule::set_tao_weight(pct(50));
        let root_tao = 100_000_000 * TAO;
        let alpha_issuance = fund_root_and_issuance(netuid, root_tao, 1_000_000 * TAO);
        // Weighted root TAO dwarfs issuance: the baseline is almost all of V0 = 0.5.
        let baseline = baseline_share(root_tao as f64, alpha_issuance);
        assert!(baseline > 0.49 && baseline < 0.5, "{baseline}");
        assert_abs_diff_eq!(
            SubtensorModule::baseline_root_emission_share(netuid).to_num::<f64>(),
            baseline,
            epsilon = 1e-9
        );

        let baseline_parts =
            emission_share_to_parts(SubtensorModule::baseline_root_emission_share(netuid));
        for validator in [
            share(baseline * 0.5),
            share(baseline * 0.999),
            baseline_parts,
        ] {
            let (uids, values) = full_vector(0, validator);
            assert_noop!(
                SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values),
                Error::<Test>::ValidatorEmissionTooLowToPreserveRoot
            );
        }
        assert_eq!(
            SubtensorModule::get_subnet_emission_weights(netuid),
            SubtensorModule::default_emission_weights()
        );
        // One part above the baseline is the smallest accepted value: the check
        // is made in u64 parts, exactly as the runtime API reports the baseline.
        assert_ok!(set_validator(netuid, 0, baseline_parts + 1));
        assert_eq!(
            SubtensorModule::get_subnet_emission_weights(netuid)[VALIDATOR as usize],
            baseline_parts + 1
        );
        // Further above the baseline is accepted too; root still earns the baseline.
        let validator = share(baseline * 1.001);
        assert_ok!(set_validator(netuid, 0, validator));
        assert_abs_diff_eq!(
            SubtensorModule::get_root_emission_share(netuid).to_num::<f64>(),
            baseline,
            epsilon = 1e-9
        );
    });
}

/// With no TAO on root the baseline is zero: any positive validator share is
/// accepted and a zero one is not (`V_new ≤ r`). Root stake arriving later
/// is paid its baseline from the next block on.
#[test]
fn test_set_without_root_stake_accepts_any_positive_share() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        SubtensorModule::set_tao_weight(pct(18));
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::ZERO);
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(1_000_000 * TAO));
        assert_eq!(
            SubtensorModule::baseline_root_emission_share(netuid),
            U96F32::saturating_from_num(0)
        );

        assert_noop!(
            set_validator(netuid, 0, 0),
            Error::<Test>::ValidatorEmissionTooLowToPreserveRoot
        );
        // A single part is enough: the floor is `baseline_parts + 1 = 1`.
        assert_ok!(set_validator(netuid, 0, 1));
        assert_ok!(set_validator(netuid, 0, pct(1)));
        assert_eq!(
            SubtensorModule::get_root_emission_share(netuid),
            U96F32::saturating_from_num(0)
        );

        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(1_000_000 * TAO));
        let issuance = SubtensorModule::get_alpha_issuance(netuid).to_u64() as f64;
        let baseline = baseline_share(1_000_000.0 * TAO as f64, issuance);
        // V = 1% is now far below the baseline: capped.
        assert!(SubtensorModule::is_root_emission_share_capped(netuid));
        assert_ok!(set_validator(netuid, 0, pct(40)));
        assert_abs_diff_eq!(
            SubtensorModule::get_root_emission_share(netuid).to_num::<f64>(),
            baseline,
            epsilon = 1e-9
        );
    });
}

#[test]
fn test_root_emission_share_formula() {
    let f = U96F32::saturating_from_num::<f64>;
    let root_tao = f(1_000_000.0);
    let alpha = f(2_000_000.0);
    // x = 0.5M, share = 0.5 · 0.5M / 2.5M = 0.1.
    assert_abs_diff_eq!(
        root_emission_share(f(0.5), f(0.5), root_tao, alpha).to_num::<f64>(),
        0.1,
        epsilon = 1e-9
    );
    // No root TAO: nothing for root.
    assert_eq!(root_emission_share(f(0.5), f(0.5), f(0.0), alpha), f(0.0));
    // No issuance: root takes the whole validator share.
    assert_eq!(
        root_emission_share(f(0.41), f(0.18), root_tao, f(0.0)),
        f(0.41)
    );
    // Round trip through u64 parts.
    assert_eq!(emission_share_to_parts(f(1.0)), ONE);
    assert_eq!(emission_share_to_parts(f(0.0)), 0);
    assert_abs_diff_eq!(
        SubtensorModule::emission_weight_to_fixed(emission_share_to_parts(f(0.25))).to_num::<f64>(),
        0.25,
        epsilon = 1e-9
    );
}

/// Mainnet SN4-like numbers: 5.443M TAO on root, 6.457M alpha issued,
/// TaoWeight 18%, owner 18% / validator 41%. Root's baseline is about 5.4% of
/// emission, so the validator share may go down to just above that.
#[test]
fn test_sn4_like_subnet_accepts_5_5_percent_and_rejects_5_3_percent() {
    new_test_ext(1).execute_with(|| {
        InitialSubnetOwnerCut::set(11_796); // mainnet 18%
        let netuid = new_subnet();
        SubtensorModule::set_tao_weight(pct(18));
        let root_tao = 5_443_000 * TAO;
        let alpha_issuance = fund_root_and_issuance(netuid, root_tao, 6_457_000 * TAO);
        let owner = SubtensorModule::default_owner_emission();
        assert_abs_diff_eq!(
            SubtensorModule::get_float_subnet_validator_emission_cut(netuid).to_num::<f64>(),
            0.41,
            epsilon = 1e-4
        );
        let baseline = baseline_share(root_tao as f64, alpha_issuance);
        assert_abs_diff_eq!(baseline, 0.054, epsilon = 1e-3);

        let before = root_alpha_for(netuid, 1e9);
        let (uids, values) = full_vector(owner, emission_fraction(53, 1000));
        assert_noop!(
            SubtensorModule::do_set_subnet_emission_weights(netuid, uids, values),
            Error::<Test>::ValidatorEmissionTooLowToPreserveRoot
        );
        let (uids, values) = full_vector(owner, emission_fraction(55, 1000));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        assert_abs_diff_eq!(root_alpha_for(netuid, 1e9), before, epsilon = 1.0);
        InitialSubnetOwnerCut::set(0);
    });
}

#[test]
fn test_emission_weights_info_reports_vector_and_root_shares() {
    new_test_ext(1).execute_with(|| {
        assert!(SubtensorModule::get_subnet_emission_weights_info(NetUid::from(42)).is_none());
        let netuid = new_subnet();
        SubtensorModule::set_tao_weight(pct(18));
        let alpha_issuance = fund_root_and_issuance(netuid, 1_000_000 * TAO, 1_000_000 * TAO);
        let (uids, values) = full_vector(pct(10), pct(25));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid,
            uids,
            values.clone()
        ));
        let info = SubtensorModule::get_subnet_emission_weights_info(netuid).unwrap();
        assert_eq!(info.netuid, netuid.into());
        assert_eq!(
            info.weights.iter().map(|w| w.0).collect::<Vec<u64>>(),
            values
        );
        assert_eq!(info.burn.0, 0);
        assert_eq!(info.recycle.0, 0);
        assert_eq!(info.owner.0, pct(10));
        assert_eq!(info.validator.0, pct(25));
        assert_eq!(info.miner.0, ONE - pct(10) - pct(25));
        let baseline = baseline_share(1_000_000.0 * TAO as f64, alpha_issuance);
        assert_abs_diff_eq!(
            info.root_baseline_share.0 as f64 / ONE as f64,
            baseline,
            epsilon = 1e-9
        );
        assert_eq!(info.root_share, info.root_baseline_share);
        assert!(!info.created_via_lease);

        // Capped: the baseline overtakes V, root_share reports the slot.
        fund_root_and_issuance(netuid, 1_000_000_000 * TAO, 1_000_000 * TAO);
        let info = SubtensorModule::get_subnet_emission_weights_info(netuid).unwrap();
        assert!(info.root_baseline_share.0 > pct(25));
        assert_eq!(info.root_share.0, pct(25));
    });
}

/// The read-once view (what the EVM getter charges `EMISSION_WEIGHTS_VIEW_READS`
/// reads for) must agree with the per-netuid helpers.
#[test]
fn test_emission_weights_view_matches_per_netuid_helpers() {
    new_test_ext(1).execute_with(|| {
        assert_eq!(EMISSION_WEIGHTS_VIEW_READS, 2);
        let netuid = new_subnet();
        let (uids, values) = full_vector(pct(10), pct(40));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        let view = SubtensorModule::read_subnet_emission_weights_view(netuid);
        assert_eq!(
            view.weights,
            SubtensorModule::get_subnet_emission_weights(netuid)
        );
        assert!(!view.created_via_lease);
        SubnetCreatedViaLease::<Test>::insert(netuid, true);
        assert!(SubtensorModule::read_subnet_emission_weights_view(netuid).created_via_lease);
    });
}

// ---------------------------------------------------------------------------
// Coinbase: slots are honoured exactly and root payout is preserved
// ---------------------------------------------------------------------------

/// Run one `emit_to_subnets` on a fresh subnet with a funded root and return
/// `(alpha_emission, owner, server, validator, root_divs)`.
fn emit_once(validator_share: Option<u64>) -> (u64, u64, u64, u64, u64) {
    let netuid = new_subnet();
    SubtensorModule::set_tao_weight(pct(18));
    SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(100_000_000_000_000_u64));
    setup_reserves(
        netuid,
        TaoBalance::from(1_000_000_000_000_000_u64),
        AlphaBalance::from(1_000_000_000_000_000_u64),
    );
    assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
    if let Some(validator) = validator_share {
        let (uids, values) = full_vector(pct(10), validator);
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
    }

    let tao_emission = U96F32::from_num(12_345_678);
    let subnet_emissions = BTreeMap::from([(netuid, tao_emission)]);
    let alpha_emission = SubtensorModule::get_block_emission_for_issuance(
        SubtensorModule::get_alpha_issuance(netuid).into(),
    )
    .unwrap_or(0);
    let credit = SubtensorModule::mint_tao(12_345_678.into());
    SubtensorModule::emit_to_subnets(&[netuid], &subnet_emissions, credit, true);
    (
        alpha_emission,
        PendingOwnerCut::<Test>::get(netuid).to_u64(),
        PendingServerEmission::<Test>::get(netuid).to_u64(),
        PendingValidatorEmission::<Test>::get(netuid).to_u64(),
        PendingRootAlphaDivs::<Test>::get(netuid).to_u64(),
    )
}

#[test]
fn test_coinbase_honours_emission_weight_slots_exactly() {
    new_test_ext(1).execute_with(|| {
        let validator = pct(40);
        let (alpha_emission, owner, server, validator_divs, root_divs) = emit_once(Some(validator));
        let alpha = alpha_emission as f64;
        assert_abs_diff_eq!(owner as f64, alpha * 0.10, epsilon = 200.0);
        // Miner slot = 1 - 0.10 - 0.40 = 0.50 of emission, untouched by root.
        assert_abs_diff_eq!(server as f64, alpha * 0.50, epsilon = 200.0);
        // Validator slot = 0.40 of emission, root carved out of it.
        assert_abs_diff_eq!(
            (validator_divs + root_divs) as f64,
            alpha * 0.40,
            epsilon = 200.0
        );
        assert!(root_divs > 0);
        // Nothing is created or lost.
        assert_abs_diff_eq!(
            (owner + server + validator_divs + root_divs) as f64,
            alpha,
            epsilon = 400.0
        );
    });
}

/// One `emit_to_subnets` with an explicit burn/recycle/owner/validator vector.
/// Returns `(alpha_emission, owner, server, validator, root, burned_delta,
/// recycled_delta, alpha_out_delta, issuance_delta)`.
fn emit_once_with_burn_recycle(
    burn: u64,
    recycle: u64,
    owner: u64,
    validator: u64,
) -> (u64, u64, u64, u64, u64, u64, u64, i128, i128) {
    let netuid = new_subnet();
    SubtensorModule::set_tao_weight(pct(18));
    SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(100_000_000_000_000_u64));
    setup_reserves(
        netuid,
        TaoBalance::from(1_000_000_000_000_000_u64),
        AlphaBalance::from(1_000_000_000_000_000_u64),
    );
    assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
    let (uids, values) = full_vector_with_burn_recycle(burn, recycle, owner, validator);
    assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
        netuid, uids, values
    ));

    let tao_emission = U96F32::from_num(12_345_678);
    let subnet_emissions = BTreeMap::from([(netuid, tao_emission)]);
    let alpha_emission = SubtensorModule::get_block_emission_for_issuance(
        SubtensorModule::get_alpha_issuance(netuid).into(),
    )
    .unwrap_or(0);
    let burned_before = pallet_alpha_assets::AlphaBurned::<Test>::get(netuid).to_u64();
    let recycled_before = pallet_alpha_assets::AlphaRecycled::<Test>::get(netuid).to_u64();
    let alpha_out_before = SubnetAlphaOut::<Test>::get(netuid).to_u64();
    let issuance_before = pallet_alpha_assets::TotalAlphaIssuance::<Test>::get(netuid).to_u64();
    let credit = SubtensorModule::mint_tao(12_345_678.into());
    SubtensorModule::emit_to_subnets(&[netuid], &subnet_emissions, credit, true);
    (
        alpha_emission,
        PendingOwnerCut::<Test>::get(netuid).to_u64(),
        PendingServerEmission::<Test>::get(netuid).to_u64(),
        PendingValidatorEmission::<Test>::get(netuid).to_u64(),
        PendingRootAlphaDivs::<Test>::get(netuid).to_u64(),
        pallet_alpha_assets::AlphaBurned::<Test>::get(netuid).to_u64() - burned_before,
        pallet_alpha_assets::AlphaRecycled::<Test>::get(netuid).to_u64() - recycled_before,
        SubnetAlphaOut::<Test>::get(netuid).to_u64() as i128 - alpha_out_before as i128,
        pallet_alpha_assets::TotalAlphaIssuance::<Test>::get(netuid).to_u64() as i128
            - issuance_before as i128,
    )
}

#[test]
fn test_coinbase_burn_raises_alpha_burned_and_pays_remaining_slots() {
    new_test_ext(1).execute_with(|| {
        let burn = pct(10);
        let owner = pct(10);
        let validator = pct(40);
        let (
            alpha_emission,
            owner_paid,
            server,
            validator_divs,
            root_divs,
            burned,
            recycled,
            _alpha_out_delta,
            _issuance_delta,
        ) = emit_once_with_burn_recycle(burn, 0, owner, validator);
        let alpha = alpha_emission as f64;
        assert_abs_diff_eq!(burned as f64, alpha * 0.10, epsilon = 200.0);
        assert_eq!(recycled, 0);
        assert_abs_diff_eq!(owner_paid as f64, alpha * 0.10, epsilon = 200.0);
        // Miner = 1 - 0.10 burn - 0.10 owner - 0.40 validator = 0.40.
        assert_abs_diff_eq!(server as f64, alpha * 0.40, epsilon = 200.0);
        assert_abs_diff_eq!(
            (validator_divs + root_divs) as f64,
            alpha * 0.40,
            epsilon = 200.0
        );
        assert_abs_diff_eq!(
            (burned + owner_paid + server + validator_divs + root_divs) as f64,
            alpha,
            epsilon = 400.0
        );
    });
}

#[test]
fn test_coinbase_recycle_lowers_issuance_and_raises_alpha_recycled() {
    new_test_ext(1).execute_with(|| {
        let recycle = pct(10);
        let owner = pct(10);
        let validator = pct(40);
        let (
            alpha_emission,
            owner_paid,
            server,
            validator_divs,
            root_divs,
            burned,
            recycled,
            alpha_out_delta,
            issuance_delta,
        ) = emit_once_with_burn_recycle(0, recycle, owner, validator);
        let alpha = alpha_emission as f64;
        assert_eq!(burned, 0);
        assert_abs_diff_eq!(recycled as f64, alpha * 0.10, epsilon = 200.0);
        // Recycle removes alpha from SubnetAlphaOut and TotalAlphaIssuance after mint.
        // Net alpha_out rise is mint − recycle; issuance rise is mint − recycle
        // (plus any pool-side alpha_in mint, which is identical across runs).
        assert!(recycled > 0);
        assert_abs_diff_eq!(owner_paid as f64, alpha * 0.10, epsilon = 200.0);
        assert_abs_diff_eq!(server as f64, alpha * 0.40, epsilon = 200.0);
        assert_abs_diff_eq!(
            (validator_divs + root_divs) as f64,
            alpha * 0.40,
            epsilon = 200.0
        );
        // Compared to a no-recycle run, SubnetAlphaOut and issuance are lower by ~recycle.
        let (alpha_control, _, _, _, _, _, recycled_control, alpha_out_control, issuance_control) =
            emit_once_with_burn_recycle(0, 0, owner, validator);
        assert_eq!(alpha_emission, alpha_control);
        assert_eq!(recycled_control, 0);
        assert_abs_diff_eq!(
            (alpha_out_control - alpha_out_delta) as f64,
            recycled as f64,
            epsilon = 2.0
        );
        assert_abs_diff_eq!(
            (issuance_control - issuance_delta) as f64,
            recycled as f64,
            epsilon = 2.0
        );
    });
}

#[test]
fn test_root_payout_is_invariant_to_validator_share() {
    new_test_ext(1).execute_with(|| {
        // Baseline: default vector, V = V₀ = 0.5 (mock owner cut is 0).
        let (alpha_a, _, _, _, root_a) = emit_once(None);
        assert!(root_a > 0);

        // Halve V to 0.25 (owner 10%, miner 65%) under the same pre-state: root is paid
        // its baseline off the top, so its alpha is exactly where it was.
        let (alpha_b, _, server_b, validator_b, root_b) = emit_once(Some(ONE / 4));
        assert_eq!(alpha_a, alpha_b);
        // Root's share of the smaller validator slot is larger...
        let root_share_b = root_b as f64 / (validator_b + root_b) as f64;
        let root_share_a = root_a as f64 / alpha_a as f64 * 2.0;
        assert!(root_share_b > root_share_a);
        // ...and root's absolute alpha is preserved.
        assert_abs_diff_eq!(root_b as f64, root_a as f64, epsilon = root_a as f64 * 1e-6);
        // Validators absorbed the cut, miners gained.
        assert_abs_diff_eq!(server_b as f64, alpha_b as f64 * 0.65, epsilon = 200.0);
    });
}

/// Through the real coinbase: after one set, root's alpha per block equals
/// `baseline · alpha_out` as root TAO, alpha issuance and the global TaoWeight
/// drift, with no further call. Alpha validators get the rest of the slot.
#[test]
fn test_coinbase_pays_root_its_live_baseline_every_block_without_further_sets() {
    new_test_ext(1).execute_with(|| {
        InitialSubnetOwnerCut::set(11_796);
        let netuid = new_subnet();
        SubtensorModule::set_tao_weight(pct(18));
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(100_000_000 * TAO));
        setup_reserves(
            netuid,
            TaoBalance::from(1_000_000 * TAO),
            AlphaBalance::from(1_000_000 * TAO),
        );
        assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
        // A/T = 0.01 at set time; V = 50% clears the baseline comfortably.
        assert_ok!(set_validator(netuid, pct(10), pct(50)));

        let emissions = BTreeMap::from([(netuid, U96F32::from_num(1_000_000_000_u64))]);
        let mut blocks = 0u32;
        // Issuance stays under the 21M alpha cap so a block still emits alpha.
        for (root_tao, alpha_out, tao_weight) in [
            (100_000_000 * TAO, 1_000_000 * TAO, pct(18)),
            (100_000_000 * TAO, 10_000_000 * TAO, pct(18)),
            (100_000_000 * TAO, 19_000_000 * TAO, pct(18)),
            (1_000_000 * TAO, 10_000_000 * TAO, pct(18)),
            (10_000_000 * TAO, 10_000_000 * TAO, pct(5)),
            (10_000_000 * TAO, 10_000_000 * TAO, pct(50)),
            (10_000_000 * TAO, 10_000_000 * TAO, ONE),
        ] {
            SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(root_tao));
            SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(alpha_out));
            SubtensorModule::set_tao_weight(tao_weight);
            let issuance = SubtensorModule::get_alpha_issuance(netuid).to_u64() as f64;
            let baseline = baseline_share(root_tao as f64, issuance);
            assert!(baseline < 0.5, "{baseline}");

            let root_before = PendingRootAlphaDivs::<Test>::get(netuid).to_u64();
            let validator_before = PendingValidatorEmission::<Test>::get(netuid).to_u64();
            let credit = SubtensorModule::mint_tao(1_000_000_000_u64.into());
            SubtensorModule::emit_to_subnets(&[netuid], &emissions, credit, true);
            let emitted = SubnetAlphaOutEmission::<Test>::get(netuid).to_u64() as f64;
            assert!(emitted > 0.0);
            let root_alpha =
                (PendingRootAlphaDivs::<Test>::get(netuid).to_u64() - root_before) as f64;
            let validator_alpha =
                (PendingValidatorEmission::<Test>::get(netuid).to_u64() - validator_before) as f64;

            assert_abs_diff_eq!(root_alpha, baseline * emitted, epsilon = emitted * 1e-6);
            assert_abs_diff_eq!(
                root_alpha + validator_alpha,
                0.5 * emitted,
                epsilon = emitted * 1e-6
            );
            blocks += 1;
        }
        assert_eq!(blocks, 7);
        InitialSubnetOwnerCut::set(0);
    });
}

/// Through the real coinbase: a subnet that lowered `V` pays root exactly
/// what a subnet on the default split pays, block after block.
#[test]
fn test_coinbase_root_alpha_matches_default_split_subnet() {
    new_test_ext(1).execute_with(|| {
        InitialSubnetOwnerCut::set(11_796);
        let owner = SubtensorModule::default_owner_emission();
        let lowered = new_subnet();
        let control = add_dynamic_network(&U256::from(3), &U256::from(4));
        SubtensorModule::set_tao_weight(pct(18));
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(5_443_000 * TAO));
        for netuid in [lowered, control] {
            setup_reserves(
                netuid,
                TaoBalance::from(1_000_000 * TAO),
                AlphaBalance::from(6_457_000 * TAO),
            );
            assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
        }
        assert_ok!(set_validator(lowered, owner, pct(10)));
        assert_eq!(
            SubtensorModule::get_subnet_emission_weights(control),
            SubtensorModule::default_emission_weights()
        );

        let emission = U96F32::from_num(1_000_000_000_u64);
        let emissions = BTreeMap::from([(lowered, emission), (control, emission)]);
        for _ in 0..3 {
            let credit = SubtensorModule::mint_tao(2_000_000_000_u64.into());
            SubtensorModule::emit_to_subnets(&[lowered, control], &emissions, credit, true);
            assert_eq!(
                SubnetAlphaOutEmission::<Test>::get(lowered),
                SubnetAlphaOutEmission::<Test>::get(control)
            );
            assert_abs_diff_eq!(
                PendingRootAlphaDivs::<Test>::get(lowered).to_u64() as f64,
                PendingRootAlphaDivs::<Test>::get(control).to_u64() as f64,
                epsilon = 2.0
            );
            assert!(PendingRootAlphaDivs::<Test>::get(lowered).to_u64() > 0);
            assert!(
                PendingValidatorEmission::<Test>::get(lowered)
                    < PendingValidatorEmission::<Test>::get(control)
            );
            assert!(
                PendingServerEmission::<Test>::get(lowered)
                    > PendingServerEmission::<Test>::get(control)
            );
        }
        InitialSubnetOwnerCut::set(0);
    });
}

// ---------------------------------------------------------------------------
// Pool side (tao_in / alpha_in / injection cap) ignores the vector
// ---------------------------------------------------------------------------

/// A fresh subnet with a funded root and a TAO emission large enough that the
/// alpha-injection cap (`root_proportion × alpha_emission`) binds.
fn pool_subnet() -> (NetUid, BTreeMap<NetUid, U96F32>) {
    let netuid = new_subnet();
    SubtensorModule::set_tao_weight(pct(18));
    SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(100_000_000_000_000_u64));
    setup_reserves(
        netuid,
        TaoBalance::from(1_000_000_000_000_000_u64),
        AlphaBalance::from(1_000_000_000_000_000_u64),
    );
    assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
    let emissions = BTreeMap::from([(netuid, U96F32::from_num(POOL_TAO_EMISSION))]);
    (netuid, emissions)
}

const POOL_TAO_EMISSION: u64 = 5_000_000_000;

/// `(tao_in, alpha_in, alpha_out, excess)` for the single subnet in `emissions`.
fn subnet_terms(
    netuid: NetUid,
    emissions: &BTreeMap<NetUid, U96F32>,
) -> (U96F32, U96F32, U96F32, U96F32) {
    let (tao_in, alpha_in, alpha_out, excess) = SubtensorModule::get_subnet_terms(emissions);
    (
        tao_in[&netuid],
        alpha_in[&netuid],
        alpha_out[&netuid],
        excess[&netuid],
    )
}

#[test]
fn test_pool_injection_terms_ignore_validator_share() {
    new_test_ext(1).execute_with(|| {
        let (netuid, emissions) = pool_subnet();
        let root_prop_before = SubtensorModule::root_proportion(netuid);
        let terms_before = subnet_terms(netuid, &emissions);
        // The cap binds: some TAO is excess rather than injected.
        assert!(terms_before.3 > U96F32::from_num(0));
        let root_share_before = SubtensorModule::get_root_emission_share(netuid);

        // Halve V: root's share of emission is unchanged...
        let (uids, values) = full_vector(pct(10), ONE / 4);
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        assert_eq!(
            SubtensorModule::get_root_emission_share(netuid),
            root_share_before
        );

        // ...and the pool side is bit-for-bit unchanged.
        assert_eq!(SubtensorModule::root_proportion(netuid), root_prop_before);
        assert_eq!(subnet_terms(netuid, &emissions), terms_before);
        SubtensorModule::update_root_prop();
        assert_eq!(RootProp::<Test>::get(netuid), root_prop_before);
    });
}

#[test]
fn test_pool_reserves_move_identically_whatever_the_validator_share() {
    new_test_ext(1).execute_with(|| {
        let mut deltas = Vec::new();
        for validator_share in [None, Some(ONE / 4), Some(pct(50))] {
            let (netuid, emissions) = pool_subnet();
            if let Some(validator) = validator_share {
                let (uids, values) = full_vector(pct(10), validator);
                assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
                    netuid, uids, values
                ));
            }
            let tao_before = SubnetTAO::<Test>::get(netuid).to_u64();
            let alpha_in_before = SubnetAlphaIn::<Test>::get(netuid).to_u64();
            let alpha_out_before = SubnetAlphaOut::<Test>::get(netuid).to_u64();
            let credit = SubtensorModule::mint_tao(POOL_TAO_EMISSION.into());
            SubtensorModule::emit_to_subnets(&[netuid], &emissions, credit, true);
            deltas.push((
                SubnetTAO::<Test>::get(netuid).to_u64() as i128 - tao_before as i128,
                SubnetAlphaIn::<Test>::get(netuid).to_u64() as i128 - alpha_in_before as i128,
                SubnetAlphaOut::<Test>::get(netuid).to_u64() as i128 - alpha_out_before as i128,
                SubnetTaoInEmission::<Test>::get(netuid).to_u64(),
                SubnetAlphaInEmission::<Test>::get(netuid).to_u64(),
                SubnetAlphaOutEmission::<Test>::get(netuid).to_u64(),
            ));
        }
        assert!(
            deltas[0].0 != 0 || deltas[0].1 != 0,
            "pool did not move at all"
        );
        assert!(deltas.iter().all(|d| *d == deltas[0]), "{deltas:?}");
    });
}

// ---------------------------------------------------------------------------
// Consensus stake weighting uses the global TaoWeight, as before etao
// ---------------------------------------------------------------------------

#[test]
fn test_stake_weights_use_global_tao_weight_whatever_the_validator_share() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        let hotkey = U256::from(10);
        let coldkey = U256::from(11);
        SubtensorModule::set_tao_weight(pct(18));
        add_network(NetUid::ROOT, 1, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            AlphaBalance::from(1_000_000_000),
        );

        let (total_before, alpha_before, tao_before) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&hotkey, netuid);
        assert_eq!(alpha_before, 0);
        assert!(tao_before > 0);
        assert_abs_diff_eq!(
            total_before.to_num::<f64>(),
            tao_before.to_num::<f64>() * 0.18,
            epsilon = 1.0
        );
        let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap() as usize;
        let (network_total, _, _) = SubtensorModule::get_stake_weights_for_network(netuid);
        assert_eq!(network_total[uid], total_before);

        // Changing the validator share does not touch consensus stake weighting.
        fund_root_and_issuance(netuid, 1_000_000 * TAO, 10_000_000 * TAO);
        assert_ok!(set_validator(netuid, pct(10), pct(25)));
        let (total_set, _, tao_set) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&hotkey, netuid);
        assert_eq!(total_set, total_before);
        assert_eq!(tao_set, tao_before);
        let (network_total, _, _) = SubtensorModule::get_stake_weights_for_network(netuid);
        assert_eq!(network_total[uid], total_before);

        // Doubling the global TaoWeight: TAO counts twice as much, on every subnet.
        SubtensorModule::set_tao_weight(pct(36));
        let (total_after, _, tao_after) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&hotkey, netuid);
        assert_eq!(tao_after, tao_before);
        assert_abs_diff_eq!(
            total_after.to_num::<f64>(),
            total_before.to_num::<f64>() * 2.0,
            epsilon = 1.0
        );
    });
}

// ---------------------------------------------------------------------------
// Migration
// ---------------------------------------------------------------------------

#[test]
fn test_migration_seeds_from_legacy_owner_cut_state() {
    new_test_ext(1).execute_with(|| {
        InitialSubnetOwnerCut::set(11_796); // mainnet 18% → V0 = 41%
        let enabled = NetUid::from(1);
        let disabled = NetUid::from(2);
        let leased = NetUid::from(3);
        for netuid in [enabled, disabled, leased] {
            add_network(netuid, 1, 0);
            // Wipe whatever registration seeded so the migration is observable.
            SubnetEmissionWeights::<Test>::remove(netuid);
        }
        add_network(NetUid::ROOT, 1, 0);
        SubtensorModule::set_tao_weight(pct(18));
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(1_000_000 * TAO));
        SubnetAlphaOut::<Test>::insert(enabled, AlphaBalance::from(1_000_000 * TAO));
        SubnetAlphaOut::<Test>::insert(disabled, AlphaBalance::from(2_000_000 * TAO));
        SubnetOwnerCut::<Test>::put(11_796u16);
        OwnerCutEnabled::<Test>::insert(disabled, false);
        SubnetUidToLeaseId::<Test>::insert(leased, 0u32);
        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        let weight = migrate_seed_subnet_emission_weights::<Test>();
        assert!(weight.ref_time() > 0);
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        let owner = SubtensorModule::scale_u16_fraction_to_emission_weight(11_796);
        let on = SubtensorModule::get_subnet_emission_weights(enabled);
        assert_eq!(
            on,
            [
                0,
                0,
                owner,
                (ONE - owner) / 2,
                ONE - owner - (ONE - owner) / 2
            ]
        );
        assert_eq!(sum(&on), ONE as u128);

        let off = SubtensorModule::get_subnet_emission_weights(disabled);
        assert_eq!(off, [0, 0, 0, ONE / 2, ONE - ONE / 2]);

        // Parity with main on the default split: root earns V0 · root_proportion. The
        // owner-cut-disabled subnet was seeded at V = 0.5 but still pays root the global
        // V0 baseline (pre-etao main paid it 0.5 · root_proportion there).
        let v0 = SubtensorModule::emission_weight_to_fixed(
            SubtensorModule::default_validator_emission(),
        );
        assert_abs_diff_eq!(v0.to_num::<f64>(), 0.41, epsilon = 1e-4);
        for netuid in [enabled, disabled, leased] {
            let root_prop = SubtensorModule::root_proportion(netuid);
            assert!(root_prop > U96F32::saturating_from_num(0));
            assert!(!SubtensorModule::is_root_emission_share_capped(netuid));
            assert_abs_diff_eq!(
                SubtensorModule::get_root_emission_share(netuid).to_num::<f64>(),
                (v0 * root_prop).to_num::<f64>(),
                epsilon = 1e-9
            );
        }
        assert_abs_diff_eq!(
            SubtensorModule::get_root_emission_share(disabled).to_num::<f64>(),
            (v0 * SubtensorModule::root_proportion(disabled)).to_num::<f64>(),
            epsilon = 1e-9
        );
        assert!(
            SubtensorModule::get_root_emission_share(disabled)
                < SubtensorModule::emission_weight_to_fixed(ONE / 2)
                    * SubtensorModule::root_proportion(disabled)
        );
        assert!(SubtensorModule::get_subnet_created_via_lease(leased));
        assert!(!SubtensorModule::get_subnet_created_via_lease(enabled));
        assert!(!SubnetEmissionWeights::<Test>::contains_key(NetUid::ROOT));

        // The accrued owner cut is seeded to the legacy refund deduction
        // (issuance × global cut), independent of OwnerCutEnabled.
        for netuid in [enabled, disabled, leased] {
            let issuance = SubtensorModule::get_alpha_issuance(netuid).to_u64();
            let expected = (issuance as u128 * 11_796 / u16::MAX as u128) as u64;
            assert_eq!(
                SubnetOwnerCutAccrued::<Test>::get(netuid),
                SubtensorModule::legacy_owner_cut_accrued(netuid, 11_796),
                "netuid {netuid:?}"
            );
            // Same U96F32 arithmetic as the pre-upgrade refund: fixed-point error only.
            assert_abs_diff_eq!(
                SubnetOwnerCutAccrued::<Test>::get(netuid).to_u64() as f64,
                expected as f64,
                epsilon = expected as f64 * 1e-6
            );
        }

        // A re-run is a stamped no-op.
        SubtensorModule::set_subnet_emission_weights_unchecked(
            enabled,
            [0, 0, 0, ONE / 2, ONE / 2 + 1],
        );
        let rerun = migrate_seed_subnet_emission_weights::<Test>();
        assert_eq!(
            rerun,
            <Test as frame_system::Config>::DbWeight::get().reads(1)
        );
        assert_eq!(
            SubtensorModule::get_subnet_emission_weights(enabled),
            [0, 0, 0, ONE / 2, ONE / 2 + 1]
        );
        InitialSubnetOwnerCut::set(0);
    });
}

/// Spec 480 wrote `SubnetTaoWeight` entries on testnet; spec 481 has no such
/// storage item, so the migration clears the raw keys and nothing else.
#[test]
fn test_migration_removes_subnet_tao_weight_entries() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        let (uids, values) = full_vector(pct(10), pct(40));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        let before = SubtensorModule::get_subnet_emission_weights(netuid);

        // Three orphaned entries under the old map's prefix (Identity-hashed netuids).
        let prefix = subnet_tao_weight_prefix();
        for stale in [1u16, 2, 3] {
            let mut key = prefix.clone();
            key.extend_from_slice(&NetUid::from(stale).encode());
            sp_io::storage::set(&key, &pct(99).encode());
        }
        assert_eq!(subnet_tao_weight_entries(), 3);
        assert!(!HasMigrationRun::<Test>::get(
            REMOVE_TAO_WEIGHT_MIGRATION_NAME.to_vec()
        ));

        let weight = migrate_remove_subnet_tao_weight::<Test>();
        assert!(weight.ref_time() > 0);
        assert!(HasMigrationRun::<Test>::get(
            REMOVE_TAO_WEIGHT_MIGRATION_NAME.to_vec()
        ));
        assert_eq!(subnet_tao_weight_entries(), 0);
        assert_eq!(SubtensorModule::get_subnet_emission_weights(netuid), before);

        // A re-run is a stamped no-op even with a fresh stale key.
        let mut key = prefix.clone();
        key.extend_from_slice(&NetUid::from(9u16).encode());
        sp_io::storage::set(&key, &pct(1).encode());
        assert_eq!(
            migrate_remove_subnet_tao_weight::<Test>(),
            <Test as frame_system::Config>::DbWeight::get().reads(1)
        );
        assert_eq!(subnet_tao_weight_entries(), 1);
    });
}

// ---------------------------------------------------------------------------
// Owner-cut history and netuid reuse
// ---------------------------------------------------------------------------

#[test]
fn test_coinbase_accrues_owner_cut_history() {
    new_test_ext(1).execute_with(|| {
        let (_, owner, _, _, _) = emit_once(Some(pct(40)));
        assert!(owner > 0);
        let netuid = NetUid::from(1);
        assert_eq!(SubnetOwnerCutAccrued::<Test>::get(netuid).to_u64(), owner);

        // Draining the pending cut at epoch does not touch the history.
        PendingOwnerCut::<Test>::insert(netuid, AlphaBalance::ZERO);
        assert_eq!(SubnetOwnerCutAccrued::<Test>::get(netuid).to_u64(), owner);
    });
}

#[test]
fn test_dissolve_refund_deducts_accrued_owner_cut_not_current_share() {
    new_test_ext(1).execute_with(|| {
        let owner_cold = U256::from(70);
        let owner_hot = U256::from(71);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(netuid);

        // Legacy subnet: eligible for the lock refund.
        let reg_at = NetworkRegisteredAt::<Test>::get(netuid);
        NetworkRegistrationStartBlock::<Test>::put(reg_at.saturating_add(1));
        let lock = TaoBalance::from(2_000_000_u64);
        SubtensorModule::set_subnet_locked_balance(netuid, lock);
        // A small pot (well below the refund) so no pot leftover is folded into the
        // owner's payout, with a 1:1 reserve ratio so α prices at ~1 τ.
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_u64));

        // The owner has already been paid 500_000 α of owner cut ...
        let accrued = AlphaBalance::from(500_000_u64);
        SubnetOwnerCutAccrued::<Test>::insert(netuid, accrued);
        // ... and now zeroes the owner slot right before dissolving.
        let (uids, values) = full_vector(0, pct(50));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        assert_eq!(SubtensorModule::get_subnet_owner_emission_cut(netuid), 0);

        let price = U96F32::saturating_from_num(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into()),
        );
        let deduction: u64 = U96F32::from_num(accrued.to_u64())
            .saturating_mul(price)
            .floor()
            .saturating_to_num::<u64>();
        assert!(deduction > 1_000, "test needs a deduction above the pot");
        let expected_refund = lock.to_u64() - deduction;

        let before = SubtensorModule::get_coldkey_balance(&owner_cold);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        run_block_idle();
        let after = SubtensorModule::get_coldkey_balance(&owner_cold);
        let gain = after.to_u64() - before.to_u64();

        assert!(
            gain < lock.to_u64(),
            "zeroing the owner slot must not restore the full lock: gain {gain}"
        );
        assert_abs_diff_eq!(gain as f64, expected_refund as f64, epsilon = 2.0);
    });
}

/// A legacy (refund-eligible) subnet with a 2_000_000 τ lock, 2_000_000 α issued
/// and a tiny 1:1 pot so α prices at ~1 τ and no pot leftover reaches the owner.
fn refundable_legacy_subnet(owner_hot: U256, owner_cold: U256) -> (NetUid, TaoBalance) {
    let netuid = add_dynamic_network(&owner_hot, &owner_cold);
    remove_owner_registration_stake(netuid);
    let reg_at = NetworkRegisteredAt::<Test>::get(netuid);
    NetworkRegistrationStartBlock::<Test>::put(reg_at.saturating_add(1));
    let lock = TaoBalance::from(2_000_000_u64);
    SubtensorModule::set_subnet_locked_balance(netuid, lock);
    SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_u64));
    SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_u64));
    SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(2_000_000_u64));
    SubnetOwnerCut::<Test>::put(11_796u16);
    (netuid, lock)
}

/// `issuance × 11_796 / u16::MAX`: the legacy owner-cut deduction in α.
fn legacy_deduction_alpha(netuid: NetUid) -> u64 {
    let issuance = SubtensorModule::get_alpha_issuance(netuid).to_u64() as u128;
    (issuance * 11_796 / u16::MAX as u128) as u64
}

/// `owner_alpha` priced at the subnet's current α price, floored, as the refund does.
fn priced(netuid: NetUid, owner_alpha: u64) -> u64 {
    let price = U96F32::from_num(
        <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into()),
    );
    (U96F32::from_num(owner_alpha) * price)
        .floor()
        .to_num::<u64>()
}

#[test]
fn test_owner_cut_alpha_for_lock_refund_falls_back_to_legacy_formula() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(2_000_000_u64));
        SubnetOwnerCut::<Test>::put(11_796u16);
        let legacy = SubtensorModule::legacy_owner_cut_accrued(netuid, 11_796);
        // Same U96F32 arithmetic as the pre-upgrade refund: a few parts of
        // fixed-point error against the exact integer at 1.8e10 α.
        assert_abs_diff_eq!(
            legacy.to_u64() as f64,
            legacy_deduction_alpha(netuid) as f64,
            epsilon = 16.0
        );

        // Registration wrote an explicit zero: a seeded zero is honoured, not replaced.
        assert!(SubnetOwnerCutAccrued::<Test>::contains_key(netuid));
        assert_eq!(
            SubtensorModule::owner_cut_alpha_for_lock_refund(netuid),
            AlphaBalance::ZERO
        );
        // A real history is used as is.
        SubnetOwnerCutAccrued::<Test>::insert(netuid, AlphaBalance::from(123_u64));
        assert_eq!(
            SubtensorModule::owner_cut_alpha_for_lock_refund(netuid).to_u64(),
            123
        );
        // No entry at all (pre-migration subnet that was missed): legacy formula, never 0.
        SubnetOwnerCutAccrued::<Test>::remove(netuid);
        assert_eq!(
            SubtensorModule::owner_cut_alpha_for_lock_refund(netuid),
            legacy
        );
        assert!(legacy.to_u64() > 0);
    });
}

#[test]
fn test_dissolve_refund_without_accrued_entry_still_deducts_legacy_owner_cut() {
    new_test_ext(1).execute_with(|| {
        let owner_cold = U256::from(80);
        let owner_hot = U256::from(81);
        let (netuid, lock) = refundable_legacy_subnet(owner_hot, owner_cold);
        // Simulate a subnet the seed migration never touched.
        SubnetOwnerCutAccrued::<Test>::remove(netuid);

        let deduction = priced(netuid, legacy_deduction_alpha(netuid));
        assert!(deduction > 1_000, "test needs a deduction above the pot");

        let before = SubtensorModule::get_coldkey_balance(&owner_cold);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        run_block_idle();
        let gain = SubtensorModule::get_coldkey_balance(&owner_cold).to_u64() - before.to_u64();

        assert!(
            gain < lock.to_u64(),
            "full lock must never be refunded: {gain}"
        );
        assert_abs_diff_eq!(
            gain as f64,
            (lock.to_u64() - deduction) as f64,
            epsilon = 2.0
        );
    });
}

#[test]
fn test_migration_seeds_accrued_owner_cut_for_subnets_mid_dissolution() {
    new_test_ext(1).execute_with(|| {
        let live = NetUid::from(1);
        let queued = NetUid::from(2);
        let current = NetUid::from(3);
        for netuid in [live, queued, current] {
            add_network(netuid, 1, 0);
            SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(1_000_000_u64));
        }
        add_network(NetUid::ROOT, 1, 0);
        SubnetOwnerCut::<Test>::put(11_796u16);
        // Pre-upgrade state: nothing seeded, two subnets already dissolving.
        for netuid in [queued, current] {
            NetworksAdded::<Test>::remove(netuid);
            SubnetEmissionWeights::<Test>::remove(netuid);
        }
        DissolveCleanupQueue::<Test>::put(vec![current, queued]);
        let expected = legacy_deduction_alpha(live);
        assert_eq!(legacy_deduction_alpha(queued), expected);
        assert_eq!(legacy_deduction_alpha(current), expected);

        let pending =
            crate::subnets::dissolution::DissolveCleanupPhase::AlphaInOutStakesSettleStakes;
        let paid = crate::subnets::dissolution::DissolveCleanupPhase::NetworkMapParameters;
        assert!(pending.lock_refund_pending());
        assert!(!paid.lock_refund_pending());

        for (phase, current_seeded) in [(pending, true), (paid, false)] {
            for netuid in [live, queued, current] {
                SubnetOwnerCutAccrued::<Test>::remove(netuid);
            }
            HasMigrationRun::<Test>::remove(MIGRATION_NAME.to_vec());
            let mut status = crate::subnets::dissolution::DissolveCleanupStatus::new(current);
            status.phase = phase.clone();
            CurrentDissolveCleanupStatus::<Test>::put(status);

            migrate_seed_subnet_emission_weights::<Test>();

            assert_eq!(SubnetOwnerCutAccrued::<Test>::get(live).to_u64(), expected);
            // Queued, cleanup not started: refund pending, seeded.
            assert_eq!(
                SubnetOwnerCutAccrued::<Test>::get(queued).to_u64(),
                expected
            );
            assert_eq!(
                SubnetOwnerCutAccrued::<Test>::contains_key(current),
                current_seeded,
                "{phase:?}"
            );
            if current_seeded {
                assert_eq!(
                    SubnetOwnerCutAccrued::<Test>::get(current).to_u64(),
                    expected
                );
            }
            // Dissolving subnets get nothing else: cleanup would only orphan it.
            for netuid in [queued, current] {
                assert!(!SubnetEmissionWeights::<Test>::contains_key(netuid));
            }
        }
    });
}

#[test]
fn test_subnet_dissolving_at_upgrade_gets_reduced_refund_after_migration() {
    new_test_ext(1).execute_with(|| {
        let owner_cold = U256::from(90);
        let owner_hot = U256::from(91);
        let (netuid, lock) = refundable_legacy_subnet(owner_hot, owner_cold);
        let deduction = priced(netuid, legacy_deduction_alpha(netuid));
        assert!(deduction > 1_000);

        // Dissolve first (out of NetworksAdded, refund still owed) ...
        let before = SubtensorModule::get_coldkey_balance(&owner_cold);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(DissolveCleanupQueue::<Test>::get().contains(&netuid));
        assert!(!NetworksAdded::<Test>::contains_key(netuid));
        // ... as a pre-upgrade subnet: no accrued entry, migration not yet run.
        SubnetOwnerCutAccrued::<Test>::remove(netuid);
        HasMigrationRun::<Test>::remove(MIGRATION_NAME.to_vec());

        migrate_seed_subnet_emission_weights::<Test>();
        assert_eq!(
            SubnetOwnerCutAccrued::<Test>::get(netuid).to_u64(),
            legacy_deduction_alpha(netuid)
        );

        run_block_idle();
        let gain = SubtensorModule::get_coldkey_balance(&owner_cold).to_u64() - before.to_u64();
        assert!(
            gain < lock.to_u64(),
            "full lock must never be refunded: {gain}"
        );
        assert_abs_diff_eq!(
            gain as f64,
            (lock.to_u64() - deduction) as f64,
            epsilon = 2.0
        );
    });
}

#[test]
fn test_dissolution_clears_emission_weight_state_and_reuse_starts_clean() {
    new_test_ext(1).execute_with(|| {
        let netuid = new_subnet();
        remove_owner_registration_stake(netuid);
        let (uids, values) = full_vector(pct(20), pct(30));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            netuid, uids, values
        ));
        SubnetCreatedViaLease::<Test>::insert(netuid, true);
        SubnetOwnerCutAccrued::<Test>::insert(netuid, AlphaBalance::from(123_u64));

        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        run_block_idle();

        assert!(!SubnetEmissionWeights::<Test>::contains_key(netuid));
        assert!(!SubnetCreatedViaLease::<Test>::contains_key(netuid));
        assert!(!SubnetOwnerCutAccrued::<Test>::contains_key(netuid));

        // A plain registration reusing the netuid is not bound by the old lease.
        let reused = add_dynamic_network(&U256::from(5), &U256::from(6));
        assert_eq!(reused, netuid, "test expects the freed netuid to be reused");
        assert!(!SubtensorModule::get_subnet_created_via_lease(reused));
        assert_eq!(SubnetOwnerCutAccrued::<Test>::get(reused).to_u64(), 0);
        assert_eq!(
            SubtensorModule::get_subnet_emission_weights(reused),
            SubtensorModule::default_emission_weights()
        );
        // ... so it may lower its owner cut below the lease floor.
        let (uids, values) = full_vector(pct(5), pct(45));
        assert_ok!(SubtensorModule::do_set_subnet_emission_weights(
            reused, uids, values
        ));
    });
}

#[test]
fn test_registration_seed_clears_stale_lease_marker_even_without_cleanup() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(7);
        SubnetCreatedViaLease::<Test>::insert(netuid, true);
        SubnetOwnerCutAccrued::<Test>::insert(netuid, AlphaBalance::from(9_u64));
        SubtensorModule::seed_subnet_emission_weights(netuid);
        assert!(!SubtensorModule::get_subnet_created_via_lease(netuid));
        assert_eq!(SubnetOwnerCutAccrued::<Test>::get(netuid).to_u64(), 0);
    });
}
