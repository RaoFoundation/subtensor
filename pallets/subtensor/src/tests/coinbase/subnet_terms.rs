#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Subnet terms / registration gates for coinbase emission.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_coinbase_subnets_with_no_reg_get_no_emission() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        let netuid1 = add_dynamic_network(&U256::from(3), &U256::from(4));

        // Setup initial state
        SubtokenEnabled::<Test>::insert(netuid0, true);
        SubtokenEnabled::<Test>::insert(netuid1, true);
        FirstEmissionBlockNumber::<Test>::insert(netuid0, 0);
        FirstEmissionBlockNumber::<Test>::insert(netuid1, 0);
        // Explicitly allow registration for both subnets
        NetworkRegistrationAllowed::<Test>::insert(netuid0, true);
        NetworkRegistrationAllowed::<Test>::insert(netuid1, true);
        NetworkPowRegistrationAllowed::<Test>::insert(netuid0, false);
        NetworkPowRegistrationAllowed::<Test>::insert(netuid1, true);

        // Note that netuid0 has only one method allowed
        // And, netuid1 has *both* methods allowed
        // Both should be in the list.
        let subnets_to_emit_to_0 = SubtensorModule::get_subnets_to_emit_to(&[netuid0, netuid1]);
        // Check that both subnets are in the list
        assert_eq!(subnets_to_emit_to_0.len(), 2);
        assert!(subnets_to_emit_to_0.contains(&netuid0));
        assert!(subnets_to_emit_to_0.contains(&netuid1));

        // Disabled registration of both methods on ONLY netuid0
        NetworkRegistrationAllowed::<Test>::insert(netuid0, false);
        NetworkPowRegistrationAllowed::<Test>::insert(netuid0, false);

        // Check that netuid0 is not in the list
        let subnets_to_emit_to_1 = SubtensorModule::get_subnets_to_emit_to(&[netuid0, netuid1]);
        assert_eq!(subnets_to_emit_to_1.len(), 1);
        assert!(!subnets_to_emit_to_1.contains(&netuid0));
        // Netuid1 still in the list
        assert!(subnets_to_emit_to_1.contains(&netuid1));
    });
}

// Tests for the excess TAO condition
#[test]
fn test_coinbase_subnet_terms_with_alpha_in_gt_alpha_emission() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        mock::setup_reserves(
            netuid0,
            TaoBalance::from(1_000_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );
        // Initialize swap
        Swap::maybe_initialize_palswap(netuid0, None);

        // Set netuid0 to have price tao_emission / price > alpha_emission
        let alpha_emission = U96F32::saturating_from_num(
            SubtensorModule::get_block_emission_for_issuance(
                SubtensorModule::get_alpha_issuance(netuid0).into(),
            )
            .unwrap_or(0),
        );
        let price_to_set: U64F64 = U64F64::saturating_from_num(0.01);
        let price_to_set_fixed: U96F32 = U96F32::saturating_from_num(price_to_set);

        let tao_emission: U96F32 = U96F32::saturating_from_num(alpha_emission)
            .saturating_mul(price_to_set_fixed)
            .saturating_add(U96F32::saturating_from_num(0.01));

        // Set the price
        let tao = TaoBalance::from(1_000_000_000_u64);
        let alpha = AlphaBalance::from(
            (U64F64::saturating_from_num(u64::from(tao)) / price_to_set).to_num::<u64>(),
        );
        SubnetTAO::<Test>::insert(netuid0, tao);
        SubnetAlphaIn::<Test>::insert(netuid0, alpha);

        // Check the price is set
        assert_abs_diff_eq!(
            pallet_subtensor_swap::Pallet::<Test>::current_alpha_price(netuid0).to_num::<f64>(),
            price_to_set.to_num::<f64>(),
            epsilon = 0.001
        );

        let subnet_emissions = BTreeMap::from([(netuid0, tao_emission)]);

        // The injection cap is root_proportion * alpha_emission. Seed root stake so
        // root_proportion is well-defined and the cap is positive.
        set_full_injection_root_stake();
        let root_prop: U96F32 = SubtensorModule::root_proportion(netuid0);
        let injection_cap: U96F32 = root_prop.saturating_mul(alpha_emission);

        let (tao_in, alpha_in, alpha_out, excess_tao) =
            SubtensorModule::get_subnet_terms(&subnet_emissions);

        // Check our condition is met: the raw alpha_in exceeds the cap, so it binds.
        assert!(tao_emission / price_to_set_fixed > injection_cap);

        // alpha_out should be the alpha_emission, always
        assert_abs_diff_eq!(
            alpha_out[&netuid0].to_num::<f64>(),
            alpha_emission.to_num::<f64>(),
            epsilon = 0.01
        );

        // alpha_in should be capped at root_proportion * alpha_emission
        assert_abs_diff_eq!(
            alpha_in[&netuid0].to_num::<f64>(),
            injection_cap.to_num::<f64>(),
            epsilon = injection_cap.to_num::<f64>() / 1_000.0
        );
        // tao_in should be the alpha_in at the ratio of the price
        assert_abs_diff_eq!(
            tao_in[&netuid0].to_num::<f64>(),
            alpha_in[&netuid0]
                .saturating_mul(price_to_set_fixed)
                .to_num::<f64>(),
            epsilon = 0.01
        );

        // excess_tao should be the difference between the tao_emission and the tao_in
        assert_abs_diff_eq!(
            excess_tao[&netuid0].to_num::<f64>(),
            tao_emission.to_num::<f64>() - tao_in[&netuid0].to_num::<f64>(),
            epsilon = 0.01
        );
    });
}

#[test]
fn test_coinbase_subnet_terms_with_alpha_in_lte_alpha_emission() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        mock::setup_reserves(
            netuid0,
            TaoBalance::from(1_000_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );
        // Initialize swap
        Swap::maybe_initialize_palswap(netuid0, None);

        let alpha_emission = U96F32::saturating_from_num(
            SubtensorModule::get_block_emission_for_issuance(
                SubtensorModule::get_alpha_issuance(netuid0).into(),
            )
            .unwrap_or(0),
        );
        let tao_emission = U96F32::saturating_from_num(34566756_u64);

        let price: U96F32 = U96F32::saturating_from_num(Swap::current_alpha_price(netuid0));

        let subnet_emissions = BTreeMap::from([(netuid0, tao_emission)]);

        // The injection cap is root_proportion * alpha_emission. Seed root stake so
        // the cap is large enough that raw alpha_in stays under it (no excess).
        set_full_injection_root_stake();
        let root_prop: U96F32 = SubtensorModule::root_proportion(netuid0);
        let injection_cap: U96F32 = root_prop.saturating_mul(alpha_emission);

        let (tao_in, alpha_in, alpha_out, excess_tao) =
            SubtensorModule::get_subnet_terms(&subnet_emissions);

        // Check our condition is met: raw alpha_in stays under the cap.
        assert!(tao_emission / price <= injection_cap);

        // alpha_out should be the alpha_emission, always
        assert_abs_diff_eq!(
            alpha_out[&netuid0].to_num::<f64>(),
            alpha_emission.to_num::<f64>(),
            epsilon = 0.1
        );

        // assuming alpha_in < alpha_emission
        // Then alpha_in should be tao_emission / price
        assert_abs_diff_eq!(
            alpha_in[&netuid0].to_num::<f64>(),
            tao_emission.to_num::<f64>() / price.to_num::<f64>(),
            epsilon = 0.01
        );

        // tao_in should be the tao_emission
        assert_abs_diff_eq!(
            tao_in[&netuid0].to_num::<f64>(),
            tao_emission.to_num::<f64>(),
            epsilon = 0.01
        );

        // excess_tao should be 0
        assert_abs_diff_eq!(
            excess_tao[&netuid0].to_num::<f64>(),
            tao_emission.to_num::<f64>() - tao_in[&netuid0].to_num::<f64>(),
            epsilon = 0.01
        );
    });
}

#[test]
fn test_get_subnet_terms_alpha_emissions_cap() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(10);
        let owner_coldkey = U256::from(11);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        // The injection cap is now root_proportion * alpha_emission. Seed root stake
        // so root_proportion is well-defined, and derive the cap from the live values.
        set_full_injection_root_stake();
        let alpha_emission_i: U96F32 = U96F32::saturating_from_num(
            SubtensorModule::get_block_emission_for_issuance(
                SubtensorModule::get_alpha_issuance(netuid).into(),
            )
            .unwrap_or(0),
        );
        let injection_cap: U96F32 =
            SubtensorModule::root_proportion(netuid).saturating_mul(alpha_emission_i);

        // price = 1.0, alpha_in_i (== emissions1) <= alpha_injection_cap (not capped)
        let emissions1 = U96F32::from_num(100_000_000);
        assert!(emissions1 < injection_cap);

        let subnet_emissions1 = BTreeMap::from([(netuid, emissions1)]);
        let (_, alpha_in, _, _) = SubtensorModule::get_subnet_terms(&subnet_emissions1);

        assert_eq!(alpha_in.get(&netuid).copied().unwrap(), emissions1);

        // price = 1.0, alpha_in_i (== emissions2) > alpha_injection_cap (capped)
        let emissions2 = U96F32::from_num(10_000_000_000u64);
        assert!(emissions2 > injection_cap);

        let subnet_emissions2 = BTreeMap::from([(netuid, emissions2)]);
        let (_, alpha_in, _, _) = SubtensorModule::get_subnet_terms(&subnet_emissions2);

        assert_eq!(alpha_in.get(&netuid).copied().unwrap(), injection_cap);
    });
}
