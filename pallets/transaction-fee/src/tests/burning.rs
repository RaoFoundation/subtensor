use super::mock::*;

use frame_support::{assert_ok, dispatch::GetDispatchInfo, pallet_prelude::Zero};
use sp_runtime::traits::{AccountIdConversion, DispatchTransaction};
use subtensor_runtime_common::AlphaBalance;

#[test]
fn tao_transaction_fees_are_burned() {
    new_test_ext().execute_with(|| {
        let payer = U256::from(42u64);
        let block_builder = U256::from(MOCK_BLOCK_BUILDER);
        add_balance_to_coldkey_account(&payer, TaoBalance::from(TAO));

        let payer_balance_before = Balances::free_balance(payer);
        let block_builder_balance_before = Balances::free_balance(block_builder);
        let balances_issuance_before = Balances::total_issuance();
        let subtensor_issuance_before = SubtensorModule::get_total_issuance();
        assert_eq!(balances_issuance_before, subtensor_issuance_before);

        let call = RuntimeCall::System(frame_system::Call::remark {
            remark: vec![0; 32],
        });
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(
            TaoBalance::from(1_000u64),
        );
        assert_ok!(ext.dispatch_transaction(
            RuntimeOrigin::signed(payer).into(),
            call,
            &info,
            0,
            0,
        ));

        let charged = payer_balance_before.saturating_sub(Balances::free_balance(payer));
        assert!(!charged.is_zero());
        assert_eq!(
            Balances::free_balance(block_builder),
            block_builder_balance_before
        );
        assert_eq!(
            balances_issuance_before.saturating_sub(Balances::total_issuance()),
            charged
        );
        assert_eq!(
            subtensor_issuance_before.saturating_sub(SubtensorModule::get_total_issuance()),
            charged
        );
    });
}

#[test]
fn alpha_transaction_fees_are_burned_without_a_block_author() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let unstake_amount = AlphaBalance::from(TAO / 50);
        let setup = setup_subnets(1, 1);
        let netuid = setup.subnets[0].netuid;
        let hotkey = setup.hotkeys[0];
        setup_stake(netuid, &setup.coldkey, &hotkey, stake_amount);

        let current_balance = Balances::free_balance(setup.coldkey);
        remove_balance_from_coldkey_account(
            &setup.coldkey,
            current_balance - ExistentialDeposit::get(),
        );

        let block_builder = U256::from(MOCK_BLOCK_BUILDER);
        let block_builder_balance_before = Balances::free_balance(block_builder);
        let burn_account: U256 = BurnAccountId::get().into_account_truncating();
        let burn_balance_before = Balances::free_balance(burn_account);
        let balances_issuance_before = Balances::total_issuance();
        let subtensor_issuance_before = SubtensorModule::get_total_issuance();
        assert_eq!(balances_issuance_before, subtensor_issuance_before);
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &setup.coldkey,
            netuid,
        );

        set_mock_block_author(None);
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey,
            netuid,
            amount_unstaked: unstake_amount,
        });
        let info = call.get_dispatch_info();
        let ext = pallet_transaction_payment::ChargeTransactionPayment::<Test>::from(0.into());
        assert_ok!(ext.dispatch_transaction(
            RuntimeOrigin::signed(setup.coldkey).into(),
            call,
            &info,
            0,
            0,
        ));

        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &setup.coldkey,
            netuid,
        );
        let alpha_fee = alpha_before - alpha_after - unstake_amount;
        assert!(!alpha_fee.is_zero());

        let burned_tao = System::events()
            .iter()
            .find_map(|event_record| match &event_record.event {
                RuntimeEvent::SubtensorModule(SubtensorEvent::TransactionFeePaidWithAlpha {
                    who,
                    netuid: event_netuid,
                    alpha_fee: event_alpha_fee,
                    tao_amount,
                }) if who == &setup.coldkey
                    && *event_netuid == netuid
                    && *event_alpha_fee == alpha_fee =>
                {
                    Some(*tao_amount)
                }
                _ => None,
            })
            .expect("expected TransactionFeePaidWithAlpha event");

        assert!(!burned_tao.is_zero());
        assert_eq!(
            Balances::free_balance(burn_account) - burn_balance_before,
            burned_tao
        );
        assert_eq!(
            Balances::free_balance(block_builder),
            block_builder_balance_before
        );
        assert_eq!(Balances::total_issuance(), balances_issuance_before);
        assert_eq!(
            SubtensorModule::get_total_issuance(),
            subtensor_issuance_before
        );
        assert_eq!(
            Balances::total_issuance(),
            SubtensorModule::get_total_issuance()
        );
    });
}
