#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{assert_err, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U256;

use crate::{BalanceOf, mock::*, pallet as pallet_crowdloan};

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
