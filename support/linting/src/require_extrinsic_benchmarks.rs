#![allow(
    clippy::arithmetic_side_effects,
    clippy::collapsible_if,
    clippy::indexing_slicing,
    clippy::question_mark
)]

use super::*;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};
use syn::File;

pub struct RequireExtrinsicBenchmarks;

impl Lint for RequireExtrinsicBenchmarks {
    fn lint(_source: &File) -> Result {
        // Dispatchables and benchmarks live in different files, so build.rs runs
        // the real check once at workspace scope via `lint_workspace`.
        Ok(())
    }
}

impl RequireExtrinsicBenchmarks {
    pub fn lint_workspace(workspace_root: &Path) -> Vec<String> {
        let pallets_dir = workspace_root.join("pallets");
        if !pallets_dir.is_dir() {
            return Vec::new();
        }

        let mut rust_files = Vec::new();
        collect_runtime_rust_files(&pallets_dir, &mut rust_files);

        let mut errors = Vec::new();
        for file in rust_files {
            let Ok(source) = fs::read_to_string(&file) else {
                continue;
            };

            let dispatchables = collect_dispatchables_from_source(&source);
            if dispatchables.is_empty() {
                continue;
            }

            let pallet_root = find_pallet_root(&file, workspace_root);
            let benchmarks = collect_benchmarks_for_pallet(&pallet_root);
            let benchmark_hint = benchmark_location_hint(&pallet_root, workspace_root);
            let file_path = display_path(&file, workspace_root);

            for dispatchable in dispatchables {
                if dispatchable.name.starts_with('_') {
                    continue;
                }

                if !benchmarks.contains(&dispatchable.name) {
                    errors.push(format!(
                        "{}:{}:{}: dispatchable extrinsic `{}` is missing a matching benchmark; add `#[benchmark] fn {}(...)` to {}",
                        file_path,
                        dispatchable.line,
                        dispatchable.column,
                        dispatchable.name,
                        dispatchable.name,
                        benchmark_hint,
                    ));
                    continue;
                }

                let uses_matching_weight_info = is_benchmarked_weight_plugged(
                    &dispatchable.name,
                    dispatchable.weight_attr.as_deref(),
                )
                    || source_has_matching_weight_info_for_dispatchable(
                        &source,
                        &dispatchable.name,
                    );

                if !uses_matching_weight_info {
                    errors.push(format!(
                        "{}:{}:{}: dispatchable extrinsic `{}` has a matching benchmark but its #[pallet::weight] does not call WeightInfo::{}(...); plug the generated benchmark weight into the dispatch annotation",
                        file_path,
                        dispatchable.line,
                        dispatchable.column,
                        dispatchable.name,
                        dispatchable.name,
                    ));
                }
            }
        }

        errors
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct Dispatchable {
    name: String,
    line: usize,
    column: usize,
    weight_attr: Option<String>,
}

fn source_has_matching_weight_info_for_dispatchable(source: &str, name: &str) -> bool {
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

fn collect_dispatchables_from_source(source: &str) -> Vec<Dispatchable> {
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

fn collect_pub_fns_in_impl(
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

fn preceding_weight_attr(
    source: &str,
    masked: &str,
    item_start: usize,
    scope_start: usize,
) -> Option<String> {
    let mut cursor = item_start;
    let mut cluster_start = item_start;
    let mut saw_weight_attr = false;

    loop {
        let trimmed_end = rtrim_ws(masked, scope_start, cursor)?;
        if masked.as_bytes().get(trimmed_end) != Some(&b']') {
            break;
        }

        let attr_start = masked[scope_start..=trimmed_end].rfind("#[")? + scope_start;
        let attr = &masked[attr_start..=trimmed_end];
        let normalized = normalize_attr(attr);
        if normalized.starts_with("#[pallet::weight") {
            saw_weight_attr = true;
        }

        cluster_start = attr_start;
        cursor = attr_start;
    }

    if saw_weight_attr {
        return source.get(cluster_start..item_start).map(ToOwned::to_owned);
    }

    // Fallback for complex or macro-compacted sections: find the nearest weight
    // attribute in the current #[pallet::call] impl before the dispatchable. The
    // strict backwards attribute walk above can miss valid weights when an
    // intermediate scanner position points at `fn` instead of the full `pub fn`
    // item start. This fallback still only returns a pallet weight attribute from
    // the same impl scope, so it does not confuse nearby benchmark functions or
    // test helpers with dispatchable weights.
    let prefix = masked.get(scope_start..item_start)?;
    let attr_start = prefix.rfind("#[pallet::weight")? + scope_start;
    source.get(attr_start..item_start).map(ToOwned::to_owned)
}

const BENCHMARKED_WEIGHT_NOT_PLUGGED_ALLOW: &str = "benchmarked_weight_not_plugged";

fn has_benchmark_weightinfo_plug_ignore_attr(weight_attr_cluster: &str) -> bool {
    let attr = normalize_attr(weight_attr_cluster);
    attr.contains("allow(") && attr.contains(BENCHMARKED_WEIGHT_NOT_PLUGGED_ALLOW)
}

fn weight_attr_calls_weight_info_for(name: &str, weight_attr: &str) -> bool {
    let normalized = normalize_attr(weight_attr);
    if !normalized.contains("WeightInfo") {
        return false;
    }

    // Accept only exact WeightInfo method calls. A dispatchable named
    // `swap_coldkey` must not be considered plugged by
    // `WeightInfo::swap_coldkey_announced()`. After the method name, require a
    // call boundary: either `(` for a normal call or `::<` for turbofish.
    normalized.split("WeightInfo").skip(1).any(|suffix| {
        [format!("::{name}"), format!("::<T>::{name}")]
            .iter()
            .any(|needle| {
                suffix
                    .split(needle.as_str())
                    .skip(1)
                    .any(|rest| rest.starts_with('(') || rest.starts_with("::<"))
            })
    })
}

fn is_benchmarked_weight_plugged(name: &str, weight_attr: Option<&str>) -> bool {
    let Some(weight_attr) = weight_attr else {
        return false;
    };

    if has_benchmark_weightinfo_plug_ignore_attr(weight_attr) {
        return true;
    }

    weight_attr_calls_weight_info_for(name, weight_attr)
}

fn normalize_attr(attr: &str) -> String {
    attr.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn collect_benchmarks_for_pallet(pallet_root: &Path) -> BTreeSet<String> {
    let mut rust_files = Vec::new();
    collect_rust_files(&pallet_root.join("src"), &mut rust_files);

    let mut benchmarks = BTreeSet::new();
    for file in rust_files {
        if !is_benchmark_file(&file) {
            continue;
        }

        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        collect_frame_v2_benchmarks(&source, &mut benchmarks);
        collect_legacy_benchmarks(&source, &mut benchmarks);
    }

    benchmarks
}

fn collect_frame_v2_benchmarks(source: &str, benchmarks: &mut BTreeSet<String>) {
    let masked = mask_comments_and_strings(source);
    let mut search_from = 0;

    while let Some((_attr_start, attr_end)) = find_next_attr(&masked, search_from, "benchmark") {
        search_from = attr_end;
        let Some(fn_pos) = find_word(&masked, "fn", attr_end) else {
            continue;
        };
        let name_start = skip_ws(&masked, fn_pos + 2);
        if let Some((name, _name_end)) = parse_ident(&masked, name_start) {
            benchmarks.insert(name);
        }
    }
}

fn collect_legacy_benchmarks(source: &str, benchmarks: &mut BTreeSet<String>) {
    let Ok(tokens) = TokenStream::from_str(source) else {
        return;
    };
    collect_legacy_benchmarks_from_tokens(&tokens, benchmarks);
}

fn collect_legacy_benchmarks_from_tokens(tokens: &TokenStream, benchmarks: &mut BTreeSet<String>) {
    let tokens: Vec<_> = tokens.clone().into_iter().collect();
    let mut idx = 0;

    while idx < tokens.len() {
        match &tokens[idx] {
            TokenTree::Ident(ident) if ident == "benchmarks" => {
                if matches!(tokens.get(idx + 1), Some(TokenTree::Punct(punct)) if punct.as_char() == '!')
                {
                    if let Some(TokenTree::Group(group)) = tokens.get(idx + 2) {
                        if group.delimiter() == Delimiter::Brace {
                            collect_legacy_benchmark_names(&group.stream(), benchmarks);
                            idx += 3;
                            continue;
                        }
                    }
                }
            }
            TokenTree::Group(group) => {
                collect_legacy_benchmarks_from_tokens(&group.stream(), benchmarks);
            }
            _ => {}
        }

        idx += 1;
    }
}

fn collect_legacy_benchmark_names(tokens: &TokenStream, benchmarks: &mut BTreeSet<String>) {
    let tokens: Vec<_> = tokens.clone().into_iter().collect();
    let mut idx = 0;

    while idx < tokens.len() {
        let TokenTree::Ident(ident) = &tokens[idx] else {
            idx += 1;
            continue;
        };

        let name = ident.to_string();
        if matches!(
            name.as_str(),
            "where_clause" | "verify" | "impl_benchmark_test_suite"
        ) {
            idx += 1;
            continue;
        }

        let mut lookahead = idx + 1;
        if matches!(
            tokens.get(lookahead),
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis
        ) {
            lookahead += 1;
        }

        if matches!(
            tokens.get(lookahead),
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace
        ) {
            benchmarks.insert(name);
        }

        idx += 1;
    }
}

fn find_next_attr(masked: &str, from: usize, attr_path: &str) -> Option<(usize, usize)> {
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

fn collect_non_runtime_cfg_ranges(masked: &str) -> Vec<(usize, usize)> {
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

fn has_non_runtime_cfg_attr_before(masked: &str, item_start: usize, scope_start: usize) -> bool {
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

fn is_non_runtime_cfg_attr(attr: &str) -> bool {
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

fn skip_outer_attrs(masked: &str, mut idx: usize) -> usize {
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

fn find_item_open_brace(masked: &str, item_start: usize) -> Option<usize> {
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

fn is_in_ranges(idx: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| idx >= *start && idx < *end)
}

fn mask_comments_and_strings(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut idx = 0;

    while idx < bytes.len() {
        match (bytes[idx], bytes.get(idx + 1).copied()) {
            (b'/', Some(b'/')) => {
                out.push(' ');
                out.push(' ');
                idx += 2;
                while idx < bytes.len() && bytes[idx] != b'\n' {
                    out.push(' ');
                    idx += 1;
                }
            }
            (b'/', Some(b'*')) => {
                out.push(' ');
                out.push(' ');
                idx += 2;
                let mut depth = 1usize;
                while idx < bytes.len() && depth > 0 {
                    if bytes[idx] == b'\n' {
                        out.push('\n');
                        idx += 1;
                    } else if bytes[idx] == b'/' && bytes.get(idx + 1) == Some(&b'*') {
                        out.push(' ');
                        out.push(' ');
                        idx += 2;
                        depth += 1;
                    } else if bytes[idx] == b'*' && bytes.get(idx + 1) == Some(&b'/') {
                        out.push(' ');
                        out.push(' ');
                        idx += 2;
                        depth -= 1;
                    } else {
                        out.push(' ');
                        idx += 1;
                    }
                }
            }
            (b'"', _) => {
                out.push(' ');
                idx += 1;
                let mut escaped = false;
                while idx < bytes.len() {
                    let byte = bytes[idx];
                    if byte == b'\n' {
                        out.push('\n');
                        idx += 1;
                        break;
                    }
                    out.push(' ');
                    idx += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'"' {
                        break;
                    }
                }
            }
            (b'\'', _) if !bytes.get(idx + 1).is_some_and(|byte| is_ident_start(*byte)) => {
                out.push(' ');
                idx += 1;
                let mut escaped = false;
                while idx < bytes.len() {
                    let byte = bytes[idx];
                    if byte == b'\n' {
                        out.push('\n');
                        idx += 1;
                        break;
                    }
                    out.push(' ');
                    idx += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'\'' {
                        break;
                    }
                }
            }
            (byte, _) => {
                out.push(byte as char);
                idx += 1;
            }
        }
    }

    out
}

fn find_matching_brace(masked: &str, open_brace: usize) -> Option<usize> {
    find_matching_delimiter(masked, open_brace, b'{', b'}')
}

fn find_matching_paren(masked: &str, open_paren: usize) -> Option<usize> {
    find_matching_delimiter(masked, open_paren, b'(', b')')
}

fn find_matching_delimiter(
    masked: &str,
    open: usize,
    open_byte: u8,
    close_byte: u8,
) -> Option<usize> {
    let bytes = masked.as_bytes();
    if bytes.get(open) != Some(&open_byte) {
        return None;
    }

    let mut depth = 0usize;
    for (idx, byte) in bytes.iter().enumerate().skip(open) {
        if *byte == open_byte {
            depth += 1;
        } else if *byte == close_byte {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(idx);
            }
        }
    }

    None
}

fn find_word(masked: &str, word: &str, from: usize) -> Option<usize> {
    let mut search_from = from;
    while let Some(offset) = masked[search_from..].find(word) {
        let idx = search_from + offset;
        if starts_with_word(masked, idx, word) {
            return Some(idx);
        }
        search_from = idx + word.len();
    }

    None
}

fn starts_with_word(masked: &str, idx: usize, word: &str) -> bool {
    let bytes = masked.as_bytes();
    let word_bytes = word.as_bytes();

    if bytes.get(idx..idx + word_bytes.len()) != Some(word_bytes) {
        return false;
    }

    let before_ok = idx == 0 || !is_ident_continue(bytes[idx - 1]);
    let after_idx = idx + word_bytes.len();
    let after_ok = after_idx >= bytes.len() || !is_ident_continue(bytes[after_idx]);
    before_ok && after_ok
}

fn parse_ident(masked: &str, start: usize) -> Option<(String, usize)> {
    let bytes = masked.as_bytes();
    let first = *bytes.get(start)?;
    if !is_ident_start(first) {
        return None;
    }

    let mut end = start + 1;
    while bytes.get(end).is_some_and(|byte| is_ident_continue(*byte)) {
        end += 1;
    }

    Some((masked[start..end].to_owned(), end))
}

fn skip_ws(masked: &str, mut idx: usize) -> usize {
    let bytes = masked.as_bytes();
    while bytes
        .get(idx)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        idx += 1;
    }
    idx
}

fn rtrim_ws(masked: &str, start: usize, mut end: usize) -> Option<usize> {
    let bytes = masked.as_bytes();
    while end > start
        && bytes
            .get(end - 1)
            .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        end -= 1;
    }
    end.checked_sub(1).filter(|idx| *idx >= start)
}

fn end_of_line(masked: &str, from: usize) -> usize {
    masked[from..]
        .find('\n')
        .map(|offset| from + offset)
        .unwrap_or(masked.len())
}

fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

fn line_column(source: &str, idx: usize) -> (usize, usize) {
    let line = source[..idx].bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = source[..idx]
        .rfind('\n')
        .map(|line_start| idx - line_start)
        .unwrap_or(idx + 1);
    (line, column)
}

fn is_benchmark_file(file: &Path) -> bool {
    file.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains("benchmark"))
        || file
            .components()
            .any(|component| component.as_os_str() == "benchmarks")
}

fn is_test_or_mock_source_path(path: &Path) -> bool {
    path.components().any(|component| {
        let Some(raw_name) = component.as_os_str().to_str() else {
            return false;
        };
        let name = raw_name.to_ascii_lowercase();
        let stem = Path::new(&name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(&name);

        matches!(
            stem,
            "benchmark"
                | "benchmarks"
                | "benchmarking"
                | "mock"
                | "mocks"
                | "test"
                | "tests"
                | "testing"
                | "test_utils"
                | "test_util"
                | "test_helpers"
                | "tests_helpers"
        ) || stem.starts_with("mock_")
            || stem.ends_with("_mock")
            || stem.starts_with("test_")
            || stem.ends_with("_test")
            || stem.ends_with("_tests")
    })
}

fn find_pallet_root(file: &Path, workspace_root: &Path) -> PathBuf {
    let pallets_dir = workspace_root.join("pallets");
    let mut current = file.parent();

    while let Some(dir) = current {
        if dir.starts_with(&pallets_dir) && dir.join("Cargo.toml").is_file() {
            return dir.to_path_buf();
        }

        if dir == workspace_root {
            break;
        }

        current = dir.parent();
    }

    file.parent().unwrap_or(workspace_root).to_path_buf()
}

fn benchmark_location_hint(pallet_root: &Path, workspace_root: &Path) -> String {
    for location in [
        pallet_root.join("src/benchmarks.rs"),
        pallet_root.join("src/benchmarking.rs"),
    ] {
        if location.exists() {
            return display_path(&location, workspace_root);
        }
    }

    display_path(&pallet_root.join("src/benchmarks.rs"), workspace_root)
}

fn collect_runtime_rust_files(dir: &Path, rust_files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .components()
            .any(|component| component.as_os_str() == "target" || component.as_os_str() == ".git")
        {
            continue;
        }

        if is_test_or_mock_source_path(&path) {
            continue;
        }

        if path.is_dir() {
            collect_runtime_rust_files(&path, rust_files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            rust_files.push(path);
        }
    }
}

fn collect_rust_files(dir: &Path, rust_files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .components()
            .any(|component| component.as_os_str() == "target" || component.as_os_str() == ".git")
        {
            continue;
        }

        if path.is_dir() {
            collect_rust_files(&path, rust_files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            rust_files.push(path);
        }
    }
}

fn display_path(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {

    #[test]
    fn weight_info_matcher_requires_complete_method_name() {
        assert!(weight_attr_calls_weight_info_for(
            "swap_coldkey",
            "#[pallet::weight(T::WeightInfo::swap_coldkey())]",
        ));
        assert!(weight_attr_calls_weight_info_for(
            "swap_coldkey",
            "#[pallet::weight(T::WeightInfo::swap_coldkey::<T>())]",
        ));
        assert!(weight_attr_calls_weight_info_for(
            "proxy",
            "#[pallet::weight(WeightInfo::<T>::proxy(p.into()))]",
        ));
        assert!(weight_attr_calls_weight_info_for(
            "set_weights",
            "#[pallet::weight(::WeightInfo::set_weights())]",
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

    use super::*;

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
}
