#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::mock::*;
use crate::rpc_info::subnet_info::{HyperparamEntry, HyperparamValue, SubnetHyperparamsV3};
use crate::{BurnHalfLife, BurnIncreaseMult};
use codec::{Compact, Decode, Encode};
use std::collections::BTreeSet;
use substrate_fixed::types::{I32F32, U64F64};
use subtensor_runtime_common::{NetUid, TaoBalance};

/// Names that must always appear in V3. Adding a new hyperparam = add its
/// name here AND in the getter. Removing one = decide whether that's a
/// breaking change for clients.
const EXPECTED_V3_NAMES: &[&[u8]] = &[
    b"kappa",
    b"immunity_period",
    b"min_allowed_weights",
    b"max_weights_limit",
    b"tempo",
    b"weights_version",
    b"weights_rate_limit",
    b"activity_cutoff",
    b"activity_cutoff_factor",
    b"registration_allowed",
    b"target_regs_per_interval",
    b"min_burn",
    b"max_burn",
    b"burn_half_life",
    b"burn_increase_mult",
    b"bonds_moving_avg",
    b"max_regs_per_block",
    b"serving_rate_limit",
    b"max_validators",
    b"commit_reveal_period",
    b"commit_reveal_weights_enabled",
    b"alpha_high",
    b"alpha_low",
    b"liquid_alpha_enabled",
    b"alpha_sigmoid_steepness",
    b"yuma_version",
    b"subnet_is_active",
    b"transfers_enabled",
    b"bonds_reset_enabled",
    b"user_liquidity_enabled",
    b"owner_cut_enabled",
    b"owner_cut_auto_lock_enabled",
    b"min_childkey_take",
];

fn find<'a>(params: &'a [HyperparamEntry], name: &[u8]) -> &'a HyperparamValue {
    &params
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("missing hyperparam {:?}", std::str::from_utf8(name)))
        .value
}

#[test]
fn test_get_subnet_hyperparams_v3_returns_none_for_nonexistent_subnet() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(7);
        assert!(SubtensorModule::get_subnet_hyperparams_v3(netuid).is_none());
    });
}

#[test]
fn test_get_subnet_hyperparams_v3_contains_all_expected_names_exactly_once() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);

        let result = SubtensorModule::get_subnet_hyperparams_v3(netuid)
            .expect("subnet exists, V3 should be Some");

        let returned: Vec<&[u8]> = result.iter().map(|e| e.name.as_slice()).collect();
        let returned_set: BTreeSet<&[u8]> = returned.iter().copied().collect();

        // No duplicates.
        assert_eq!(
            returned.len(),
            returned_set.len(),
            "duplicate hyperparam name in V3 output"
        );

        // Exact-name match with the expected set — catches accidental drops
        // and accidental additions that weren't reviewed.
        let expected_set: BTreeSet<&[u8]> = EXPECTED_V3_NAMES.iter().copied().collect();
        assert_eq!(
            returned_set, expected_set,
            "V3 hyperparam name set drifted from EXPECTED_V3_NAMES"
        );
    });
}

#[test]
fn test_get_subnet_hyperparams_v3_values_reflect_storage() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);

        // Distinct, easy-to-spot values for each storage we touch.
        SubtensorModule::set_kappa(netuid, 12);
        SubtensorModule::set_immunity_period(netuid, 13);
        SubtensorModule::set_min_allowed_weights(netuid, 14);
        SubtensorModule::set_tempo_unchecked(netuid, 16);
        SubtensorModule::set_weights_version_key(netuid, 19);
        SubtensorModule::set_weights_set_rate_limit(netuid, 20);
        // `activity_cutoff` is derived: factor_milli * tempo / 1000. With tempo=16,
        // factor 1375 yields 1375 * 16 / 1000 = 22 effective cutoff blocks.
        SubtensorModule::set_activity_cutoff_factor_milli(netuid, 1375);
        SubtensorModule::set_network_registration_allowed(netuid, false);
        SubtensorModule::set_target_registrations_per_interval(netuid, 24);
        SubtensorModule::set_min_burn(netuid, TaoBalance::from(25u64));
        SubtensorModule::set_max_burn(netuid, TaoBalance::from(26u64));
        BurnHalfLife::<Test>::insert(netuid, 33u16);
        BurnIncreaseMult::<Test>::insert(netuid, U64F64::saturating_from_num(2));
        SubtensorModule::set_bonds_moving_average(netuid, 27);
        SubtensorModule::set_max_registrations_per_block(netuid, 28);
        SubtensorModule::set_serving_rate_limit(netuid, 29);
        SubtensorModule::set_max_allowed_validators(netuid, 30);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_liquid_alpha_enabled(netuid, true);
        SubtensorModule::set_alpha_sigmoid_steepness(netuid, 5i16);
        SubtensorModule::set_yuma3_enabled(netuid, true);
        SubtensorModule::set_bonds_reset(netuid, true);
        SubtensorModule::set_owner_cut_enabled_flag(netuid, true);
        SubtensorModule::set_owner_cut_auto_lock_enabled(netuid, true);
        SubtensorModule::set_min_childkey_take(31);
        SubtensorModule::set_min_childkey_take_for_subnet(netuid, 32);

        let result = SubtensorModule::get_subnet_hyperparams_v3(netuid).unwrap();
        let p = &result;

        // Bool variants
        assert_eq!(
            find(p, b"registration_allowed"),
            &HyperparamValue::Bool(false)
        );
        assert_eq!(
            find(p, b"commit_reveal_weights_enabled"),
            &HyperparamValue::Bool(true)
        );
        assert_eq!(
            find(p, b"liquid_alpha_enabled"),
            &HyperparamValue::Bool(true)
        );
        assert_eq!(
            find(p, b"bonds_reset_enabled"),
            &HyperparamValue::Bool(true)
        );
        assert_eq!(find(p, b"owner_cut_enabled"), &HyperparamValue::Bool(true));
        assert_eq!(
            find(p, b"owner_cut_auto_lock_enabled"),
            &HyperparamValue::Bool(true)
        );

        // U16 variants
        assert_eq!(find(p, b"kappa"), &HyperparamValue::U16(Compact(12)));
        assert_eq!(
            find(p, b"immunity_period"),
            &HyperparamValue::U16(Compact(13))
        );
        assert_eq!(
            find(p, b"min_allowed_weights"),
            &HyperparamValue::U16(Compact(14))
        );
        assert_eq!(find(p, b"tempo"), &HyperparamValue::U16(Compact(16)));
        assert_eq!(
            find(p, b"activity_cutoff"),
            &HyperparamValue::U64(Compact(22))
        );
        assert_eq!(
            find(p, b"activity_cutoff_factor"),
            &HyperparamValue::U32(Compact(1375))
        );
        assert_eq!(
            find(p, b"target_regs_per_interval"),
            &HyperparamValue::U16(Compact(24))
        );
        assert_eq!(
            find(p, b"burn_half_life"),
            &HyperparamValue::U16(Compact(33))
        );
        assert_eq!(
            find(p, b"max_regs_per_block"),
            &HyperparamValue::U16(Compact(28))
        );
        assert_eq!(
            find(p, b"max_validators"),
            &HyperparamValue::U16(Compact(30))
        );
        assert_eq!(find(p, b"yuma_version"), &HyperparamValue::U16(Compact(3)));
        // Effective min childkey take = max(global, per-subnet).
        assert_eq!(
            find(p, b"min_childkey_take"),
            &HyperparamValue::U16(Compact(32))
        );

        // U64 variants
        assert_eq!(
            find(p, b"weights_version"),
            &HyperparamValue::U64(Compact(19))
        );
        assert_eq!(
            find(p, b"weights_rate_limit"),
            &HyperparamValue::U64(Compact(20))
        );
        assert_eq!(
            find(p, b"bonds_moving_avg"),
            &HyperparamValue::U64(Compact(27))
        );
        assert_eq!(
            find(p, b"serving_rate_limit"),
            &HyperparamValue::U64(Compact(29))
        );

        // TaoBalance variants
        assert_eq!(
            find(p, b"min_burn"),
            &HyperparamValue::TaoBalance(Compact(TaoBalance::from(25u64)))
        );
        assert_eq!(
            find(p, b"max_burn"),
            &HyperparamValue::TaoBalance(Compact(TaoBalance::from(26u64)))
        );

        // I32F32 variant
        assert_eq!(
            find(p, b"alpha_sigmoid_steepness"),
            &HyperparamValue::I32F32(I32F32::saturating_from_num(5))
        );

        // U64F64 variant
        assert_eq!(
            find(p, b"burn_increase_mult"),
            &HyperparamValue::U64F64(U64F64::saturating_from_num(2))
        );
    });
}

#[test]
fn test_get_subnet_hyperparams_v3_yuma_version_reflects_flag() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);

        SubtensorModule::set_yuma3_enabled(netuid, false);
        assert_eq!(
            find(
                &SubtensorModule::get_subnet_hyperparams_v3(netuid).unwrap(),
                b"yuma_version",
            ),
            &HyperparamValue::U16(Compact(2))
        );

        SubtensorModule::set_yuma3_enabled(netuid, true);
        assert_eq!(
            find(
                &SubtensorModule::get_subnet_hyperparams_v3(netuid).unwrap(),
                b"yuma_version",
            ),
            &HyperparamValue::U16(Compact(3))
        );
    });
}

#[test]
fn test_get_subnet_hyperparams_v3_scale_round_trip() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);

        let original = SubtensorModule::get_subnet_hyperparams_v3(netuid).unwrap();
        let bytes = original.encode();
        let decoded =
            SubnetHyperparamsV3::decode(&mut &bytes[..]).expect("V3 must SCALE-round-trip cleanly");
        assert_eq!(original, decoded);
    });
}

#[test]
fn test_hyperparam_value_variants_round_trip() {
    let cases = [
        HyperparamValue::Bool(true),
        HyperparamValue::Bool(false),
        HyperparamValue::U16(Compact(u16::MAX)),
        HyperparamValue::U32(Compact(u32::MAX)),
        HyperparamValue::U64(Compact(u64::MAX)),
        HyperparamValue::U128(Compact(u128::MAX)),
        HyperparamValue::TaoBalance(Compact(TaoBalance::from(123_456_789u64))),
        HyperparamValue::I32F32(I32F32::saturating_from_num(-7)),
        HyperparamValue::U64F64(U64F64::saturating_from_num(42)),
    ];
    for original in &cases {
        let bytes = original.encode();
        let decoded = HyperparamValue::decode(&mut &bytes[..])
            .expect("HyperparamValue variant must round-trip");
        assert_eq!(original, &decoded);
    }
}
