//! Tests for `pallet_subtensor_utility` batch calls against the subtensor runtime.
//!
//! Ensures nested batches are rejected while flat batches of allowed calls succeed.

use super::mock::*;
use frame_support::{
    assert_ok,
    traits::{Contains, Currency},
};
use frame_system::Config;
use pallet_subtensor_utility as pallet_utility;
use sp_core::U256;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::batch_tx::test_batch_txs --exact --show-output --nocapture
#[test]
fn test_batch_txs() {
    let alice = U256::from(0);
    let bob = U256::from(1);
    let charlie = U256::from(2);
    let initial_balances = vec![
        (alice, 8_000_000_000),
        (bob, 1_000_000_000),
        (charlie, 1_000_000_000),
    ];
    test_ext_with_balances(initial_balances).execute_with(|| {
        assert_ok!(Utility::batch(
            <<Test as Config>::RuntimeOrigin>::signed(alice),
            vec![
                RuntimeCall::Balances(BalanceCall::transfer_allow_death {
                    dest: bob,
                    value: 1_000_000_000.into()
                }),
                RuntimeCall::Balances(BalanceCall::transfer_allow_death {
                    dest: charlie,
                    value: 1_000_000_000.into()
                })
            ]
        ));
        assert_eq!(Balances::total_balance(&alice), 6_000_000_000_u64.into());
        assert_eq!(Balances::total_balance(&bob), 2_000_000_000_u64.into());
        assert_eq!(Balances::total_balance(&charlie), 2_000_000_000_u64.into());
    });
}

#[test]
fn test_cant_nest_batch_txs() {
    let bob = U256::from(1);
    let charlie = U256::from(2);

    new_test_ext(1).execute_with(|| {
        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![
                RuntimeCall::Balances(BalanceCall::transfer_allow_death {
                    dest: bob,
                    value: 1_000_000_000.into(),
                }),
                RuntimeCall::Utility(pallet_utility::Call::batch {
                    calls: vec![RuntimeCall::Balances(BalanceCall::transfer_allow_death {
                        dest: charlie,
                        value: 1_000_000_000.into(),
                    })],
                }),
            ],
        });

        assert!(!<Test as Config>::BaseCallFilter::contains(&call));
    });
}

#[test]
fn test_can_batch_txs() {
    let bob = U256::from(1);

    new_test_ext(1).execute_with(|| {
        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![RuntimeCall::Balances(BalanceCall::transfer_allow_death {
                dest: bob,
                value: 1_000_000_000.into(),
            })],
        });

        assert!(<Test as Config>::BaseCallFilter::contains(&call));
    });
}

#[test]
fn test_cant_nest_batch_diff_batch_txs() {
    let charlie = U256::from(2);

    new_test_ext(1).execute_with(|| {
        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![RuntimeCall::Utility(pallet_utility::Call::force_batch {
                calls: vec![RuntimeCall::Balances(BalanceCall::transfer_allow_death {
                    dest: charlie,
                    value: 1_000_000_000.into(),
                })],
            })],
        });

        assert!(!<Test as Config>::BaseCallFilter::contains(&call));

        let call2 = RuntimeCall::Utility(pallet_utility::Call::batch_all {
            calls: vec![RuntimeCall::Utility(pallet_utility::Call::batch {
                calls: vec![RuntimeCall::Balances(BalanceCall::transfer_allow_death {
                    dest: charlie,
                    value: 1_000_000_000.into(),
                })],
            })],
        });

        assert!(!<Test as Config>::BaseCallFilter::contains(&call2));

        let call3 = RuntimeCall::Utility(pallet_utility::Call::force_batch {
            calls: vec![RuntimeCall::Utility(pallet_utility::Call::batch_all {
                calls: vec![RuntimeCall::Balances(BalanceCall::transfer_allow_death {
                    dest: charlie,
                    value: 1_000_000_000.into(),
                })],
            })],
        });

        assert!(!<Test as Config>::BaseCallFilter::contains(&call3));
    });
}
