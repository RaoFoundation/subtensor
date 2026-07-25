#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
use super::helpers::{drain_coldkey_to_existential, lock_test_miner_collateral};
use super::mock::*;
use crate::{AlphaFeeHandler, TransactionFeeHandler};
use frame_support::dispatch::GetDispatchInfo;
use frame_support::pallet_prelude::Zero;
use sp_runtime::{
    traits::DispatchTransaction,
    transaction_validity::{InvalidTransaction, TransactionValidityError},
};
use subtensor_runtime_common::AlphaBalance;
use subtensor_swap_interface::SwapHandler;

// Fully collateral-bonded stake must not pay alpha fees. Regression for the
// phantom-bond bug where fee unstake stripped stake while MinerCollateral.locked
// stayed unchanged.
//
// cargo test --package subtensor-transaction-fee --lib -- tests::alpha_fee_collateral::test_alpha_fee_rejects_fully_collateralized_stake --exact --show-output
#[test]
fn test_alpha_fee_rejects_fully_collateralized_stake() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO;
        let sn = setup_fee_test_subnets(1, 1);
        let netuid = sn.subnets[0].netuid;
        let hotkey = sn.hotkeys[0];

        fund_and_add_stake(netuid, &sn.coldkey, &hotkey, stake_amount);
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &sn.coldkey,
            netuid,
        );
        assert!(!alpha.is_zero());
        lock_test_miner_collateral(netuid, &hotkey, &sn.coldkey, alpha);
        drain_coldkey_to_existential(&sn.coldkey);

        let alpha_vec = vec![(hotkey, netuid)];
        assert_eq!(
            SubtensorModule::available_to_unstake_from_hotkey(&sn.coldkey, &hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert!(
            !<TransactionFeeHandler<Test> as AlphaFeeHandler<Test>>::can_withdraw_in_alpha(
                &sn.coldkey,
                &alpha_vec,
                1.into(),
            )
        );

        let subnet_tao_before = SubnetTAO::<Test>::get(netuid);
        let subnet_alpha_in_before = SubnetAlphaIn::<Test>::get(netuid);
        let subnet_alpha_out_before = SubnetAlphaOut::<Test>::get(netuid);
        let collateral_before =
            MinerCollateral::<Test>::get((netuid, hotkey, sn.coldkey)).expect("collateral entry");
        let aggregate_before = ColdkeyMinerCollateral::<Test>::get(netuid, sn.coldkey);

        assert_eq!(
            <TransactionFeeHandler<Test> as AlphaFeeHandler<Test>>::withdraw_in_alpha(
                &sn.coldkey,
                &alpha_vec,
                1.into(),
            ),
            Err(TransactionValidityError::Invalid(
                InvalidTransaction::Payment
            ))
        );

        // Also reject through the full charge-extension path.
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
            hotkey,
            netuid,
            amount_unstaked: AlphaBalance::from(1u64),
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
            alpha
        );
        assert_eq!(SubnetTAO::<Test>::get(netuid), subnet_tao_before);
        assert_eq!(SubnetAlphaIn::<Test>::get(netuid), subnet_alpha_in_before);
        assert_eq!(SubnetAlphaOut::<Test>::get(netuid), subnet_alpha_out_before);
        let collateral_after =
            MinerCollateral::<Test>::get((netuid, hotkey, sn.coldkey)).expect("collateral entry");
        assert_eq!(collateral_after.locked, collateral_before.locked);
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, sn.coldkey),
            aggregate_before
        );
    });
}

// Only the free (non-collateral) slice of a position may fund alpha fees.
//
// cargo test --package subtensor-transaction-fee --lib -- tests::alpha_fee_collateral::test_alpha_fee_only_from_free_stake_above_collateral --exact --show-output
#[test]
fn test_alpha_fee_only_from_free_stake_above_collateral() {
    new_test_ext().execute_with(|| {
        let stake_amount = TAO * 10;
        let sn = setup_fee_test_subnets(1, 1);
        let netuid = sn.subnets[0].netuid;
        let hotkey = sn.hotkeys[0];

        fund_and_add_stake(netuid, &sn.coldkey, &hotkey, stake_amount);
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &sn.coldkey,
            netuid,
        );
        let locked = alpha / 2.into();
        let free = alpha.saturating_sub(locked);
        assert!(!free.is_zero());
        lock_test_miner_collateral(netuid, &hotkey, &sn.coldkey, locked);
        drain_coldkey_to_existential(&sn.coldkey);

        assert_eq!(
            SubtensorModule::available_to_unstake_from_hotkey(&sn.coldkey, &hotkey, netuid),
            free
        );

        let alpha_vec = vec![(hotkey, netuid)];

        // A fee larger than the free slice must be rejected up front.
        let large_tao_fee = TaoBalance::from(TAO.saturating_mul(20));
        let alpha_for_large =
            pallet_subtensor_swap::Pallet::<Test>::get_alpha_amount_for_tao(netuid, large_tao_fee);
        assert!(
            alpha_for_large > free,
            "test needs a TAO fee quote larger than free stake (got {alpha_for_large:?} vs free {free:?})"
        );
        assert!(
            !<TransactionFeeHandler<Test> as AlphaFeeHandler<Test>>::can_withdraw_in_alpha(
                &sn.coldkey,
                &alpha_vec,
                large_tao_fee,
            )
        );

        // A small fee that fits in the free slice succeeds and never touches
        // the locked collateral accounting.
        let small_tao_fee = TaoBalance::from(1_000_000u64); // 0.001 TAO
        let alpha_for_small =
            pallet_subtensor_swap::Pallet::<Test>::get_alpha_amount_for_tao(netuid, small_tao_fee);
        assert!(!alpha_for_small.is_zero());
        assert!(alpha_for_small <= free);
        assert!(
            <TransactionFeeHandler<Test> as AlphaFeeHandler<Test>>::can_withdraw_in_alpha(
                &sn.coldkey,
                &alpha_vec,
                small_tao_fee,
            )
        );

        let collateral_before = MinerCollateral::<Test>::get((netuid, hotkey, sn.coldkey))
            .expect("collateral entry")
            .locked;
        let (taken, _tao_out, fee_netuid) =
            <TransactionFeeHandler<Test> as AlphaFeeHandler<Test>>::withdraw_in_alpha(
                &sn.coldkey,
                &alpha_vec,
                small_tao_fee,
            )
            .expect("free-slice fee should withdraw");
        assert_eq!(fee_netuid, netuid);
        assert_eq!(taken, alpha_for_small);

        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &sn.coldkey,
            netuid,
        );
        assert_eq!(alpha_after, alpha.saturating_sub(taken));
        assert!(alpha_after >= locked);
        assert_eq!(
            MinerCollateral::<Test>::get((netuid, hotkey, sn.coldkey))
                .expect("collateral entry")
                .locked,
            collateral_before
        );
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, sn.coldkey),
            locked
        );
    });
}
