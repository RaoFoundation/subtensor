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
// only bytes routed *through* jemalloc are observable, so this test drives the
// global allocator with a large live allocation and asserts jemalloc tracked it.
// If the `#[global_allocator]`/`cfg` wiring is broken the Vec goes to glibc and
// the delta stays at zero - well below the threshold - and the test fails.
//
// The delta is read from jemalloc's *thread-local* "bytes ever allocated by this
// thread" counter, not the process-global `stats.allocated`. `cargo test` runs the
// node's tests in parallel inside one process, and the global stat is perturbed by
// every other test's allocations and frees, so a before/after delta around it is
// racy: a concurrent free can drop it below threshold (false failure) and a
// concurrent allocation can lift it (false pass). The thread-local counter only
// ever counts bytes the *calling* thread routes through the global allocator, so
// it is immune to concurrent tests; it is also monotonic (a free cannot decrease
// it), so the only way it stays flat across a large live allocation is if that
// allocation went to glibc because jemalloc is linked but not active.
#[cfg(all(test, unix, feature = "jemalloc-allocator"))]
mod jemalloc_allocator_feature {
    use tikv_jemalloc_ctl::thread;

    // A live allocation routed through the *global* allocator. Large enough to
    // dwarf any bookkeeping the stat reads do themselves.
    const PAYLOAD: usize = 16 * 1024 * 1024;

    // The workspace denies `expect_used`/`unwrap_used`, and `tikv_jemalloc_ctl`'s
    // Error is a transparent wrapper that does not implement std::error::Error,
    // so the Result cannot be propagated with `?` either. Resolve the stat handle
    // by hand instead.
    fn allocated_pointer() -> thread::ThreadLocal<u64> {
        match thread::allocatedp::read() {
            Ok(pointer) => pointer,
            Err(error) => panic!("read thread.allocatedp: {error:?}"),
        }
    }

    #[test]
    fn jemalloc_is_the_active_global_allocator() {
        // `read()` returns a thread-local pointer to the cumulative byte count
        // for *this* thread; `get()` dereferences it (safe: the raw-pointer read
        // is encapsulated in `tikv_jemalloc_ctl`). Thread stats are live, so -
        // unlike `stats::allocated` - no epoch refresh is needed before a read.
        let pointer = allocated_pointer();
        let before = pointer.get();

        // Held across the second read so the allocator cannot reclaim it first.
        let hold = vec![0u8; PAYLOAD];

        let after = pointer.get();

        assert!(
            after.saturating_sub(before) >= PAYLOAD as u64,
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
