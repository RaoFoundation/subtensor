use frame_support::traits::Contains;
use node_subtensor_runtime::{NoNestingCallFilter, RuntimeCall};
use subtensor_runtime_common::{AccountId, ProxyType};

fn batch(calls: Vec<RuntimeCall>) -> RuntimeCall {
    RuntimeCall::Utility(pallet_subtensor_utility::Call::batch { calls })
}

fn remark() -> RuntimeCall {
    RuntimeCall::System(frame_system::Call::remark { remark: vec![] })
}

fn derivative(call: RuntimeCall) -> RuntimeCall {
    RuntimeCall::Utility(pallet_subtensor_utility::Call::as_derivative {
        index: 0,
        call: Box::new(call),
    })
}

#[test]
fn proxy_cannot_hide_a_nested_utility_batch() {
    let proxy = RuntimeCall::Proxy(pallet_subtensor_proxy::Call::proxy {
        real: AccountId::new([1; 32]).into(),
        force_proxy_type: Some(ProxyType::Any),
        call: Box::new(batch(vec![remark()])),
    });
    assert!(!NoNestingCallFilter::contains(&batch(vec![proxy])));
}

#[test]
fn multisig_cannot_hide_a_nested_utility_batch() {
    let multisig = RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1 {
        other_signatories: vec![AccountId::new([2; 32])],
        call: Box::new(batch(vec![remark()])),
    });
    assert!(!NoNestingCallFilter::contains(&batch(vec![multisig])));
}

#[test]
fn a_single_batch_with_wrapped_non_utility_calls_remains_allowed() {
    let proxy = RuntimeCall::Proxy(pallet_subtensor_proxy::Call::proxy {
        real: AccountId::new([3; 32]).into(),
        force_proxy_type: Some(ProxyType::Any),
        call: Box::new(remark()),
    });
    assert!(NoNestingCallFilter::contains(&batch(vec![proxy])));
}

#[test]
fn ordinary_calls_and_one_top_level_batch_remain_allowed() {
    assert!(NoNestingCallFilter::contains(&remark()));
    assert!(NoNestingCallFilter::contains(&batch(vec![remark()])));
}

#[test]
fn a_wrapper_around_one_top_level_batch_remains_allowed() {
    assert!(NoNestingCallFilter::contains(&derivative(batch(vec![
        remark(),
    ]))));
}

#[test]
fn the_same_wrapped_batch_is_rejected_inside_an_outer_batch() {
    assert!(!NoNestingCallFilter::contains(&batch(vec![derivative(
        batch(vec![remark()]),
    )])));
}

#[test]
fn sudo_around_one_top_level_batch_remains_allowed() {
    let sudo = RuntimeCall::Sudo(pallet_sudo::Call::sudo {
        call: Box::new(batch(vec![remark()])),
    });
    assert!(NoNestingCallFilter::contains(&sudo));
}
