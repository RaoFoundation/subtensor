//! Substrate Node Subtensor CLI library.
#![warn(missing_docs)]

// Use jemalloc as the global allocator when the `jemalloc-allocator` feature is enabled (off by
// default). Archive nodes serving sustained RPC load exhibit unbounded anonymous-memory growth that
// matches the glibc-malloc arena-fragmentation profile across the node's hundreds of threads;
// jemalloc (the allocator the polkadot binary ships with) avoids it. See issue #2724.
#[cfg(all(unix, feature = "jemalloc-allocator"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
mod chain_spec;
mod cli;
mod client;
mod clone_spec;
mod command;
mod conditional_evm_block_import;
mod consensus;
mod dev_keystore;
mod ethereum;
mod rpc;
mod service;

fn main() -> sc_cli::Result<()> {
    command::run()
}
