use super::mock::*;
use frame_support::{
    assert_ok,
    dispatch::GetDispatchInfo,
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
fn test_nested_variable_work_call_refunds_from_its_own_declaration() {
    let coldkey = U256::from(1);
    let inner_call = RuntimeCall::SubtensorModule(crate::Call::swap_coldkey_announced {
        new_coldkey: U256::from(2),
    });
    let outer_call = RuntimeCall::Utility(pallet_utility::Call::batch {
        calls: vec![inner_call.clone()],
    });
    let declared_weight = outer_call.get_dispatch_info().call_weight;

    new_test_ext(1).execute_with(|| {
        let Ok(post_info) = Utility::batch(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            vec![inner_call],
        ) else {
            panic!("utility batch returns dispatch post info");
        };
        let Some(actual_weight) = post_info.actual_weight else {
            panic!("utility batch reports its nested actual weight");
        };

        assert!(
            actual_weight.all_lt(declared_weight),
            "nested actual weight {actual_weight:?} must refund from declaration \
             {declared_weight:?}"
        );
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
