#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
use super::mock::*;
use frame_support::assert_ok;
use frame_support::dispatch::GetDispatchInfo;
use sp_runtime::traits::DispatchTransaction;
use subtensor_runtime_common::AlphaBalance;
use subtensor_swap_interface::SwapHandler;

// cargo test --package subtensor-transaction-fee --lib -- tests::move_transfer_swap_stake_fees::test_move_stake_fees_alpha --exact --show-output
#[test]
fn test_move_stake_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_fee_test_subnets(2, 2);
        fund_and_add_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(
            &sn.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        // Move stake
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::move_stake {
            origin_hotkey: sn.hotkeys[0],
            destination_hotkey: sn.hotkeys[1],
            origin_netuid: sn.subnets[0].netuid,
            destination_netuid: sn.subnets[1].netuid,
            alpha_amount: unstake_amount,
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        assert_ok!(ext.dispatch_transaction(
            RuntimeOrigin::signed(sn.coldkey).into(),
            call,
            &info,
            0,
            0,
        ));

        let final_balance = Balances::free_balance(sn.coldkey);
        let alpha_after_0 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        // Ensure stake was moved
        let alpha_after_1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[1],
            &sn.coldkey,
            sn.subnets[1].netuid,
        );
        assert!(alpha_after_1 > 0.into());

        let actual_tao_fee = balance_before - final_balance;
        let actual_alpha_fee = alpha_before - alpha_after_0 - unstake_amount;

        // Extrinsic should pay fees in Alpha
        assert_eq!(actual_tao_fee, 0.into());
        assert!(actual_alpha_fee > 0.into());
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::move_transfer_swap_stake_fees::test_transfer_stake_fees_alpha --exact --show-output
#[test]
fn test_transfer_stake_fees_alpha() {
    new_test_ext().execute_with(|| {
        let destination_coldkey = U256::from(100000);
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_fee_test_subnets(2, 2);
        fund_and_add_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(
            &sn.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        // Transfer stake
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake {
            destination_coldkey,
            hotkey: sn.hotkeys[0],
            origin_netuid: sn.subnets[0].netuid,
            destination_netuid: sn.subnets[1].netuid,
            alpha_amount: unstake_amount,
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        assert_ok!(ext.dispatch_transaction(
            RuntimeOrigin::signed(sn.coldkey).into(),
            call,
            &info,
            0,
            0,
        ));

        let final_balance = Balances::free_balance(sn.coldkey);
        let alpha_after_0 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        // Ensure stake was transferred
        let alpha_after_1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &destination_coldkey,
            sn.subnets[1].netuid,
        );
        assert!(alpha_after_1 > 0.into());

        let actual_tao_fee = balance_before - final_balance;
        let actual_alpha_fee = alpha_before - alpha_after_0 - unstake_amount;

        // Extrinsic should pay fees in Alpha
        assert_eq!(actual_tao_fee, 0.into());
        assert!(actual_alpha_fee > 0.into());
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::move_transfer_swap_stake_fees::test_transfer_stake_full_amount_fails_when_alpha_fee_reduces_available_stake --exact --show-output
#[test]
fn test_transfer_stake_full_amount_fails_when_alpha_fee_reduces_available_stake() {
    new_test_ext().execute_with(|| {
        let destination_coldkey = U256::from(100000);
        let stake_amount = TAO;
        let sn = setup_fee_test_subnets(2, 2);
        fund_and_add_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(&sn.coldkey, current_balance);
        assert_eq!(Balances::free_balance(sn.coldkey), TaoBalance::ZERO);

        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake {
            destination_coldkey,
            hotkey: sn.hotkeys[0],
            origin_netuid: sn.subnets[0].netuid,
            destination_netuid: sn.subnets[1].netuid,
            alpha_amount: alpha_before,
        });
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());

        let inner = ext
            .dispatch_transaction(RuntimeOrigin::signed(sn.coldkey).into(), call, &info, 0, 0)
            .expect("alpha fee payment should validate");
        assert_eq!(
            inner.unwrap_err().error,
            Error::<Test>::NotEnoughStakeToWithdraw.into()
        );

        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let actual_alpha_fee = alpha_before - alpha_after;
        assert!(actual_alpha_fee > AlphaBalance::ZERO);
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &sn.hotkeys[0],
                &destination_coldkey,
                sn.subnets[1].netuid,
            ),
            AlphaBalance::ZERO
        );
    });

    new_test_ext().execute_with(|| {
        let destination_coldkey = U256::from(100000);
        let stake_amount = TAO;
        let sn = setup_fee_test_subnets(2, 2);
        fund_and_add_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(&sn.coldkey, current_balance);
        assert_eq!(Balances::free_balance(sn.coldkey), TaoBalance::ZERO);

        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let full_amount_call =
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake {
                destination_coldkey,
                hotkey: sn.hotkeys[0],
                origin_netuid: sn.subnets[0].netuid,
                destination_netuid: sn.subnets[1].netuid,
                alpha_amount: alpha_before,
            });
        let info = full_amount_call.get_dispatch_info();
        let tao_fee = pallet_transaction_payment::Pallet::<Test>::compute_fee(0, &info, 0.into());
        let alpha_fee = pallet_subtensor_swap::Pallet::<Test>::get_alpha_amount_for_tao(
            sn.subnets[0].netuid,
            tao_fee,
        );
        assert!(alpha_fee > AlphaBalance::ZERO);

        let transfer_amount = alpha_before - alpha_fee;
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake {
            destination_coldkey,
            hotkey: sn.hotkeys[0],
            origin_netuid: sn.subnets[0].netuid,
            destination_netuid: sn.subnets[1].netuid,
            alpha_amount: transfer_amount,
        });
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());

        let inner = ext
            .dispatch_transaction(RuntimeOrigin::signed(sn.coldkey).into(), call, &info, 0, 0)
            .expect("alpha fee payment should validate");
        assert_ok!(inner);

        assert_eq!(Balances::free_balance(sn.coldkey), TaoBalance::ZERO);
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &sn.hotkeys[0],
                &sn.coldkey,
                sn.subnets[0].netuid,
            ),
            AlphaBalance::ZERO
        );
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &sn.hotkeys[0],
                &destination_coldkey,
                sn.subnets[1].netuid,
            ) > AlphaBalance::ZERO
        );
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::move_transfer_swap_stake_fees::test_swap_stake_fees_alpha --exact --show-output
#[test]
fn test_swap_stake_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_fee_test_subnets(2, 2);
        fund_and_add_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(
            &sn.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        // Swap stake
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_stake {
            hotkey: sn.hotkeys[0],
            origin_netuid: sn.subnets[0].netuid,
            destination_netuid: sn.subnets[1].netuid,
            alpha_amount: unstake_amount,
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        assert_ok!(ext.dispatch_transaction(
            RuntimeOrigin::signed(sn.coldkey).into(),
            call,
            &info,
            0,
            0,
        ));

        let final_balance = Balances::free_balance(sn.coldkey);
        let alpha_after_0 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        // Ensure stake was transferred
        let alpha_after_1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[1].netuid,
        );
        assert!(alpha_after_1 > 0.into());

        let actual_tao_fee = balance_before - final_balance;
        let actual_alpha_fee = alpha_before - alpha_after_0 - unstake_amount;

        // Extrinsic should pay fees in Alpha
        assert_eq!(actual_tao_fee, 0.into());
        assert!(actual_alpha_fee > 0.into());
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::move_transfer_swap_stake_fees::test_swap_stake_limit_fees_alpha --exact --show-output
#[test]
fn test_swap_stake_limit_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_fee_test_subnets(2, 2);
        fund_and_add_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(
            &sn.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        // Swap stake limit
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_stake_limit {
            hotkey: sn.hotkeys[0],
            origin_netuid: sn.subnets[0].netuid,
            destination_netuid: sn.subnets[1].netuid,
            alpha_amount: unstake_amount,
            limit_price: 1_000.into(),
            allow_partial: false,
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        assert_ok!(ext.dispatch_transaction(
            RuntimeOrigin::signed(sn.coldkey).into(),
            call,
            &info,
            0,
            0,
        ));

        let final_balance = Balances::free_balance(sn.coldkey);
        let alpha_after_0 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        // Ensure stake was transferred
        let alpha_after_1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[1].netuid,
        );
        assert!(alpha_after_1 > 0.into());

        let actual_tao_fee = balance_before - final_balance;
        let actual_alpha_fee = alpha_before - alpha_after_0 - unstake_amount;

        // Extrinsic should pay fees in Alpha
        assert_eq!(actual_tao_fee, 0.into());
        assert!(actual_alpha_fee > 0.into());
    });
}
