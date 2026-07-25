//! Workspace path helpers for walking `pallets/` while skipping tests, mocks, and benchmarks.

use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn is_benchmark_file(file: &Path) -> bool {
    file.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains("benchmark"))
        || file
            .components()
            .any(|component| component.as_os_str() == "benchmarks")
}

pub(super) fn is_test_or_mock_source_path(path: &Path) -> bool {
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

pub(super) fn find_pallet_root(file: &Path, workspace_root: &Path) -> PathBuf {
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

pub(super) fn benchmark_location_hint(pallet_root: &Path, workspace_root: &Path) -> String {
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

pub(super) fn collect_runtime_rust_files(dir: &Path, rust_files: &mut Vec<PathBuf>) {
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

pub(super) fn collect_rust_files(dir: &Path, rust_files: &mut Vec<PathBuf>) {
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

pub(super) fn display_path(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .display()
        .to_string()
}
