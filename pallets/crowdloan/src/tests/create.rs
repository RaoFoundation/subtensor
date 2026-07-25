#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{assert_err, assert_ok, traits::StorePreimage};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U256;
use sp_runtime::DispatchError;
use subtensor_runtime_common::TaoBalance;

use crate::{BalanceOf, CrowdloanInfo, mock::*, pallet as pallet_crowdloan};

#[test]
fn test_create_succeeds() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 300.into();
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

            let crowdloan_id = 0;
            let funds_account =
                pallet_crowdloan::Pallet::<Test>::crowdloan_funds_account(crowdloan_id);
            // ensure the crowdloan is stored correctly
            let call = pallet_preimage::Pallet::<Test>::bound(*noop_call()).unwrap();
            assert_eq!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id),
                Some(CrowdloanInfo {
                    creator,
                    deposit,
                    min_contribution,
                    cap,
                    end,
                    funds_account,
                    raised: deposit,
                    target_address: None,
                    call: Some(call),
                    finalized: false,
                    contributors_count: 1,
                })
            );
            // ensure the crowdloan account has the deposit
            assert_eq!(Balances::free_balance(funds_account), deposit);
            // ensure the creator has been deducted the deposit
            assert_eq!(
                Balances::free_balance(creator),
                TaoBalance::from(100) - deposit
            );
            // ensure the contributions have been updated
            assert_eq!(
                pallet_crowdloan::Contributions::<Test>::iter_prefix(crowdloan_id)
                    .collect::<Vec<_>>(),
                vec![(creator, deposit)]
            );
            // ensure the raised amount is updated correctly
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.raised == deposit)
            );
            // ensure the event is emitted
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Created {
                    crowdloan_id,
                    creator,
                    end,
                    cap,
                }
                .into()
            );
            // ensure next crowdloan id is incremented
            assert_eq!(
                pallet_crowdloan::NextCrowdloanId::<Test>::get(),
                crowdloan_id + 1
            );
        });
}

#[test]
fn test_create_fails_if_bad_origin() {
    TestState::default().build_and_execute(|| {
        let deposit: BalanceOf<Test> = 50.into();
        let min_contribution: BalanceOf<Test> = 10.into();
        let cap: BalanceOf<Test> = 300.into();
        let end: BlockNumberFor<Test> = 50;

        assert_err!(
            Crowdloan::create(
                RuntimeOrigin::none(),
                deposit,
                min_contribution,
                cap,
                end,
                Some(noop_call()),
                None
            ),
            DispatchError::BadOrigin
        );

        assert_err!(
            Crowdloan::create(
                RuntimeOrigin::root(),
                deposit,
                min_contribution,
                cap,
                end,
                Some(noop_call()),
                None
            ),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_create_fails_if_deposit_is_too_low() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 20.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 300.into();
            let end: BlockNumberFor<Test> = 50;

            assert_err!(
                Crowdloan::create(
                    RuntimeOrigin::signed(creator),
                    deposit,
                    min_contribution,
                    cap,
                    end,
                    Some(noop_call()),
                    None
                ),
                pallet_crowdloan::Error::<Test>::DepositTooLow
            );
        });
}

#[test]
fn test_create_fails_if_cap_is_not_greater_than_deposit() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 40.into();
            let end: BlockNumberFor<Test> = 50;

            assert_err!(
                Crowdloan::create(
                    RuntimeOrigin::signed(creator),
                    deposit,
                    min_contribution,
                    cap,
                    end,
                    Some(noop_call()),
                    None
                ),
                pallet_crowdloan::Error::<Test>::CapTooLow
            );
        });
}

#[test]
fn test_create_fails_if_min_contribution_is_too_low() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 5.into();
            let cap: BalanceOf<Test> = 300.into();
            let end: BlockNumberFor<Test> = 50;

            assert_err!(
                Crowdloan::create(
                    RuntimeOrigin::signed(creator),
                    deposit,
                    min_contribution,
                    cap,
                    end,
                    Some(noop_call()),
                    None
                ),
                pallet_crowdloan::Error::<Test>::MinimumContributionTooLow
            );
        });
}

#[test]
fn test_create_fails_if_call_and_target_address_are_provided() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 300.into();
            let end: BlockNumberFor<Test> = 50;
            let target_address: AccountOf<Test> = U256::from(42);

            assert_err!(
                Crowdloan::create(
                    RuntimeOrigin::signed(creator),
                    deposit,
                    min_contribution,
                    cap,
                    end,
                    Some(noop_call()),
                    Some(target_address),
                ),
                pallet_crowdloan::Error::<Test>::InvalidFinalizationConfig
            );
        });
}

#[test]
fn test_create_fails_if_call_and_target_address_are_missing() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 300.into();
            let end: BlockNumberFor<Test> = 50;

            assert_err!(
                Crowdloan::create(
                    RuntimeOrigin::signed(creator),
                    deposit,
                    min_contribution,
                    cap,
                    end,
                    None,
                    None,
                ),
                pallet_crowdloan::Error::<Test>::InvalidFinalizationConfig
            );
        });
}

#[test]
fn test_create_fails_if_end_is_in_the_past() {
    let current_block_number: BlockNumberFor<Test> = 10;

    TestState::default()
        .with_block_number(current_block_number)
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 300.into();
            let end: BlockNumberFor<Test> = current_block_number - 5;

            assert_err!(
                Crowdloan::create(
                    RuntimeOrigin::signed(creator),
                    deposit,
                    min_contribution,
                    cap,
                    end,
                    Some(noop_call()),
                    None
                ),
                pallet_crowdloan::Error::<Test>::CannotEndInPast
            );
        });
}

#[test]
fn test_create_fails_if_block_duration_is_too_short() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 300.into();
            let end: BlockNumberFor<Test> = 11;

            assert_err!(
                Crowdloan::create(
                    RuntimeOrigin::signed(creator),
                    deposit,
                    min_contribution,
                    cap,
                    end,
                    Some(noop_call()),
                    None
                ),
                pallet_crowdloan::Error::<Test>::BlockDurationTooShort
            );
        });
}

#[test]
fn test_create_fails_if_block_duration_is_too_long() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 300.into();
            let end: BlockNumberFor<Test> = 1000;

            assert_err!(
                Crowdloan::create(
                    RuntimeOrigin::signed(creator),
                    deposit,
                    min_contribution,
                    cap,
                    end,
                    Some(noop_call()),
                    None
                ),
                pallet_crowdloan::Error::<Test>::BlockDurationTooLong
            );
        });
}

#[test]
fn test_create_fails_if_creator_has_insufficient_balance() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 200.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 300.into();
            let end: BlockNumberFor<Test> = 50;

            assert_err!(
                Crowdloan::create(
                    RuntimeOrigin::signed(creator),
                    deposit,
                    min_contribution,
                    cap,
                    end,
                    Some(noop_call()),
                    None
                ),
                pallet_crowdloan::Error::<Test>::InsufficientBalance
            );
        });
}
