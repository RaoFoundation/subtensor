#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{assert_err, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U256;
use sp_runtime::DispatchError;

use crate::{BalanceOf, CrowdloanId, mock::*, pallet as pallet_crowdloan};

#[test]
fn test_update_end_succeeds() {
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

            let crowdloan_id: CrowdloanId = 0;
            let new_end: BlockNumberFor<Test> = 60;

            // update the end
            assert_ok!(Crowdloan::update_end(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                new_end
            ));

            // ensure the end is updated
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.end == new_end)
            );
            // ensure the event is emitted
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::EndUpdated {
                    crowdloan_id,
                    new_end
                }
                .into()
            );
        });
}

#[test]
fn test_update_end_fails_if_bad_origin() {
    TestState::default().build_and_execute(|| {
        let crowdloan_id: CrowdloanId = 0;

        assert_err!(
            Crowdloan::update_end(RuntimeOrigin::none(), crowdloan_id, 60),
            DispatchError::BadOrigin
        );

        assert_err!(
            Crowdloan::update_end(RuntimeOrigin::root(), crowdloan_id, 60),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_update_end_fails_if_crowdloan_does_not_exist() {
    TestState::default().build_and_execute(|| {
        let crowdloan_id: CrowdloanId = 0;

        assert_err!(
            Crowdloan::update_end(RuntimeOrigin::signed(U256::from(1)), crowdloan_id, 60),
            pallet_crowdloan::Error::<Test>::InvalidCrowdloanId
        );
    });
}

#[test]
fn test_update_end_fails_if_crowdloan_has_been_finalized() {
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

            let crowdloan_id: CrowdloanId = 0;

            // some contribution
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
            assert_ok!(Crowdloan::finalize(
                RuntimeOrigin::signed(creator),
                crowdloan_id
            ));

            // try update the end
            let new_end: BlockNumberFor<Test> = 60;
            assert_err!(
                Crowdloan::update_end(RuntimeOrigin::signed(creator), crowdloan_id, new_end),
                pallet_crowdloan::Error::<Test>::AlreadyFinalized
            );
        });
}

#[test]
fn test_update_end_fails_if_not_creator() {
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

            let crowdloan_id: CrowdloanId = 0;
            let new_end: BlockNumberFor<Test> = 60;

            // try update the end
            assert_err!(
                Crowdloan::update_end(RuntimeOrigin::signed(U256::from(2)), crowdloan_id, new_end),
                pallet_crowdloan::Error::<Test>::InvalidOrigin
            );
        });
}

#[test]
fn test_update_end_fails_if_new_end_is_in_past() {
    TestState::default()
        .with_block_number(50)
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 100.into();
            let end: BlockNumberFor<Test> = 100;

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
            let new_end: BlockNumberFor<Test> = 40;

            // try update the end to a past block number
            assert_err!(
                Crowdloan::update_end(RuntimeOrigin::signed(creator), crowdloan_id, new_end),
                pallet_crowdloan::Error::<Test>::CannotEndInPast
            );
        });
}

#[test]
fn test_update_end_fails_if_block_duration_is_too_short() {
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
            run_to_block(50);

            let crowdloan_id: CrowdloanId = 0;
            let new_end: BlockNumberFor<Test> = 51;

            // try update the end to a block number that is too long
            assert_err!(
                Crowdloan::update_end(RuntimeOrigin::signed(creator), crowdloan_id, new_end),
                pallet_crowdloan::Error::<Test>::BlockDurationTooShort
            );
        });
}

#[test]
fn test_update_end_fails_if_block_duration_is_too_long() {
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

            let crowdloan_id: CrowdloanId = 0;
            let new_end: BlockNumberFor<Test> = 1000;

            // try update the end to a block number that is too long
            assert_err!(
                Crowdloan::update_end(RuntimeOrigin::signed(creator), crowdloan_id, new_end),
                pallet_crowdloan::Error::<Test>::BlockDurationTooLong
            );
        });
}
