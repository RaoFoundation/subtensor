//! Unit tests for extrinsic↔benchmark pairing and WeightInfo plug detection.

use super::benchmark_scan::{collect_frame_v2_benchmarks, collect_legacy_benchmarks};
use super::dispatchable_scan::collect_dispatchables_from_source;
use super::pallet_paths::is_test_or_mock_source_path;
use super::weight_info_plug::{
    is_benchmarked_weight_plugged, source_has_matching_weight_info_for_dispatchable,
    weight_attr_calls_weight_info_for,
};
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn weightinfo_plug_helper_accepts_captured_attrs_and_custom_allow() {
    let batch_attr = r#"
        #[pallet::call_index(0)]
        #[pallet::weight({
            let (dispatch_weight, pays) = Pallet::<T>::weight_and_dispatch_class(calls);
            let dispatch_weight = dispatch_weight
                .saturating_add(T::WeightInfo::batch(calls.len() as u32));
            (dispatch_weight, DispatchClass::Normal, pays)
        })]
    "#;
    assert!(is_benchmarked_weight_plugged("batch", Some(batch_attr)));

    let allow_attr = r#"
        #[allow(unknown_lints, benchmarked_weight_not_plugged)]
        #[pallet::weight(store_encrypted_weight())]
    "#;
    assert!(is_benchmarked_weight_plugged(
        "store_encrypted",
        Some(allow_attr),
    ));
}

#[test]
fn weightinfo_plug_matcher_accepts_exact_methods_and_rejects_prefixes() {
    assert!(weight_attr_calls_weight_info_for(
        "batch",
        "#[pallet::weight(T::WeightInfo::batch(calls.len() as u32))]",
    ));
    assert!(weight_attr_calls_weight_info_for(
        "proxy",
        "#[pallet::weight(WeightInfo::<T>::proxy(p.into()))]",
    ));
    assert!(weight_attr_calls_weight_info_for(
        "set_weights",
        "#[pallet::weight(::WeightInfo::set_weights())]",
    ));
    assert!(weight_attr_calls_weight_info_for(
        "set_fee_rate",
        "#[pallet::weight(<T as Config>::WeightInfo::set_fee_rate())]",
    ));
    assert!(weight_attr_calls_weight_info_for(
        "swap_coldkey",
        "#[pallet::weight(T::WeightInfo::swap_coldkey::<T>())]",
    ));

    assert!(!weight_attr_calls_weight_info_for(
        "swap_coldkey",
        "#[pallet::weight(T::WeightInfo::swap_coldkey_announced())]",
    ));
    assert!(!weight_attr_calls_weight_info_for(
        "set_weight",
        "#[pallet::weight(T::WeightInfo::set_weights())]",
    ));
}

#[test]
fn source_fallback_accepts_valid_complex_weight_attrs() {
    let source = r#"
        #[pallet::call]
        impl<T: Config> Pallet<T> {
            #[pallet::call_index(0)]
            #[pallet::weight({
                let (dispatch_weight, pays) = Pallet::<T>::weight_and_dispatch_class(calls);
                let dispatch_weight = dispatch_weight
                    .saturating_add(T::WeightInfo::batch(calls.len() as u32));
                (dispatch_weight, DispatchClass::Normal, pays)
            })]
            pub fn batch(origin: OriginFor<T>, calls: Vec<T::RuntimeCall>) -> DispatchResult {
                Ok(())
            }
        }
    "#;

    assert!(source_has_matching_weight_info_for_dispatchable(
        source, "batch"
    ));
}

#[test]
fn source_fallback_does_not_use_previous_dispatchable_weight_attr() {
    let source = r#"
        #[pallet::call]
        impl<T: Config> Pallet<T> {
            #[pallet::call_index(0)]
            #[pallet::weight(T::WeightInfo::first())]
            pub fn first(origin: OriginFor<T>) -> DispatchResult { Ok(()) }

            #[pallet::call_index(1)]
            #[pallet::weight(Weight::from_parts(1, 0))]
            pub fn second(origin: OriginFor<T>) -> DispatchResult { Ok(()) }
        }
    "#;

    assert!(source_has_matching_weight_info_for_dispatchable(
        source, "first"
    ));
    assert!(!source_has_matching_weight_info_for_dispatchable(
        source, "second"
    ));
}

#[test]
fn weightinfo_plug_check_accepts_common_valid_forms() {
    assert!(weight_attr_calls_weight_info_for(
        "batch",
        "#[pallet::weight({ let w = T::WeightInfo::batch(calls.len() as u32); w })]",
    ));
    assert!(weight_attr_calls_weight_info_for(
        "set_fee_rate",
        "#[pallet::weight(<T as Config>::WeightInfo::set_fee_rate())]",
    ));
    assert!(weight_attr_calls_weight_info_for(
        "set_weights",
        "#[pallet::weight((::WeightInfo::set_weights(), DispatchClass::Normal, Pays::No))]",
    ));
    assert!(weight_attr_calls_weight_info_for(
        "proxy",
        "#[pallet::weight((WeightInfo::<T>::proxy(T::MaxProxies::get()), DispatchClass::Normal))]",
    ));
    assert!(!weight_attr_calls_weight_info_for(
        "proxy",
        "#[pallet::weight(Weight::from_parts(10, 0))]",
    ));
}

#[test]
fn dispatchable_weight_attr_is_found_for_complex_weight_blocks() {
    let input = r#"
        #[pallet::call]
        impl<T: Config> Pallet<T> {
            #[pallet::call_index(0)]
            #[pallet::weight({
                let (dispatch_weight, pays) = Pallet::<T>::weight_and_dispatch_class(calls);
                let dispatch_weight = dispatch_weight
                    .saturating_add(T::WeightInfo::batch(calls.len() as u32));
                (dispatch_weight, DispatchClass::Normal, pays)
            })]
            pub fn batch(origin: OriginFor<T>, calls: Vec<T::RuntimeCall>) -> DispatchResult {
                Ok(())
            }
        }
    "#;

    let dispatchables = collect_dispatchables_from_source(input);
    let batch = dispatchables
        .iter()
        .find(|dispatchable| dispatchable.name == "batch")
        .expect("batch dispatchable is collected");

    assert!(is_benchmarked_weight_plugged(
        &batch.name,
        batch.weight_attr.as_deref(),
    ));
}

#[test]
fn custom_allow_attr_skips_weightinfo_plug_check() {
    let dispatch_source = r#"
        #[pallet::call]
        impl<T: Config> Pallet<T> {
            #[allow(unknown_lints, benchmarked_weight_not_plugged)]
            #[pallet::call_index(2)]
            #[pallet::weight(store_encrypted_weight())]
            pub fn store_encrypted(origin: OriginFor<T>) -> DispatchResult { Ok(()) }
        }
    "#;

    let dispatchables = collect_dispatchables_from_source(dispatch_source);
    let dispatchable = dispatchables
        .iter()
        .find(|dispatchable| dispatchable.name == "store_encrypted")
        .expect("store_encrypted dispatchable is collected");

    assert!(is_benchmarked_weight_plugged(
        &dispatchable.name,
        dispatchable.weight_attr.as_deref()
    ));
}

#[test]
fn collects_dispatchables_from_pallet_call_impl() {
    let input = r#"
        #[pallet::call]
        impl<T: Config> Pallet<T> {
            #[pallet::call_index(0)]
            pub fn set_weights(origin: OriginFor<T>) -> DispatchResult {
                Ok(())
            }

            fn helper() {}
        }
    "#;

    let dispatchables = collect_dispatchables_from_source(input);
    assert_eq!(dispatchables.len(), 1);
    assert_eq!(dispatchables[0].name, "set_weights");
}

#[test]
fn ignores_cfg_test_pallet_call_impls() {
    let input = r#"
        #[cfg(test)]
        mod tests {
            #[pallet::call]
            impl<T: Config> Pallet<T> {
                pub fn mock_only(origin: OriginFor<T>) -> DispatchResult {
                    Ok(())
                }
            }
        }

        #[pallet::call]
        impl<T: Config> Pallet<T> {
            pub fn real_call(origin: OriginFor<T>) -> DispatchResult {
                Ok(())
            }
        }
    "#;

    let dispatchables = collect_dispatchables_from_source(input);
    assert_eq!(dispatchables.len(), 1);
    assert_eq!(dispatchables[0].name, "real_call");
}

#[test]
fn ignores_cfg_test_dispatchable_fns_inside_real_call_impls() {
    let input = r#"
        #[pallet::call]
        impl<T: Config> Pallet<T> {
            #[cfg(test)]
            pub fn mock_only(origin: OriginFor<T>) -> DispatchResult {
                Ok(())
            }

            pub fn real_call(origin: OriginFor<T>) -> DispatchResult {
                Ok(())
            }
        }
    "#;

    let dispatchables = collect_dispatchables_from_source(input);
    assert_eq!(dispatchables.len(), 1);
    assert_eq!(dispatchables[0].name, "real_call");
}

#[test]
fn ignores_feature_gated_dispatchable_fns_inside_real_call_impls() {
    let input = r#"
        #[pallet::call]
        impl<T: Config> Pallet<T> {
            #[cfg(feature = "pow-faucet")]
            pub fn faucet(origin: OriginFor<T>) -> DispatchResult {
                Ok(())
            }

            pub fn real_call(origin: OriginFor<T>) -> DispatchResult {
                Ok(())
            }
        }
    "#;

    let dispatchables = collect_dispatchables_from_source(input);
    assert_eq!(dispatchables.len(), 1);
    assert_eq!(dispatchables[0].name, "real_call");
}

#[test]
fn recognizes_mock_and_test_paths_as_non_runtime() {
    assert!(is_test_or_mock_source_path(Path::new(
        "pallets/example/src/mock.rs"
    )));
    assert!(is_test_or_mock_source_path(Path::new(
        "pallets/example/src/tests/register.rs"
    )));
    assert!(is_test_or_mock_source_path(Path::new(
        "pallets/example/src/benchmarking.rs"
    )));
    assert!(!is_test_or_mock_source_path(Path::new(
        "pallets/example/src/macros/dispatches.rs"
    )));
}

#[test]
fn collects_frame_v2_benchmarks() {
    let input = r#"
        #[benchmarks]
        mod benchmarks {
            #[benchmark]
            fn set_weights() {
                #[block]
                {}
            }

            fn helper() {}
        }
    "#;

    let mut benchmarks = BTreeSet::new();
    collect_frame_v2_benchmarks(input, &mut benchmarks);
    assert!(benchmarks.contains("set_weights"));
    assert!(!benchmarks.contains("helper"));
}

#[test]
fn collects_legacy_benchmarks_macro_names() {
    let input = r#"
        benchmarks! {
            where_clause { where T: Config }

            set_weights {
                let caller = account("caller", 0, 0);
            }: _(RawOrigin::Signed(caller))
            verify {}
        }
    "#;

    let mut benchmarks = BTreeSet::new();
    collect_legacy_benchmarks(input, &mut benchmarks);
    assert!(benchmarks.contains("set_weights"));
    assert!(!benchmarks.contains("where_clause"));
    assert!(!benchmarks.contains("verify"));
}

#[test]
fn register_limit_is_missing_when_no_matching_benchmark_exists() {
    let dispatch_source = r#"
        #[pallet::call]
        impl<T: Config> Pallet<T> {
            #[pallet::call_index(134)]
            pub fn register_limit(origin: OriginFor<T>) -> DispatchResult { Ok(()) }
        }
    "#;
    let benchmark_source = r#"
        #[benchmarks]
        mod benchmarks {
            #[benchmark]
            fn root_register() { #[block] {} }
        }
    "#;

    let dispatchables = collect_dispatchables_from_source(dispatch_source);
    let mut benchmarks = BTreeSet::new();
    collect_frame_v2_benchmarks(benchmark_source, &mut benchmarks);

    assert_eq!(dispatchables[0].name, "register_limit");
    assert!(!benchmarks.contains(&dispatchables[0].name));
}
