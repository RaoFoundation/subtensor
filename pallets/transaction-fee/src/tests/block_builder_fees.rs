#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
use super::mock::*;
use frame_support::assert_ok;
use frame_support::dispatch::GetDispatchInfo;
use frame_support::pallet_prelude::Zero;
use sp_runtime::traits::DispatchTransaction;

// cargo test --package subtensor-transaction-fee --lib -- tests::block_builder_fees::test_add_stake_fees_go_to_block_builder --exact --show-output
#[test]
fn test_add_stake_fees_go_to_block_builder() {
    new_test_ext().execute_with(|| {
        // Portion of swap fees that should go to the block builder
        let block_builder_fee_portion = 1.;

        // Get the block builder balance
        let block_builder = U256::from(MOCK_BLOCK_BUILDER);
        let block_builder_balance_before = Balances::free_balance(block_builder);

        let stake_amount = TAO;
        let sn = setup_fee_test_subnets(1, 1);

        // Simulate add stake to get the expected TAO fee
        let (_, swap_fee) = swap_tao_to_alpha(sn.subnets[0].netuid, stake_amount.into());

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
