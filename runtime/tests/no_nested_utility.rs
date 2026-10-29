use frame_support::traits::Contains;
use node_subtensor_runtime::{NoNestingCallFilter, RuntimeCall};
use subtensor_runtime_common::{AccountId, ProxyType};

fn batch(calls: Vec<RuntimeCall>) -> RuntimeCall {
    RuntimeCall::Utility(pallet_subtensor_utility::Call::batch { calls })
}

fn remark() -> RuntimeCall {
    RuntimeCall::System(frame_system::Call::remark { remark: vec![] })
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
