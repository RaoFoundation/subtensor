#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
use crate::{AlphaFeeHandler, SubtensorTxFeeHandler, TransactionFeeHandler, TransactionSource};
use approx::assert_abs_diff_eq;
use frame_support::dispatch::GetDispatchInfo;
use frame_support::pallet_prelude::Zero;
use frame_support::traits::Currency;
use frame_support::{assert_err, assert_ok};
use sp_runtime::{
    traits::{DispatchTransaction, TransactionExtension, TxBaseImplication},
    transaction_validity::{InvalidTransaction, TransactionValidityError},
};
use subtensor_runtime_common::AlphaBalance;
use subtensor_swap_interface::SwapHandler;

use mock::*;
mod mock;

// cargo test --package subtensor-transaction-fee --lib -- tests::test_remove_stake_fees_tao --exact --show-output
#[test]
fn test_remove_stake_fees_tao() {
    new_test_ext().execute_with(|| {
        use frame_support::traits::Hooks;
        use sp_runtime::traits::SaturatedConversion;

        type BN = frame_system::pallet_prelude::BlockNumberFor<Test>;

        // Advance blocks and run hooks so staking-op rate limit windows reset.
        let jump_blocks = |delta: u64| {
            let current_bn: BN = frame_system::Pallet::<Test>::block_number();

            // Finish current block.
            <SubtensorModule as Hooks<BN>>::on_finalize(current_bn);
            <frame_system::Pallet<Test> as Hooks<BN>>::on_finalize(current_bn);

            let current_u64: u64 = current_bn.saturated_into();
            // Use a delta that won’t land on tempo boundaries (tempo is set to 10 in setup_subnets).
            let next_u64: u64 = current_u64.saturating_add(delta);
            let next_bn: BN = next_u64.saturated_into();

            frame_system::Pallet::<Test>::set_block_number(next_bn);

            // Start next block.
            <frame_system::Pallet<Test> as Hooks<BN>>::on_initialize(next_bn);
            <SubtensorModule as Hooks<BN>>::on_initialize(next_bn);
        };

        let stake_amount = TaoBalance::from(TAO);
        let unstake_amount = AlphaBalance::from(TAO / 50);

        // setup_subnets() -> register_ok_neuron() calls SubtensorModule::register(...)
        // which now requires sufficient balance to stake during registration.
        // setup_subnets() uses coldkey=10000 and first neuron hotkey=20001.
        let register_prefund = stake_amount
            .saturating_mul(10_000.into()) // generous buffer
            .saturating_add(ExistentialDeposit::get());
        add_balance_to_coldkey_account(&U256::from(10000), register_prefund);
        add_balance_to_coldkey_account(&U256::from(20001), register_prefund);

        let sn = setup_subnets(1, 1);

        // Avoid staking-op rate limit between registration and staking.
        jump_blocks(1_000_001);

        setup_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount.into(),
        );
        add_balance_to_coldkey_account(&sn.coldkey, TaoBalance::from(TAO));

        // Avoid staking-op rate limit between add_stake and remove_stake.
        jump_blocks(1_000_001);

        // Simulate stake removal to get how much TAO should we get for unstaked Alpha
        let (expected_unstaked_tao, _swap_fee) =
            mock::swap_alpha_to_tao(sn.subnets[0].netuid, unstake_amount);

        // Remove stake
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey: sn.hotkeys[0],
            netuid: sn.subnets[0].netuid,
            amount_unstaked: unstake_amount,
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());

        // dispatch_transaction() is nested:
        // - Outer Result: validation / payment extension checks
        // - Inner Result: actual runtime call dispatch result
        let inner = ext
            .dispatch_transaction(RuntimeOrigin::signed(sn.coldkey).into(), call, &info, 0, 0)
            .expect("Expected Ok(_) from dispatch_transaction (validation)");
        assert_ok!(inner);

        let final_balance = Balances::free_balance(sn.coldkey);
        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        let actual_tao_fee =
            balance_before + TaoBalance::from(expected_unstaked_tao) - final_balance;
        let actual_alpha_fee = alpha_before - alpha_after - unstake_amount;

        // Remove stake extrinsic should pay fees in TAO because ck has sufficient TAO balance
        assert!(actual_tao_fee > 0.into());
        assert_eq!(actual_alpha_fee, AlphaBalance::from(0));

        let events = System::events();
        assert!(events.iter().any(|event_record| {
            matches!(
                &event_record.event,
                RuntimeEvent::TransactionPayment(
                    pallet_transaction_payment::Event::TransactionFeePaid { .. }
                )
            )
        }));
        assert!(!events.iter().any(|event_record| {
            matches!(
                &event_record.event,
                RuntimeEvent::SubtensorModule(SubtensorEvent::TransactionFeePaidWithAlpha { .. })
            )
        }));
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::test_rejects_multi_subnet_alpha_fee_deduction --exact --show-output
#[test]
fn test_rejects_multi_subnet_alpha_fee_deduction() {
    new_test_ext().execute_with(|| {
        let sn = setup_subnets(2, 1);
        let stake_amount = TAO;
        setup_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );
        setup_stake(
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
// cargo test --package subtensor-transaction-fee --lib -- tests::test_remove_stake_fees_alpha --exact --show-output
#[test]
fn test_remove_stake_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(1, 1);
        setup_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        // Simulate stake removal to get how much TAO should we get for unstaked Alpha
        // after the alpha-fee pre-withdrawal has already moved the pool.
        let (expected_unstaked_tao, swap_fee) = mock::quote_remove_stake_after_alpha_fee(
            &sn.coldkey,
            &sn.hotkeys[0],
            sn.subnets[0].netuid,
            unstake_amount,
        );

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(
            &sn.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        // Get the block builder balance
        let block_builder = U256::from(MOCK_BLOCK_BUILDER);
        let block_builder_balance_before = Balances::free_balance(block_builder);

        // Remove stake
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey: sn.hotkeys[0],
            netuid: sn.subnets[0].netuid,
            amount_unstaked: unstake_amount,
        });

        System::reset_events();

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
        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        let actual_tao_fee =
            balance_before + TaoBalance::from(expected_unstaked_tao) - final_balance;
        let actual_alpha_fee = alpha_before - alpha_after - unstake_amount;

        // Remove stake extrinsic should pay fees in Alpha
        assert_abs_diff_eq!(actual_tao_fee, 0.into(), epsilon = 10.into());
        assert!(actual_alpha_fee > 0.into());

        // Assert that swapped TAO from alpha fee goes to block author
        let block_builder_fee_portion = 1.;
        let expected_block_builder_swap_reward = swap_fee as f64 * block_builder_fee_portion;
        let expected_tx_fee = 14000.; // Use very low value (0.000014) for less test flakiness, value before we 10x tx fees
        let block_builder_balance_after = Balances::free_balance(block_builder);
        let actual_block_builder_reward =
            block_builder_balance_after - block_builder_balance_before;
        assert!(
            u64::from(actual_block_builder_reward) as f64
                >= expected_block_builder_swap_reward + expected_tx_fee
        );

        let events = System::events();
        let alpha_event = events
            .iter()
            .position(|event_record| {
                matches!(
                    &event_record.event,
                    RuntimeEvent::SubtensorModule(SubtensorEvent::TransactionFeePaidWithAlpha {
                        who,
                        netuid,
                        alpha_fee,
                        tao_amount: _,
                    }) if who == &sn.coldkey && *alpha_fee == actual_alpha_fee && *netuid == sn.subnets[0].netuid
                )
            })
            .expect("expected TransactionFeePaidWithAlpha event");
        let tao_event = events
            .iter()
            .position(|event_record| {
                matches!(
                    &event_record.event,
                    RuntimeEvent::TransactionPayment(
                        pallet_transaction_payment::Event::TransactionFeePaid { who, .. }
                    ) if who == &sn.coldkey
                )
            })
            .expect("expected TransactionFeePaid event");

        assert!(
            alpha_event < tao_event,
            "expected TransactionFeePaidWithAlpha before TransactionFeePaid"
        );
    });
}

#[test]
fn test_alpha_fee_withdraw_failure_aborts_and_rolls_back() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(1, 1);
        let netuid = sn.subnets[0].netuid;
        let hotkey = sn.hotkeys[0];

        setup_stake(netuid, &sn.coldkey, &hotkey, stake_amount);

        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(
            &sn.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        // Force the alpha-fee unstake to fail after AMM bookkeeping by draining
        // the subnet account used by transfer_tao_from_subnet.
        let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        Balances::make_free_balance_be(&subnet_account, 0.into());

        let block_builder = U256::from(MOCK_BLOCK_BUILDER);
        let block_builder_balance_before = Balances::free_balance(block_builder);
        let signer_balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &sn.coldkey,
            netuid,
        );
        let subnet_alpha_in_before = SubnetAlphaIn::<Test>::get(netuid);
        let subnet_alpha_out_before = SubnetAlphaOut::<Test>::get(netuid);
        let subnet_tao_before = SubnetTAO::<Test>::get(netuid);
        let total_stake_before = TotalStake::<Test>::get();
        let subnet_volume_before = SubnetVolume::<Test>::get(netuid);

        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey,
            netuid,
            amount_unstaked: unstake_amount,
        });
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());

        let result =
            ext.dispatch_transaction(RuntimeOrigin::signed(sn.coldkey).into(), call, &info, 0, 0);

        assert_eq!(
            result.unwrap_err(),
            TransactionValidityError::Invalid(InvalidTransaction::Payment)
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &sn.coldkey,
                netuid,
            ),
            alpha_before
        );
        assert_eq!(SubnetAlphaIn::<Test>::get(netuid), subnet_alpha_in_before);
        assert_eq!(SubnetAlphaOut::<Test>::get(netuid), subnet_alpha_out_before);
        assert_eq!(SubnetTAO::<Test>::get(netuid), subnet_tao_before);
        assert_eq!(TotalStake::<Test>::get(), total_stake_before);
        assert_eq!(SubnetVolume::<Test>::get(netuid), subnet_volume_before);
        assert_eq!(Balances::free_balance(sn.coldkey), signer_balance_before);
        assert_eq!(
            Balances::free_balance(block_builder),
            block_builder_balance_before
        );

        assert!(!System::events().iter().any(|event_record| {
            matches!(
                &event_record.event,
                RuntimeEvent::SubtensorModule(SubtensorEvent::TransactionFeePaidWithAlpha { .. })
            )
        }));
    });
}

// Test that unstaking on root with no free balance results in charging fees from
// staked amount
//
// cargo test --package subtensor-transaction-fee --lib -- tests::test_remove_stake_root --exact --show-output
#[test]
fn test_remove_stake_root() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = TAO / 10;
        let netuid = NetUid::from(0);
        let coldkey = U256::from(100000);
        let hotkey = U256::from(100001);

        // Root stake
        add_network(netuid, 10);
        pallet_subtensor::Owner::<Test>::insert(hotkey, coldkey);
        pallet_subtensor::SubtokenEnabled::<Test>::insert(NetUid::from(0), true);
        setup_stake(netuid, &coldkey, &hotkey, stake_amount);

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(coldkey);
        remove_balance_from_coldkey_account(&coldkey, current_balance - ExistentialDeposit::get());

        // Remove stake
        let balance_before = Balances::free_balance(coldkey);
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey,
            netuid,
            amount_unstaked: unstake_amount.into(),
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        assert_ok!(ext.dispatch_transaction(
            RuntimeOrigin::signed(coldkey).into(),
            call,
            &info,
            0,
            0,
        ));

        let final_balance = Balances::free_balance(coldkey);
        let alpha_after =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        let actual_tao_fee = balance_before + unstake_amount.into() - final_balance;
        let actual_alpha_fee =
            AlphaBalance::from(stake_amount) - alpha_after - unstake_amount.into();

        // Remove stake extrinsic should pay fees in Alpha (withdrawn from staked TAO)
        assert_eq!(actual_tao_fee, 0.into());
        assert!(actual_alpha_fee > 0.into());
    });
}

// Test that unstaking 100% of stake on root is possible with no free balance
//
// cargo test --package subtensor-transaction-fee --lib -- tests::test_remove_stake_completely_root --exact --show-output
#[test]
fn test_remove_stake_completely_root() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = TAO;
        let netuid = NetUid::from(0);
        let coldkey = U256::from(100000);
        let hotkey = U256::from(100001);

        // Root stake
        add_network(netuid, 10);
        pallet_subtensor::Owner::<Test>::insert(hotkey, coldkey);
        pallet_subtensor::SubtokenEnabled::<Test>::insert(NetUid::from(0), true);
        setup_stake(netuid, &coldkey, &hotkey, stake_amount);

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(coldkey);
        remove_balance_from_coldkey_account(&coldkey, current_balance - ExistentialDeposit::get());

        // Remove stake
        let balance_before = Balances::free_balance(coldkey);
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey,
            netuid,
            amount_unstaked: unstake_amount.into(),
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        assert_ok!(ext.dispatch_transaction(
            RuntimeOrigin::signed(coldkey).into(),
            call,
            &info,
            0,
            0,
        ));

        let final_balance = Balances::free_balance(coldkey);
        let alpha_after =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        assert_eq!(alpha_after, 0.into());
        assert!(final_balance > balance_before);
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::test_remove_stake_completely_fees_alpha --exact --show-output
#[test]
fn test_remove_stake_completely_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let sn = setup_subnets(1, 1);
        setup_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        // Simulate stake removal to get how much TAO should we get for unstaked Alpha
        let unstake_amount = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let (expected_unstaked_tao, _swap_fee) =
            mock::swap_alpha_to_tao(sn.subnets[0].netuid, unstake_amount);

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(
            &sn.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        // Remove stake
        let balance_before = Balances::free_balance(sn.coldkey);
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey: sn.hotkeys[0],
            netuid: sn.subnets[0].netuid,
            amount_unstaked: unstake_amount,
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
        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        // Effectively, the fee is paid in TAO in this case because user receives less TAO,
        // and all Alpha is gone, and it is not measurable in Alpha
        let actual_fee = balance_before + expected_unstaked_tao.into() - final_balance;
        assert_eq!(alpha_after, 0.into());
        assert!(actual_fee > 0.into());
    });
}

// Validation should fail if both TAO and Alpha balance are lower than tx fees,
// so that transaction is not included in the block
#[test]
fn test_remove_stake_not_enough_balance_for_fees() {
    new_test_ext().execute_with(|| {
        let stake_amount = TaoBalance::from(TAO);
        let sn = setup_subnets(1, 1);

        add_balance_to_coldkey_account(
            &sn.coldkey,
            stake_amount
                .saturating_mul(2.into()) // buffer so staking doesn't attempt to drain the account
                .saturating_add(ExistentialDeposit::get()),
        );
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(sn.coldkey),
            sn.hotkeys[0],
            sn.subnets[0].netuid,
            stake_amount.into(),
        ));

        // Simulate stake removal to get how much TAO should we get for unstaked Alpha
        let current_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(
            &sn.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        // For-set Alpha balance to low
        let new_current_stake = AlphaBalance::from(1_000);
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
            current_stake - new_current_stake,
        );

        // Remove stake
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey: sn.hotkeys[0],
            netuid: sn.subnets[0].netuid,
            amount_unstaked: new_current_stake,
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        let result = ext.validate(
            RuntimeOrigin::signed(sn.coldkey).into(),
            &call.clone(),
            &info,
            10,
            (),
            &TxBaseImplication(()),
            TransactionSource::External,
        );

        assert_eq!(
            result.unwrap_err(),
            TransactionValidityError::Invalid(InvalidTransaction::Payment)
        );
    });
}

// No TAO balance, Alpha fees. If Alpha price is high, it is enough to pay fees, but when Alpha price
// is low, the validation fails
//
// cargo test --package subtensor-transaction-fee --lib -- tests::test_remove_stake_edge_alpha --exact --show-output
#[test]
fn test_remove_stake_edge_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let sn = setup_subnets(1, 1);
        setup_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        // Simulate stake removal to get how much TAO should we get for unstaked Alpha
        let current_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(
            &sn.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        // For-set Alpha balance to low, but enough to pay tx fees at the current Alpha price
        let new_current_stake = AlphaBalance::from(2_000_000);
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
            current_stake - new_current_stake,
        );

        // Remove stake
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey: sn.hotkeys[0],
            netuid: sn.subnets[0].netuid,
            amount_unstaked: new_current_stake,
        });

        // Dispatch the extrinsic with ChargeTransactionPayment extension
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        let result = ext.validate(
            RuntimeOrigin::signed(sn.coldkey).into(),
            &call.clone(),
            &info,
            10,
            (),
            &TxBaseImplication(()),
            TransactionSource::External,
        );

        // Ok - Validation passed
        assert_ok!(result);

        // Lower Alpha price to 0.0001 so that there is not enough alpha to cover tx fees
        SubnetTAO::<Test>::insert(sn.subnets[0].netuid, TaoBalance::from(1_000_000));
        SubnetAlphaIn::<Test>::insert(sn.subnets[0].netuid, AlphaBalance::from(10_000_000_000_u64));

        let result_low_alpha_price = ext.validate(
            RuntimeOrigin::signed(sn.coldkey).into(),
            &call.clone(),
            &info,
            10,
            (),
            &TxBaseImplication(()),
            TransactionSource::External,
        );
        assert_eq!(
            result_low_alpha_price.unwrap_err(),
            TransactionValidityError::Invalid(InvalidTransaction::Payment)
        );
    });
}

// Validation passes, but transaction fails => TAO fees are paid
//
// cargo test --package subtensor-transaction-fee --lib -- tests::test_remove_stake_failing_transaction_tao_fees --exact --show-output
#[test]
fn test_remove_stake_failing_transaction_tao_fees() {
    new_test_ext().execute_with(|| {
        let stake_amount = TaoBalance::from(TAO);
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(1, 1);

        add_balance_to_coldkey_account(
            &sn.coldkey,
            stake_amount
                .saturating_mul(2.into()) // buffer so staking doesn't attempt to drain the account
                .saturating_add(ExistentialDeposit::get()),
        );
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(sn.coldkey),
            sn.hotkeys[0],
            sn.subnets[0].netuid,
            stake_amount.into(),
        ));

        add_balance_to_coldkey_account(&sn.coldkey, TAO.into());

        // Make unstaking fail by reducing liquidity to critical
        SubnetTAO::<Test>::insert(sn.subnets[0].netuid, TaoBalance::from(1));

        // Remove stake
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey: sn.hotkeys[0],
            netuid: sn.subnets[0].netuid,
            amount_unstaked: unstake_amount,
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
        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        let actual_tao_fee = balance_before - final_balance;

        // Remove stake extrinsic should pay fees in TAO because ck has sufficient TAO balance
        assert!(actual_tao_fee > 0.into());
        assert_eq!(alpha_before, alpha_after);
    });
}

// Validation passes, but transaction fails (artificially disable subtoken) =>
// Alpha fees are still paid
//
// cargo test --package subtensor-transaction-fee --lib -- tests::test_remove_stake_failing_transaction_alpha_fees --exact --show-output
#[test]
fn test_remove_stake_failing_transaction_alpha_fees() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(1, 1);
        setup_stake(
            sn.subnets[0].netuid,
            &sn.coldkey,
            &sn.hotkeys[0],
            stake_amount,
        );

        // Provide adequate TAO reserve so that sim swap works ok in validation
        SubnetTAO::<Test>::insert(sn.subnets[0].netuid, TaoBalance::from(1_000_000_000_u64));

        // Provide Alpha reserve so that price is about 1.0
        SubnetAlphaIn::<Test>::insert(sn.subnets[0].netuid, AlphaBalance::from(1_000_000_000_u64));

        // Forse-set signer balance to ED
        let current_balance = Balances::free_balance(sn.coldkey);
        remove_balance_from_coldkey_account(
            &sn.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        // Disable subtoken so that removing stake tx fails (still allows the validation to pass)
        pallet_subtensor::SubtokenEnabled::<Test>::insert(sn.subnets[0].netuid, false);

        // Remove stake
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey: sn.hotkeys[0],
            netuid: sn.subnets[0].netuid,
            amount_unstaked: alpha_before,
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
        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        let actual_tao_fee = balance_before - final_balance;
        let actual_alpha_fee = alpha_before - alpha_after;

        // Remove stake extrinsic should pay fees in Alpha
        assert_eq!(actual_tao_fee, 0.into());
        assert!(actual_alpha_fee > 0.into());
        assert!(actual_alpha_fee < unstake_amount);
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::test_remove_stake_limit_fees_alpha --exact --show-output
#[test]
fn test_remove_stake_limit_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(1, 1);
        setup_stake(
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

        // Remove stake limit
        let balance_before = Balances::free_balance(sn.coldkey);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake_limit {
            hotkey: sn.hotkeys[0],
            netuid: sn.subnets[0].netuid,
            amount_unstaked: unstake_amount,
            limit_price: 1_000.into(),
            allow_partial: false,
        });

        System::reset_events();

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
        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &sn.hotkeys[0],
            &sn.coldkey,
            sn.subnets[0].netuid,
        );

        let expected_unstaked_tao = System::events()
            .iter()
            .rev()
            .find_map(|event_record| match &event_record.event {
                RuntimeEvent::SubtensorModule(SubtensorEvent::StakeRemoved(
                    coldkey,
                    hotkey,
                    tao_amount,
                    alpha_amount,
                    netuid,
                    fee_paid,
                )) if coldkey == &sn.coldkey
                    && hotkey == &sn.hotkeys[0]
                    && *netuid == sn.subnets[0].netuid
                    && (*alpha_amount + AlphaBalance::from(*fee_paid) == unstake_amount) =>
                {
                    Some(*tao_amount)
                }
                _ => None,
            })
            .expect("expected StakeRemoved event for remove_stake_limit");

        let actual_tao_fee = balance_before + expected_unstaked_tao - final_balance;
        let actual_alpha_fee = alpha_before - alpha_after - unstake_amount;

        // Remove stake extrinsic should pay fees in Alpha
        assert_abs_diff_eq!(actual_tao_fee, 0.into(), epsilon = 100.into());
        assert!(actual_alpha_fee > 0.into());
    });
}

// cargo test --package subtensor-transaction-fee --lib -- tests::test_unstake_all_fees_alpha --exact --show-output
#[test]
fn test_unstake_all_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let sn = setup_subnets(10, 1);
        let coldkey = U256::from(100000);
        for i in 0..10 {
            setup_stake(sn.subnets[i].netuid, &coldkey, &sn.hotkeys[0], stake_amount);
        }

        // Root stake
        add_network(NetUid::from(0), 10);
        pallet_subtensor::SubtokenEnabled::<Test>::insert(NetUid::from(0), true);
        setup_stake(0.into(), &coldkey, &sn.hotkeys[0], stake_amount);

        // Simulate stake removal to get how much TAO should we get for unstaked Alpha
        let mut expected_unstaked_tao = 0;
        for i in 0..10 {
            let unstake_amount = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &sn.hotkeys[0],
                &coldkey,
                sn.subnets[i].netuid,
            );

            let (tao, _swap_fee) = mock::swap_alpha_to_tao(sn.subnets[i].netuid, unstake_amount);
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

// cargo test --package subtensor-transaction-fee --lib -- tests::test_unstake_all_alpha_fees_alpha --exact --show-output
#[test]
fn test_unstake_all_alpha_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let sn = setup_subnets(10, 1);
        let coldkey = U256::from(100000);
        for i in 0..10 {
            setup_stake(sn.subnets[i].netuid, &coldkey, &sn.hotkeys[0], stake_amount);
        }

        // Simulate stake removal to get how much TAO should we get for unstaked Alpha
        let mut expected_unstaked_tao = 0;
        for i in 0..10 {
            let unstake_amount = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &sn.hotkeys[0],
                &coldkey,
                sn.subnets[i].netuid,
            );

            let (tao, _swap_fee) = mock::swap_alpha_to_tao(sn.subnets[i].netuid, unstake_amount);
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

// cargo test --package subtensor-transaction-fee --lib -- tests::test_move_stake_fees_alpha --exact --show-output
#[test]
fn test_move_stake_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(2, 2);
        setup_stake(
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

// cargo test --package subtensor-transaction-fee --lib -- tests::test_transfer_stake_fees_alpha --exact --show-output
#[test]
fn test_transfer_stake_fees_alpha() {
    new_test_ext().execute_with(|| {
        let destination_coldkey = U256::from(100000);
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(2, 2);
        setup_stake(
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

// cargo test --package subtensor-transaction-fee --lib -- tests::test_transfer_stake_full_amount_fails_when_alpha_fee_reduces_available_stake --exact --show-output
#[test]
fn test_transfer_stake_full_amount_fails_when_alpha_fee_reduces_available_stake() {
    new_test_ext().execute_with(|| {
        let destination_coldkey = U256::from(100000);
        let stake_amount = TAO;
        let sn = setup_subnets(2, 2);
        setup_stake(
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
        let sn = setup_subnets(2, 2);
        setup_stake(
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

// cargo test --package subtensor-transaction-fee --lib -- tests::test_swap_stake_fees_alpha --exact --show-output
#[test]
fn test_swap_stake_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(2, 2);
        setup_stake(
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

// cargo test --package subtensor-transaction-fee --lib -- tests::test_swap_stake_limit_fees_alpha --exact --show-output
#[test]
fn test_swap_stake_limit_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(2, 2);
        setup_stake(
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

// cargo test --package subtensor-transaction-fee --lib -- tests::test_burn_alpha_fees_alpha --exact --show-output
#[test]
fn test_burn_alpha_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let alpha_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(1, 1);
        setup_stake(
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

// cargo test --package subtensor-transaction-fee --lib -- tests::test_recycle_alpha_fees_alpha --exact --show-output
#[test]
fn test_recycle_alpha_fees_alpha() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let alpha_amount = AlphaBalance::from(TAO / 50);
        let sn = setup_subnets(1, 1);
        setup_stake(
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

// cargo test --package subtensor-transaction-fee --lib -- tests::test_add_stake_fees_go_to_block_builder --exact --show-output
#[test]
fn test_add_stake_fees_go_to_block_builder() {
    new_test_ext().execute_with(|| {
        // Portion of swap fees that should go to the block builder
        let block_builder_fee_portion = 1.;

        // Get the block builder balance
        let block_builder = U256::from(MOCK_BLOCK_BUILDER);
        let block_builder_balance_before = Balances::free_balance(block_builder);

        let stake_amount = TAO;
        let sn = setup_subnets(1, 1);

        // Simulate add stake to get the expected TAO fee
        let (_, swap_fee) = mock::swap_tao_to_alpha(sn.subnets[0].netuid, stake_amount.into());

        add_balance_to_coldkey_account(&sn.coldkey, (stake_amount * 10).into());

        // Stake
        let balance_before = Balances::free_balance(sn.coldkey);
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake {
            hotkey: sn.hotkeys[0],
            netuid: sn.subnets[0].netuid,
            amount_staked: stake_amount.into(),
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
        let actual_tao_fee = balance_before - stake_amount.into() - final_balance;
        assert!(!actual_tao_fee.is_zero());

        // Expect that block builder balance has increased by both the swap fee and the transaction fee
        let expected_block_builder_swap_reward = swap_fee as f64 * block_builder_fee_portion;
        let expected_tx_fee = 14000.; // Use very low value (0.000014) for less test flakiness, value before we 10x tx fees
        let block_builder_balance_after = Balances::free_balance(block_builder);
        let actual_reward = block_builder_balance_after - block_builder_balance_before;
        assert!(
            u64::from(actual_reward) as f64 >= expected_block_builder_swap_reward + expected_tx_fee
        );
    });
}
