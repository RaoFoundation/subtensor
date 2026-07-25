#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
//! Shared numeric assert helper for children tests.

pub(super) fn close(value: u64, target: u64, eps: u64, msg: &str) {
    assert!(
        (value as i64 - target as i64).abs() <= eps as i64,
        "{msg}: value = {value}, target = {target}, eps = {eps}"
    )
}
