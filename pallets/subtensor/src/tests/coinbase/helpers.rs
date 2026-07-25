#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]
//! Shared fixtures for coinbase emission tests.

use super::super::mock::*;
use crate::*;
use subtensor_runtime_common::TaoBalance;

pub(super) fn close(value: u64, target: u64, eps: u64) {
    assert!(
        (value as i64 - target as i64).abs() < eps as i64,
        "Assertion failed: value = {value}, target = {target}, eps = {eps}"
    )
}

/// Seed a large root stake with full TAO weight so that
/// `root_proportion = tao_weight / (tao_weight + alpha_issuance)` is ~1.
/// This keeps the alpha-injection cap (`root_proportion * alpha_emission`) from
/// spuriously binding for small per-subnet emissions, preserving the liquidity
/// injection behavior these tests were written for.
pub(super) fn set_full_injection_root_stake() {
    SubnetTAO::<Test>::insert(
        NetUid::ROOT,
        TaoBalance::from(1_000_000_000_000_000_000_u64),
    );
    SubtensorModule::set_tao_weight(u64::MAX);
}
