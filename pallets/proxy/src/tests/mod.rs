//! Proxy pallet unit tests, split by concept for discoverability.

#![cfg(test)]
#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

mod mock;

mod announcements;
mod poke_deposit;
mod proxy_lifecycle;
mod proxy_type_filter;
mod pure_proxy;
mod real_pays_fee;

#[allow(unused_imports)] // used by `impl_benchmark_test_suite!` in benchmarking.rs
pub use mock::{Test, new_test_ext};
