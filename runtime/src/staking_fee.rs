//! Fee discounts that leave admission and execution weight alone.
//!
//! Three subsidies, applied per fee-bearing leaf call (descending through batch and
//! proxy wrappers whose declared weight includes their inner calls):
//! 1. the `StakingHotkeys` scan is billed at [`STAKING_HOTKEYS_FEE_ALLOWANCE`] keys;
//! 2. root claims are billed at [`ROOT_CLAIM_FEE_ALLOWANCE`] claim units;
//! 3. every call whose declared weight grew after spec 459 is billed at most its 459
//!    declared weight ([`fee_weight_cap_459`]).

use crate::transaction_payment_wrapper::{FeeWeightDiscount, fee_dispatch_info};
use crate::{Balance, Runtime, RuntimeCall, TransactionPayment, Weight};
use frame_support::dispatch::{DispatchInfo, GetDispatchInfo};
use pallet_subtensor::Call as SubtensorCall;
use pallet_subtensor::staking::BasketFlushWork;
use pallet_subtensor_proxy::Call as ProxyCall;
use pallet_subtensor_utility::Call as UtilityCall;
use pallet_transaction_payment::{FeeDetails, RuntimeDispatchInfo};
use sp_std::vec;
use sp_std::vec::Vec;
use subtensor_runtime_common::Token;

/// Fee allowance only. The admission cap and the execution scan remain 256 keys.
pub const STAKING_HOTKEYS_FEE_ALLOWANCE: u32 = 4;

/// Fee allowance for root claims, in claim units (hotkeys plus basket holding rows), the
/// same count the admission envelope uses. Admission and execution keep the full 256-unit
/// (coldkey-wide) or 129-unit (single hotkey) envelope plus the flat flush allowance.
pub const ROOT_CLAIM_FEE_ALLOWANCE: u32 = STAKING_HOTKEYS_FEE_ALLOWANCE;

/// Weight a claim is charged for: `units` claim units plus the flush work one hotkey with
/// `units` queued credits and `units` holdings can do (`4Q + 2H` quotes, `Q` rows; see
/// `basket_flush_work_bound`).
pub fn root_claim_fee_weight(units: u32) -> Weight {
    let rows = u64::from(units);
    pallet_subtensor::Pallet::<Runtime>::root_claim_weight_for_work(
        units,
        BasketFlushWork::new(rows.saturating_mul(6), rows),
    )
}

fn root_claim_discount(limit: u32) -> Weight {
    pallet_subtensor::Pallet::<Runtime>::root_claim_declared_weight_for(limit)
        .saturating_sub(root_claim_fee_weight(ROOT_CLAIM_FEE_ALLOWANCE))
}

/// Declared `call_weight` (ref_time) each call quoted on spec 459: `payment_queryInfo`
/// on finney at block 9 088 000 via the archive node, one unit of every argument.
/// `payment_queryInfo` reports the declared call weight, so these are the fee weights
/// users paid before the 464-466 benchmark regen and security envelopes. A call listed
/// here is never billed more than this (the 467 halving then applies on top). Calls
/// that did not exist on 459, or that got cheaper, are not listed.
pub fn fee_weight_cap_459(call: &RuntimeCall) -> Option<Weight> {
    let ref_time: u64 = match call {
        RuntimeCall::SubtensorModule(call) => match call {
            SubtensorCall::burned_register { .. } => 4_175_124_000,
            SubtensorCall::register_limit { .. } => 4_155_012_000,
            SubtensorCall::register { .. } => 4_174_683_000,
            SubtensorCall::swap_hotkey { .. } | SubtensorCall::swap_hotkey_v2 { .. } => {
                672_080_000_000
            }
            SubtensorCall::swap_coldkey { .. } => 27_790_000_000,
            SubtensorCall::swap_coldkey_announced { .. } => 26_760_000_000,
            SubtensorCall::terminate_lease { .. } => 64_056_000_937,
            SubtensorCall::stake_into_basket { .. } => 147_420_000_000,
            SubtensorCall::claim_root { .. } | SubtensorCall::claim_root_with_hotkey { .. } => {
                249_916_000_000
            }
            SubtensorCall::unstake_all { .. } => 225_000_000,
            SubtensorCall::unstake_all_alpha { .. } => 4_069_000_000,
            SubtensorCall::remove_stake { .. } => 2_409_000_000,
            SubtensorCall::remove_stake_limit { .. } => 2_472_000_000,
            SubtensorCall::remove_stake_full_limit { .. } => 2_500_000_000,
            SubtensorCall::move_stake { .. } => 1_370_000_000,
            SubtensorCall::move_stake_limit { .. } => 4_194_000_000,
            SubtensorCall::transfer_stake { .. } => 1_427_000_000,
            SubtensorCall::transfer_stake_and_hotkey { .. } => 1_506_000_000,
            SubtensorCall::swap_stake { .. } => 4_073_000_000,
            SubtensorCall::swap_stake_limit { .. } => 4_169_000_000,
            SubtensorCall::burn_alpha { .. } => 912_000_000,
            SubtensorCall::recycle_alpha { .. } => 1_133_000_000,
            SubtensorCall::set_min_collateral { .. } => 281_000_000,
            _ => return None,
        },
        _ => return None,
    };
    // Only ref_time prices; leave proof_size alone.
    Some(Weight::from_parts(ref_time, u64::MAX))
}

/// Number of `StakingHotkeys` walks the declaration of `call` includes.
fn staking_walks(call: &RuntimeCall) -> u64 {
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
        ) => 1,
        RuntimeCall::SubtensorModule(
            SubtensorCall::unstake_all { .. } | SubtensorCall::unstake_all_alpha { .. },
        ) => {
            // Match unstake_all_worst_case_work exactly, including small networks.
            u64::from(pallet_subtensor::TotalNetworks::<Runtime>::get())
                .min(u64::from(pallet_subtensor::MAX_UNSTAKE_ALL_LEGS))
        }
        _ => 0,
    }
}

fn is_bulk_unstake(call: &RuntimeCall) -> bool {
    matches!(
        call,
        RuntimeCall::SubtensorModule(
            SubtensorCall::unstake_all { .. } | SubtensorCall::unstake_all_alpha { .. }
        )
    )
}

fn staking_scan_discount(call: &RuntimeCall) -> Weight {
    let subsidized_keys =
        pallet_subtensor::MAX_STAKING_HOTKEYS.saturating_sub(STAKING_HOTKEYS_FEE_ALLOWANCE);
    <Runtime as frame_system::Config>::DbWeight::get().reads(
        u64::from(subsidized_keys)
            .saturating_mul(pallet_subtensor::STAKING_HOTKEYS_WALK_READS_PER_ENTRY)
            .saturating_mul(staking_walks(call)),
    )
}

fn claim_discount(call: &RuntimeCall) -> Weight {
    match call {
        RuntimeCall::SubtensorModule(SubtensorCall::claim_root { .. }) => {
            root_claim_discount(pallet_subtensor::Pallet::<Runtime>::root_claim_declared_work())
        }
        RuntimeCall::SubtensorModule(SubtensorCall::claim_root_with_hotkey { .. }) => {
            root_claim_discount(
                pallet_subtensor::Pallet::<Runtime>::root_claim_hotkey_declared_work(),
            )
        }
        _ => Weight::zero(),
    }
}

/// The weight one leaf call is billed for, given its declared `call_weight`.
pub fn leaf_fee_weight(call: &RuntimeCall, declared: Weight) -> Weight {
    let fee_weight = declared
        .saturating_sub(staking_scan_discount(call))
        .saturating_sub(claim_discount(call));
    match fee_weight_cap_459(call) {
        Some(cap) => fee_weight.min(cap),
        None => fee_weight,
    }
}

/// Fee-bearing leaves of `call`. Only descend through wrappers whose declared weight
/// includes their inner calls' weights; in particular, never descend into `with_weight`.
fn leaves(call: &RuntimeCall) -> Vec<&RuntimeCall> {
    let mut pending = vec![call];
    let mut out = Vec::new();
    while let Some(call) = pending.pop() {
        match call {
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
            _ => out.push(call),
        }
    }
    out
}

impl FeeWeightDiscount<RuntimeCall> for Runtime {
    fn fee_weight_discount(call: &RuntimeCall, info: &DispatchInfo) -> Weight {
        let leaves = leaves(call);
        if let [leaf] = leaves.as_slice() {
            if core::ptr::eq(*leaf, call) {
                return info
                    .call_weight
                    .saturating_sub(leaf_fee_weight(call, info.call_weight));
            }
        }
        // Wrapped leaves: their declared weight is re-derived here. This repeats the
        // computation FRAME already did for the wrapper's own weight, so every storage
        // read hits the overlay cache.
        leaves.iter().fold(Weight::zero(), |discount, leaf| {
            let declared = leaf.get_dispatch_info().call_weight;
            discount.saturating_add(declared.saturating_sub(leaf_fee_weight(leaf, declared)))
        })
    }

    fn fee_discount_overhead(call: &RuntimeCall) -> Weight {
        // One TotalNetworks read in validation and another in preparation. The
        // declaration already prices the scan itself at the full execution cap.
        // Claim discounts and 459 caps are pure arithmetic on the declared weights.
        if leaves(call).into_iter().any(is_bulk_unstake) {
            <Runtime as frame_system::Config>::DbWeight::get().reads(2)
        } else {
            Weight::zero()
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
            &fee_dispatch_info(info, Runtime::fee_weight_discount(call, info)),
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

    /// Discount for a call priced from its own dispatch info.
    fn discount(call: &RuntimeCall) -> Weight {
        Runtime::fee_weight_discount(call, &call.get_dispatch_info())
    }

    /// Cap every listed call's fee weight at its 459 declaration.
    fn capped(call: &RuntimeCall, fee_weight: Weight) -> Weight {
        match fee_weight_cap_459(call) {
            Some(cap) => fee_weight.min(cap),
            None => fee_weight,
        }
    }

    fn remove_base() -> Weight {
        <Runtime as pallet_subtensor::Config>::WeightInfo::remove_stake().saturating_add(
            <Runtime as pallet_subtensor::Config>::WeightInfo::check_coldkey_swap_extension(),
        )
    }

    fn scan(keys: u64) -> Weight {
        <Runtime as frame_system::Config>::DbWeight::get()
            .reads(keys.saturating_mul(14).saturating_add(1))
    }

    fn claim_root_with_hotkey() -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::claim_root_with_hotkey {
            hotkey: AccountId::from([2_u8; 32]),
        })
    }

    fn claim_root() -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::claim_root {
            subnets: Default::default(),
        })
    }

    fn extension() -> Weight {
        <Runtime as pallet_subtensor::Config>::WeightInfo::check_coldkey_swap_extension()
    }

    /// Each claim call with the envelope its dispatch declares.
    fn claim_cases() -> [(RuntimeCall, Weight); 2] {
        [
            (
                claim_root_with_hotkey(),
                SubtensorModule::root_claim_declared_weight_for(
                    SubtensorModule::root_claim_hotkey_declared_work(),
                ),
            ),
            (
                claim_root(),
                SubtensorModule::root_claim_declared_weight_for(
                    SubtensorModule::root_claim_declared_work(),
                ),
            ),
        ]
    }

    #[test]
    fn claim_fee_estimate_prices_the_allowance_and_reports_full_execution_weight() {
        new_test_ext().execute_with(|| {
            let fee_weight = root_claim_fee_weight(ROOT_CLAIM_FEE_ALLOWANCE);
            assert_eq!(SubtensorModule::root_claim_declared_work(), 256);
            assert_eq!(SubtensorModule::root_claim_hotkey_declared_work(), 129);
            for (call, declared) in claim_cases() {
                assert!(fee_weight.all_lt(declared));
                let info = call.get_dispatch_info();
                assert_eq!(info.call_weight, declared.saturating_add(extension()));

                let discount = discount(&call);
                assert_eq!(discount, declared.saturating_sub(fee_weight));
                assert!(discount.all_lt(info.call_weight));
                assert_eq!(Runtime::fee_discount_overhead(&call), Weight::zero());

                let expected_info = DispatchInfo {
                    call_weight: fee_weight.saturating_add(extension()),
                    ..info
                };
                let quote = query_info(&call, &info, 100, false);
                assert_eq!(quote.weight, info.total_weight());
                assert_eq!(
                    quote.partial_fee,
                    TransactionPayment::compute_fee(100, &expected_info, Balance::ZERO)
                );
                assert!(
                    quote.partial_fee < TransactionPayment::compute_fee(100, &info, Balance::ZERO)
                );
            }
        });
    }

    #[test]
    fn claim_payment_refunds_light_work_and_caps_heavy_work() {
        let light = <Runtime as pallet_subtensor::Config>::WeightInfo::claim_root(1)
            .saturating_add(<Runtime as pallet_subtensor::Config>::WeightInfo::claim_root_scan(2));
        for (call, declared) in claim_cases() {
            for execution_weight in [light, declared] {
                new_test_ext().execute_with(|| {
                    let tip = Balance::new(1_000_000);
                    let payment = ChargeTransactionPaymentWrapper::<Runtime>::new(tip);
                    let info = DispatchInfo {
                        extension_weight: payment.weight(&call),
                        ..call.get_dispatch_info()
                    };
                    let fee_info = fee_dispatch_info(&info, discount(&call));
                    let charged_info = DispatchInfo {
                        call_weight: execution_weight.min(fee_info.call_weight),
                        ..info
                    };
                    let before = Balances::free_balance(signer());
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
                    let charged = before.saturating_sub(Balances::free_balance(signer()));
                    assert_eq!(
                        charged,
                        TransactionPayment::compute_fee(100, &charged_info, tip)
                    );
                    let capped = TransactionPayment::compute_fee(100, &fee_info, tip);
                    if execution_weight == light {
                        assert!(charged < capped, "light claims refund below the allowance");
                    } else {
                        assert_eq!(charged, capped, "heavy claims pay the allowance only");
                    }
                    assert_eq!(
                        post.actual_weight,
                        Some(execution_weight.saturating_add(info.extension_weight)),
                        "fee discount must not reclaim block execution capacity"
                    );
                });
            }
        }
    }

    #[test]
    fn batched_claims_receive_each_inner_discount() {
        new_test_ext().execute_with(|| {
            let hotkey_discount = discount(&claim_root_with_hotkey());
            let coldkey_discount = discount(&claim_root());
            assert!(hotkey_discount.all_lt(coldkey_discount));

            let call = RuntimeCall::Proxy(ProxyCall::proxy {
                real: signer().into(),
                force_proxy_type: None,
                call: Box::new(RuntimeCall::Utility(UtilityCall::batch_all {
                    calls: vec![
                        claim_root_with_hotkey(),
                        claim_root_with_hotkey(),
                        claim_root(),
                    ],
                })),
            });
            let discount = discount(&call);
            assert_eq!(
                discount,
                hotkey_discount
                    .saturating_mul(2)
                    .saturating_add(coldkey_discount)
            );
            let info = call.get_dispatch_info();
            assert!(discount.all_lt(info.call_weight));
            assert_eq!(
                fee_dispatch_info(&info, discount).call_weight,
                info.call_weight.saturating_sub(discount)
            );
            assert_eq!(Runtime::fee_discount_overhead(&call), Weight::zero());
        });
    }

    #[test]
    fn fee_estimate_prices_four_keys_and_reports_full_execution_weight() {
        new_test_ext().execute_with(|| {
            let call = remove_stake();
            let info = call.get_dispatch_info();
            assert_eq!(pallet_subtensor::MAX_STAKING_HOTKEYS, 256);
            assert_eq!(info.call_weight, remove_base().saturating_add(scan(256)));
            let expected_info = DispatchInfo {
                call_weight: capped(&call, remove_base().saturating_add(scan(4))),
                ..info
            };
            assert_eq!(
                discount(&call),
                info.call_weight.saturating_sub(expected_info.call_weight)
            );
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
                    call_weight: capped(&call, remove_base().saturating_add(scan(keys.min(4)))),
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
            let one = discount(&inner);
            let call = RuntimeCall::Proxy(ProxyCall::proxy {
                real: signer().into(),
                force_proxy_type: None,
                call: Box::new(RuntimeCall::Utility(UtilityCall::batch_all {
                    calls: vec![inner.clone(), inner.clone()],
                })),
            });
            assert_eq!(discount(&call), one.saturating_mul(2));
            let info = call.get_dispatch_info();
            let fee_info = fee_dispatch_info(&info, discount(&call));
            assert_eq!(
                fee_info.call_weight,
                info.call_weight.saturating_sub(one.saturating_mul(2))
            );

            let overridden = RuntimeCall::Utility(UtilityCall::with_weight {
                call: Box::new(inner),
                weight: Weight::from_parts(1, 0),
            });
            assert_eq!(discount(&overridden), Weight::zero());
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
                let declared = call.get_dispatch_info().call_weight;
                let fee_weight = capped(&call, declared.saturating_sub(per_walk.saturating_mul(legs)));
                assert_eq!(discount(&call), declared.saturating_sub(fee_weight));
                assert!(fee_weight.ref_time() <= 225_000_000, "unstake_all bills at most its 459 weight");
                assert_eq!(
                    call.get_dispatch_info().call_weight,
                    SubtensorModule::unstake_all_declared_weight().saturating_add(
                        <Runtime as pallet_subtensor::Config>::WeightInfo::check_coldkey_swap_extension(),
                    )
                );
            }
        });
    }

    /// Calls whose declared weight grew after 459 (benchmark regen or security envelopes).
    fn regressed_calls() -> Vec<RuntimeCall> {
        let other = AccountId::from([2_u8; 32]);
        vec![
            RuntimeCall::SubtensorModule(SubtensorCall::burned_register {
                netuid: NetUid::from(1),
                hotkey: other.clone(),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::register_limit {
                netuid: NetUid::from(1),
                hotkey: other.clone(),
                limit_price: 1,
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::swap_hotkey {
                hotkey: other.clone(),
                new_hotkey: signer(),
                netuid: None,
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::swap_coldkey {
                old_coldkey: other.clone(),
                new_coldkey: signer(),
                swap_cost: Balance::new(1),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::stake_into_basket {
                hotkey: other.clone(),
                amount_staked: Balance::new(1),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::unstake_all_alpha {
                hotkey: other.clone(),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::recycle_alpha {
                hotkey: other.clone(),
                amount: AlphaBalance::new(1),
                netuid: NetUid::from(1),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::burn_alpha {
                hotkey: other.clone(),
                amount: AlphaBalance::new(1),
                netuid: NetUid::from(1),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::set_min_collateral {
                netuid: NetUid::from(1),
                hotkey: other,
                min_locked: AlphaBalance::new(1),
            }),
            remove_stake(),
            claim_root_with_hotkey(),
        ]
    }

    #[test]
    fn regressed_calls_bill_at_most_their_459_weight() {
        new_test_ext().execute_with(|| {
            pallet_subtensor::TotalNetworks::<Runtime>::put(100);
            for call in regressed_calls() {
                let info = call.get_dispatch_info();
                let cap = fee_weight_cap_459(&call).unwrap();
                let fee_weight = info.call_weight.saturating_sub(discount(&call));
                assert!(
                    fee_weight.ref_time() <= cap.ref_time(),
                    "{call:?} bills {} > 459 cap {}",
                    fee_weight.ref_time(),
                    cap.ref_time()
                );
                assert_eq!(cap.proof_size(), u64::MAX, "the cap prices ref_time only");
                let expected_info = DispatchInfo {
                    call_weight: fee_weight,
                    ..info
                };
                let quote = query_info(&call, &info, 100, false);
                assert_eq!(quote.weight, info.total_weight());
                assert_eq!(
                    quote.partial_fee,
                    TransactionPayment::compute_fee(100, &expected_info, Balance::ZERO)
                );
            }
        });
    }

    #[test]
    fn batched_regressed_calls_are_capped_leaf_by_leaf() {
        new_test_ext().execute_with(|| {
            let calls = regressed_calls();
            let expected = calls.iter().fold(Weight::zero(), |acc, call| {
                acc.saturating_add(discount(call))
            });
            let batch = RuntimeCall::Utility(UtilityCall::batch_all { calls });
            let info = batch.get_dispatch_info();
            let batch_discount = Runtime::fee_weight_discount(&batch, &info);
            assert_eq!(batch_discount, expected);
            assert!(batch_discount.all_lt(info.call_weight));
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
                assert_eq!(discount(&call), Weight::zero());
                assert_eq!(Runtime::fee_discount_overhead(&call), Weight::zero());
                assert_eq!(
                    query_info(&call, &info, 100, false).partial_fee,
                    TransactionPayment::compute_fee(100, &info, Balance::ZERO)
                );
            }
        });
    }
}
