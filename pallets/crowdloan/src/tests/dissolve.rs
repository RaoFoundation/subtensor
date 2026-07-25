#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{StorageDoubleMap, assert_err, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U256;
use sp_runtime::DispatchError;

use crate::{BalanceOf, CrowdloanId, mock::*, pallet as pallet_crowdloan};

#[test]
fn test_dissolve_succeeds() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
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

            let crowdloan_id: CrowdloanId = 0;
            assert_ok!(Crowdloan::set_max_contribution(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                Some(cap)
            ));

            // run some blocks past end
            run_to_block(60);

            // ensure the contributor count is correct
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.contributors_count == 1)
            );

            // dissolve the crowdloan
            assert_ok!(Crowdloan::dissolve(
                RuntimeOrigin::signed(creator),
                crowdloan_id
            ));

            // ensure the crowdloan is removed from the crowdloans map
            assert!(pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id).is_none());

            // ensure the contributions are removed
            assert!(!pallet_crowdloan::Contributions::<Test>::contains_prefix(
                crowdloan_id
            ));

            // ensure the maximum contribution is removed
            assert!(pallet_crowdloan::MaxContributions::<Test>::get(crowdloan_id).is_none());

            // ensure the event is emitted
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Dissolved { crowdloan_id }.into()
            )
        });
}

#[test]
fn test_dissolve_fails_if_bad_origin() {
    TestState::default().build_and_execute(|| {
        let crowdloan_id: CrowdloanId = 0;

        assert_err!(
            Crowdloan::dissolve(RuntimeOrigin::none(), crowdloan_id),
            DispatchError::BadOrigin
        );

        assert_err!(
            Crowdloan::dissolve(RuntimeOrigin::root(), crowdloan_id),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_dissolve_fails_if_crowdloan_does_not_exist() {
    TestState::default().build_and_execute(|| {
        let crowdloan_id: CrowdloanId = 0;
        assert_err!(
            Crowdloan::dissolve(RuntimeOrigin::signed(U256::from(1)), crowdloan_id),
            pallet_crowdloan::Error::<Test>::InvalidCrowdloanId
        );
    });
}

#[test]
fn test_dissolve_fails_if_crowdloan_has_been_finalized() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 100.into())
        .build_and_execute(|| {
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

            // run some blocks
            run_to_block(10);

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

            // try dissolve the crowdloan
            assert_err!(
                Crowdloan::dissolve(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::AlreadyFinalized
            );
        });
}

#[test]
fn test_dissolve_fails_if_origin_is_not_creator() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
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

            // run some blocks
            run_to_block(10);

            // some contribution
            let crowdloan_id: CrowdloanId = 0;

            // try dissolve the crowdloan
            assert_err!(
                Crowdloan::dissolve(RuntimeOrigin::signed(U256::from(2)), crowdloan_id),
                pallet_crowdloan::Error::<Test>::InvalidOrigin
            );
        });
}

#[test]
fn test_dissolve_fails_if_not_everyone_has_been_refunded() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .with_balance(U256::from(2), 100.into())
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

            // run some blocks
            run_to_block(10);

            // some contribution
            let crowdloan_id: CrowdloanId = 0;
            let contributor: AccountOf<Test> = U256::from(2);
            let amount: BalanceOf<Test> = 50.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                crowdloan_id,
                amount
            ));

            // run some blocks
            run_to_block(10);

            // try to dissolve the crowdloan
            let crowdloan_id = 0;
            assert_err!(
                Crowdloan::dissolve(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::NotReadyToDissolve
            );
        });
}
