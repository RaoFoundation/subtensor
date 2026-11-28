//! Weights for `pallet_rails`.
//!
//! NOT MEASURED. Every value below is a placeholder: the storage accesses of the call at the
//! runtime's database weight, plus a flat execution allowance. The `run-benchmarks` workflow
//! measures `benchmarking.rs` on the reference machine and rewrites this file; that must
//! happen before a production network enables the pallet.
//!
//! `execute`, `bridge_out` and `open_market` also call into staking. The benchmarks do not
//! go there; the calls add `RailsStaking::weight()`, which the runtime builds from the
//! measured weights of the subtensor pallet.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]
#![allow(dead_code)]

use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};
use core::marker::PhantomData;

/// Weight functions needed for `pallet_rails`.
pub trait WeightInfo {
	fn execute() -> Weight;
	fn ack_outbound() -> Weight;
	fn bridge_out() -> Weight;
	fn set_gateway() -> Weight;
	fn set_guardian() -> Weight;
	fn set_paused() -> Weight;
	fn register_token() -> Weight;
	fn set_tao_cap() -> Weight;
	fn open_market() -> Weight;
	fn list_market() -> Weight;
	fn set_market_cap() -> Weight;
	fn skip_order() -> Weight;
	fn set_message_reserve() -> Weight;
	fn fund_messages() -> Weight;
	fn defer_outbound() -> Weight;
	fn set_max_order_slots() -> Weight;
	fn grant_slots() -> Weight;
}

/// Weights for `pallet_rails` using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// The check of the bridge's account, a tUSD mint, a pool swap, a reserve transfer, a
	/// queued message and a receipt.
	fn execute() -> Weight {
		Weight::from_parts(500_000_000, 30_000)
			.saturating_add(T::DbWeight::get().reads(43_u64))
			.saturating_add(T::DbWeight::get().writes(28_u64))
	}
	/// Storage: `Rails::{Gateway, Paused, NextOutbound, NextMessage, Deferred, OrderSlots,
	/// MessageReserve, MessageCeiling, Outbox, HeadSince}`, `System::Account` x2 (r:11 w:5)
	fn ack_outbound() -> Weight {
		Weight::from_parts(60_000_000, 8_000)
			.saturating_add(T::DbWeight::get().reads(11_u64))
			.saturating_add(T::DbWeight::get().writes(5_u64))
	}
	/// A payment into the bridge's account, a pool swap, a tUSD burn and a queued message.
	fn bridge_out() -> Weight {
		Weight::from_parts(500_000_000, 30_000)
			.saturating_add(T::DbWeight::get().reads(40_u64))
			.saturating_add(T::DbWeight::get().writes(27_u64))
	}
	/// Storage: `System::Account` (r:1 w:0), `Rails::Gateway` (r:0 w:1)
	fn set_gateway() -> Weight {
		Weight::from_parts(30_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `Rails::Guardian` (r:0 w:1)
	fn set_guardian() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `Rails::{Guardian, Paused}` (r:1 w:1)
	fn set_paused() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `Tusd::Bridges` (r:1 w:0), `Rails::Tokens` (r:0 w:1)
	fn register_token() -> Weight {
		Weight::from_parts(40_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `Rails::TaoCap` (r:0 w:1)
	fn set_tao_cap() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `Rails::Markets` (r:1 w:1)
	fn open_market() -> Weight {
		Weight::from_parts(50_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `Rails::{Markets, Tokens, NextMessage, NextOutbound, Deferred, OrderSlots,
	/// MessageReserve, Outbox, HeadSince}`, `Tusd::Bridges`, `System::Account` (r:11 w:5)
	fn list_market() -> Weight {
		Weight::from_parts(80_000_000, 8_000)
			.saturating_add(T::DbWeight::get().reads(11_u64))
			.saturating_add(T::DbWeight::get().writes(5_u64))
	}
	/// Storage: `Rails::Markets` (r:1 w:1)
	fn set_market_cap() -> Weight {
		Weight::from_parts(30_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `Rails::{NextOrder, OrderSlots, Receipts}` (r:2 w:4)
	fn skip_order() -> Weight {
		Weight::from_parts(40_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(2_u64))
			.saturating_add(T::DbWeight::get().writes(4_u64))
	}
	/// Storage: `Tusd::Bridges`, `Rails::{NextMessage, NextOutbound, Deferred, OrderSlots,
	/// MessageReserve, MessageCeiling}`, `System::Account` (r:6 w:2)
	fn set_message_reserve() -> Weight {
		Weight::from_parts(40_000_000, 6_000)
			.saturating_add(T::DbWeight::get().reads(6_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
	/// Storage: `Tusd::Bridges`, `System::Account` x2 (r:3 w:2)
	fn fund_messages() -> Weight {
		Weight::from_parts(60_000_000, 8_000)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
	/// Storage: `Rails::{Gateway, Paused, HeadSince, NextOutbound, NextMessage, Deferred}`
	/// (r:6 w:3)
	fn defer_outbound() -> Weight {
		Weight::from_parts(40_000_000, 6_000)
			.saturating_add(T::DbWeight::get().reads(6_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	/// Storage: `Tusd::Bridges` (r:1 w:0), `Rails::MaxOrderSlots` (r:0 w:1)
	fn set_max_order_slots() -> Weight {
		Weight::from_parts(30_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `Rails::{Paused, OrderSlots, MaxOrderSlots, NextMessage, NextOutbound,
	/// Deferred, MessageReserve, Outbox, HeadSince}`, `Tusd::Bridges`, `System::Account`
	/// (r:10 w:4)
	fn grant_slots() -> Weight {
		Weight::from_parts(60_000_000, 8_000)
			.saturating_add(T::DbWeight::get().reads(10_u64))
			.saturating_add(T::DbWeight::get().writes(4_u64))
	}
}

// For backwards compatibility and tests.
impl WeightInfo for () {
	fn execute() -> Weight {
		Weight::from_parts(500_000_000, 30_000)
			.saturating_add(RocksDbWeight::get().reads(43_u64))
			.saturating_add(RocksDbWeight::get().writes(28_u64))
	}
	fn ack_outbound() -> Weight {
		Weight::from_parts(60_000_000, 8_000)
			.saturating_add(RocksDbWeight::get().reads(11_u64))
			.saturating_add(RocksDbWeight::get().writes(5_u64))
	}
	fn bridge_out() -> Weight {
		Weight::from_parts(500_000_000, 30_000)
			.saturating_add(RocksDbWeight::get().reads(40_u64))
			.saturating_add(RocksDbWeight::get().writes(27_u64))
	}
	fn set_gateway() -> Weight {
		Weight::from_parts(30_000_000, 4_000)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn set_guardian() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn set_paused() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn register_token() -> Weight {
		Weight::from_parts(40_000_000, 4_000)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn set_tao_cap() -> Weight {
		Weight::from_parts(30_000_000, 2_000)
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn open_market() -> Weight {
		Weight::from_parts(50_000_000, 4_000)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn list_market() -> Weight {
		Weight::from_parts(80_000_000, 8_000)
			.saturating_add(RocksDbWeight::get().reads(11_u64))
			.saturating_add(RocksDbWeight::get().writes(5_u64))
	}
	fn set_market_cap() -> Weight {
		Weight::from_parts(30_000_000, 4_000)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn skip_order() -> Weight {
		Weight::from_parts(40_000_000, 4_000)
			.saturating_add(RocksDbWeight::get().reads(2_u64))
			.saturating_add(RocksDbWeight::get().writes(4_u64))
	}
	fn set_message_reserve() -> Weight {
		Weight::from_parts(40_000_000, 6_000)
			.saturating_add(RocksDbWeight::get().reads(6_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
	fn fund_messages() -> Weight {
		Weight::from_parts(60_000_000, 8_000)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
	fn defer_outbound() -> Weight {
		Weight::from_parts(40_000_000, 6_000)
			.saturating_add(RocksDbWeight::get().reads(6_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn set_max_order_slots() -> Weight {
		Weight::from_parts(30_000_000, 4_000)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn grant_slots() -> Weight {
		Weight::from_parts(60_000_000, 8_000)
			.saturating_add(RocksDbWeight::get().reads(10_u64))
			.saturating_add(RocksDbWeight::get().writes(4_u64))
	}
}
