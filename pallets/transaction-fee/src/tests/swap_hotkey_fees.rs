#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
use super::mock::*;
use crate::{SubtensorTxFeeHandler, TransactionFeeHandler};

// cargo test --package subtensor-transaction-fee --lib -- tests::swap_hotkey_fees::test_swap_hotkey_fees_alpha --exact --show-output
#[test]
fn test_swap_hotkey_fees_alpha() {
    new_test_ext().execute_with(|| {
        let sn = setup_fee_test_subnets(2, 2);
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

        // swap_hotkey and swap_hotkey_v2 move alpha stake off the origin hotkey,
        // so their fees must be eligible to be paid in alpha on every subnet that
        // hotkey has stake. Before the fix `fees_in_alpha` returned an empty vec
        // for these calls, forcing a TAO fee (and rejecting alpha-only callers).

        // netuid = None -> every subnet the origin hotkey has stake on (2 here).
        let call_all = RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_hotkey {
            hotkey: sn.hotkeys[0],
            new_hotkey: sn.hotkeys[1],
            netuid: None,
        });
        let alpha_vec_all =
            SubtensorTxFeeHandler::<Balances, TransactionFeeHandler<Test>>::fees_in_alpha::<Test>(
                &sn.coldkey,
                &call_all,
            );
        assert_eq!(alpha_vec_all.len(), 2);

        // netuid = Some(single) -> only that subnet.
        let call_one = RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_hotkey {
            hotkey: sn.hotkeys[0],
            new_hotkey: sn.hotkeys[1],
            netuid: Some(sn.subnets[0].netuid),
        });
        let alpha_vec_one =
            SubtensorTxFeeHandler::<Balances, TransactionFeeHandler<Test>>::fees_in_alpha::<Test>(
                &sn.coldkey,
                &call_one,
            );
        assert_eq!(alpha_vec_one.len(), 1);

        // swap_hotkey_v2 moves the same alpha and must be eligible too.
        let call_v2 = RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_hotkey_v2 {
            hotkey: sn.hotkeys[0],
            new_hotkey: sn.hotkeys[1],
            netuid: None,
            keep_stake: false,
        });
        let alpha_vec_v2 =
            SubtensorTxFeeHandler::<Balances, TransactionFeeHandler<Test>>::fees_in_alpha::<Test>(
                &sn.coldkey,
                &call_v2,
            );
        assert_eq!(alpha_vec_v2.len(), 2);
    });
}
