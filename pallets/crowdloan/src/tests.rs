#![cfg(test)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{StorageDoubleMap, assert_err, assert_ok, traits::StorePreimage};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U256;
use sp_runtime::DispatchError;
use subtensor_runtime_common::TaoBalance;

use crate::{BalanceOf, CrowdloanId, CrowdloanInfo, mock::*, pallet as pallet_crowdloan};

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
            let funds_account = pallet_crowdloan::Pallet::<Test>::funds_account(crowdloan_id);
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
fn test_set_max_contribution_fails_if_max_contribution_is_too_low() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let max_contribution: BalanceOf<Test> = 40.into();
            let cap: BalanceOf<Test> = 300.into();
            let end: BlockNumberFor<Test> = 50;

            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                deposit,
                min_contribution,
                cap,
                end,
                Some(noop_call()),
                None
            ));

            assert_err!(
                Crowdloan::set_max_contribution(
                    RuntimeOrigin::signed(creator),
                    0,
                    Some(max_contribution)
                ),
                pallet_crowdloan::Error::<Test>::MaximumContributionTooLow
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
            let funds_account = pallet_crowdloan::Pallet::<Test>::funds_account(crowdloan_id);
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
            let funds_account = pallet_crowdloan::Pallet::<Test>::funds_account(crowdloan_id);
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
            let funds_account = pallet_crowdloan::Pallet::<Test>::funds_account(crowdloan_id);
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
            let funds_account = pallet_crowdloan::Pallet::<Test>::funds_account(crowdloan_id);
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
            let funds_account = pallet_crowdloan::Pallet::<Test>::funds_account(crowdloan_id);
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

#[test]
fn test_finalize_succeeds() {
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
            let call = Box::new(RuntimeCall::TestPallet(
                pallet_test::Call::<Test>::transfer_funds {
                    dest: U256::from(42),
                },
            ));

            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                deposit,
                min_contribution,
                cap,
                end,
                Some(call),
                None
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

            // finalize the crowdloan
            assert_ok!(Crowdloan::finalize(
                RuntimeOrigin::signed(creator),
                crowdloan_id
            ));

            // ensure the transfer was a success from the dispatched call
            assert_eq!(
                pallet_balances::Pallet::<Test>::free_balance(U256::from(42)),
                100.into()
            );

            // ensure the crowdloan is marked as finalized
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.finalized)
            );

            // ensure the event is emitted
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Finalized { crowdloan_id }.into()
            );

            // ensure the current crowdloan id was accessible from the dispatched call
            assert_eq!(
                pallet_test::PassedCrowdloanId::<Test>::get(),
                Some(crowdloan_id)
            );
        });
}

#[test]
fn test_finalize_succeeds_with_target_address() {
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
            let target_address: AccountOf<Test> = U256::from(42);

            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                deposit,
                min_contribution,
                cap,
                end,
                None,
                Some(target_address),
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

            // ensure the target address has received the funds
            assert_eq!(
                pallet_balances::Pallet::<Test>::free_balance(target_address),
                100.into()
            );

            // ensure the crowdloan is marked as finalized
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.finalized)
            );

            // ensure the event is emitted
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::Finalized { crowdloan_id }.into()
            );
        })
}

#[test]
fn test_finalize_fails_if_call_and_target_address_are_provided() {
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

            run_to_block(10);

            let crowdloan_id: CrowdloanId = 0;
            let contributor: AccountOf<Test> = U256::from(2);
            let amount: BalanceOf<Test> = 50.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                crowdloan_id,
                amount
            ));

            let target_address: AccountOf<Test> = U256::from(42);
            pallet_crowdloan::Crowdloans::<Test>::mutate(crowdloan_id, |crowdloan| {
                crowdloan.as_mut().unwrap().target_address = Some(target_address);
            });

            run_to_block(60);

            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::InvalidFinalizationConfig
            );

            assert_eq!(
                pallet_balances::Pallet::<Test>::free_balance(target_address),
                0.into()
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| !c.finalized)
            );
        });
}

#[test]
fn test_finalize_fails_if_call_and_target_address_are_missing() {
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

            run_to_block(10);

            let crowdloan_id: CrowdloanId = 0;
            let contributor: AccountOf<Test> = U256::from(2);
            let amount: BalanceOf<Test> = 50.into();
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                crowdloan_id,
                amount
            ));

            pallet_crowdloan::Crowdloans::<Test>::mutate(crowdloan_id, |crowdloan| {
                crowdloan.as_mut().unwrap().call = None;
            });

            run_to_block(60);

            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::InvalidFinalizationConfig
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| !c.finalized)
            );
        });
}

#[test]
fn test_finalize_fails_if_bad_origin() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let crowdloan_id: CrowdloanId = 0;

            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::none(), crowdloan_id),
                DispatchError::BadOrigin
            );

            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::root(), crowdloan_id),
                DispatchError::BadOrigin
            );
        });
}

#[test]
fn test_finalize_fails_if_crowdloan_does_not_exist() {
    TestState::default()
        .with_balance(U256::from(1), 100.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let crowdloan_id: CrowdloanId = 0;

            // try to finalize
            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::InvalidCrowdloanId
            );
        });
}

#[test]
fn test_finalize_fails_if_not_creator_origin() {
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
                None
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

            // try finalize the crowdloan
            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::signed(contributor), crowdloan_id),
                pallet_crowdloan::Error::<Test>::InvalidOrigin
            );
        });
}

#[test]
fn test_finalize_fails_if_crowdloan_cap_is_not_raised() {
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
            let amount: BalanceOf<Test> = 49.into(); // below cap

            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                crowdloan_id,
                amount
            ));

            // run some more blocks past the end of the contribution period
            run_to_block(60);

            // try finalize the crowdloan
            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::CapNotRaised
            );
        });
}

#[test]
fn test_finalize_fails_if_crowdloan_has_already_been_finalized() {
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

            // try finalize the crowdloan a second time
            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::AlreadyFinalized
            );
        });
}

#[test]
fn test_finalize_fails_if_call_fails() {
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
            let call = Box::new(RuntimeCall::TestPallet(
                pallet_test::Call::<Test>::failing_extrinsic {},
            ));

            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                deposit,
                min_contribution,
                cap,
                end,
                Some(call),
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

            // try finalize the crowdloan
            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_test::Error::<Test>::ShouldFail
            );
        });
}

#[test]
fn test_finalize_fails_if_another_finalize_is_in_progress() {
    TestState::default()
        .with_balance(U256::from(1), 300.into())
        .with_balance(U256::from(2), 300.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let contributor: AccountOf<Test> = U256::from(2);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 100.into();
            let end: BlockNumberFor<Test> = 50;
            let first_crowdloan_id: CrowdloanId = 0;
            let second_crowdloan_id: CrowdloanId = 1;

            let nested_finalize_call = Box::new(RuntimeCall::Crowdloan(pallet_crowdloan::Call::<
                Test,
            >::finalize {
                crowdloan_id: second_crowdloan_id,
            }));

            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                deposit,
                min_contribution,
                cap,
                end,
                Some(nested_finalize_call),
                None,
            ));
            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                deposit,
                min_contribution,
                cap,
                end,
                Some(noop_call()),
                None,
            ));

            run_to_block(10);

            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                first_crowdloan_id,
                50.into()
            ));
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                second_crowdloan_id,
                50.into()
            ));

            run_to_block(60);

            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::signed(creator), first_crowdloan_id),
                pallet_crowdloan::Error::<Test>::AlreadyFinalizing
            );

            assert_eq!(pallet_crowdloan::CurrentCrowdloanId::<Test>::get(), None);
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(first_crowdloan_id)
                    .is_some_and(|c| !c.finalized)
            );
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(second_crowdloan_id)
                    .is_some_and(|c| !c.finalized)
            );
        });
}

// The finalize `call` cannot re-enter `withdraw` on the same crowdloan: it is rejected and
// the extrinsic reverts, so no funds move and `raised` stays consistent with the real balance.
#[test]
fn test_finalize_blocks_reentrant_withdraw() {
    TestState::default()
        .with_balance(U256::from(1), 200.into()) // creator
        .with_balance(U256::from(2), 200.into()) // contributor
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let contributor: AccountOf<Test> = U256::from(2);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 100.into();
            let end: BlockNumberFor<Test> = 50;
            let crowdloan_id: CrowdloanId = 0;

            // The finalize call re-enters `withdraw` on the same crowdloan.
            let reentrant_call = Box::new(RuntimeCall::Crowdloan(
                pallet_crowdloan::Call::<Test>::withdraw { crowdloan_id },
            ));

            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                deposit,
                min_contribution,
                cap,
                end,
                Some(reentrant_call),
                None,
            ));
            run_to_block(10);

            // Creator contributes 30 over the deposit (total 80); contributor fills the cap.
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                30.into()
            ));
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                crowdloan_id,
                20.into()
            ));

            let funds_account = pallet_crowdloan::Pallet::<Test>::funds_account(crowdloan_id);
            assert_eq!(Balances::free_balance(funds_account), cap);
            let creator_balance_before = Balances::free_balance(creator);

            run_to_block(60);

            // Finalize dispatches the re-entrant withdraw, which is rejected with
            // `AlreadyFinalized`. Wrap in a storage layer to model the per-extrinsic
            // transaction the runtime applies in production, so the revert is observable.
            let outcome = frame_support::storage::with_storage_layer(|| {
                Crowdloan::finalize(RuntimeOrigin::signed(creator), crowdloan_id)
            });
            assert_err!(outcome, pallet_crowdloan::Error::<Test>::AlreadyFinalized);

            // No funds were extracted and accounting is intact.
            assert_eq!(Balances::free_balance(creator), creator_balance_before);
            assert_eq!(Balances::free_balance(funds_account), cap);
            assert_eq!(pallet_crowdloan::CurrentCrowdloanId::<Test>::get(), None);
            let crowdloan = pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id).unwrap();
            assert!(!crowdloan.finalized);
            assert_eq!(crowdloan.raised, cap);

            // Contributor funds are not frozen: the contributor can still withdraw.
            assert_ok!(Crowdloan::withdraw(
                RuntimeOrigin::signed(contributor),
                crowdloan_id
            ));
            assert_eq!(Balances::free_balance(contributor), 200.into());
        });
}

// A re-entrant `refund` embedded as the finalize call is likewise rejected before moving funds.
#[test]
fn test_finalize_blocks_reentrant_refund() {
    TestState::default()
        .with_balance(U256::from(1), 200.into()) // creator
        .with_balance(U256::from(2), 200.into()) // contributor
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let contributor: AccountOf<Test> = U256::from(2);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let cap: BalanceOf<Test> = 100.into();
            let end: BlockNumberFor<Test> = 50;
            let crowdloan_id: CrowdloanId = 0;

            let reentrant_call = Box::new(RuntimeCall::Crowdloan(
                pallet_crowdloan::Call::<Test>::refund { crowdloan_id },
            ));

            assert_ok!(Crowdloan::create(
                RuntimeOrigin::signed(creator),
                deposit,
                min_contribution,
                cap,
                end,
                Some(reentrant_call),
                None,
            ));
            run_to_block(10);

            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                30.into()
            ));
            assert_ok!(Crowdloan::contribute(
                RuntimeOrigin::signed(contributor),
                crowdloan_id,
                20.into()
            ));

            let funds_account = pallet_crowdloan::Pallet::<Test>::funds_account(crowdloan_id);
            run_to_block(60);

            // The re-entrant refund hits the `finalized` guard before transferring anything.
            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::AlreadyFinalized
            );
            assert_eq!(Balances::free_balance(funds_account), cap);
        });
}

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
            let funds_account = pallet_crowdloan::Pallet::<Test>::funds_account(crowdloan_id);
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

#[test]
fn test_update_min_contribution_succeeds() {
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
            let new_min_contribution: BalanceOf<Test> = 20.into();

            // update the min contribution
            assert_ok!(Crowdloan::update_min_contribution(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                new_min_contribution
            ));

            // ensure the min contribution is updated
            assert!(
                pallet_crowdloan::Crowdloans::<Test>::get(crowdloan_id)
                    .is_some_and(|c| c.min_contribution == new_min_contribution)
            );
            // ensure the event is emitted
            assert_eq!(
                last_event(),
                pallet_crowdloan::Event::<Test>::MinContributionUpdated {
                    crowdloan_id,
                    new_min_contribution
                }
                .into()
            );
        });
}

#[test]
fn test_update_min_contribution_fails_if_bad_origin() {
    TestState::default().build_and_execute(|| {
        let crowdloan_id: CrowdloanId = 0;

        assert_err!(
            Crowdloan::update_min_contribution(RuntimeOrigin::none(), crowdloan_id, 20.into()),
            DispatchError::BadOrigin
        );

        assert_err!(
            Crowdloan::update_min_contribution(RuntimeOrigin::root(), crowdloan_id, 20.into()),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_update_min_contribution_fails_if_crowdloan_does_not_exist() {
    TestState::default().build_and_execute(|| {
        let crowdloan_id: CrowdloanId = 0;

        assert_err!(
            Crowdloan::update_min_contribution(
                RuntimeOrigin::signed(U256::from(1)),
                crowdloan_id,
                20.into()
            ),
            pallet_crowdloan::Error::<Test>::InvalidCrowdloanId
        );
    });
}

#[test]
fn test_update_min_contribution_fails_if_crowdloan_has_been_finalized() {
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
            run_to_block(50);

            // finalize the crowdloan
            let crowdloan_id: CrowdloanId = 0;
            assert_ok!(Crowdloan::finalize(
                RuntimeOrigin::signed(creator),
                crowdloan_id
            ));

            // try update the min contribution
            let new_min_contribution: BalanceOf<Test> = 20.into();
            assert_err!(
                Crowdloan::update_min_contribution(
                    RuntimeOrigin::signed(creator),
                    crowdloan_id,
                    new_min_contribution
                ),
                pallet_crowdloan::Error::<Test>::AlreadyFinalized
            );
        });
}

#[test]
fn test_update_min_contribution_fails_if_not_creator() {
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
            let new_min_contribution: BalanceOf<Test> = 20.into();

            // try update the min contribution
            assert_err!(
                Crowdloan::update_min_contribution(
                    RuntimeOrigin::signed(U256::from(2)),
                    crowdloan_id,
                    new_min_contribution
                ),
                pallet_crowdloan::Error::<Test>::InvalidOrigin
            );
        });
}

#[test]
fn test_update_min_contribution_fails_if_new_min_contribution_is_too_low() {
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
            let new_min_contribution: BalanceOf<Test> = 9.into();

            // try update the min contribution
            assert_err!(
                Crowdloan::update_min_contribution(
                    RuntimeOrigin::signed(creator),
                    crowdloan_id,
                    new_min_contribution
                ),
                pallet_crowdloan::Error::<Test>::MinimumContributionTooLow
            );
        });
}

#[test]
fn test_update_min_contribution_fails_if_new_min_contribution_exceeds_max_contribution() {
    TestState::default()
        .with_balance(U256::from(1), 200.into())
        .build_and_execute(|| {
            let creator: AccountOf<Test> = U256::from(1);
            let deposit: BalanceOf<Test> = 50.into();
            let min_contribution: BalanceOf<Test> = 10.into();
            let max_contribution: BalanceOf<Test> = 60.into();
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

            let crowdloan_id: CrowdloanId = 0;
            assert_ok!(Crowdloan::set_max_contribution(
                RuntimeOrigin::signed(creator),
                crowdloan_id,
                Some(max_contribution)
            ));

            assert_err!(
                Crowdloan::update_min_contribution(
                    RuntimeOrigin::signed(creator),
                    crowdloan_id,
                    70.into()
                ),
                pallet_crowdloan::Error::<Test>::MinimumContributionTooHigh
            );
        });
}

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
