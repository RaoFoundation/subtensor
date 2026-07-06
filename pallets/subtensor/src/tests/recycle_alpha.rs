use super::mock;
use super::mock::*;
use crate::*;
use approx::assert_abs_diff_eq;
use frame_support::{assert_noop, assert_ok, traits::Currency};
use share_pool::SafeFloat;
use sp_core::U256;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::{AlphaBalance, Token};
use subtensor_swap_interface::SwapHandler;

#[test]
fn test_recycle_success() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let owner_coldkey = U256::from(1001);
        let owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        let initial_balance = 1_000_000_000;
        Balances::make_free_balance_be(&coldkey, initial_balance.into());

        // associate coldkey and hotkey
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        assert!(SubtensorModule::if_subnet_exist(netuid));

        // add stake to coldkey-hotkey pair so we can recycle it
        let stake = 200_000;
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake.into(), netuid);

        // get initial total issuance and alpha out
        let initial_alpha = TotalHotkeyAlpha::<Test>::get(hotkey, netuid);
        let initial_net_alpha = SubnetAlphaOut::<Test>::get(netuid);

        // amount to recycle
        let recycle_amount = AlphaBalance::from(stake / 2);

        // recycle
        assert_ok!(SubtensorModule::recycle_alpha(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            recycle_amount,
            netuid
        ));

        assert!(TotalHotkeyAlpha::<Test>::get(hotkey, netuid) < initial_alpha);
        assert!(SubnetAlphaOut::<Test>::get(netuid) < initial_net_alpha);
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid)
                < initial_alpha
        );

        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::AlphaRecycled(..))
            )
        }));
    });
}

#[test]
fn test_recycle_two_stakers() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let other_coldkey = U256::from(3);

        let owner_coldkey = U256::from(1001);
        let owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        let initial_balance = 1_000_000_000;
        Balances::make_free_balance_be(&coldkey, initial_balance.into());

        // associate coldkey and hotkey
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        assert!(SubtensorModule::if_subnet_exist(netuid));

        // add stake to coldkey-hotkey pair so we can recycle it
        let stake = 200_000;
        let (expected_alpha, _) = mock::swap_tao_to_alpha(netuid, stake.into());
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake.into(), netuid);

        // add some stake to other coldkey on same hotkey.
        increase_stake_on_coldkey_hotkey_account(&other_coldkey, &hotkey, stake.into(), netuid);

        // get initial total issuance and alpha out
        let initial_alpha = TotalHotkeyAlpha::<Test>::get(hotkey, netuid);
        let initial_net_alpha = SubnetAlphaOut::<Test>::get(netuid);

        // amount to recycle
        let recycle_amount = AlphaBalance::from(stake / 2);

        // recycle
        assert_ok!(SubtensorModule::recycle_alpha(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            recycle_amount,
            netuid
        ));

        assert!(TotalHotkeyAlpha::<Test>::get(hotkey, netuid) < initial_alpha);
        assert!(SubnetAlphaOut::<Test>::get(netuid) < initial_net_alpha);
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid)
                < stake.into()
        );
        // Make sure the other coldkey has no change
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &other_coldkey,
                netuid
            ),
            expected_alpha,
            epsilon = 2.into()
        );

        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::AlphaRecycled(..))
            )
        }));
    });
}

#[test]
fn test_recycle_staker_is_nominator() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let other_coldkey = U256::from(3);

        let owner_coldkey = U256::from(1001);
        let owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        let initial_balance = 1_000_000_000;
        Balances::make_free_balance_be(&coldkey, initial_balance.into());

        // associate coldkey and hotkey
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        assert!(SubtensorModule::if_subnet_exist(netuid));

        // add stake to coldkey-hotkey pair so we can recycle it
        let stake = 200_000;
        let (expected_alpha, _) = mock::swap_tao_to_alpha(netuid, stake.into());
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake.into(), netuid);

        // add some stake to other coldkey on same hotkey.
        // Note: this coldkey DOES NOT own the hotkey, so it is a nominator.
        increase_stake_on_coldkey_hotkey_account(&other_coldkey, &hotkey, stake.into(), netuid);
        // Verify the ownership
        assert_ne!(
            SubtensorModule::get_owning_coldkey_for_hotkey(&hotkey),
            other_coldkey
        );

        // get initial total issuance and alpha out
        let initial_alpha = TotalHotkeyAlpha::<Test>::get(hotkey, netuid);
        let initial_net_alpha = SubnetAlphaOut::<Test>::get(netuid);

        // amount to recycle
        let recycle_amount = AlphaBalance::from(stake / 2);

        // recycle from nominator coldkey
        assert_ok!(SubtensorModule::recycle_alpha(
            RuntimeOrigin::signed(other_coldkey),
            hotkey,
            recycle_amount,
            netuid
        ));

        assert!(TotalHotkeyAlpha::<Test>::get(hotkey, netuid) < initial_alpha);
        assert!(SubnetAlphaOut::<Test>::get(netuid) < initial_net_alpha);
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &other_coldkey,
                netuid
            ) < stake.into()
        );
        // Make sure the other coldkey has no change
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            expected_alpha,
            epsilon = 2.into()
        );

        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::AlphaRecycled(..))
            )
        }));
    });
}

#[test]
fn test_burn_success() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let owner_coldkey = U256::from(1001);
        let owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        let initial_balance = 1_000_000_000;
        Balances::make_free_balance_be(&coldkey, initial_balance.into());

        // associate coldkey and hotkey
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        assert!(SubtensorModule::if_subnet_exist(netuid));

        // add stake to coldkey-hotkey pair so we can recycle it
        let stake = 200_000;
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake.into(), netuid);

        // get initial total issuance and alpha out
        let initial_alpha = TotalHotkeyAlpha::<Test>::get(hotkey, netuid);
        let initial_net_alpha = SubnetAlphaOut::<Test>::get(netuid);

        // amount to recycle
        let burn_amount = stake / 2;

        // burn
        assert_ok!(SubtensorModule::burn_alpha(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            burn_amount.into(),
            netuid
        ));

        assert!(TotalHotkeyAlpha::<Test>::get(hotkey, netuid) < initial_alpha);
        assert!(SubnetAlphaOut::<Test>::get(netuid) == initial_net_alpha);
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid)
                < stake.into()
        );

        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::AlphaBurned(..))
            )
        }));
    });
}

#[test]
fn test_burn_staker_is_nominator() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let other_coldkey = U256::from(3);

        let owner_coldkey = U256::from(1001);
        let owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        let initial_balance = 1_000_000_000;
        Balances::make_free_balance_be(&coldkey, initial_balance.into());

        // associate coldkey and hotkey
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        assert!(SubtensorModule::if_subnet_exist(netuid));

        // add stake to coldkey-hotkey pair so we can recycle it
        let stake = 200_000;
        let (expected_alpha, _) = mock::swap_tao_to_alpha(netuid, stake.into());
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake.into(), netuid);

        // add some stake to other coldkey on same hotkey.
        // Note: this coldkey DOES NOT own the hotkey, so it is a nominator.
        increase_stake_on_coldkey_hotkey_account(&other_coldkey, &hotkey, stake.into(), netuid);

        // get initial total issuance and alpha out
        let initial_alpha = TotalHotkeyAlpha::<Test>::get(hotkey, netuid);
        let initial_net_alpha = SubnetAlphaOut::<Test>::get(netuid);

        // amount to recycle
        let burn_amount = AlphaBalance::from(stake / 2);

        // burn from nominator coldkey
        assert_ok!(SubtensorModule::burn_alpha(
            RuntimeOrigin::signed(other_coldkey),
            hotkey,
            burn_amount,
            netuid
        ));

        assert!(TotalHotkeyAlpha::<Test>::get(hotkey, netuid) < initial_alpha);
        assert!(SubnetAlphaOut::<Test>::get(netuid) == initial_net_alpha);
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &other_coldkey,
                netuid
            ) < stake.into()
        );
        // Make sure the other coldkey has no change
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            expected_alpha,
            epsilon = 2.into()
        );

        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::AlphaBurned(..))
            )
        }));
    });
}

#[test]
fn test_burn_two_stakers() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let other_coldkey = U256::from(3);

        let owner_coldkey = U256::from(1001);
        let owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        let initial_balance = 1_000_000_000;
        Balances::make_free_balance_be(&coldkey, initial_balance.into());

        // associate coldkey and hotkey
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        assert!(SubtensorModule::if_subnet_exist(netuid));

        // add stake to coldkey-hotkey pair so we can recycle it
        let stake = 200_000;
        let (expected_alpha, _) = mock::swap_tao_to_alpha(netuid, stake.into());
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake.into(), netuid);

        // add some stake to other coldkey on same hotkey.
        increase_stake_on_coldkey_hotkey_account(&other_coldkey, &hotkey, stake.into(), netuid);

        // get initial total issuance and alpha out
        let initial_alpha = TotalHotkeyAlpha::<Test>::get(hotkey, netuid);
        let initial_net_alpha = SubnetAlphaOut::<Test>::get(netuid);

        // amount to recycle
        let burn_amount = AlphaBalance::from(stake / 2);

        // burn from coldkey
        assert_ok!(SubtensorModule::burn_alpha(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            burn_amount,
            netuid
        ));

        assert!(TotalHotkeyAlpha::<Test>::get(hotkey, netuid) < initial_alpha);
        assert!(SubnetAlphaOut::<Test>::get(netuid) == initial_net_alpha);
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid)
                < stake.into()
        );
        // Make sure the other coldkey has no change
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &other_coldkey,
                netuid
            ),
            expected_alpha,
            epsilon = 2.into()
        );

        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::AlphaBurned(..))
            )
        }));
    });
}

#[test]
fn test_recycle_errors() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let wrong_hotkey = U256::from(3);

        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // Create root subnet
        migrations::migrate_create_root_network::migrate_create_root_network::<Test>();

        let initial_balance = 1_000_000_000;
        Balances::make_free_balance_be(&coldkey, initial_balance.into());

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        let stake_amount = 200_000;
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake_amount.into(), netuid);

        assert_noop!(
            SubtensorModule::recycle_alpha(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                100_000.into(),
                99.into() // non-existent subnet
            ),
            Error::<Test>::SubnetNotExists
        );

        assert_noop!(
            SubtensorModule::recycle_alpha(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                100_000.into(),
                NetUid::ROOT,
            ),
            Error::<Test>::CannotBurnOrRecycleOnRootSubnet
        );

        assert_noop!(
            SubtensorModule::recycle_alpha(
                RuntimeOrigin::signed(coldkey),
                wrong_hotkey,
                100_000.into(),
                netuid
            ),
            Error::<Test>::HotKeyAccountNotExists
        );

        // make it pass the stake check
        TotalHotkeyAlpha::<Test>::set(
            hotkey,
            netuid,
            SubnetAlphaOut::<Test>::get(netuid).saturating_mul(2.into()),
        );

        assert_noop!(
            SubtensorModule::recycle_alpha(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                SubnetAlphaOut::<Test>::get(netuid) + 1.into(),
                netuid
            ),
            Error::<Test>::InsufficientLiquidity
        );
    });
}

#[test]
fn test_burn_errors() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let wrong_hotkey = U256::from(3);

        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // Create root subnet
        migrations::migrate_create_root_network::migrate_create_root_network::<Test>();

        let initial_balance = 1_000_000_000;
        Balances::make_free_balance_be(&coldkey, initial_balance.into());

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        let stake_amount = 200_000;
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake_amount.into(), netuid);

        assert_noop!(
            SubtensorModule::burn_alpha(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                100_000.into(),
                99.into() // non-existent subnet
            ),
            Error::<Test>::SubnetNotExists
        );

        assert_noop!(
            SubtensorModule::burn_alpha(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                100_000.into(),
                NetUid::ROOT,
            ),
            Error::<Test>::CannotBurnOrRecycleOnRootSubnet
        );

        assert_noop!(
            SubtensorModule::burn_alpha(
                RuntimeOrigin::signed(coldkey),
                wrong_hotkey,
                100_000.into(),
                netuid
            ),
            Error::<Test>::HotKeyAccountNotExists
        );

        // make it pass the hotkey alpha check
        TotalHotkeyAlpha::<Test>::set(
            hotkey,
            netuid,
            SubnetAlphaOut::<Test>::get(netuid).saturating_mul(2.into()),
        );

        assert_noop!(
            SubtensorModule::burn_alpha(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                SubnetAlphaOut::<Test>::get(netuid) + 1.into(),
                netuid
            ),
            Error::<Test>::InsufficientLiquidity
        );
    });
}

#[test]
fn test_recycle_precision() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;
        let tao_reserve = TaoBalance::from(1_000_000_000_u64);
        let alpha_reserve = AlphaBalance::from(1_000_000_000_u64);
        SubnetAlphaIn::<Test>::insert(netuid, alpha_reserve);
        SubnetTAO::<Test>::insert(netuid, tao_reserve);

        Balances::make_free_balance_be(&coldkey, 1_000_000_000.into());
        // sanity check
        assert!(SubtensorModule::if_subnet_exist(netuid));

        // add stake to coldkey-hotkey pair so we can recycle it
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake.into(), netuid);

        // amount to recycle
        let recycle_amount = AlphaBalance::from(stake / 2);

        // Modify the alpha pool denominator so it's low-precision (denominator = share = 1e-9)
        let denominator = SafeFloat::from(1)
            .div(&SafeFloat::from(1_000_000_000))
            .unwrap_or_default();
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, denominator.clone());
        AlphaV2::<Test>::insert((&hotkey, &coldkey, netuid), denominator);
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // recycle, expect error due to precision loss
        assert_ok!(SubtensorModule::recycle_alpha(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            recycle_amount,
            netuid
        ));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            AlphaBalance::from(stake / 2)
        );
    });
}

#[test]
fn test_burn_precision() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;
        let tao_reserve = TaoBalance::from(1_000_000_000_u64);
        let alpha_reserve = AlphaBalance::from(1_000_000_000_u64);
        SubnetAlphaIn::<Test>::insert(netuid, alpha_reserve);
        SubnetTAO::<Test>::insert(netuid, tao_reserve);

        Balances::make_free_balance_be(&coldkey, 1_000_000_000.into());
        // sanity check
        assert!(SubtensorModule::if_subnet_exist(netuid));

        // add stake to coldkey-hotkey pair so we can recycle it
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake.into(), netuid);

        // amount to recycle
        let burn_amount = AlphaBalance::from(stake / 2);

        // Modify the alpha pool denominator so it's low-precision (denominator = share = 1e-9)
        let denominator = SafeFloat::from(1)
            .div(&SafeFloat::from(1_000_000_000))
            .unwrap_or_default();
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, denominator.clone());
        AlphaV2::<Test>::insert((&hotkey, &coldkey, netuid), denominator);
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // burn, expect error due to precision loss
        assert_ok!(SubtensorModule::burn_alpha(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            burn_amount,
            netuid
        ));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            AlphaBalance::from(stake / 2)
        );
    });
}

#[test]
fn test_add_stake_burn_success() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55453);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        remove_owner_registration_stake(netuid);

        mock::setup_reserves(
            netuid,
            (amount * 1_000_000).into(),
            (amount * 10_000_000).into(),
        );

        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        // Check we have zero staked before transfer
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id),
            TaoBalance::ZERO
        );

        // Execute add_stake_burn - this stakes TAO to get Alpha, then burns the Alpha
        assert_ok!(SubtensorModule::add_stake_burn(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.into(),
            None,
        ));

        // After "add stake and burn", hotkey should have zero stake since alpha is burned immediately
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id),
            TaoBalance::ZERO
        );

        // We spent TAO
        assert_abs_diff_eq!(
            SubtensorModule::get_coldkey_balance(&coldkey_account_id),
            0u64.into(),
            epsilon = 1u64.into()
        );

        // Verify AlphaBurned event was emitted
        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::AlphaBurned(..))
            )
        }));

        // Verify AddStakeBurn event was emitted
        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::AddStakeBurn { .. })
            )
        }));
    });
}

#[test]
fn test_add_stake_burn_with_limit_success() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55453);
        let amount: u64 = 100_000_000_000; // 100 TAO - moderate amount

        // Add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        remove_owner_registration_stake(netuid);

        // Setup reserves with large liquidity to minimize slippage
        let tao_reserve = TaoBalance::from(1_000_000_000_000_u64); // 1000 TAO
        let alpha_in = AlphaBalance::from(1_000_000_000_000_u64); // 1000 Alpha
        mock::setup_reserves(netuid, tao_reserve, alpha_in);

        // Verify current price is 1.0
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        assert_eq!(current_price, U96F32::from_num(1.0));

        // Give coldkey sufficient balance
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        let initial_balance = SubtensorModule::get_coldkey_balance(&coldkey_account_id);

        // Setup limit price at 2.0 TAO per Alpha
        // With 100 TAO into 1000/1000 pool, price moves from 1.0 to ~1.21
        let limit_price = TaoBalance::from(2_000_000_000); // 2.0 TAO per Alpha

        // Execute add_stake_burn with limit
        assert_ok!(SubtensorModule::add_stake_burn(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.into(),
            Some(limit_price),
        ));

        // After "add stake and burn", hotkey should have zero stake since alpha is burned immediately
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id),
            TaoBalance::ZERO
        );

        // TAO should have been spent
        let final_balance = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
        assert!(
            final_balance < initial_balance,
            "TAO should have been spent"
        );

        // Final price should be between initial (1.0) and limit (2.0)
        let final_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        assert!(
            final_price.to_num::<f64>() >= 1.0 && final_price.to_num::<f64>() <= 2.0,
            "Final price {} should be between 1.0 and 2.0",
            final_price.to_num::<f64>()
        );

        // Verify AlphaBurned event was emitted
        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::AlphaBurned(..))
            )
        }));
    });
}

#[test]
fn test_add_stake_burn_non_owner_succeeds() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let coldkey_account_id = U256::from(2);
        let non_owner_coldkey = U256::from(3);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        // Add network with coldkey_account_id as owner
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);

        mock::setup_reserves(
            netuid,
            (amount * 1_000_000).into(),
            (amount * 10_000_000).into(),
        );

        // Give non-owner some balance
        add_balance_to_coldkey_account(&non_owner_coldkey, amount.into());

        // Any signed origin can atomically stake and burn.
        assert_ok!(SubtensorModule::add_stake_burn(
            RuntimeOrigin::signed(non_owner_coldkey),
            hotkey_account_id,
            netuid,
            amount.into(),
            None,
        ));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &non_owner_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );

        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::AddStakeBurn { .. })
            )
        }));
    });
}

#[test]
fn test_add_stake_burn_nonexistent_subnet_fails() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let coldkey_account_id = U256::from(2);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        // Give some balance
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        // Try to call add_stake_burn on non-existent subnet
        let nonexistent_netuid = NetUid::from(999);
        assert_noop!(
            SubtensorModule::add_stake_burn(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                nonexistent_netuid,
                amount.into(),
                None,
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

#[test]
fn test_add_stake_burn_insufficient_balance_fails() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let coldkey_account_id = U256::from(2);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        // Add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);

        mock::setup_reserves(
            netuid,
            (amount * 1_000_000).into(),
            (amount * 10_000_000).into(),
        );

        // Try to call add_stake_burn without sufficient balance
        assert_noop!(
            SubtensorModule::add_stake_burn(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount.into(),
                None,
            ),
            Error::<Test>::NotEnoughBalanceToStake
        );
    });
}
