#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
use super::mock::*;
use frame_support::assert_ok;
use frame_support::dispatch::GetDispatchInfo;
use sp_runtime::traits::DispatchTransaction;
use subtensor_runtime_common::AlphaBalance;

// cargo test --package subtensor-transaction-fee --lib -- tests::burn_recycle_alpha_fees::test_burn_alpha_fees_alpha --exact --show-output
#[test]
fn test_burn_alpha_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let alpha_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_fee_test_subnets(1, 1);
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

        // Burn alpha
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::burn_alpha {
            hotkey: sn.hotkeys[0],
            amount: alpha_amount,
            netuid: sn.subnets[0].netuid,
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

        let actual_tao_fee = balance_before - final_balance;
        let actual_alpha_fee = alpha_before - alpha_after_0 - alpha_amount;

        // Extrinsic should pay fees in Alpha
        assert_eq!(actual_tao_fee, 0.into());
        assert!(actual_alpha_fee > 0.into());
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::burn_recycle_alpha_fees::test_recycle_alpha_fees_alpha --exact --show-output
#[test]
fn test_recycle_alpha_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let alpha_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_fee_test_subnets(1, 1);
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

        // Recycle alpha
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::recycle_alpha {
            hotkey: sn.hotkeys[0],
            amount: alpha_amount,
            netuid: sn.subnets[0].netuid,
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

        let actual_tao_fee = balance_before - final_balance;
        let actual_alpha_fee = alpha_before - alpha_after_0 - alpha_amount;

        // Extrinsic should pay fees in Alpha
        assert_eq!(actual_tao_fee, 0.into());
        assert!(actual_alpha_fee > 0.into());
    });
}
