use super::*;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};
use syn::{Attribute, File, ImplItemFn, ItemFn, ItemImpl, ItemMacro, Visibility, visit::Visit};

pub struct RequireExtrinsicBenchmarks;

impl Lint for RequireExtrinsicBenchmarks {
    fn lint(_source: &File) -> Result {
        // This is a workspace-level lint because dispatchables and benchmarks live
        // in separate files. build.rs calls `lint_workspace` once with the
        // workspace root instead of running this per source file.
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
        let mut benchmark_cache: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();

        for file in rust_files {
            let Ok(source) = fs::read_to_string(&file) else {
                continue;
            };

            if !source.contains("pallet::call") && !source.contains("pallet :: call") {
                continue;
            }

            let Ok(tokens) = TokenStream::from_str(&source) else {
                continue;
            };
            let Ok(parsed) = syn::parse2::<File>(tokens) else {
                continue;
            };

            let dispatchables = collect_dispatchables(&parsed);
            if dispatchables.is_empty() {
                continue;
            }

            let pallet_root = find_pallet_root(&file, workspace_root);
            let benchmarks = benchmark_cache
                .entry(pallet_root.clone())
                .or_insert_with(|| collect_benchmarks_for_pallet(&pallet_root));
            let benchmark_hint = benchmark_location_hint(&pallet_root, workspace_root);
            let file_path = display_path(&file, workspace_root);

            for dispatchable in dispatchables {
                if benchmarks.contains(&dispatchable.name) {
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

#[derive(Debug)]
struct Dispatchable {
    name: String,
    line: usize,
    column: usize,
}

fn collect_dispatchables(source: &File) -> Vec<Dispatchable> {
    let mut visitor = DispatchableVisitor::default();
    visitor.visit_file(source);
    visitor.dispatchables
}

#[derive(Default)]
struct DispatchableVisitor {
    in_pallet_call_impl: bool,
    dispatchables: Vec<Dispatchable>,
}

impl<'ast> Visit<'ast> for DispatchableVisitor {
    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let previous = self.in_pallet_call_impl;
        if node.attrs.iter().any(is_pallet_call_attr) {
            self.in_pallet_call_impl = true;
        }

        syn::visit::visit_item_impl(self, node);
        self.in_pallet_call_impl = previous;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        if self.in_pallet_call_impl
            && matches!(node.vis, Visibility::Public(_))
            && !is_allowed(&node.attrs)
        {
            let loc = node.sig.ident.span().start();
            self.dispatchables.push(Dispatchable {
                name: node.sig.ident.to_string(),
                line: loc.line,
                column: loc.column,
            });
        }

        syn::visit::visit_impl_item_fn(self, node);
    }
}

fn collect_benchmarks_for_pallet(pallet_root: &Path) -> BTreeSet<String> {
    let mut rust_files = Vec::new();
    collect_rust_files(&pallet_root.join("src"), &mut rust_files);

    let mut benchmarks = BTreeSet::new();
    for file in rust_files {
        let path = file.to_string_lossy();
        let file_name = file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !file_name.contains("benchmark") && !path.contains("/benchmarks/") {
            continue;
        }

        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        let Ok(tokens) = TokenStream::from_str(&source) else {
            continue;
        };
        let Ok(parsed) = syn::parse2::<File>(tokens) else {
            continue;
        };

        let mut visitor = BenchmarkVisitor::default();
        visitor.visit_file(&parsed);
        benchmarks.extend(visitor.benchmarks);
    }

    benchmarks
}

#[derive(Default)]
struct BenchmarkVisitor {
    benchmarks: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for BenchmarkVisitor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if node.attrs.iter().any(is_benchmark_attr) {
            self.benchmarks.insert(node.sig.ident.to_string());
        }

        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        if node.attrs.iter().any(is_benchmark_attr) {
            self.benchmarks.insert(node.sig.ident.to_string());
        }

        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_item_macro(&mut self, node: &'ast ItemMacro) {
        if node
            .mac
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "benchmarks")
        {
            collect_legacy_benchmarks(&node.mac.tokens, &mut self.benchmarks);
        }

        syn::visit::visit_item_macro(self, node);
    }
}

fn collect_legacy_benchmarks(tokens: &TokenStream, benchmarks: &mut BTreeSet<String>) {
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

fn is_pallet_call_attr(attribute: &Attribute) -> bool {
    let has_pallet = attribute
        .path()
        .segments
        .iter()
        .any(|segment| segment.ident == "pallet");
    let is_call = attribute
        .path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "call");

    has_pallet && is_call
}

fn is_benchmark_attr(attribute: &Attribute) -> bool {
    attribute
        .path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "benchmark")
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
    let common_locations = [
        pallet_root.join("src/benchmarks.rs"),
        pallet_root.join("src/benchmarking.rs"),
    ];

    for location in common_locations {
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
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "target")
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
    use quote::quote;

    fn parse(input: proc_macro2::TokenStream) -> File {
        syn::parse2(input).unwrap()
    }

    #[test]
    fn collects_dispatchables_from_pallet_call_impl() {
        let input = parse(quote! {
            #[pallet::call]
            impl<T: Config> Pallet<T> {
                #[pallet::call_index(0)]
                pub fn set_weights(origin: OriginFor<T>) -> DispatchResult {
                    Ok(())
                }

                fn helper() {}
            }
        });

        let dispatchables = collect_dispatchables(&input);
        assert_eq!(dispatchables.len(), 1);
        assert_eq!(dispatchables[0].name, "set_weights");
    }

    #[test]
    fn collects_frame_v2_benchmarks() {
        let input = parse(quote! {
            #[benchmarks]
            mod benchmarks {
                #[benchmark]
                fn set_weights() {
                    #[block]
                    {}
                }

                fn helper() {}
            }
        });

        let mut visitor = BenchmarkVisitor::default();
        visitor.visit_file(&input);
        assert!(visitor.benchmarks.contains("set_weights"));
        assert!(!visitor.benchmarks.contains("helper"));
    }

    #[test]
    fn collects_legacy_benchmarks_macro_names() {
        let input = parse(quote! {
            benchmarks! {
                where_clause { where T: Config }

                set_weights {
                    let caller = account("caller", 0, 0);
                }: _(RawOrigin::Signed(caller))
                verify {}
            }
        });

        let mut visitor = BenchmarkVisitor::default();
        visitor.visit_file(&input);
        assert!(visitor.benchmarks.contains("set_weights"));
        assert!(!visitor.benchmarks.contains("where_clause"));
        assert!(!visitor.benchmarks.contains("verify"));
    }
}
