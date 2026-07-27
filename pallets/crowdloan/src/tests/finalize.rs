#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{assert_err, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U256;
use sp_runtime::DispatchError;

use crate::{BalanceOf, CrowdloanId, mock::*, pallet as pallet_crowdloan};

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

            let funds_account =
                pallet_crowdloan::Pallet::<Test>::crowdloan_funds_account(crowdloan_id);
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

            let funds_account =
                pallet_crowdloan::Pallet::<Test>::crowdloan_funds_account(crowdloan_id);
            run_to_block(60);

            // The re-entrant refund hits the `finalized` guard before transferring anything.
            assert_err!(
                Crowdloan::finalize(RuntimeOrigin::signed(creator), crowdloan_id),
                pallet_crowdloan::Error::<Test>::AlreadyFinalized
            );
            assert_eq!(Balances::free_balance(funds_account), cap);
        });
}
