
//! Weights for `pallet_lending` (synthetic shorts).
//!
//! PLACEHOLDER: not yet measured for the synthetic-short calls. Each value is
//! copied from the last measured `pallet_lending` run (2026-11-24, AMD EPYC
//! 7763) for the call with the closest or heavier shape, so every trading call
//! is charged at least the old `close` envelope (two AMM trades, the buy-back
//! search and interest). Regenerate with the benchmark CLI before enabling.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]
#![allow(dead_code)]

use frame_support::{traits::Get, weights::{Weight, RuntimeDbWeight, constants::RocksDbWeight}};
use core::marker::PhantomData;

/// Weight functions needed for `pallet_lending`.
pub trait WeightInfo {
	fn open_short() -> Weight;
	fn increase_short() -> Weight;
	fn add_collateral() -> Weight;
	fn withdraw_collateral() -> Weight;
	fn close_short() -> Weight;
	fn poke() -> Weight;
	fn set_enabled() -> Weight;
	fn set_synth_params() -> Weight;
	/// One scheduled interest charge plus its health check (`on_idle`).
	fn charge_interest() -> Weight;
	/// One paced buy-back for a deleveraging position (`on_initialize`).
	fn deleverage_step() -> Weight;
	/// One reserve buy-back of residual alpha (`on_initialize`).
	fn retire_residual() -> Weight;
	/// One TWAP sample for one subnet (`on_initialize`).
	fn twap_sample() -> Weight;
	/// Settling one position during subnet dissolution.
	fn settle_dissolution() -> Weight;
}

/// Old `close` (6.10 ms, 53 r / 50 w) with old `open_exact`'s 61 reads.
fn trade_envelope(db: RuntimeDbWeight) -> Weight {
	Weight::from_parts(6_098_685_000, 11306)
		.saturating_add(db.reads(61_u64))
		.saturating_add(db.writes(50_u64))
}

/// Old `collect` (8.24 ms, 62 r / 52 w).
fn charge_envelope(db: RuntimeDbWeight) -> Weight {
	Weight::from_parts(8_237_070_000, 11306)
		.saturating_add(db.reads(62_u64))
		.saturating_add(db.writes(52_u64))
}

/// Old `add_collateral` (8 r / 2 w).
fn add_collateral_envelope(db: RuntimeDbWeight) -> Weight {
	Weight::from_parts(100_859_000, 5153)
		.saturating_add(db.reads(8_u64))
		.saturating_add(db.writes(2_u64))
}

/// Old `update_reference` (11 r / 4 w).
fn twap_envelope(db: RuntimeDbWeight) -> Weight {
	Weight::from_parts(57_166_000, 5944)
		.saturating_add(db.reads(11_u64))
		.saturating_add(db.writes(4_u64))
}

/// Old `set_enabled`.
fn set_enabled_envelope(db: RuntimeDbWeight) -> Weight {
	Weight::from_parts(5_771_000, 0).saturating_add(db.writes(1_u64))
}

/// Old `set_synth_params`.
fn set_params_envelope(db: RuntimeDbWeight) -> Weight {
	Weight::from_parts(6_652_000, 0).saturating_add(db.writes(1_u64))
}

/// Weights for `pallet_lending` using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	fn open_short() -> Weight { trade_envelope(T::DbWeight::get()) }
	fn increase_short() -> Weight { trade_envelope(T::DbWeight::get()) }
	fn add_collateral() -> Weight { add_collateral_envelope(T::DbWeight::get()) }
	fn withdraw_collateral() -> Weight { trade_envelope(T::DbWeight::get()) }
	fn close_short() -> Weight { trade_envelope(T::DbWeight::get()) }
	fn poke() -> Weight { trade_envelope(T::DbWeight::get()) }
	fn set_enabled() -> Weight { set_enabled_envelope(T::DbWeight::get()) }
	fn set_synth_params() -> Weight { set_params_envelope(T::DbWeight::get()) }
	fn charge_interest() -> Weight { charge_envelope(T::DbWeight::get()) }
	fn deleverage_step() -> Weight { trade_envelope(T::DbWeight::get()) }
	fn retire_residual() -> Weight { trade_envelope(T::DbWeight::get()) }
	fn twap_sample() -> Weight { twap_envelope(T::DbWeight::get()) }
	fn settle_dissolution() -> Weight { trade_envelope(T::DbWeight::get()) }
}

// For backwards compatibility and tests.
impl WeightInfo for () {
	fn open_short() -> Weight { trade_envelope(RocksDbWeight::get()) }
	fn increase_short() -> Weight { trade_envelope(RocksDbWeight::get()) }
	fn add_collateral() -> Weight { add_collateral_envelope(RocksDbWeight::get()) }
	fn withdraw_collateral() -> Weight { trade_envelope(RocksDbWeight::get()) }
	fn close_short() -> Weight { trade_envelope(RocksDbWeight::get()) }
	fn poke() -> Weight { trade_envelope(RocksDbWeight::get()) }
	fn set_enabled() -> Weight { set_enabled_envelope(RocksDbWeight::get()) }
	fn set_synth_params() -> Weight { set_params_envelope(RocksDbWeight::get()) }
	fn charge_interest() -> Weight { charge_envelope(RocksDbWeight::get()) }
	fn deleverage_step() -> Weight { trade_envelope(RocksDbWeight::get()) }
	fn retire_residual() -> Weight { trade_envelope(RocksDbWeight::get()) }
	fn twap_sample() -> Weight { twap_envelope(RocksDbWeight::get()) }
	fn settle_dissolution() -> Weight { trade_envelope(RocksDbWeight::get()) }
}
