use rayon::prelude::*;
use std::{
    env, fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc::channel,
};
use walkdir::WalkDir;

use subtensor_linting::*;

fn main() {
    // need to list all rust directories here
    println!("cargo:rerun-if-changed=pallets");
    println!("cargo:rerun-if-changed=node");
    println!("cargo:rerun-if-changed=runtime");
    println!("cargo:rerun-if-changed=lints");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=support");
    // Get the root directory of the workspace
    let workspace_root = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = Path::new(&workspace_root);

    // Collect all Rust source files in the workspace
    let rust_files = collect_rust_files(workspace_root);

    // Channel used to communicate errors back to the main thread from the parallel processing
    // as we process each Rust file
    let (tx, rx) = channel();

    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(64 * 1024 * 1024)
        .build()
        .expect("build script lint thread pool can be created");

    pool.install(|| {
        // Parse each rust file with syn and run the linting suite on it in parallel.
        rust_files.par_iter().for_each_with(tx.clone(), |tx, file| {
            let is_test = file.display().to_string().contains("test");
            let Ok(content) = fs::read_to_string(file) else {
                return;
            };
            let Ok(parsed_tokens) = proc_macro2::TokenStream::from_str(&content) else {
                return;
            };
            let Ok(parsed_file) = syn::parse2::<syn::File>(parsed_tokens) else {
                return;
            };

            let track_lint = |result: Result| {
                let Err(errors) = result else {
                    return;
                };
                let relative_path = file.strip_prefix(workspace_root).unwrap_or(file.as_path());
                for error in errors {
                    let loc = error.span().start();
                    let file_path = relative_path.display();
                    // note that spans can't go across thread boundaries without losing their location
                    // info so we we serialize here and send a String
                    tx.send(format!(
                        "cargo:warning={}:{}:{}: {}",
                        file_path, loc.line, loc.column, error,
                    ))
                    .unwrap();
                }
            };

            track_lint(ForbidAsPrimitiveConversion::lint(&parsed_file));
            track_lint(ForbidKeysRemoveCall::lint(&parsed_file));
            track_lint(RequireFreezeStruct::lint(&parsed_file));
            track_lint(RequireExplicitPalletIndex::lint(&parsed_file));

            if is_test {
                track_lint(ForbidSaturatingMath::lint(&parsed_file));
            }
        });
    });

    // Collect and print all errors after the parallel processing is done
    drop(tx); // Close the sending end of the channel

    for error in rx {
        println!("{error}");
    }
}

/// Recursively collects all Rust files in the given directory
fn collect_rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut rust_files = Vec::new();

    for entry in WalkDir::new(dir) {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();

        // Skip any path that contains "target" directory
        if path
            .components()
            .any(|component| component.as_os_str() == "target")
            || path.ends_with("build.rs")
        {
            continue;
        }

        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            rust_files.push(path.to_path_buf());
        }
    }

    rust_files
}
