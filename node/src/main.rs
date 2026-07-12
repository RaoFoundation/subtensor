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

// Regression guard for the optional `jemalloc-allocator` feature.
//
// The feature is off by default, so the default build never compiles it, and a
// `tikv-jemallocator` bump, a `#[global_allocator]` typo, or a `cfg` mistake can
// leave the binary *compiling* while jemalloc is no longer the active global
// allocator (glibc silently stays in charge). A compile check cannot catch that:
// `stats.allocated` only counts bytes routed through jemalloc, so this test
// drives the global allocator with a large live allocation and asserts jemalloc
// tracked it. If the `#[global_allocator]`/`cfg` wiring is broken the Vec goes
// to glibc and the delta is just a few bytes of `mallctl` noise - well below the
// threshold - and the test fails.
#[cfg(all(test, unix, feature = "jemalloc-allocator"))]
mod jemalloc_allocator_feature {
    use tikv_jemalloc_ctl::{epoch, stats};

    // A live allocation routed through the *global* allocator. Large enough to
    // dwarf any bookkeeping the `mallctl` calls do themselves.
    const PAYLOAD: usize = 16 * 1024 * 1024;

    // The workspace denies `expect_used`/`unwrap_used`, and `tikv_jemalloc_ctl`'s
    // Error is a transparent wrapper that does not implement std::error::Error,
    // so the Result cannot be propagated with `?` either. Unwrap the two stat
    // helpers by hand instead.
    fn advance_epoch() {
        if let Err(error) = epoch::advance() {
            panic!("advance jemalloc epoch: {error:?}");
        }
    }

    fn allocated_bytes() -> usize {
        match stats::allocated::read() {
            Ok(bytes) => bytes,
            Err(error) => panic!("read stats.allocated: {error:?}"),
        }
    }

    #[test]
    fn jemalloc_is_the_active_global_allocator() {
        // jemalloc caches stats; advance the epoch to refresh them before each read.
        advance_epoch();
        let before = allocated_bytes();

        // Held across the second read so jemalloc cannot reclaim it first.
        let hold = vec![0u8; PAYLOAD];

        advance_epoch();
        let after = allocated_bytes();

        assert!(
            after.saturating_sub(before) > PAYLOAD / 2,
            "jemalloc did not track a {}-byte global-allocator allocation \
             (before={}, after={}): it is linked but not the active global \
             allocator, so the jemalloc-allocator feature wiring is broken",
            PAYLOAD,
            before,
            after,
        );

        drop(hold);
    }
}
