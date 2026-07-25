#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
//! Tests for [`crate::staking::remove_stake`] core remove / fee / precision paths.

use approx::assert_abs_diff_eq;
use frame_support::sp_runtime::DispatchError;
use frame_support::{assert_err, assert_noop, assert_ok, traits::Currency};
use frame_system::RawOrigin;
use sp_core::{Get, U256};
use substrate_fixed::types::{U64F64, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

use super::super::mock;
use super::super::mock::*;
use crate::*;

#[test]
fn test_remove_stake_ok_no_emission() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let coldkey_account_id = U256::from(4343);
        let hotkey_account_id = U256::from(4968585);
        let amount = DefaultMinStake::<Test>::get() * 10.into();

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

        // Clear any implicit existing stake so we can fully remove exactly `amount`
        let existing = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );
        if !existing.is_zero() {
            SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid,
                existing,
            );
        }

        // Create stake without relying on any emission/weights assumptions
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            amount.to_u64().into(),
        );

        let expected_stake: AlphaBalance = amount.to_u64().into();
        let epsilon_stake: AlphaBalance = (amount.to_u64() / 1000).into();

        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            ),
            expected_stake,
            epsilon = epsilon_stake
        );

        // Snapshot baselines before we top up SubnetTAO / TotalStake
        let base_total_stake = SubtensorModule::get_total_stake();
        let balance_before = SubtensorModule::get_coldkey_balance(&coldkey_account_id);

        // Add subnet TAO so remove_stake can pay out (keep original pattern)
        let (amount_tao, fee) = mock::swap_alpha_to_tao(netuid, amount.to_u64().into());
        SubnetTAO::<Test>::mutate(netuid, |v| *v += amount_tao + fee.into());
        TotalStake::<Test>::mutate(|v| *v += amount_tao + fee.into());

        // Do the magic
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.to_u64().into()
        ));

        // we do not expect the exact amount due to slippage, but it must increase meaningfully
        let balance_after = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
        assert!(balance_after > balance_before);
        assert!(
            (balance_after - balance_before) > amount / 10.into() * 9.into() - fee.into(),
            "Payout lower than expected lower bound"
        );

        // All stake removed
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            )
            .is_zero()
        );

        // Total stake should net-increase only by fee (everything else returned)
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake(),
            base_total_stake + fee.into(),
            epsilon = SubtensorModule::get_total_stake() / 100_000.into()
        );
    });
}

#[test]
fn test_remove_stake_amount_too_low() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let coldkey_account_id = U256::from(4343);
        let hotkey_account_id = U256::from(4968585);
        let amount: u64 = 10_000;

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

        // Ensure deterministic starting stake for this (hotkey,coldkey,netuid)
        let existing = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );
        if !existing.is_zero() {
            SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid,
                existing,
            );
        }

        // Give the neuron some stake to remove
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            amount.into(),
        );

        // Removing zero should fail
        assert_noop!(
            SubtensorModule::remove_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                AlphaBalance::ZERO
            ),
            Error::<Test>::AmountTooLow
        );
    });
}

#[test]
fn test_remove_stake_below_min_stake() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let coldkey_account_id = U256::from(4343);
        let hotkey_account_id = U256::from(4968585);

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

        // Clear any implicit existing stake so the test always starts below-min
        let existing = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );
        if !existing.is_zero() {
            SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid,
                existing,
            );
        }

        let min_stake = DefaultMinStake::<Test>::get();
        let amount = AlphaBalance::from(min_stake.to_u64() / 2);

        // Give the neuron some *below-min* stake to remove
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            amount,
        );

        // Unstake less than full stake -> leaves a non-zero remainder below min -> errors
        assert_noop!(
            SubtensorModule::remove_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount - 1.into()
            ),
            Error::<Test>::AmountTooLow
        );

        // Unstaking full stake - works
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount
        ));
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid,
            )
            .is_zero()
        );
    });
}

#[test]
fn test_remove_stake_err_signature() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(4968585);
        let amount = AlphaBalance::from(10000); // Amount to be removed
        let netuid = NetUid::from(1);

        assert_err!(
            SubtensorModule::remove_stake(
                RawOrigin::None.into(),
                hotkey_account_id,
                netuid,
                amount,
            ),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_remove_stake_ok_hotkey_does_not_belong_to_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey_id = U256::from(544);
        let hotkey_id = U256::from(54544);
        let other_cold_key = U256::from(99498);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;
        let netuid = add_dynamic_network(&hotkey_id, &coldkey_id);

        // Give the neuron some stake to remove
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &other_cold_key,
            netuid,
            amount.into(),
        );

        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(other_cold_key),
            hotkey_id,
            netuid,
            amount.into(),
        ));
    });
}

#[test]
fn test_remove_stake_no_enough_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey_id = U256::from(544);
        let hotkey_id = U256::from(54544);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;
        let netuid = add_dynamic_network(&hotkey_id, &coldkey_id);
        remove_owner_registration_stake(netuid);

        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_id),
            TaoBalance::ZERO
        );

        assert_err!(
            SubtensorModule::remove_stake(
                RuntimeOrigin::signed(coldkey_id),
                hotkey_id,
                netuid,
                amount.into(),
            ),
            Error::<Test>::AmountTooLow
        );
    });
}

#[test]
fn test_remove_stake_total_balance_no_change() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let hotkey_account_id = U256::from(571337);
        let coldkey_account_id = U256::from(71337);
        let amount: u64 = DefaultMinStake::<Test>::get().to_u64() * 10;

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

        // Set fee rate to 0 so that alpha fee is not moved to block producer
        pallet_subtensor_swap::FeeRate::<Test>::insert(netuid, 0);
        let fee: u64 = 0;

        // Clear any implicit existing stake so the test is deterministic
        let existing = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );
        if !existing.is_zero() {
            SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid,
                existing,
            );
        }

        let balance_before = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
        let total_balance_before = Balances::total_balance(&coldkey_account_id);
        let base_total_stake = SubtensorModule::get_total_stake();

        // Give the neuron some stake to remove
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            amount.into(),
        );

        // Add subnet TAO for the equivalent amount added at price
        let amount_tao = U96F32::from_num(amount)
            * U96F32::from_num(
                <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into()),
            );
        let amount_tao: TaoBalance = amount_tao.to_num::<u64>().into();
        SubnetTAO::<Test>::mutate(netuid, |v| *v += amount_tao);
        TotalStake::<Test>::mutate(|v| *v += amount_tao);

        // Remove stake
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.into()
        ));

        let balance_after = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
        let total_balance_after = Balances::total_balance(&coldkey_account_id);

        // Free balance should increase by roughly the TAO paid out (net of swap mechanics)
        assert!(balance_after > balance_before);
        assert!(
            (balance_after - balance_before) > amount_tao / 10.into() * 9.into() - fee.into(),
            "Payout lower than expected lower bound"
        );

        // Total balance should track the same change (since stake becomes free)
        assert!(total_balance_after > total_balance_before);

        // Total stake should net-increase only by fee
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake(),
            base_total_stake + fee.into(),
            epsilon = SubtensorModule::get_total_stake() / 10_000_000.into()
        );

        assert_abs_diff_eq!(
            total_balance_after - total_balance_before,
            amount_tao - fee.into(),
            epsilon = TaoBalance::from(amount) / 1000.into()
        );
    });
}

#[test]
fn test_remove_stake_insufficient_liquidity() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let amount_staked = DefaultMinStake::<Test>::get().to_u64() * 10;

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, amount_staked.into());

        // Simulate stake for hotkey
        let reserve = u64::MAX / 1000;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        let alpha = SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid,
            amount_staked.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        // Set the liquidity at lowest possible value so that all staking requests fail
        let reserve = u64::from(mock::SwapMinimumReserve::get()) - 1;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Check the error
        assert_noop!(
            SubtensorModule::remove_stake(RuntimeOrigin::signed(coldkey), hotkey, netuid, alpha),
            Error::<Test>::InsufficientLiquidity
        );

        // Mock more liquidity - remove becomes successful
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(amount_staked + 1));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(alpha.to_u64() / 1000 + 1));
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            alpha
        ),);
    });
}

#[test]
fn test_remove_stake_total_issuance_no_change() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let hotkey_account_id = U256::from(581337);
        let coldkey_account_id = U256::from(81337);
        let amount: u64 = DefaultMinStake::<Test>::get().to_u64() * 10;

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

        // Set fee rate to 0 so that alpha fee is not moved to block producer
        pallet_subtensor_swap::FeeRate::<Test>::insert(netuid, 0);

        // Ensure the coldkey has at least 'amount' more balance available for staking
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        mock::setup_reserves(netuid, (amount * 100).into(), (amount * 100).into());

        // Baselines (after registration + funding)
        let balance_before_stake = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
        let issuance_before = Balances::total_issuance();
        let base_total_stake = SubtensorModule::get_total_stake();

        // Stake exactly `amount` TAO
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            TaoBalance::from(amount),
        ));

        let issuance_after_stake = Balances::total_issuance();

        // Staking burns `amount` from balances issuance in this system design.
        assert_abs_diff_eq!(issuance_before, issuance_after_stake, epsilon = 1.into());

        // Remove all stake
        let stake_alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );

        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            stake_alpha,
        ));

        let issuance_after_unstake = Balances::total_issuance();

        // Ground-truth fee/loss is the net issuance reduction after stake+unstake.
        let fee_balance = issuance_before.saturating_sub(issuance_after_unstake);
        let total_fee_actual: u64 = fee_balance.into();

        // Final coldkey balance should be baseline minus the effective fee.
        let balance_after = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
        assert_abs_diff_eq!(
            balance_after,
            (balance_before_stake.saturating_sub(total_fee_actual.into())).into(),
            epsilon = 50.into()
        );

        // Stake should be cleared.
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            )
            .is_zero()
        );

        // Total stake should only increase by what stayed in pools (fees/rounding).
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake(),
            base_total_stake + TaoBalance::from(total_fee_actual),
            epsilon = TaoBalance::from(500u64)
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::remove_stake::test_remove_prev_epoch_stake --exact --show-output --nocapture
#[test]
fn test_remove_prev_epoch_stake() {
    new_test_ext(1).execute_with(|| {
        // Test case: (amount_to_stake, AlphaDividendsPerSubnet, TotalHotkeyAlphaLastEpoch, expected_fee)
        [
            // No previous epoch stake and low hotkey stake
            (
                DefaultMinStake::<Test>::get().to_u64() * 10,
                0_u64,
                1000_u64,
            ),
            // Same, but larger amount to stake - we get 0.005% for unstake
            (1_000_000_000, 0_u64, 1000_u64),
            (100_000_000_000, 0_u64, 1000_u64),
            // Lower previous epoch stake than current stake
            // Staking/unstaking 100 TAO, divs / total = 0.1 => fee is 1 TAO
            (100_000_000_000, 1_000_000_000_u64, 10_000_000_000_u64),
            // Staking/unstaking 100 TAO, divs / total = 0.001 => fee is 0.01 TAO
            (100_000_000_000, 10_000_000_u64, 10_000_000_000_u64),
            // Higher previous epoch stake than current stake
            (1_000_000_000, 100_000_000_000_u64, 100_000_000_000_000_u64),
        ]
        .into_iter()
        .for_each(|(amount_to_stake, alpha_divs, hotkey_alpha)| {
            let alpha_divs = AlphaBalance::from(alpha_divs);
            let hotkey_alpha = AlphaBalance::from(hotkey_alpha);
            let subnet_owner_coldkey = U256::from(1);
            let subnet_owner_hotkey = U256::from(2);
            let hotkey_account_id = U256::from(581337);
            let coldkey_account_id = U256::from(81337);
            let amount = amount_to_stake;
            let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
            register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

            // Give it some $$$ in his coldkey balance
            add_balance_to_coldkey_account(&coldkey_account_id, amount.into());
            AlphaDividendsPerSubnet::<Test>::insert(netuid, hotkey_account_id, alpha_divs);
            TotalHotkeyAlphaLastEpoch::<Test>::insert(hotkey_account_id, netuid, hotkey_alpha);
            let balance_before = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
            mock::setup_reserves(
                netuid,
                (amount_to_stake * 10).into(),
                (amount_to_stake * 10).into(),
            );

            // Stake to hotkey account, and check if the result is ok
            let (_, fee) = mock::swap_tao_to_alpha(netuid, amount.into());
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount.into()
            ));

            // Remove all stake
            let stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid,
            );

            let fee = mock::swap_alpha_to_tao(netuid, stake).1 + fee;
            assert_ok!(SubtensorModule::remove_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                stake
            ));

            // Measure actual fee
            let balance_after = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
            let actual_fee = balance_before - balance_after;

            assert_abs_diff_eq!(actual_fee, fee.into(), epsilon = (fee / 100).into());
        });
    });
}

/************************************************************
    staking::remove_stake_from_hotkey_account() tests
************************************************************/
#[test]
fn test_remove_stake_from_hotkey_account() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let hotkey_id = U256::from(5445);
        let coldkey_id = U256::from(5443433);
        let amount: AlphaBalance = 10_000u64.into();

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey_id, coldkey_id, 192213123);

        // Baselines before adding stake.
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
        );
        let total_before = SubtensorModule::get_total_stake_for_hotkey(&hotkey_id);

        // Add alpha stake directly through the internal helper.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
            amount,
        );

        // Alpha stake should increase by exactly the credited alpha amount.
        let alpha_after_add = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
        );
        assert_eq!(alpha_after_add, alpha_before.saturating_add(amount));

        // Tao-equivalent total stake should have increased from baseline.
        assert!(SubtensorModule::get_total_stake_for_hotkey(&hotkey_id) > total_before);

        // Remove exactly the same alpha amount.
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
            amount,
        );

        // Alpha stake should return to its original baseline.
        let alpha_after_remove = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
        );
        assert_eq!(alpha_after_remove, alpha_before);

        // Tao-equivalent total stake should also return to baseline.
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_id),
            total_before,
            epsilon = 10.into()
        );
    });
}

#[test]
fn test_remove_stake_from_hotkey_account_registered_in_various_networks() {
    new_test_ext(1).execute_with(|| {
        let hotkey_id = U256::from(5445);
        let coldkey_id = U256::from(5443433);
        let amount: u64 = 10_000;
        let netuid = add_dynamic_network(&hotkey_id, &coldkey_id);
        remove_owner_registration_stake(netuid);
        let netuid_ex = add_dynamic_network(&hotkey_id, &coldkey_id);
        remove_owner_registration_stake(netuid_ex);

        let neuron_uid = match SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey_id) {
            Ok(k) => k,
            Err(e) => panic!("Error: {e:?}"),
        };

        let neuron_uid_ex = match SubtensorModule::get_uid_for_net_and_hotkey(netuid_ex, &hotkey_id)
        {
            Ok(k) => k,
            Err(e) => panic!("Error: {e:?}"),
        };

        // Add some stake that can be removed
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
            amount.into(),
        );

        assert_eq!(
            SubtensorModule::get_stake_for_uid_and_subnetwork(netuid, neuron_uid),
            amount.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_uid_and_subnetwork(netuid_ex, neuron_uid_ex),
            AlphaBalance::ZERO
        );

        // Remove all stake
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
            amount.into(),
        );

        //
        assert_eq!(
            SubtensorModule::get_stake_for_uid_and_subnetwork(netuid, neuron_uid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_uid_and_subnetwork(netuid_ex, neuron_uid_ex),
            AlphaBalance::ZERO
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::remove_stake::test_remove_stake_fee_goes_to_subnet_tao --exact --show-output --nocapture
#[ignore = "fees no go to liquidity providers"]
#[test]
fn test_remove_stake_fee_goes_to_subnet_tao() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let tao_to_stake = DefaultMinStake::<Test>::get() * 10.into();

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        let subnet_tao_before = SubnetTAO::<Test>::get(netuid);

        // Add stake
        add_balance_to_coldkey_account(&coldkey, tao_to_stake.into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            tao_to_stake
        ));

        // Remove all stake
        let alpha_to_unstake =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            alpha_to_unstake
        ));
        let subnet_tao_after = SubnetTAO::<Test>::get(netuid);

        // Subnet TAO should have increased by 2x fee as a result of staking + unstaking
        assert_abs_diff_eq!(
            subnet_tao_before,
            subnet_tao_after,
            epsilon = (alpha_to_unstake.to_u64() / 1000).into()
        );

        // User balance should decrease by 2x fee as a result of staking + unstaking
        let balance_after = SubtensorModule::get_coldkey_balance(&coldkey);
        assert_abs_diff_eq!(
            balance_after,
            tao_to_stake,
            epsilon = tao_to_stake / 1000.into()
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::remove_stake::test_remove_stake_fee_realistic_values --exact --show-output --nocapture
#[ignore = "fees are now calculated on the SwapInterface side"]
#[test]
fn test_remove_stake_fee_realistic_values() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let alpha_to_unstake = AlphaBalance::from(111_180_000_000_u64);
        let alpha_divs = AlphaBalance::from(2_816_190);

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);

        // Mock a realistic scenario:
        //   Subnet 1 has 3896 TAO and 128_011 Alpha in reserves, which
        //   makes its price ~0.03.
        //   A hotkey has 111 Alpha stake and is unstaking all Alpha.
        //   Alpha dividends of this hotkey are ~0.0028
        //   This makes fee be equal ~0.0028 Alpha ~= 84000 rao
        let tao_reserve = 3_896_056_559_708_u64;
        let alpha_in = 128_011_331_299_964_u64;
        mock::setup_reserves(netuid, tao_reserve.into(), alpha_in.into());
        AlphaDividendsPerSubnet::<Test>::insert(netuid, hotkey, alpha_divs);
        TotalHotkeyAlphaLastEpoch::<Test>::insert(hotkey, netuid, alpha_to_unstake);

        // Add stake first time to init TotalHotkeyAlpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            alpha_to_unstake,
        );

        // Remove stake to measure fee
        let balance_before = SubtensorModule::get_coldkey_balance(&coldkey);
        let (expected_tao, expected_fee) = mock::swap_alpha_to_tao(netuid, alpha_to_unstake);

        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            alpha_to_unstake
        ));

        // Calculate expected fee
        let balance_after = SubtensorModule::get_coldkey_balance(&coldkey);
        // FIXME since fee is calculated by SwapInterface and the values here are after fees, the
        // actual_fee is 0. but it's left here to discuss in review
        let actual_fee = expected_tao - (balance_after - balance_before);
        log::info!("Actual fee: {actual_fee:?}");

        assert_abs_diff_eq!(
            actual_fee,
            expected_fee.into(),
            epsilon = (expected_fee / 1000).into()
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::test_remove_99_999_per_cent_stake_works_precisely --exact --show-output
#[test]
fn test_remove_99_9991_per_cent_stake_works_precisely() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let hotkey_account_id = U256::from(581337);
        let coldkey_account_id = U256::from(81337);
        let amount = 10_000_000_000_u64;
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

        // Set fee rate to 0 so that alpha fee is not moved to block producer.
        pallet_subtensor_swap::FeeRate::<Test>::insert(netuid, 0);

        // Give it some $$$ in his coldkey balance (in addition to any leftover buffer from registration)
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        // Stake to hotkey account.
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.into()
        ));

        // Remove 99.9991% stake.
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );
        let coldkey_balance_before_remove =
            SubtensorModule::get_coldkey_balance(&coldkey_account_id);

        let remove_amount = AlphaBalance::from(
            (U64F64::from_num(alpha) * U64F64::from_num(0.999991)).to_num::<u64>(),
        );

        // Expected TAO returned by swapping exactly the removed alpha.
        let (expected_returned_balance, _) = mock::swap_alpha_to_tao(netuid, remove_amount);

        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            remove_amount,
        ));

        // Compare the returned delta, not the absolute coldkey balance, because
        // registration / staking can leave a small pre-existing balance on coldkey.
        let coldkey_balance_after_remove =
            SubtensorModule::get_coldkey_balance(&coldkey_account_id);
        let actual_returned_balance = TaoBalance::from(
            coldkey_balance_after_remove
                .to_u64()
                .saturating_sub(coldkey_balance_before_remove.to_u64()),
        );

        assert_abs_diff_eq!(
            actual_returned_balance,
            expected_returned_balance,
            epsilon = 10.into(),
        );

        assert!(!SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id).is_zero());

        let new_alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );
        assert_eq!(new_alpha, alpha - remove_amount);
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::remove_stake::test_remove_99_9989_per_cent_stake_leaves_a_little --exact --show-output
#[test]
fn test_remove_99_9989_per_cent_stake_leaves_a_little() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let hotkey_account_id = U256::from(581337);
        let coldkey_account_id = U256::from(81337);
        let amount = 10_000_000_000;
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

        // Set fee rate to 0 so that alpha fee is not moved to block producer
        // to avoid false success in this test
        pallet_subtensor_swap::FeeRate::<Test>::insert(netuid, 0);

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        // Stake to hotkey account, and check if the result is ok
        let (_, fee) = mock::swap_tao_to_alpha(netuid, amount.into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.into()
        ));

        // Remove 99.9989% stake
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );
        let fee =
            mock::swap_alpha_to_tao(netuid, ((alpha.to_u64() as f64 * 0.99) as u64).into()).1 + fee;
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            (U64F64::from_num(alpha.to_u64()) * U64F64::from_num(0.99))
                .to_num::<u64>()
                .into()
        ));

        // Check that all alpha was unstaked and 99% TAO balance was returned (less fees)
        // let fee = <Test as Config>::SwapInterface::approx_fee_amount(netuid.into(), (amount as f64 * 0.99) as u64);
        assert_abs_diff_eq!(
            SubtensorModule::get_coldkey_balance(&coldkey_account_id).to_u64(),
            (amount as f64 * 0.99) as u64 - fee,
            epsilon = amount / 1000,
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id).to_u64(),
            (amount as f64 * 0.01) as u64,
            epsilon = amount / 1000,
        );
        let new_alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );
        assert_abs_diff_eq!(
            new_alpha,
            AlphaBalance::from((alpha.to_u64() as f64 * 0.01) as u64),
            epsilon = 10.into()
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::remove_stake::test_remove_root_updates_counters --exact --show-output
#[test]
fn test_remove_root_updates_counters() {
    new_test_ext(0).execute_with(|| {
        let hotkey_account_id = U256::from(561337);
        let coldkey_account_id = U256::from(61337);
        add_network(NetUid::ROOT, 10, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(coldkey_account_id).clone(),
            hotkey_account_id,
        ));
        let stake_amount = TaoBalance::from(1_000_000_000);

        // Give it some $$$ in his coldkey balance
        let initial_balance = stake_amount + ExistentialDeposit::get();
        add_balance_to_coldkey_account(&coldkey_account_id, initial_balance);

        // Setup existing stake
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            NetUid::ROOT,
            AlphaBalance::from(stake_amount.to_u64()),
        );

        // Setup TotalStake, SubnetAlphaOut and SubnetTAO (because we are going to unstake)
        TotalStake::<Test>::set(stake_amount);
        SubnetTAO::<Test>::insert(NetUid::ROOT, stake_amount);
        SubnetAlphaOut::<Test>::insert(NetUid::ROOT, AlphaBalance::from(stake_amount.to_u64()));

        // Stake to hotkey account, and check if the result is ok
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            NetUid::ROOT,
            AlphaBalance::from(stake_amount.to_u64())
        ));

        // Check if stake has been decreased
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id);
        assert_eq!(new_stake, 0.into());

        // Check if total stake has decreased accordingly.
        assert_eq!(SubtensorModule::get_total_stake(), 0.into());

        // SubnetTAO updated
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), 0.into());

        // SubnetAlphaIn updated
        assert_eq!(
            SubnetAlphaIn::<Test>::get(NetUid::ROOT),
            AlphaBalance::from(stake_amount.to_u64())
        );

        // SubnetAlphaOut updated
        assert_eq!(SubnetAlphaOut::<Test>::get(NetUid::ROOT), 0.into());

        // SubnetVolume updated
        assert_eq!(
            SubnetVolume::<Test>::get(NetUid::ROOT),
            stake_amount.to_u64() as u128
        );
    });
}
