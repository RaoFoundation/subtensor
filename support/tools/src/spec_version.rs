//! CLI: print the runtime `spec_version` from `node_subtensor_runtime::VERSION`.
//!
//! Used by release/CI scripts that need the on-chain-facing runtime version without parsing
//! `runtime/src/lib.rs` by hand.

use node_subtensor_runtime::VERSION;

fn main() {
    println!("{}", VERSION.spec_version);
}
