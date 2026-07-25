#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Liquidity inject-and-maybe-swap and TAO materialization.

use super::helpers::*;
use super::prelude::*;

// Tests for the inject and swap are in the right order.
#[test]
fn test_coinbase_inject_and_maybe_swap_does_not_skew_reserves() {
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

        let tao_in = BTreeMap::from([(netuid0, U96F32::saturating_from_num(123))]);
        let alpha_in = BTreeMap::from([(netuid0, U96F32::saturating_from_num(456))]);
        // We have excess TAO, so we will be swapping with it.
        let excess_tao = BTreeMap::from([(netuid0, U96F32::saturating_from_num(789100))]);

        // Run the inject and maybe swap
        let credit = SubtensorModule::mint_tao((123 + 789100).into());
        SubtensorModule::inject_and_maybe_swap(&[netuid0], &tao_in, &alpha_in, &excess_tao, credit);

        let tao_in_after = SubnetTAO::<Test>::get(netuid0);
        let alpha_in_after = SubnetAlphaIn::<Test>::get(netuid0);

        // Make sure that when we inject and swap, we do it in the right order.
        // Thereby not skewing the ratio away from the price.
        let ratio_after: U96F32 = U96F32::saturating_from_num(alpha_in_after.to_u64())
            .saturating_div(U96F32::saturating_from_num(tao_in_after.to_u64()));
        let price_after: U96F32 = U96F32::saturating_from_num(
            pallet_subtensor_swap::Pallet::<Test>::current_alpha_price(netuid0).to_num::<f64>(),
        );
        assert_abs_diff_eq!(
            ratio_after.to_num::<f64>(),
            price_after.to_num::<f64>(),
            epsilon = 1.0
        );
    });
}

#[test]
fn test_coinbase_failed_tao_materialization_does_not_activate_current_tao() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
        let initial_reserve = TaoBalance::from(1_000_000_u64);
        let reservoir_tao = TaoBalance::from(100_u64);
        let current_tao = TaoBalance::from(200_u64);
        let current_alpha = AlphaBalance::from(100_u64);

        mock::setup_reserves(netuid, initial_reserve, AlphaBalance::from(1_000_000_u64));
        Swap::maybe_initialize_palswap(netuid, None);
        pallet_subtensor_swap::BalancerTaoReservoir::<Test>::insert(netuid, reservoir_tao);

        let tao_in = BTreeMap::from([(netuid, U96F32::saturating_from_num(current_tao))]);
        let alpha_in = BTreeMap::from([(netuid, U96F32::saturating_from_num(current_alpha))]);
        let excess_tao = BTreeMap::new();
        let credit = SubtensorModule::mint_tao(TaoBalance::ZERO);

        SubtensorModule::inject_and_maybe_swap(&[netuid], &tao_in, &alpha_in, &excess_tao, credit);

        assert_eq!(
            SubnetTAO::<Test>::get(netuid),
            initial_reserve.saturating_add(reservoir_tao)
        );
        assert_eq!(SubnetTaoInEmission::<Test>::get(netuid), reservoir_tao);
        assert_eq!(
            SubnetProtocolFlow::<Test>::get(netuid),
            reservoir_tao.to_u64() as i64
        );
        assert_eq!(
            pallet_subtensor_swap::BalancerTaoReservoir::<Test>::get(netuid),
            TaoBalance::ZERO
        );
    });
}

#[test]
fn test_alpha_reservoir_counts_toward_subnet_issuance_across_blocks() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
        let alpha_in = AlphaBalance::from(10_000_u64);
        let alpha_out = AlphaBalance::from(20_000_u64);
        let reservoir_alpha = AlphaBalance::from(30_000_u64);

        SubnetAlphaIn::<Test>::insert(netuid, alpha_in);
        SubnetAlphaOut::<Test>::insert(netuid, alpha_out);
        pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::insert(netuid, reservoir_alpha);

        let expected = alpha_in
            .saturating_add(alpha_out)
            .saturating_add(reservoir_alpha);
        assert_eq!(SubtensorModule::get_alpha_issuance(netuid), expected);

        System::set_block_number(System::block_number().saturating_add(1));

        assert_eq!(SubnetAlphaIn::<Test>::get(netuid), alpha_in);
        assert_eq!(
            pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::get(netuid),
            reservoir_alpha
        );
        assert_eq!(SubtensorModule::get_alpha_issuance(netuid), expected);
    });
}

#[test]
fn test_coinbase_inject_and_maybe_swap_reverts_excess_tao_deposit_on_swap_failure() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
        let tao_to_swap = TaoBalance::from(789_100_u64);

        mock::setup_reserves(
            netuid,
            TaoBalance::from(1_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_u64),
        );
        Swap::maybe_initialize_palswap(netuid, None);

        // Force the buy swap to fail after the excess TAO credit is deposited.
        SubnetAlphaIn::<Test>::set(
            netuid,
            AlphaBalance::from(u64::from(mock::SwapMinimumReserve::get()) - 1),
        );
        assert!(
            SubtensorModule::swap_tao_for_alpha(
                netuid,
                tao_to_swap,
                <Test as Config>::SwapInterface::max_price(),
                true,
            )
            .is_err()
        );

        let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        let chain_before = Balances::free_balance(subnet_account);
        let subnet_tao_before = SubnetTAO::<Test>::get(netuid);
        let total_issuance_before = TotalIssuance::<Test>::get();
        let balances_issuance_before = Balances::total_issuance();

        let tao_in = BTreeMap::from([(netuid, zero)]);
        let alpha_in = BTreeMap::from([(netuid, zero)]);
        let excess_tao = BTreeMap::from([(netuid, U96F32::saturating_from_num(tao_to_swap))]);
        let credit = SubtensorModule::mint_tao(tao_to_swap);

        SubtensorModule::inject_and_maybe_swap(&[netuid], &tao_in, &alpha_in, &excess_tao, credit);

        assert_eq!(Balances::free_balance(subnet_account), chain_before);
        assert_eq!(SubnetTAO::<Test>::get(netuid), subnet_tao_before);
        assert_eq!(SubnetExcessTao::<Test>::get(netuid), TaoBalance::ZERO);
        assert_eq!(TotalIssuance::<Test>::get(), total_issuance_before);
        assert_eq!(Balances::total_issuance(), balances_issuance_before);
    });
}
