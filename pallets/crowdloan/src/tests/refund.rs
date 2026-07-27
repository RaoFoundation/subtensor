#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{assert_err, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U256;
use sp_runtime::DispatchError;
use subtensor_runtime_common::TaoBalance;

use crate::{BalanceOf, CrowdloanId, mock::*, pallet as pallet_crowdloan};

#[test]
fn test_refund_succeeds() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 100.into())
        .with_balance(U256::from(3), 100.into())
        .with_balance(U256::from(4), 100.into())
        .with_balance(U256::from(5), 100.into())
        .with_balance(U256::from(6), 100.into())
        .with_balance(U256::from(7), 100.into())
        .build_and_execute(|| {
            // create a crowdloan
            let creator: AccountOf<Test> = U256::from(1);
            let initial_deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 400.into();
            let end: BlockNumberFor<Test> = 50;
            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                initial_deposit,
                min_contribution,
                cap,
                end,
                Some(noop_call()),
                None,
            ));

            // run some blocks
            run_to_block(10);

            // make 6 contributions to reach 350 raised amount (initial deposit + contributions)
            let crowdloan_id: CrowdloanId = 0;
            let amount: BalanceOf<Test> = 50.into();
            for i in 2..8 {
                let contributor: AccountOf<Test> = U256::from(i);
                assert_ok!(Crowdloan::contribute(
                    RuntimeOrigin::signed(contributor),
                    crowdloan_id,
                    amount
                ));
            }

            // ensure the contributor count is correct
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 7)
            );

            // run some more blocks before the end of the contribution period
            run_to_block(20);

            //  first round of refund
            assert_ok!(Crowdloan::refund(
                RuntimeOrigin::signed(creator),
                crowdloan_id
            ));

            // ensure the contributor count is correct, we processed 5 refunds
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 2)
            );

            // ensure the crowdloan account has the correct amount
            let funds_account =
                pallet_crowdloan::Pallet::<Test>::crowdloan_funds_account(crowdloan_id);
            assert_eq!(
                Balances::free_balance(funds_account),
                TaoBalance::from(350) - TaoBalance::from(5) * amount
            );
            // ensure raised amount is updated correctly
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id).is_some_and(
                    |c| c.raised == TaoBalance::from(350) - TaoBalance::from(5) * amount
                )
            );
            // ensure the event is emitted
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::PartiallyRefunded { crowdloan_id }.into()
            );

            // run some more blocks past the end of the contribution period
            run_to_block(70);

            //  second round of refund
            assert_ok!(Crowdloan::refund(
                RuntimeOrigin::signed(creator),
                crowdloan_id
            ));

            // ensure the contributor count is correct, we processed 1 more refund
            // keeping deposit
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 1)
            );

            // ensure the crowdloan account has the correct amount
            assert_eq!(
                pallet_balances::Pallet::<Test>::free_balance(funds_account),
                initial_deposit
            );
            // ensure the raised amount is updated correctly
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.raised == initial_deposit)
            );

            // ensure creator has the correct amount
            assert_eq!(
                pallet_balances::Pallet::<Test>::free_balance(creator),
                initial_deposit
            );

            // ensure each contributor has been refunded and  removed from the crowdloan
            for i in 2..8 {
                let contributor: AccountOf<Test> = U256::from(i);
                assert_eq!(
                    pallet_balances::Pallet::<Test>::free_balance(contributor),
                    100.into()
                );
                assert_eq!(
                    pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, contributor),
                    None,
                );
            }

            // ensure the event is emitted
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::AllRefunded { crowdloan_id }.into()
            );
        })
}

#[test]
fn test_refund_fails_if_bad_or_invalid_origin() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            // create a crowdloan
            let crowdloan_id: CrowdloanId = 0;
            let creator: AccountOf<Test> = U256::from(1);
            let initial_deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 300.into();
            let end: BlockNumberFor<Test> = 50;
            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                initial_deposit,
                min_contribution,
                cap,
                end,
                Some(noop_call()),
                None,
            ));

            assert_err!(
                Crowdloan::refund(RuntimeOrigin::none(), crowdloan_id),
                DispatchError::BadOrigin
            );

            assert_err!(
                Crowdloan::refund(RuntimeOrigin::root(), crowdloan_id),
                DispatchError::BadOrigin
            );

            // run some blocks
            run_to_block(60);

            // try to refund
            let unknown_contributor: AccountOf<Test> = U256::from(2);
            assert_err!(
                Crowdloan::refund(RuntimeOrigin::signed(unknown_contributor), crowdloan_id),
                pallet_crowdloan::Error::<Test>::InvalidOrigin,
            );
        });
}

#[test]
fn test_refund_fails_if_crowdloan_does_not_exist() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let crowdloan_id: CrowdloanId = 0;

            assert_err!(
                Crowdloan::refund(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::InvalidCrowdloanId
            );
        });
}
