//! Build script for the `node-subtensor` binary.
//!
//! Emits Substrate cargo keys (impl version / commit) and reruns when git HEAD changes.

use substrate_build_script_utils::{generate_cargo_keys, rerun_if_git_head_changed};

fn main() {
    generate_cargo_keys();

    rerun_if_git_head_changed();
}
