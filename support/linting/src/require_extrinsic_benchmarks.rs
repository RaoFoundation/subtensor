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
        collect_rust_files(&pallets_dir, &mut rust_files);

        let mut errors = Vec::new();
        for file in rust_files {
            let Ok(source) = fs::read_to_string(&file) else {
                continue;
            };

            if !contains_pallet_call_attr(&source) {
                continue;
            }

            let dispatchables = collect_dispatchables_from_source(&source);
            if dispatchables.is_empty() {
                continue;
            }

            let pallet_root = find_pallet_root(&file, workspace_root);
            let benchmarks = collect_benchmarks_for_pallet(&pallet_root);
            let benchmark_hint = benchmark_location_hint(&pallet_root, workspace_root);
            let file_path = display_path(&file, workspace_root);

            for dispatchable in dispatchables {
                if benchmarks.contains(&dispatchable.name) || dispatchable.name.starts_with('_') {
                    continue;
                }

                errors.push(format!(
                    "{}:{}:{}: dispatchable extrinsic `{}` is missing a matching benchmark; add `#[benchmark] fn {}(...)` to {}",
                    file_path,
                    dispatchable.line,
                    dispatchable.column,
                    dispatchable.name,
                    dispatchable.name,
                    benchmark_hint,
                ));
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
}

fn collect_dispatchables_from_source(source: &str) -> Vec<Dispatchable> {
    let masked = mask_comments_and_strings(source);
    let mut dispatchables = Vec::new();
    let mut search_from = 0;

    while let Some((_attr_start, attr_end)) = find_next_attr(&masked, search_from, "pallet::call") {
        search_from = attr_end;

        let Some(impl_pos) = find_word(&masked, "impl", attr_end) else {
            continue;
        };
        let Some(open_brace) = masked[impl_pos..].find('{').map(|offset| impl_pos + offset) else {
            continue;
        };
        let Some(close_brace) = find_matching_brace(&masked, open_brace) else {
            continue;
        };

        collect_pub_fns_in_impl(
            source,
            &masked,
            open_brace + 1,
            close_brace,
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
                        dispatchables.push(Dispatchable { name, line, column });
                    }
                }

                idx += 3;
            }
            _ => idx += 1,
        }
    }
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
                collect_legacy_benchmarks_from_tokens(&group.stream(), benchmarks)
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

fn contains_pallet_call_attr(source: &str) -> bool {
    find_next_attr(&mask_comments_and_strings(source), 0, "pallet::call").is_some()
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
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

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
