#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{assert_err, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U256;
use sp_runtime::DispatchError;
use subtensor_runtime_common::TaoBalance;

use crate::{BalanceOf, CrowdloanId, mock::*, pallet as pallet_crowdloan};

#[test]
fn test_withdraw_from_contributor_succeeds() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 100.into())
        .with_balance(U256::from(3), 100.into())
        .build_and_execute(|| {
            // create a crowdloan
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
                None
            ));

            // run some blocks
            run_to_block(10);

            // contribute to the crowdloan
            let crowdloan_id: CrowdloanId = 0;

            let contributor1: AccountOf<Test> = U256::from(2);
            let amount1: BalanceOf<Test> = 100.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor1),
                crowdloan_id,
                amount1
            ));

            let contributor2: AccountOf<Test> = U256::from(3);
            let amount2: BalanceOf<Test> = 100.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor2),
                crowdloan_id,
                amount2
            ));

            // run some more blocks past the end of the contribution period
            run_to_block(60);

            // ensure the contributor count is correct
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 3)
            );

            // withdraw from contributor1
            assert_ok!(Crowdloan::withdraw(
                RuntimeOrigin::signed(contributor1),
                crowdloan_id
            ));
            // ensure the contributor1 contribution has been removed
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, contributor1),
                None,
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 2)
            );
            // ensure the contributor1 has the correct amount
            assert_eq!(
                pallet_balances::Pallet::<Test>::free_balance(contributor1),
                100.into()
            );

            // withdraw from contributor2
            assert_ok!(Crowdloan::withdraw(
                RuntimeOrigin::signed(contributor2),
                crowdloan_id
            ));
            // ensure the contributor2 contribution has been removed
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, contributor2),
                None,
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 1)
            );
            // ensure the contributor2 has the correct amount
            assert_eq!(
                pallet_balances::Pallet::<Test>::free_balance(contributor2),
                100.into()
            );

            // ensure the crowdloan account has the correct amount
            let funds_account =
                pallet_crowdloan::Pallet::<Test>::crowdloan_funds_account(crowdloan_id);
            assert_eq!(Balances::free_balance(funds_account), initial_deposit);
            // ensure the crowdloan raised amount is updated correctly
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.raised == initial_deposit)
            );
        });
}

#[test]
fn test_withdraw_from_creator_with_contribution_over_deposit_succeeds() {
    TestState::default()
        .with_balance(U256::from(1), 200.into())
        .build_and_execute(|| {
            // create a crowdloan
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
                None
            ));

            // contribute to the crowdloan as the creator
            let crowdloan_id: CrowdloanId = 0;

            let amount: BalanceOf<Test> = 100.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                amount
            ));

            // ensure the contributor count is correct
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 1)
            );

            // withdraw
            let crowdloan_id: CrowdloanId = 0;
            assert_ok!(Crowdloan::withdraw(
                RuntimeOrigin::signed(creator),
                crowdloan_id
            ));

            // ensure the creator has the correct amount
            assert_eq!(
                pallet_balances::Pallet::<Test>::free_balance(creator),
                TaoBalance::from(200) - initial_deposit
            );
            // ensure the creator contribution has been removed
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, creator),
                Some(initial_deposit),
            );
            // ensure the contributor count hasn't changed because deposit is kept
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 1)
            );

            // ensure the crowdloan account has the correct amount
            let funds_account =
                pallet_crowdloan::Pallet::<Test>::crowdloan_funds_account(crowdloan_id);
            assert_eq!(Balances::free_balance(funds_account), initial_deposit);
            // ensure the crowdloan raised amount is updated correctly
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.raised == initial_deposit)
            );
        });
}

#[test]
fn test_withdraw_fails_from_creator_with_no_contribution_over_deposit() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 200.into())
        .build_and_execute(|| {
            // create a crowdloan
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
                None
            ));

            // try to withdraw
            let crowdloan_id: CrowdloanId = 0;
            assert_err!(
                Crowdloan::withdraw(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::DepositCannotBeWithdrawn
            );

            // ensure the crowdloan account has the correct amount
            let funds_account =
                pallet_crowdloan::Pallet::<Test>::crowdloan_funds_account(crowdloan_id);
            assert_eq!(Balances::free_balance(funds_account), initial_deposit);
            // ensure the crowdloan raised amount is updated correctly
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.raised == initial_deposit)
            );
        });
}

#[test]
fn test_withdraw_fails_if_bad_origin() {
    TestState::default().build_and_execute(|| {
        let crowdloan_id: CrowdloanId = 0;

        assert_err!(
            Crowdloan::withdraw(RuntimeOrigin::none(), crowdloan_id),
            DispatchError::BadOrigin
        );

        assert_err!(
            Crowdloan::withdraw(RuntimeOrigin::root(), crowdloan_id),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_withdraw_fails_if_crowdloan_does_not_exists() {
    TestState::default().build_and_execute(|| {
        let contributor: AccountOf<Test> = U256::from(1);
        let crowdloan_id: CrowdloanId = 0;

        assert_err!(
            Crowdloan::withdraw(RuntimeOrigin::signed(contributor), crowdloan_id),
            pallet_crowdloan::Error::<Test>::InvalidCrowdloanId
        );
    });
}

#[test]
fn test_withdraw_fails_if_crowdloan_has_already_been_finalized() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 200.into())
        .build_and_execute(|| {
            // create a crowdloan
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 100.into();
            let end: BlockNumberFor<Test> = 50;

            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                deposit,
                min_contribution,
                cap,
                end,
                Some(noop_call()),
                None,
            ));

            // some contribution
            let crowdloan_id: CrowdloanId = 0;
            let contributor: AccountOf<Test> = U256::from(2);
            let amount: BalanceOf<Test> = 50.into();

            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                crowdloan_id,
                amount
            ));

            // run some more blocks past the end of the contribution period
            run_to_block(60);

            // finalize the crowdloan
            assert_ok!(Crowdloan::finalize(
                RuntimeOrigin::signed(creator),
                crowdloan_id
            ));

            // try to withdraw
            assert_err!(
                Crowdloan::withdraw(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::AlreadyFinalized
            );
        });
}

#[test]
fn test_withdraw_fails_if_no_contribution_exists() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 200.into())
        .build_and_execute(|| {
            // create a crowdloan
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
                None
            ));

            // run some more blocks past the end of the contribution period
            run_to_block(60);

            // try to withdraw
            let crowdloan_id: CrowdloanId = 0;
            let contributor: AccountOf<Test> = U256::from(2);
            assert_err!(
                Crowdloan::withdraw(RuntimeOrigin::signed(contributor), crowdloan_id),
                pallet_crowdloan::Error::<Test>::NoContribution
            );
        });
}
