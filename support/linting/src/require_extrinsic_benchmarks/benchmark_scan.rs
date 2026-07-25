//! Collect benchmark function names from FRAME v2 `#[benchmark]` and legacy `benchmarks!`.

use super::dispatchable_scan::find_next_attr;
use super::pallet_paths::{collect_rust_files, is_benchmark_file};
use super::source_scan::*;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::str::FromStr;

pub(super) fn collect_benchmarks_for_pallet(pallet_root: &Path) -> BTreeSet<String> {
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

pub(super) fn collect_frame_v2_benchmarks(source: &str, benchmarks: &mut BTreeSet<String>) {
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

pub(super) fn collect_legacy_benchmarks(source: &str, benchmarks: &mut BTreeSet<String>) {
    let Ok(tokens) = TokenStream::from_str(source) else {
        return;
    };
    collect_legacy_benchmarks_from_tokens(&tokens, benchmarks);
}

pub(super) fn collect_legacy_benchmarks_from_tokens(
    tokens: &TokenStream,
    benchmarks: &mut BTreeSet<String>,
) {
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

pub(super) fn collect_legacy_benchmark_names(
    tokens: &TokenStream,
    benchmarks: &mut BTreeSet<String>,
) {
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
