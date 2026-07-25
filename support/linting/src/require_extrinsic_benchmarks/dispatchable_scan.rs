//! Locate `#[pallet::call]` dispatchables and their `#[pallet::weight]` attribute clusters.
//!
//! Skips `#[cfg(test)]` / feature-gated call impls so mock-only extrinsics are not required
//! to have production benchmarks.

use super::Dispatchable;
use super::source_scan::*;

pub(super) fn collect_dispatchables_from_source(source: &str) -> Vec<Dispatchable> {
    let masked = mask_comments_and_strings(source);
    let non_runtime_ranges = collect_non_runtime_cfg_ranges(&masked);
    let mut dispatchables = Vec::new();
    let mut search_from = 0;

    while let Some((attr_start, attr_end)) = find_next_attr(&masked, search_from, "pallet::call") {
        search_from = attr_end;

        if is_in_ranges(attr_start, &non_runtime_ranges)
            || has_non_runtime_cfg_attr_before(&masked, attr_start, 0)
        {
            continue;
        }

        let Some(impl_pos) = find_word(&masked, "impl", attr_end) else {
            continue;
        };
        let Some(open_brace) = masked[impl_pos..].find('{').map(|offset| impl_pos + offset) else {
            continue;
        };
        let Some(close_brace) = find_matching_brace(&masked, open_brace) else {
            continue;
        };

        if is_in_ranges(impl_pos, &non_runtime_ranges) {
            search_from = close_brace + 1;
            continue;
        }

        collect_pub_fns_in_impl(
            source,
            &masked,
            open_brace + 1,
            close_brace,
            &non_runtime_ranges,
            &mut dispatchables,
        );
        search_from = close_brace + 1;
    }

    dispatchables
}

pub(super) fn collect_pub_fns_in_impl(
    source: &str,
    masked: &str,
    start: usize,
    end: usize,
    non_runtime_ranges: &[(usize, usize)],
    dispatchables: &mut Vec<Dispatchable>,
) {
    let bytes = masked.as_bytes();
    let mut idx = start;
    let mut depth = 0usize;

    while idx < end {
        match bytes[idx] {
            b'{' => {
                depth = depth.saturating_add(1);
                idx += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                idx += 1;
            }
            _ if depth == 0 && starts_with_word(masked, idx, "pub") => {
                if is_in_ranges(idx, non_runtime_ranges)
                    || has_non_runtime_cfg_attr_before(masked, idx, start)
                {
                    idx += 3;
                    continue;
                }

                let mut cursor = skip_ws(masked, idx + 3);

                // Support `pub(crate) fn` even though FRAME dispatchables are normally `pub fn`.
                if masked.as_bytes().get(cursor) == Some(&b'(') {
                    if let Some(close) = find_matching_paren(masked, cursor) {
                        cursor = skip_ws(masked, close + 1);
                    }
                }

                if starts_with_word(masked, cursor, "fn") {
                    cursor = skip_ws(masked, cursor + 2);
                    if let Some((name, _name_end)) = parse_ident(masked, cursor) {
                        let (line, column) = line_column(source, cursor);
                        let weight_attr = preceding_weight_attr(source, masked, idx, start);
                        dispatchables.push(Dispatchable {
                            name,
                            line,
                            column,
                            weight_attr,
                        });
                    }
                }

                idx += 3;
            }
            _ => idx += 1,
        }
    }
}

pub(super) fn preceding_weight_attr(
    source: &str,
    masked: &str,
    item_start: usize,
    scope_start: usize,
) -> Option<String> {
    // `item_start` is the beginning of the dispatchable item as found by the
    // source scanner, normally the `pub` in `pub fn`. Find the nearest
    // #[pallet::weight(...)] before that item, then expand backward over the
    // contiguous attribute cluster that belongs to the same dispatchable.
    let prefix = masked.get(scope_start..item_start)?;
    let attr_start = prefix.rfind("#[pallet::weight")? + scope_start;

    // Do not accidentally reuse a previous dispatchable's weight attr when the
    // current item has no weight. If another function begins between the attr
    // and this item, this attr is not for the current dispatchable.
    let attr_to_item = masked.get(attr_start..item_start)?;
    if attr_to_item.contains("pub fn ") || attr_to_item.contains("pub(crate) fn ") {
        return None;
    }

    let mut cluster_start = attr_start;
    let mut cursor = attr_start;
    while let Some(trimmed_end) = rtrim_ws(masked, scope_start, cursor) {
        if masked.as_bytes().get(trimmed_end) != Some(&b']') {
            break;
        }
        let Some(prev_attr_start) = masked
            .get(scope_start..=trimmed_end)
            .and_then(|section| section.rfind("#["))
        else {
            break;
        };
        cluster_start = scope_start + prev_attr_start;
        cursor = cluster_start;
    }

    source.get(cluster_start..item_start).map(ToOwned::to_owned)
}

pub(super) fn find_next_attr(masked: &str, from: usize, attr_path: &str) -> Option<(usize, usize)> {
    let mut search_from = from;
    while let Some(offset) = masked[search_from..].find("#[") {
        let start = search_from + offset;
        let Some(close) = masked[start..].find(']').map(|offset| start + offset) else {
            return None;
        };
        let normalized: String = masked[start..=close]
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        let expected = format!("#[{attr_path}");

        if normalized.starts_with(&expected)
            && matches!(
                normalized.as_bytes().get(expected.len()),
                Some(b']') | Some(b'(')
            )
        {
            return Some((start, close + 1));
        }

        search_from = close + 1;
    }

    None
}

pub(super) fn collect_non_runtime_cfg_ranges(masked: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut search_from = 0;

    while let Some((attr_start, attr_end)) = find_next_attr(masked, search_from, "cfg") {
        search_from = attr_end;

        if !is_non_runtime_cfg_attr(&masked[attr_start..attr_end]) {
            continue;
        }

        let item_start = skip_outer_attrs(masked, skip_ws(masked, attr_end));
        let Some(open_brace) = find_item_open_brace(masked, item_start) else {
            ranges.push((attr_start, end_of_line(masked, attr_end)));
            continue;
        };
        let Some(close_brace) = find_matching_brace(masked, open_brace) else {
            ranges.push((attr_start, end_of_line(masked, attr_end)));
            continue;
        };

        ranges.push((attr_start, close_brace + 1));
        search_from = close_brace + 1;
    }

    ranges
}

pub(super) fn has_non_runtime_cfg_attr_before(
    masked: &str,
    item_start: usize,
    scope_start: usize,
) -> bool {
    let mut cursor = item_start;

    loop {
        let Some(trimmed_end) = rtrim_ws(masked, scope_start, cursor) else {
            return false;
        };
        if masked.as_bytes().get(trimmed_end) != Some(&b']') {
            return false;
        }

        let Some(attr_start) = masked[scope_start..=trimmed_end].rfind("#[") else {
            return false;
        };
        let attr_start = scope_start + attr_start;
        let attr = &masked[attr_start..=trimmed_end];
        if is_non_runtime_cfg_attr(attr) {
            return true;
        }

        cursor = attr_start;
    }
}

pub(super) fn is_non_runtime_cfg_attr(attr: &str) -> bool {
    let normalized: String = attr.chars().filter(|ch| !ch.is_whitespace()).collect();
    let Some(cfg) = normalized
        .strip_prefix("#[cfg(")
        .and_then(|value| value.strip_suffix(")]"))
    else {
        return false;
    };

    cfg == "test"
        || cfg.contains("feature=")
        || cfg.starts_with("all(test,")
        || cfg.starts_with("any(test,")
        || cfg.contains(",test,")
        || cfg.contains(",test)")
}

pub(super) fn skip_outer_attrs(masked: &str, mut idx: usize) -> usize {
    loop {
        idx = skip_ws(masked, idx);
        if !masked[idx..].starts_with("#[") {
            return idx;
        }
        let Some(close) = masked[idx..].find(']').map(|offset| idx + offset) else {
            return idx;
        };
        idx = close + 1;
    }
}

pub(super) fn find_item_open_brace(masked: &str, item_start: usize) -> Option<usize> {
    let bytes = masked.as_bytes();
    let mut idx = item_start;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;

    while idx < bytes.len() {
        match bytes[idx] {
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'<' => angle_depth += 1,
            b'>' => angle_depth = angle_depth.saturating_sub(1),
            b';' if paren_depth == 0 && bracket_depth == 0 && angle_depth == 0 => return None,
            b'{' if paren_depth == 0 && bracket_depth == 0 && angle_depth == 0 => return Some(idx),
            _ => {}
        }
        idx += 1;
    }

    None
}

pub(super) fn is_in_ranges(idx: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| idx >= *start && idx < *end)
}
