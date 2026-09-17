//! Subsidize the staking-hotkey scan without reducing its execution weight.

use crate::transaction_payment_wrapper::{FeeWeightDiscount, fee_dispatch_info};
use crate::{Balance, Runtime, RuntimeCall, TransactionPayment, Weight};
use frame_support::{dispatch::DispatchInfo, traits::Get};
use pallet_subtensor::Call as SubtensorCall;
use pallet_subtensor_proxy::Call as ProxyCall;
use pallet_subtensor_utility::Call as UtilityCall;
use pallet_transaction_payment::{FeeDetails, RuntimeDispatchInfo};
use sp_std::vec;
use subtensor_runtime_common::Token;

/// Fee allowance only. The admission cap and the execution scan remain 256 keys.
pub const STAKING_HOTKEYS_FEE_ALLOWANCE: u32 = 4;

/// Count declarations that include the scan. Only descend through wrappers that
/// include their inner calls' weights; in particular, never discount `with_weight`.
fn scan_counts(call: &RuntimeCall) -> (u64, u64) {
    let mut pending = vec![call];
    let (mut single, mut bulk) = (0_u64, 0_u64);
    while let Some(call) = pending.pop() {
        match call {
            RuntimeCall::SubtensorModule(
                SubtensorCall::remove_stake { .. }
                | SubtensorCall::remove_stake_limit { .. }
                | SubtensorCall::remove_stake_full_limit { .. }
                | SubtensorCall::move_stake { .. }
                | SubtensorCall::move_stake_limit { .. }
                | SubtensorCall::transfer_stake { .. }
                | SubtensorCall::transfer_stake_and_hotkey { .. }
                | SubtensorCall::swap_stake { .. }
                | SubtensorCall::swap_stake_limit { .. },
            ) => single = single.saturating_add(1),
            RuntimeCall::SubtensorModule(
                SubtensorCall::unstake_all { .. } | SubtensorCall::unstake_all_alpha { .. },
            ) => bulk = bulk.saturating_add(1),
            RuntimeCall::Utility(
                UtilityCall::batch { calls }
                | UtilityCall::batch_all { calls }
                | UtilityCall::force_batch { calls },
            ) => pending.extend(calls),
            RuntimeCall::Utility(UtilityCall::if_else { main, fallback }) => {
                pending.push(main);
                pending.push(fallback);
            }
            RuntimeCall::Utility(
                UtilityCall::as_derivative { call, .. }
                | UtilityCall::dispatch_as { call, .. }
                | UtilityCall::dispatch_as_fallible { call, .. },
            )
            | RuntimeCall::Proxy(
                ProxyCall::proxy { call, .. } | ProxyCall::proxy_announced { call, .. },
            ) => pending.push(call),
            _ => {}
        }
    }
    (single, bulk)
}

impl FeeWeightDiscount<RuntimeCall> for Runtime {
    fn fee_weight_discount(call: &RuntimeCall) -> Weight {
        let (single, bulk) = scan_counts(call);
        let bulk_legs = if bulk == 0 {
            0
        } else {
            // Match unstake_all_worst_case_work exactly, including small networks.
            u64::from(pallet_subtensor::TotalNetworks::<Runtime>::get())
                .min(u64::from(pallet_subtensor::MAX_UNSTAKE_ALL_LEGS))
        };
        let walks = single.saturating_add(bulk.saturating_mul(bulk_legs));
        let subsidized_keys =
            pallet_subtensor::MAX_STAKING_HOTKEYS.saturating_sub(STAKING_HOTKEYS_FEE_ALLOWANCE);
        <Runtime as frame_system::Config>::DbWeight::get().reads(
            u64::from(subsidized_keys)
                .saturating_mul(pallet_subtensor::STAKING_HOTKEYS_WALK_READS_PER_ENTRY)
                .saturating_mul(walks),
        )
    }

    fn fee_discount_overhead(call: &RuntimeCall) -> Weight {
        // One TotalNetworks read in validation and another in preparation. The
        // declaration already prices the scan itself at the full execution cap.
        if scan_counts(call).1 == 0 {
            Weight::zero()
        } else {
            <Runtime as frame_system::Config>::DbWeight::get().reads(2)
        }
    }
}

pub fn query_fee_details(
    call: &RuntimeCall,
    info: &DispatchInfo,
    len: u32,
    is_bare: bool,
) -> FeeDetails<Balance> {
    if is_bare {
        FeeDetails {
            inclusion_fee: None,
            tip: Balance::ZERO,
        }
    } else {
        TransactionPayment::compute_fee_details(
            len,
            &fee_dispatch_info(info, Runtime::fee_weight_discount(call)),
            Balance::ZERO,
        )
    }
}

pub fn query_info(
    call: &RuntimeCall,
    info: &DispatchInfo,
    len: u32,
    is_bare: bool,
) -> RuntimeDispatchInfo<Balance> {
    RuntimeDispatchInfo {
        // RPC callers still need the real weight for block capacity estimates.
        weight: info.total_weight(),
        class: info.class,
        partial_fee: query_fee_details(call, info, len, is_bare).final_fee(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::transaction_payment_wrapper::ChargeTransactionPaymentWrapper;
    use crate::{
        Balances, BuildStorage, RuntimeGenesisConfig, RuntimeOrigin, SubtensorModule, System,
    };
    use frame_support::{
        assert_ok,
        dispatch::{GetDispatchInfo, Pays, PostDispatchInfo},
    };
    use pallet_subtensor::weights::WeightInfo;
    use sp_runtime::traits::{DispatchTransaction, TransactionExtension};
    use subtensor_runtime_common::{AccountId, AlphaBalance, NetUid};

    fn signer() -> AccountId {
        AccountId::from([1_u8; 32])
    }

    fn new_test_ext() -> sp_io::TestExternalities {
        let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig {
            balances: pallet_balances::GenesisConfig {
                balances: vec![(signer(), Balance::new(1_000_000_000))],
                dev_accounts: None,
            },
            ..Default::default()
        }
        .build_storage()
        .unwrap()
        .into();
        ext.execute_with(|| System::set_block_number(1));
        ext
    }

    fn remove_stake() -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::remove_stake {
            hotkey: AccountId::from([2_u8; 32]),
            netuid: NetUid::from(1),
            amount_unstaked: AlphaBalance::new(1_000_000),
        })
    }

    fn remove_base() -> Weight {
        <Runtime as pallet_subtensor::Config>::WeightInfo::remove_stake()
    }

    fn scan(keys: u64) -> Weight {
        <Runtime as frame_system::Config>::DbWeight::get()
            .reads(keys.saturating_mul(14).saturating_add(1))
    }

    #[test]
    fn fee_estimate_prices_four_keys_and_reports_full_execution_weight() {
        new_test_ext().execute_with(|| {
            let call = remove_stake();
            let info = call.get_dispatch_info();
            assert_eq!(pallet_subtensor::MAX_STAKING_HOTKEYS, 256);
            assert_eq!(info.call_weight, remove_base().saturating_add(scan(256)));
            let expected_info = DispatchInfo {
                call_weight: remove_base().saturating_add(scan(4)),
                ..info
            };
            let quote = query_info(&call, &info, 100, false);
            assert_eq!(quote.weight, info.total_weight());
            assert_eq!(
                quote.partial_fee,
                TransactionPayment::compute_fee(100, &expected_info, Balance::ZERO)
            );
            assert_eq!(
                query_info(&call, &info, 100, true).partial_fee,
                Balance::ZERO
            );
            assert!(
                query_fee_details(&call, &info, 100, true)
                    .inclusion_fee
                    .is_none()
            );
        });
    }

    #[test]
    fn payment_caps_fees_without_refunding_execution_weight_or_tips() {
        for keys in [1_u64, 4, 256] {
            new_test_ext().execute_with(|| {
                let call = remove_stake();
                let tip = Balance::new(1_000_000);
                let payment = ChargeTransactionPaymentWrapper::<Runtime>::new(tip);
                let info = DispatchInfo {
                    extension_weight: payment.weight(&call),
                    ..call.get_dispatch_info()
                };
                let expected_info = DispatchInfo {
                    call_weight: remove_base().saturating_add(scan(keys.min(4))),
                    ..info
                };
                let before = Balances::free_balance(signer());
                let execution_weight = remove_base().saturating_add(scan(keys));
                let post = payment
                    .test_run(
                        RuntimeOrigin::signed(signer()),
                        &call,
                        &info,
                        100,
                        0,
                        |_| {
                            Ok(PostDispatchInfo {
                                actual_weight: Some(execution_weight),
                                pays_fee: Pays::Yes,
                            })
                        },
                    )
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    before.saturating_sub(Balances::free_balance(signer())),
                    TransactionPayment::compute_fee(100, &expected_info, tip)
                );
                assert_eq!(
                    post.actual_weight,
                    Some(execution_weight.saturating_add(info.extension_weight)),
                    "fee discount must not reclaim block execution capacity"
                );
            });
        }
    }

    #[test]
    fn failed_calls_still_pay_only_the_discounted_declaration() {
        new_test_ext().execute_with(|| {
            let call = remove_stake();
            let info = call.get_dispatch_info();
            let expected = query_info(&call, &info, 100, false).partial_fee;
            let before = Balances::free_balance(signer());
            let result = ChargeTransactionPaymentWrapper::<Runtime>::new(Balance::ZERO).test_run(
                RuntimeOrigin::signed(signer()),
                &call,
                &info,
                100,
                0,
                |_| Err(sp_runtime::DispatchError::Other("failed").into()),
            );
            assert!(result.unwrap().is_err());
            assert_eq!(
                before.saturating_sub(Balances::free_balance(signer())),
                expected
            );
        });
    }

    #[test]
    fn zero_fee_post_dispatch_is_preserved() {
        new_test_ext().execute_with(|| {
            let call = remove_stake();
            let info = call.get_dispatch_info();
            let before = Balances::free_balance(signer());
            assert_ok!(
                ChargeTransactionPaymentWrapper::<Runtime>::new(Balance::ZERO).test_run(
                    RuntimeOrigin::signed(signer()),
                    &call,
                    &info,
                    100,
                    0,
                    |_| Ok(PostDispatchInfo {
                        actual_weight: None,
                        pays_fee: Pays::No
                    })
                )
            );
            assert_eq!(before, Balances::free_balance(signer()));
        });
    }

    #[test]
    fn nested_batches_and_proxies_receive_each_inner_discount() {
        new_test_ext().execute_with(|| {
            let inner = remove_stake();
            let one = Runtime::fee_weight_discount(&inner);
            let call = RuntimeCall::Proxy(ProxyCall::proxy {
                real: signer().into(),
                force_proxy_type: None,
                call: Box::new(RuntimeCall::Utility(UtilityCall::batch_all {
                    calls: vec![inner.clone(), inner.clone()],
                })),
            });
            assert_eq!(Runtime::fee_weight_discount(&call), one.saturating_mul(2));
            let info = call.get_dispatch_info();
            let fee_info = fee_dispatch_info(&info, Runtime::fee_weight_discount(&call));
            assert_eq!(
                fee_info.call_weight,
                info.call_weight.saturating_sub(one.saturating_mul(2))
            );

            let overridden = RuntimeCall::Utility(UtilityCall::with_weight {
                call: Box::new(inner),
                weight: Weight::from_parts(1, 0),
            });
            assert_eq!(Runtime::fee_weight_discount(&overridden), Weight::zero());
        });
    }

    #[test]
    fn bulk_discount_matches_the_declared_network_envelope() {
        new_test_ext().execute_with(|| {
            let call =
                RuntimeCall::SubtensorModule(SubtensorCall::unstake_all { hotkey: signer() });
            let per_walk = scan(256).saturating_sub(scan(4));
            for networks in [0_u16, 1, 2, 16, 100] {
                pallet_subtensor::TotalNetworks::<Runtime>::put(networks);
                let legs = u64::from(networks).min(16);
                assert_eq!(
                    Runtime::fee_weight_discount(&call),
                    per_walk.saturating_mul(legs)
                );
                assert_eq!(
                    call.get_dispatch_info().call_weight,
                    SubtensorModule::unstake_all_declared_weight()
                );
            }
        });
    }

    #[test]
    fn add_stake_and_unrelated_fees_are_unchanged() {
        new_test_ext().execute_with(|| {
            let calls = [
                RuntimeCall::SubtensorModule(SubtensorCall::add_stake {
                    hotkey: signer(),
                    netuid: NetUid::from(1),
                    amount_staked: Balance::new(1_000_000),
                }),
                RuntimeCall::System(frame_system::Call::remark { remark: vec![] }),
            ];
            for call in calls {
                let info = call.get_dispatch_info();
                assert_eq!(Runtime::fee_weight_discount(&call), Weight::zero());
                assert_eq!(Runtime::fee_discount_overhead(&call), Weight::zero());
                assert_eq!(
                    query_info(&call, &info, 100, false).partial_fee,
                    TransactionPayment::compute_fee(100, &info, Balance::ZERO)
                );
            }
        });
    }
}
