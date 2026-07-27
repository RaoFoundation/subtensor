#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! emit_to_subnets with/without root sell.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_coinbase_emit_to_subnets_with_no_root_sell() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        // Set owner cut to ~10%
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        mock::setup_reserves(
            netuid0,
            TaoBalance::from(1_000_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );
        // Initialize swap
        Swap::maybe_initialize_palswap(netuid0, None);

        let tao_emission = U96F32::saturating_from_num(12345678);
        let subnet_emissions = BTreeMap::from([(netuid0, tao_emission)]);

        // NO root sell
        let root_sell_flag = false;

        let alpha_emission = U96F32::saturating_from_num(
            SubtensorModule::get_block_emission_for_issuance(
                SubtensorModule::get_alpha_issuance(netuid0).into(),
            )
            .unwrap_or(0),
        );
        let price: U96F32 = U96F32::saturating_from_num(Swap::current_alpha_price(netuid0));
        let (tao_in, alpha_in, alpha_out, excess_tao) =
            SubtensorModule::compute_subnet_emission_terms(&subnet_emissions);
        // Based on the price, we should have NO excess TAO
        assert!(tao_emission / price <= alpha_emission);

        // ==== Run the emit to subnets =====
        let credit = SubtensorModule::mint_tao(12345678.into());
        SubtensorModule::emit_to_subnets(&[netuid0], &subnet_emissions, credit, root_sell_flag);

        // Find the owner cut expected
        let owner_cut: U96F32 = SubtensorModule::get_float_subnet_owner_cut();
        let owner_cut_expected: U96F32 = owner_cut.saturating_mul(alpha_emission);
        log::info!("owner_cut_expected: {owner_cut_expected:?}");
        log::info!("alpha_emission: {alpha_emission:?}");
        log::info!("owner_cut: {owner_cut:?}");

        let alpha_issuance: U96F32 =
            U96F32::saturating_from_num(SubtensorModule::get_alpha_issuance(netuid0));
        let root_tao: U96F32 = U96F32::saturating_from_num(SubnetTAO::<Test>::get(NetUid::ROOT));
        let tao_weight: U96F32 = root_tao.saturating_mul(SubtensorModule::get_tao_weight());
        let root_prop: U96F32 = tao_weight
            .checked_div(tao_weight.saturating_add(alpha_issuance))
            .unwrap_or(U96F32::min_value());
        // Expect root alpha divs to be root prop * alpha emission
        let expected_root_alpha_divs: AlphaBalance = AlphaBalance::from(
            root_prop
                .saturating_mul(alpha_emission)
                .saturating_to_num::<u64>(),
        );

        // ===== Check that the pending emissions are set correctly =====
        // Owner cut is as expected
        assert_abs_diff_eq!(
            PendingOwnerCut::<Test>::get(netuid0).to_u64(),
            owner_cut_expected.saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
        // NO root sell, so no root alpha divs
        assert_eq!(
            PendingRootAlphaDivs::<Test>::get(netuid0),
            AlphaBalance::ZERO
        );
        // Should be alpha_emission minus the owner cut,
        assert_abs_diff_eq!(
            PendingServerEmission::<Test>::get(netuid0).to_u64(),
            alpha_emission
                .saturating_sub(owner_cut_expected)
                .saturating_div(U96F32::saturating_from_num(2))
                .saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
        // We ALWAYS deduct the root alpha divs
        assert_abs_diff_eq!(
            PendingValidatorEmission::<Test>::get(netuid0).to_u64(),
            alpha_emission
                .saturating_sub(owner_cut_expected)
                .saturating_div(U96F32::saturating_from_num(2))
                .saturating_sub(expected_root_alpha_divs.to_u64().into())
                .saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
    });
}

#[test]
fn test_coinbase_emit_to_subnets_with_root_sell() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        // Set owner cut to ~10%
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        mock::setup_reserves(
            netuid0,
            TaoBalance::from(1_000_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );
        // Initialize swap
        Swap::maybe_initialize_palswap(netuid0, None);

        let tao_emission = U96F32::saturating_from_num(12345678);
        let subnet_emissions = BTreeMap::from([(netuid0, tao_emission)]);

        // NO root sell
        let root_sell_flag = true;

        let alpha_emission: U96F32 = U96F32::saturating_from_num(
            SubtensorModule::get_block_emission_for_issuance(
                SubtensorModule::get_alpha_issuance(netuid0).into(),
            )
            .unwrap_or(0),
        );
        let price: U96F32 = U96F32::saturating_from_num(Swap::current_alpha_price(netuid0));
        let (tao_in, alpha_in, alpha_out, excess_tao) =
            SubtensorModule::compute_subnet_emission_terms(&subnet_emissions);
        // Based on the price, we should have NO excess TAO
        assert!(tao_emission / price <= alpha_emission);

        // ==== Run the emit to subnets =====
        let credit = SubtensorModule::mint_tao(12345678.into());
        SubtensorModule::emit_to_subnets(&[netuid0], &subnet_emissions, credit, root_sell_flag);

        // Find the owner cut expected
        let owner_cut: U96F32 = SubtensorModule::get_float_subnet_owner_cut();
        let owner_cut_expected: U96F32 = owner_cut.saturating_mul(alpha_emission);
        log::info!("owner_cut_expected: {owner_cut_expected:?}");
        log::info!("alpha_emission: {alpha_emission:?}");
        log::info!("owner_cut: {owner_cut:?}");

        let alpha_issuance: U96F32 =
            U96F32::saturating_from_num(SubtensorModule::get_alpha_issuance(netuid0));
        let root_tao: U96F32 = U96F32::saturating_from_num(SubnetTAO::<Test>::get(NetUid::ROOT));
        let tao_weight: U96F32 = root_tao.saturating_mul(SubtensorModule::get_tao_weight());
        let root_prop: U96F32 = tao_weight
            .checked_div(tao_weight.saturating_add(alpha_issuance))
            .unwrap_or(U96F32::min_value());
        // Expect root alpha divs to be root prop * alpha emission
        let expected_root_alpha_divs: AlphaBalance = AlphaBalance::from(
            root_prop
                .saturating_mul(alpha_emission)
                .saturating_to_num::<u64>(),
        );

        // ===== Check that the pending emissions are set correctly =====
        // Owner cut is as expected
        assert_abs_diff_eq!(
            PendingOwnerCut::<Test>::get(netuid0).to_u64(),
            owner_cut_expected.saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
        // YES root sell, so we have root alpha divs
        assert_abs_diff_eq!(
            PendingRootAlphaDivs::<Test>::get(netuid0).to_u64(),
            expected_root_alpha_divs.to_u64(),
            epsilon = 200_u64
        );
        // Should be alpha_emission minus the owner cut
        assert_abs_diff_eq!(
            PendingServerEmission::<Test>::get(netuid0).to_u64(),
            alpha_emission
                .saturating_sub(owner_cut_expected)
                .saturating_div(U96F32::saturating_from_num(2))
                .saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
        // Validator emission is also minus root alpha divs
        assert_abs_diff_eq!(
            PendingValidatorEmission::<Test>::get(netuid0).to_u64(),
            alpha_emission
                .saturating_sub(owner_cut_expected)
                .saturating_div(U96F32::saturating_from_num(2))
                .saturating_sub(expected_root_alpha_divs.to_u64().into())
                .saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
    });
}
