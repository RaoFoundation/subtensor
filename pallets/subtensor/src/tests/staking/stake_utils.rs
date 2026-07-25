#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
//! Tests for [`crate::staking::stake_utils`] swap fee / large-swap helpers.

use approx::assert_abs_diff_eq;
use frame_support::assert_ok;
use sp_core::U256;
use subtensor_runtime_common::{AlphaBalance, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

use super::super::mock;
use super::super::mock::*;
use crate::*;

#[test]
fn test_swap_fees_tao_correctness() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let coldkey = U256::from(4);
        let block_builder = U256::from(12345u64);
        let amount = TaoBalance::from(1_000_000_000_u64);
        let owner_balance_before = amount * 10.into();
        let user_balance_before = amount * 100.into();

        // add network
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        add_balance_to_coldkey_account(&owner_coldkey, owner_balance_before);
        add_balance_to_coldkey_account(&coldkey, user_balance_before);

        // Forse-set alpha in and tao reserve to make price equal 0.25
        let tao_reserve = TaoBalance::from(100_000_000_000_u64);
        let alpha_in = AlphaBalance::from(400_000_000_000_u64);
        mock::setup_reserves(netuid, tao_reserve, alpha_in);

        // Check starting "total TAO"
        let block_builder_balance_before = SubtensorModule::get_coldkey_balance(&block_builder);
        let total_tao_before = user_balance_before
            + owner_balance_before
            + SubnetTAO::<Test>::get(netuid)
            + block_builder_balance_before;

        // Get alpha for owner
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(owner_coldkey),
            owner_hotkey,
            netuid,
            amount.into(),
        ));

        // Add owner coldkey Alpha as concentrated liquidity
        // between current price current price + 0.01
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into())
                .to_num::<f64>()
                + 0.0001;
        let limit_price = current_price + 0.01;

        // Limit-buy and then sell all alpha for user to hit owner liquidity
        assert_ok!(SubtensorModule::add_stake_limit(
            RuntimeOrigin::signed(coldkey),
            owner_hotkey,
            netuid,
            amount.into(),
            ((limit_price * u64::MAX as f64) as u64).into(),
            true
        ));

        let user_alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &owner_hotkey,
            &coldkey,
            netuid,
        );
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey),
            owner_hotkey,
            netuid,
            user_alpha,
        ));

        // TODO: This block is for balancer swap
        // Cause tao fees to propagate to SubnetTAO
        // let (claimed_tao_fees, _) =
        //     <Test as pallet::Config>::SwapInterface::adjust_protocol_liquidity(
        //         netuid,
        //         0.into(),
        //         0.into(),
        //     );
        // SubnetTAO::<Test>::mutate(netuid, |tao| *tao += claimed_tao_fees);

        // Check ending "total TAO"
        let owner_balance_after = SubtensorModule::get_coldkey_balance(&owner_coldkey);
        let user_balance_after = SubtensorModule::get_coldkey_balance(&coldkey);
        let block_builder_balance_after = SubtensorModule::get_coldkey_balance(&block_builder);

        let total_tao_after = user_balance_after
            + owner_balance_after
            + SubnetTAO::<Test>::get(netuid)
            + block_builder_balance_after;

        // Total TAO does not change, leave some epsilon for rounding
        assert_abs_diff_eq!(total_tao_before, total_tao_after, epsilon = 2.into());
    });
}

#[test]
fn test_large_swap() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let coldkey = U256::from(100);

        // add network
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_000_u64.into());
        let swap_amount = TaoBalance::from(100_000_000_000_000_u64);
        let tao = TaoBalance::from(swap_amount.to_u64() / 1000);
        let alpha = AlphaBalance::from(1_000_000_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid, tao);
        SubnetAlphaIn::<Test>::insert(netuid, alpha);

        // Force the swap to initialize
        <Test as pallet::Config>::SwapInterface::init_swap(netuid, None);

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            owner_hotkey,
            netuid,
            swap_amount,
        ));
    });
}
