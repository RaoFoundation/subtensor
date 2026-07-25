#![allow(
    clippy::arithmetic_side_effects,
    clippy::collapsible_if,
    clippy::indexing_slicing,
    clippy::question_mark
)]

//! Ensure every runtime dispatchable has a matching benchmark and plugs `WeightInfo`.
//!
//! `Lint::lint` is a no-op per file: dispatchables and benchmarks live in different paths, so
//! [`RequireExtrinsicBenchmarks::lint_workspace`] (invoked from workspace `build.rs`) walks
//! `pallets/` once and reports missing benchmarks or unplugged weights.

use super::*;
use std::fs;
use std::path::Path;
use syn::File;

mod benchmark_scan;
mod dispatchable_scan;
mod pallet_paths;
mod source_scan;
mod weight_info_plug;

use benchmark_scan::collect_benchmarks_for_pallet;
use dispatchable_scan::collect_dispatchables_from_source;
use pallet_paths::{
    benchmark_location_hint, collect_runtime_rust_files, display_path, find_pallet_root,
};
use weight_info_plug::{
    is_benchmarked_weight_plugged, source_has_matching_weight_info_for_dispatchable,
};

/// Workspace lint: every non-`_` dispatchable must have a same-named benchmark and WeightInfo plug.
pub struct RequireExtrinsicBenchmarks;

impl Lint for RequireExtrinsicBenchmarks {
    fn lint(_source: &File) -> Result {
        // Dispatchables and benchmarks live in different files, so build.rs runs
        // the real check once at workspace scope via `lint_workspace`.
        Ok(())
    }
}

impl RequireExtrinsicBenchmarks {
    /// Scan all runtime pallet sources under `workspace_root/pallets` for unpaired extrinsics.
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

/// One `pub fn` / `pub(crate) fn` found inside a `#[pallet::call]` impl.
#[derive(Debug, Clone, Eq, PartialEq)]
struct Dispatchable {
    name: String,
    line: usize,
    column: usize,
    weight_attr: Option<String>,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests;
