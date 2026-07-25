#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Shared fixtures for network dissolve / register-owner-alpha tests.

use super::prelude::*;

/// Run the same α-out destroy steps as `remove_data_for_dissolved_networks` (post-root-cleanup).
pub(super) fn destroy_alpha_in_out_stakes_full_pipeline_for_test(netuid: NetUid) {
    run_destroy_alpha_in_out_stakes_full_pipeline(netuid);
}

pub(super) fn owner_alpha_from_lock_and_price(lock_cost_u64: u64, price: U64F64) -> u64 {
    let alpha = (U64F64::from_num(lock_cost_u64)
        .checked_div(price)
        .unwrap_or_default())
    .floor();

    if alpha > U64F64::from_num(u64::MAX) {
        u64::MAX
    } else {
        alpha.to_num::<u64>()
    }
}
