#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
use super::mock::*;
use crate::{AlphaFeeHandler, SubtensorTxFeeHandler, TransactionFeeHandler};
use frame_support::dispatch::GetDispatchInfo;
use frame_support::{assert_err, assert_ok};
use sp_runtime::{
    traits::DispatchTransaction,
    transaction_validity::{InvalidTransaction, TransactionValidityError},
};

// cargo test --package subtensor-transaction-fee --lib -- tests::unstake_all_fees::test_rejects_multi_subnet_alpha_fee_deduction --exact --show-output
#[test]
fn test_rejects_multi_subnet_alpha_fee_deduction() {
    new_test_ext().execute_with(|| {
        let sn = setup_fee_test_subnets(2, 1);
        let stake_amount = TAO;
        fund_and_add_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );
        fund_and_add_stake(
            sn.subnets[1].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        let alpha_before_0 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let alpha_before_1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[1].netuid,
        );

        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::unstake_all {
            hotkey: sn.hotkeys[0],
        });
        let alpha_vec =
            SubtensorTxFeeHandler::<Balances, TransactionFeeHandler<Test>>::fees_in_alpha::<Test>(
                &sn.coldkey,
                &call,
            );
        assert_eq!(alpha_vec.len(), 2);

        assert!(
            !<TransactionFeeHandler<Test> as AlphaFeeHandler<Test>>::can_withdraw_in_alpha(
                &sn.coldkey,
                &alpha_vec,
                1.into(),
            )
        );
        assert_eq!(
            <TransactionFeeHandler<Test> as AlphaFeeHandler<Test>>::withdraw_in_alpha(
                &sn.coldkey,
                &alpha_vec,
                1.into(),
            ),
            Ok((0.into(), 0.into(), NetUid::ROOT))
        );

        let alpha_after_0 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let alpha_after_1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[1].netuid,
        );

        assert_eq!(alpha_before_0, alpha_after_0);
        assert_eq!(alpha_before_1, alpha_after_1);
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::unstake_all_fees::test_unstake_all_fees_alpha --exact --show-output
#[test]
fn test_unstake_all_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let sn = setup_fee_test_subnets(10, 1);
        let coldkey = U256::from(100000);
        for i in 0..10 {
            fund_and_add_stake(sn.subnets[i].netuid, &coldkey, &sn.hotkeys[0], stake_amount);
        }

        // Root stake
        add_network(NetUid::from(0), 10);
        pallet_subtensor::SubtokenEnabled::<Test>::insert(NetUid::from(0), true);
        fund_and_add_stake(0.into(), &coldkey, &sn.hotkeys[0], stake_amount);

        // Simulate stake removal to get how much TAO should we get for unstaked Alpha
        let mut expected_unstaked_tao = 0;
        for i in 0..10 {
            let unstake_amount = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &sn.hotkeys[0],
                &coldkey,
                sn.subnets[i].netuid,
            );

            let (tao, _swap_fee) = swap_alpha_to_tao(sn.subnets[i].netuid, unstake_amount);
            expected_unstaked_tao += tao;
        }

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(coldkey);
        remove_balance_from_coldkey_account(&coldkey, current_balance - ExistentialDeposit::get());

        // Unstake all
        let balance_before = Balances::free_balance(sn.coldkey);
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::unstake_all {
            hotkey: sn.hotkeys[0],
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        // Get invalid payment because we cannot pay fees in multiple alphas
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        assert_err!(
            ext.clone().dispatch_transaction(
                RuntimeOrigin::signed(coldkey).into(),
                call.clone(),
                &info,
                0,
                0,
            ),
            TransactionValidityError::Invalid(InvalidTransaction::Payment),
        );

        // Give the coldkey TAO balance - now should unstake ok
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_u64.into());
        assert_ok!(ext.dispatch_transaction(
            RuntimeOrigin::signed(coldkey).into(),
            call,
            &info,
            0,
            0,
        ));

        let final_balance = Balances::free_balance(sn.coldkey);

        // Effectively, the fee is paid in TAO in this case because user receives less TAO,
        // and all Alpha is gone, and it is not measurable in Alpha
        let actual_fee = balance_before + expected_unstaked_tao.into() - final_balance;
        assert!(actual_fee > 0.into());

        // Check that all subnets got unstaked
        for i in 0..10 {
            let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &sn.hotkeys[0],
                &sn.coldkey,
                sn.subnets[i].netuid,
            );
            assert_eq!(alpha_after, 0.into());
        }
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::unstake_all_fees::test_unstake_all_alpha_fees_alpha --exact --show-output
#[test]
fn test_unstake_all_alpha_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let sn = setup_fee_test_subnets(10, 1);
        let coldkey = U256::from(100000);
        for i in 0..10 {
            fund_and_add_stake(sn.subnets[i].netuid, &coldkey, &sn.hotkeys[0], stake_amount);
        }

        // Simulate stake removal to get how much TAO should we get for unstaked Alpha
        let mut expected_unstaked_tao = 0;
        for i in 0..10 {
            let unstake_amount = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &sn.hotkeys[0],
                &coldkey,
                sn.subnets[i].netuid,
            );

            let (tao, _swap_fee) = swap_alpha_to_tao(sn.subnets[i].netuid, unstake_amount);
            expected_unstaked_tao += tao;
        }

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(coldkey);
        remove_balance_from_coldkey_account(&coldkey, current_balance - ExistentialDeposit::get());

        // Unstake all
        let balance_before = Balances::free_balance(sn.coldkey);
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::unstake_all_alpha {
            hotkey: sn.hotkeys[0],
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        // Get invalid payment because we cannot pay fees in multiple alphas
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        assert_err!(
            ext.clone().dispatch_transaction(
                RuntimeOrigin::signed(coldkey).into(),
                call.clone(),
                &info,
                0,
                0,
            ),
            TransactionValidityError::Invalid(InvalidTransaction::Payment),
        );

        // Give the coldkey TAO balance - now should unstake ok
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_u64.into());
        assert_ok!(ext.dispatch_transaction(
            RuntimeOrigin::signed(coldkey).into(),
            call,
            &info,
            0,
            0,
        ));

        let final_balance = Balances::free_balance(sn.coldkey);

        // Effectively, the fee is paid in TAO in this case because user receives less TAO,
        // and all Alpha is gone, and it is not measurable in Alpha
        let actual_fee = balance_before + expected_unstaked_tao.into() - final_balance;
        assert!(actual_fee > 0.into());

        // Check that all subnets got unstaked
        for i in 0..10 {
            let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &sn.hotkeys[0],
                &sn.coldkey,
                sn.subnets[i].netuid,
            );
            assert_eq!(alpha_after, 0.into());
        }
    });
}
