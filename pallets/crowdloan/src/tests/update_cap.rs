#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{assert_err, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U256;
use sp_runtime::DispatchError;

use crate::{BalanceOf, CrowdloanId, mock::*, pallet as pallet_crowdloan};

#[test]
fn test_update_cap_succeeds() {
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

            // try update the cap
            let crowdloan_id: CrowdloanId = 0;
            let new_cap: BalanceOf<Test> = 200.into();
            assert_ok!(Crowdloan::update_cap(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                new_cap
            ));

            // ensure the cap is updated
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.cap == new_cap)
            );
            // ensure the event is emitted
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::CapUpdated {
                    crowdloan_id,
                    new_cap
                }
                .into()
            );
        });
}

#[test]
fn test_update_cap_fails_if_bad_origin() {
    TestState::default().build_and_execute(|| {
        let crowdloan_id: CrowdloanId = 0;

        assert_err!(
            Crowdloan::update_cap(RuntimeOrigin::none(), crowdloan_id, 200.into()),
            DispatchError::BadOrigin
        );

        assert_err!(
            Crowdloan::update_cap(RuntimeOrigin::root(), crowdloan_id, 200.into()),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_update_cap_fails_if_crowdloan_does_not_exist() {
    TestState::default().build_and_execute(|| {
        let crowdloan_id: CrowdloanId = 0;

        assert_err!(
            Crowdloan::update_cap(
                RuntimeOrigin::signed(U256::from(1)),
                crowdloan_id,
                200.into()
            ),
            pallet_crowdloan::Error::<Test>::InvalidCrowdloanId
        );
    });
}

#[test]
fn test_update_cap_fails_if_crowdloan_has_been_finalized() {
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
            run_to_block(60);

            // finalize the crowdloan
            let crowdloan_id: CrowdloanId = 0;
            assert_ok!(Crowdloan::finalize(
                RuntimeOrigin::signed(creator),
                crowdloan_id
            ));

            // try update the cap
            let new_cap: BalanceOf<Test> = 200.into();
            assert_err!(
                Crowdloan::update_cap(RuntimeOrigin::signed(creator), crowdloan_id, new_cap),
                pallet_crowdloan::Error::<Test>::AlreadyFinalized
            );
        });
}

#[test]
fn test_update_cap_fails_if_not_creator() {
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

            // try update the cap
            let crowdloan_id: CrowdloanId = 0;
            let new_cap: BalanceOf<Test> = 200.into();
            assert_err!(
                Crowdloan::update_cap(RuntimeOrigin::signed(U256::from(2)), crowdloan_id, new_cap),
                pallet_crowdloan::Error::<Test>::InvalidOrigin
            );
        });
}

#[test]
fn test_update_cap_fails_if_new_cap_is_too_low() {
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

            // try update the cap
            let crowdloan_id: CrowdloanId = 0;
            let new_cap: BalanceOf<Test> = 49.into();
            assert_err!(
                Crowdloan::update_cap(RuntimeOrigin::signed(creator), crowdloan_id, new_cap),
                pallet_crowdloan::Error::<Test>::CapTooLow
            );
        });
}
