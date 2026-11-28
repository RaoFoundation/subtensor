//! Weights for `pallet_tusd`.
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

/// Weight functions needed for `pallet_tusd`.
pub trait WeightInfo {
	fn transfer() -> Weight;
	fn set_bridge() -> Weight;
	fn set_bridge_enabled() -> Weight;
}

/// Weights for `pallet_tusd` using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `Tusd::Balances` (r:2 w:2)
	fn transfer() -> Weight {
		Weight::from_parts(50_000_000, 6_196)
			.saturating_add(T::DbWeight::get().reads(2_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
	/// Storage: `Tusd::BridgeByPortal` (r:1 w:1)
	/// Storage: `Tusd::Bridges` (r:1 w:1)
	fn set_bridge() -> Weight {
		Weight::from_parts(50_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(2_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
	/// Storage: `Tusd::Bridges` (r:1 w:1)
	fn set_bridge_enabled() -> Weight {
		Weight::from_parts(30_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
}

// For backwards compatibility and tests.
impl WeightInfo for () {
	fn transfer() -> Weight {
		Weight::from_parts(50_000_000, 6_196)
			.saturating_add(RocksDbWeight::get().reads(2_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
	fn set_bridge() -> Weight {
		Weight::from_parts(50_000_000, 4_000)
			.saturating_add(RocksDbWeight::get().reads(2_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
	fn set_bridge_enabled() -> Weight {
		Weight::from_parts(30_000_000, 4_000)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
}
