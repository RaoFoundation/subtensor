#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{assert_err, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U256;
use sp_runtime::DispatchError;
use subtensor_runtime_common::TaoBalance;

use crate::{BalanceOf, CrowdloanId, mock::*, pallet as pallet_crowdloan};

#[test]
fn test_contribute_succeeds() {
    TestState::default()
        .with_balance(U256::from(1), 200.into())
        .with_balance(U256::from(2), 500.into())
        .with_balance(U256::from(3), 200.into())
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

            let crowdloan_id: CrowdloanId = 0;

            // only the creator has contributed so far
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 1)
            );

            // first contribution to the crowdloan from creator
            let amount: BalanceOf<Test> = 50.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                amount
            ));
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Contributed {
                    crowdloan_id,
                    contributor: creator,
                    amount,
                }
                .into()
            );
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, creator),
                Some(100.into())
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 1)
            );
            assert_eq!(
                Balances::free_balance(creator),
                TaoBalance::from(200) - amount - initial_deposit
            );

            // second contribution to the crowdloan
            let contributor1: AccountOf<Test> = U256::from(2);
            let amount: BalanceOf<Test> = 100.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor1),
                crowdloan_id,
                amount
            ));
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Contributed {
                    crowdloan_id,
                    contributor: contributor1,
                    amount,
                }
                .into()
            );
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, contributor1),
                Some(100.into())
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 2)
            );
            assert_eq!(
                Balances::free_balance(contributor1),
                TaoBalance::from(500) - amount
            );

            // third contribution to the crowdloan
            let contributor2: AccountOf<Test> = U256::from(3);
            let amount: BalanceOf<Test> = 50.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor2),
                crowdloan_id,
                amount
            ));
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Contributed {
                    crowdloan_id,
                    contributor: contributor2,
                    amount,
                }
                .into()
            );
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, contributor2),
                Some(50.into())
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 3)
            );
            assert_eq!(
                Balances::free_balance(contributor2),
                TaoBalance::from(200) - amount
            );

            // ensure the contributions are present in the funds account
            let funds_account =
                pallet_crowdloan::Pallet::<Test>::crowdloan_funds_account(crowdloan_id);
            assert_eq!(Balances::free_balance(funds_account), 250.into());

            // ensure the crowdloan raised amount is updated correctly
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.raised == 250.into())
            );
        });
}

#[test]
fn test_contribute_succeeds_if_contribution_will_make_the_raised_amount_exceed_the_cap() {
    TestState::default()
        .with_balance(U256::from(1), 200.into())
        .with_balance(U256::from(2), 500.into())
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

            // first contribution to the crowdloan from creator
            let crowdloan_id: CrowdloanId = 0;
            let amount: BalanceOf<Test> = 50.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                amount
            ));
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Contributed {
                    crowdloan_id,
                    contributor: creator,
                    amount,
                }
                .into()
            );
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, creator),
                Some(100.into())
            );
            assert_eq!(
                Balances::free_balance(creator),
                TaoBalance::from(200) - amount - initial_deposit
            );

            // second contribution to the crowdloan above the cap
            let contributor1: AccountOf<Test> = U256::from(2);
            let amount: BalanceOf<Test> = 300.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor1),
                crowdloan_id,
                amount
            ));
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Contributed {
                    crowdloan_id,
                    contributor: contributor1,
                    amount: 200.into(), // the amount is capped at the cap
                }
                .into()
            );
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, contributor1),
                Some(200.into())
            );
            assert_eq!(Balances::free_balance(contributor1), (500 - 200).into());

            // ensure the contributions are present in the crowdloan account up to the cap
            let funds_account =
                pallet_crowdloan::Pallet::<Test>::crowdloan_funds_account(crowdloan_id);
            assert_eq!(Balances::free_balance(funds_account), 300.into());

            // ensure the crowdloan raised amount is updated correctly
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.raised == 300.into())
            );
        });
}

#[test]
fn test_contribute_caps_amount_at_max_contribution() {
    TestState::default()
        .with_balance(U256::from(1), 200.into())
        .with_balance(U256::from(2), 500.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let initial_deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let max_contribution: BalanceOf<Test> = 120.into();
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

            run_to_block(10);

            let crowdloan_id: CrowdloanId = 0;
            assert_ok!(Crowdloan::set_max_contribution(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                Some(max_contribution)
            ));

            let contributor: AccountOf<Test> = U256::from(2);
            let amount: BalanceOf<Test> = 200.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                crowdloan_id,
                amount
            ));
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Contributed {
                    crowdloan_id,
                    contributor,
                    amount: max_contribution,
                }
                .into()
            );
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, contributor),
                Some(max_contribution)
            );
            assert_eq!(
                Balances::free_balance(contributor),
                TaoBalance::from(500) - max_contribution
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.raised == initial_deposit + max_contribution)
            );

            assert_err!(
                Crowdloan::contribute(
                    RuntimeOrigin::signed(contributor),
                    crowdloan_id,
                    min_contribution
                ),
                pallet_crowdloan::Error::<Test>::MaxContributionReached
            );
        });
}

#[test]
fn test_contribute_can_be_capped_below_minimum_when_filling_cap() {
    TestState::default()
        .with_balance(U256::from(1), 200.into())
        .with_balance(U256::from(2), 100.into())
        .with_balance(U256::from(3), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let initial_deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 115.into();
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

            run_to_block(10);

            let crowdloan_id: CrowdloanId = 0;
            let first_contributor: AccountOf<Test> = U256::from(2);
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(first_contributor),
                crowdloan_id,
                60.into()
            ));

            let final_contributor: AccountOf<Test> = U256::from(3);
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(final_contributor),
                crowdloan_id,
                min_contribution
            ));

            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Contributed {
                    crowdloan_id,
                    contributor: final_contributor,
                    amount: 5.into(),
                }
                .into()
            );
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, final_contributor),
                Some(5.into())
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.raised == cap)
            );
        });
}

#[test]
fn test_contribute_can_be_capped_below_minimum_when_reaching_max_contribution() {
    TestState::default()
        .with_balance(U256::from(1), 200.into())
        .with_balance(U256::from(2), 500.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let initial_deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let max_contribution: BalanceOf<Test> = 105.into();
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

            run_to_block(10);

            let crowdloan_id: CrowdloanId = 0;
            assert_ok!(Crowdloan::set_max_contribution(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                Some(max_contribution)
            ));

            let contributor: AccountOf<Test> = U256::from(2);
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                crowdloan_id,
                100.into()
            ));
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                crowdloan_id,
                min_contribution
            ));

            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Contributed {
                    crowdloan_id,
                    contributor,
                    amount: 5.into(),
                }
                .into()
            );
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::get(crowdloan_id, contributor),
                Some(max_contribution)
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.raised == initial_deposit + max_contribution)
            );
        });
}

#[test]
fn test_contribute_fails_if_bad_origin() {
    TestState::default().build_and_execute(|| {
        let crowdloan_id: CrowdloanId = 0;
        let amount: BalanceOf<Test> = 100.into();

        assert_err!(
            Crowdloan::contribute(RuntimeOrigin::none(), crowdloan_id, amount),
            DispatchError::BadOrigin
        );

        assert_err!(
            Crowdloan::contribute(RuntimeOrigin::root(), crowdloan_id, amount),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_contribute_fails_if_crowdloan_does_not_exist() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let contributor: AccountOf<Test> = U256::from(1);
            let crowdloan_id: CrowdloanId = 0;
            let amount: BalanceOf<Test> = 20.into();

            assert_err!(
                Crowdloan::contribute(RuntimeOrigin::signed(contributor), crowdloan_id, amount),
                pallet_crowdloan::Error::<Test>::InvalidCrowdloanId
            );
        });
}

#[test]
fn test_contribute_fails_if_contribution_period_ended() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 100.into())
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

            // run past the end of the crowdloan
            run_to_block(60);

            // contribute to the crowdloan
            let contributor: AccountOf<Test> = U256::from(2);
            let crowdloan_id: CrowdloanId = 0;
            let amount: BalanceOf<Test> = 20.into();
            assert_err!(
                Crowdloan::contribute(RuntimeOrigin::signed(contributor), crowdloan_id, amount),
                pallet_crowdloan::Error::<Test>::ContributionPeriodEnded
            );
        });
}

#[test]
fn test_contribute_fails_if_cap_has_been_raised() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 1000.into())
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

            // first contribution to the crowdloan fully raise the cap
            let crowdloan_id: CrowdloanId = 0;
            let contributor1: AccountOf<Test> = U256::from(2);
            let amount: BalanceOf<Test> = cap - initial_deposit;
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor1),
                crowdloan_id,
                amount
            ));

            // second contribution to the crowdloan
            let contributor2: AccountOf<Test> = U256::from(3);
            let amount: BalanceOf<Test> = 10.into();
            assert_err!(
                Crowdloan::contribute(RuntimeOrigin::signed(contributor2), crowdloan_id, amount),
                pallet_crowdloan::Error::<Test>::CapRaised
            );
        });
}

#[test]
fn test_contribute_fails_if_contribution_is_below_minimum_contribution() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 100.into())
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
            let contributor: AccountOf<Test> = U256::from(2);
            let crowdloan_id: CrowdloanId = 0;
            let amount: BalanceOf<Test> = 5.into();
            assert_err!(
                Crowdloan::contribute(RuntimeOrigin::signed(contributor), crowdloan_id, amount),
                pallet_crowdloan::Error::<Test>::ContributionTooLow
            )
        });
}

#[test]
fn test_contribute_fails_if_max_contributors_has_been_reached() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 100.into())
        .with_balance(U256::from(3), 100.into())
        .with_balance(U256::from(4), 100.into())
        .with_balance(U256::from(5), 100.into())
        .with_balance(U256::from(6), 100.into())
        .with_balance(U256::from(7), 100.into())
        .with_balance(U256::from(8), 100.into())
        .with_balance(U256::from(9), 100.into())
        .with_balance(U256::from(10), 100.into())
        .with_balance(U256::from(11), 100.into())
        .build_and_execute(|| {
            // create a crowdloan
            let creator: AccountOf<Test> = U256::from(1);
            let initial_deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 1000.into();
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
            let amount: BalanceOf<Test> = 20.into();
            for i in 2..=10 {
                let contributor: AccountOf<Test> = U256::from(i);
                assert_ok!(Crowdloan::contribute(
                    RuntimeOrigin::signed(contributor),
                    crowdloan_id,
                    amount
                ));
            }

            // try to contribute
            let contributor: AccountOf<Test> = U256::from(10);
            assert_err!(
                Crowdloan::contribute(RuntimeOrigin::signed(contributor), crowdloan_id, amount),
                pallet_crowdloan::Error::<Test>::MaxContributorsReached
            );
        });
}

#[test]
fn test_contribute_fails_if_contributor_has_insufficient_balance() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 50.into())
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
            let contributor: AccountOf<Test> = U256::from(2);
            let amount: BalanceOf<Test> = 100.into();

            assert_err!(
                Crowdloan::contribute(RuntimeOrigin::signed(contributor), crowdloan_id, amount),
                pallet_crowdloan::Error::<Test>::InsufficientBalance
            );
        });
}
