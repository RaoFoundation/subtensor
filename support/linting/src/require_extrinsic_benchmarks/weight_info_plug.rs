//! Check that a dispatchable's `#[pallet::weight]` plugs generated `WeightInfo::<name>`.
//!
//! Also recognizes the custom allow marker `benchmarked_weight_not_plugged` (paired with
//! `unknown_lints`) used when a weight expression intentionally does not call WeightInfo.

pub(super) fn source_has_matching_weight_info_for_dispatchable(source: &str, name: &str) -> bool {
    // If weight_attr capture failed, fall back to the source text around the
    // dispatchable itself. We intentionally keep this as a fallback instead of
    // replacing the structured collection path: the lint is a source scanner
    // over FRAME macro input, and complex #[pallet::weight({ ... })] blocks can
    // confuse the backwards attribute walk even though the dispatch is valid.
    for needle in [format!("pub fn {name}"), format!("pub(crate) fn {name}")] {
        let mut search_from = 0usize;

        while let Some(offset) = source
            .get(search_from..)
            .and_then(|tail| tail.find(&needle))
        {
            let fn_pos = search_from.saturating_add(offset);
            let Some(prefix) = source.get(..fn_pos) else {
                break;
            };
            let Some(attr_start) = prefix.rfind("#[pallet::weight") else {
                search_from = fn_pos.saturating_add(needle.len());
                continue;
            };
            let Some(attr) = source.get(attr_start..fn_pos) else {
                search_from = fn_pos.saturating_add(needle.len());
                continue;
            };

            // If another dispatchable starts between that attr and this function,
            // the attr belongs to the earlier dispatchable, not this one.
            if attr.contains("pub fn ") || attr.contains("pub(crate) fn ") {
                search_from = fn_pos.saturating_add(needle.len());
                continue;
            }

            let normalized = normalize_attr(attr);
            if normalized.contains("benchmarked_weight_not_plugged")
                || weight_attr_calls_weight_info_for(name, attr)
            {
                return true;
            }

            search_from = fn_pos.saturating_add(needle.len());
        }
    }

    false
}

pub(super) const BENCHMARKED_WEIGHT_NOT_PLUGGED_ALLOW: &str = "benchmarked_weight_not_plugged";

pub(super) fn has_benchmark_weightinfo_plug_ignore_attr(weight_attr_cluster: &str) -> bool {
    let attr = normalize_attr(weight_attr_cluster);
    attr.contains("allow(") && attr.contains(BENCHMARKED_WEIGHT_NOT_PLUGGED_ALLOW)
}

pub(super) fn weight_attr_calls_weight_info_for(name: &str, weight_attr: &str) -> bool {
    let normalized = normalize_attr(weight_attr);
    if !normalized.contains("WeightInfo") {
        return false;
    }

    let mut search_from = 0usize;
    while let Some(relative_method_start) = normalized
        .get(search_from..)
        .and_then(|tail| tail.find(name))
    {
        let method_start = search_from + relative_method_start;

        // Method name must be reached through `::name`, not as part of another
        // identifier. This rejects `WeightInfo::swap_coldkey_announced()` for a
        // dispatchable named `swap_coldkey`.
        if method_start < 2 || normalized.get(method_start - 2..method_start) != Some("::") {
            search_from = method_start.saturating_add(name.len());
            continue;
        }

        let after_name = method_start.saturating_add(name.len());
        if !is_call_boundary_after_method(&normalized, after_name) {
            search_from = after_name;
            continue;
        }

        let before_method = &normalized[..method_start - 2];
        let Some(weight_info_start) = before_method.rfind("WeightInfo") else {
            search_from = after_name;
            continue;
        };
        let after_weight_info = weight_info_start.saturating_add("WeightInfo".len());
        let between = &before_method[after_weight_info..];

        // Accept:
        //   T::WeightInfo::foo(...)
        //   <T as Config>::WeightInfo::foo(...)
        //   ::WeightInfo::foo(...)
        //   WeightInfo::<T>::foo(...)
        if between.is_empty() || turbofish_suffix_consumes_all(between) {
            return true;
        }

        search_from = after_name;
    }

    false
}

pub(super) fn is_call_boundary_after_method(source: &str, after_name: usize) -> bool {
    match source.get(after_name..) {
        Some(rest) if rest.starts_with('(') => true,
        Some(rest) if rest.starts_with("::<") => skip_turbofish_generics(source, after_name)
            .and_then(|call_start| source.get(call_start..))
            .is_some_and(|rest| rest.starts_with('(')),
        _ => false,
    }
}

pub(super) fn turbofish_suffix_consumes_all(suffix: &str) -> bool {
    suffix.starts_with("::<")
        && skip_turbofish_generics(suffix, 0).is_some_and(|end| end == suffix.len())
}

pub(super) fn skip_turbofish_generics(source: &str, start: usize) -> Option<usize> {
    if !source.get(start..)?.starts_with("::<") {
        return None;
    }

    let bytes = source.as_bytes();
    let mut idx = start.checked_add(3)?;
    let mut angle_depth = 1usize;

    while let Some(byte) = bytes.get(idx).copied() {
        match byte {
            b'<' => angle_depth = angle_depth.saturating_add(1),
            b'>' => {
                angle_depth = angle_depth.saturating_sub(1);
                if angle_depth == 0 {
                    return idx.checked_add(1);
                }
            }
            _ => {}
        }
        idx = idx.checked_add(1)?;
    }

    None
}

pub(super) fn is_benchmarked_weight_plugged(name: &str, weight_attr: Option<&str>) -> bool {
    let Some(weight_attr) = weight_attr else {
        return false;
    };

    // This is our custom-lint allow marker. It intentionally uses an unknown
    // lint name plus `unknown_lints` so rustc accepts the attribute while this
    // source scanner can still recognize it.
    if normalize_attr(weight_attr).contains("benchmarked_weight_not_plugged") {
        return true;
    }

    has_benchmark_weightinfo_plug_ignore_attr(weight_attr)
        || weight_attr_calls_weight_info_for(name, weight_attr)
}

pub(super) fn normalize_attr(attr: &str) -> String {
    attr.chars().filter(|ch| !ch.is_whitespace()).collect()
}
