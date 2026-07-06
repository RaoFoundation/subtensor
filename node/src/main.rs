//! Substrate Node Subtensor CLI library.
#![warn(missing_docs)]

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
