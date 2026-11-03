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

fn safe_wrappers(call: RuntimeCall) -> Vec<RuntimeCall> {
    vec![
        RuntimeCall::Utility(pallet_subtensor_utility::Call::as_safe_derivative {
            index: 0,
            call: Box::new(call.clone()),
        }),
        RuntimeCall::Utility(pallet_subtensor_utility::Call::as_protected_derivative {
            index: 0,
            call: Box::new(call.clone()),
        }),
        RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1_safe {
            other_signatories: vec![AccountId::new([2; 32])],
            call: Box::new(call.clone()),
        }),
        RuntimeCall::Multisig(pallet_multisig::Call::as_multi_safe {
            threshold: 2,
            other_signatories: vec![AccountId::new([2; 32])],
            maybe_timepoint: None,
            call: Box::new(call),
            max_weight: frame_support::weights::Weight::MAX,
        }),
    ]
}

#[test]
fn safe_wrappers_cannot_hide_any_nested_batch_variant() {
    for inner in [
        batch(vec![remark()]),
        RuntimeCall::Utility(pallet_subtensor_utility::Call::batch_all {
            calls: vec![remark()],
        }),
        RuntimeCall::Utility(pallet_subtensor_utility::Call::force_batch {
            calls: vec![remark()],
        }),
    ] {
        for wrapped in safe_wrappers(inner) {
            for outer in [
                batch(vec![wrapped.clone()]),
                RuntimeCall::Utility(pallet_subtensor_utility::Call::batch_all {
                    calls: vec![wrapped.clone()],
                }),
                RuntimeCall::Utility(pallet_subtensor_utility::Call::force_batch {
                    calls: vec![wrapped.clone()],
                }),
            ] {
                assert!(!NoNestingCallFilter::contains(&outer), "accepted {outer:?}");
            }
        }
    }
}

#[test]
fn safe_wrappers_allow_one_batch_and_wrapped_leaf_calls() {
    for wrapped in safe_wrappers(batch(vec![remark()])) {
        assert!(NoNestingCallFilter::contains(&wrapped));
    }
    for wrapped in safe_wrappers(remark()) {
        assert!(NoNestingCallFilter::contains(&batch(vec![wrapped])));
    }
}

#[test]
fn signed_safe_derivative_dispatches_leaf_but_rejects_hidden_batch() -> Result<(), String> {
    use frame_support::{assert_err, assert_ok};
    use node_subtensor_runtime::{RuntimeEvent, RuntimeGenesisConfig, RuntimeOrigin, System};
    use sp_runtime::{BuildStorage, traits::Dispatchable};

    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default().build_storage()?.into();
    ext.execute_with(|| {
        System::set_block_number(1);
        let leaf = RuntimeCall::System(frame_system::Call::remark_with_event { remark: vec![1] });
        let wrap = |call| {
            RuntimeCall::Utility(pallet_subtensor_utility::Call::as_safe_derivative {
                index: 0,
                call: Box::new(call),
            })
        };
        let signer = AccountId::new([1; 32]);
        assert_ok!(batch(vec![wrap(leaf.clone())]).dispatch(RuntimeOrigin::signed(signer.clone())));
        assert!(System::events().iter().any(|event| matches!(
            event.event,
            RuntimeEvent::System(frame_system::Event::Remarked { .. })
        )));

        System::reset_events();
        assert_err!(
            batch(vec![wrap(batch(vec![leaf]))])
                .dispatch(RuntimeOrigin::signed(signer))
                .map_err(|error| error.error),
            sp_runtime::DispatchError::from(
                frame_system::Error::<node_subtensor_runtime::Runtime>::CallFiltered,
            )
        );
        assert!(System::events().is_empty());
    });
    Ok(())
}
