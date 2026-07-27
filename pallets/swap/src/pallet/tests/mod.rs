//! Integration tests for `pallet-subtensor-swap` (TAO↔alpha balancer AMM).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use approx::assert_abs_diff_eq;
use frame_support::weights::WeightMeter;
use frame_support::{assert_noop, assert_ok};
use sp_arithmetic::Perquintill;
use sp_runtime::DispatchError;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{NetUid, Token};
use subtensor_swap_interface::{Order as OrderT, SwapHandler};

use super::*;
use crate::mock::*;
use crate::pallet::swap_step::*;

/// Minimum alpha price used as a sell-side limit in tests (rao-normalized).
#[allow(dead_code)]
fn get_min_price() -> U64F64 {
    U64F64::from_num(Pallet::<Test>::min_price_inner::<TaoBalance>())
        / U64F64::from_num(1_000_000_000)
}

/// Maximum alpha price used as a buy-side limit in tests (rao-normalized).
#[allow(dead_code)]
fn get_max_price() -> U64F64 {
    U64F64::from_num(Pallet::<Test>::max_price_inner::<TaoBalance>())
        / U64F64::from_num(1_000_000_000)
}

/// Clamp fixed-point `t` into `[a, b]` (debug helper).
#[allow(dead_code)]
fn clamp_fixed_between(t: U64F64, a: U64F64, b: U64F64) -> U64F64 {
    if t < a {
        a
    } else if t > b {
        b
    } else {
        t
    }
}

/// Trace the current balancer alpha price for `netuid` (debug helper).
#[allow(dead_code)]
fn print_current_price(netuid: NetUid) {
    let current_price = Pallet::<Test>::current_price(netuid);
    log::trace!("Current price: {current_price:.6}");
}

mod adjust_protocol_liquidity;
mod clear_protocol_liquidity;
mod migrate_swapv3_to_balancer;
mod set_fee_rate;
mod swap_execution;
mod swap_initialization;
mod swap_input_limits;
