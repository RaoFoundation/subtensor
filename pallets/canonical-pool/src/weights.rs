//! Weights for `pallet_canonical_pool`.
//!
//! NOT MEASURED. Every value below is a placeholder: the storage accesses of the call at the
//! runtime's database weight, plus a flat execution allowance. The `run-benchmarks` workflow
//! measures `benchmarking.rs` on the reference machine and rewrites this file; that must
//! happen before a production network enables the pallet.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]
#![allow(dead_code)]

use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};
use core::marker::PhantomData;

/// Weight functions needed for `pallet_canonical_pool`.
pub trait WeightInfo {
	fn add_liquidity() -> Weight;
	fn remove_liquidity() -> Weight;
	fn swap_tusd_for_tao() -> Weight;
	fn swap_tao_for_tusd() -> Weight;
	fn set_enabled() -> Weight;
	fn set_fee() -> Weight;
	fn set_swap_caps() -> Weight;
}

/// Weights for `pallet_canonical_pool` using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `CanonicalPool::{Enabled, ReserveTao, ReserveTusd, TotalShares, Accumulator, Shares}` (r:6 w:5)
	/// Storage: `System::Account` (r:2 w:2)
	/// Storage: `Tusd::Balances` (r:2 w:2)
	fn add_liquidity() -> Weight {
		Weight::from_parts(150_000_000, 12_000)
			.saturating_add(T::DbWeight::get().reads(10_u64))
			.saturating_add(T::DbWeight::get().writes(9_u64))
	}
	/// Storage: `CanonicalPool::{Shares, TotalShares, ReserveTao, ReserveTusd, Accumulator}` (r:5 w:5)
	/// Storage: `System::Account` (r:2 w:2)
	/// Storage: `Tusd::Balances` (r:2 w:2)
	fn remove_liquidity() -> Weight {
		Weight::from_parts(150_000_000, 12_000)
			.saturating_add(T::DbWeight::get().reads(9_u64))
			.saturating_add(T::DbWeight::get().writes(9_u64))
	}
	/// Storage: `CanonicalPool::{Enabled, TotalShares, ReserveTao, ReserveTusd, Caps, Fee, RecentFlow, Accumulator}` (r:8 w:4)
	/// Storage: `System::Account` (r:2 w:2)
	/// Storage: `Tusd::Balances` (r:2 w:2)
	fn swap_tusd_for_tao() -> Weight {
		Weight::from_parts(300_000_000, 12_000)
			.saturating_add(T::DbWeight::get().reads(12_u64))
			.saturating_add(T::DbWeight::get().writes(8_u64))
	}
	/// Storage: `CanonicalPool::{Enabled, TotalShares, ReserveTao, ReserveTusd, Caps, Fee, RecentFlow, Accumulator}` (r:8 w:4)
	/// Storage: `System::Account` (r:2 w:2)
	/// Storage: `Tusd::Balances` (r:2 w:2)
	fn swap_tao_for_tusd() -> Weight {
		Weight::from_parts(300_000_000, 12_000)
			.saturating_add(T::DbWeight::get().reads(12_u64))
			.saturating_add(T::DbWeight::get().writes(8_u64))
	}
	/// Storage: `CanonicalPool::Enabled` (r:0 w:1)
	fn set_enabled() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `CanonicalPool::Fee` (r:0 w:1)
	fn set_fee() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `CanonicalPool::Caps` (r:0 w:1)
	fn set_swap_caps() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
}

// For backwards compatibility and tests.
impl WeightInfo for () {
	fn add_liquidity() -> Weight {
		Weight::from_parts(150_000_000, 12_000)
			.saturating_add(RocksDbWeight::get().reads(10_u64))
			.saturating_add(RocksDbWeight::get().writes(9_u64))
	}
	fn remove_liquidity() -> Weight {
		Weight::from_parts(150_000_000, 12_000)
			.saturating_add(RocksDbWeight::get().reads(9_u64))
			.saturating_add(RocksDbWeight::get().writes(9_u64))
	}
	fn swap_tusd_for_tao() -> Weight {
		Weight::from_parts(300_000_000, 12_000)
			.saturating_add(RocksDbWeight::get().reads(12_u64))
			.saturating_add(RocksDbWeight::get().writes(8_u64))
	}
	fn swap_tao_for_tusd() -> Weight {
		Weight::from_parts(300_000_000, 12_000)
			.saturating_add(RocksDbWeight::get().reads(12_u64))
			.saturating_add(RocksDbWeight::get().writes(8_u64))
	}
	fn set_enabled() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn set_fee() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn set_swap_caps() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
}
