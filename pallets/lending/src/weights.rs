//! Dispatch and hook weights for lift-based lending.
//!
//! Production `SubstrateWeight` values below are **conservative placeholders**
//! derived from estimated RocksDB read/write counts and proof-size caps. They
//! are not measured on reference hardware. Re-run
//! `scripts/benchmark_all.sh pallet_lending` on the CI reference machine and
//! apply the generated patch before treating these as final.
use core::marker::PhantomData;
use frame_support::{
    traits::Get,
    weights::{Weight, constants::RocksDbWeight},
};

pub trait WeightInfo {
    fn open() -> Weight;
    fn open_exact() -> Weight;
    fn open_long_and_sell() -> Weight;
    fn close() -> Weight;
    fn repay() -> Weight;
    fn set_enabled() -> Weight;
    fn set_lift_params() -> Weight;
    fn set_min_price_impact() -> Weight;
    fn warm_reference() -> Weight;
    fn collect() -> Weight;
    fn update_reference() -> Weight;
    fn settle() -> Weight;
}

/// Conservative production weights. Each extrinsic weight includes a base
/// compute allowance plus RocksDB read/write costs for the hot path.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
    /// Open / increase: reference, params, book, capacity, lift, escrow, Due.
    /// Long admission uses aggregate `long_collateral_alpha` (O(1)), not a
    /// full-position scan. Estimated ~45 reads, ~25 writes, swaps in compute pad.
    fn open() -> Weight {
        Weight::from_parts(250_000_000, 32_000)
            .saturating_add(T::DbWeight::get().reads(45_u64))
            .saturating_add(T::DbWeight::get().writes(25_u64))
    }
    fn open_exact() -> Weight {
        Self::open()
    }
    /// Raise = open long + unstake/sell leg.
    fn open_long_and_sell() -> Weight {
        Weight::from_parts(350_000_000, 40_000)
            .saturating_add(T::DbWeight::get().reads(55_u64))
            .saturating_add(T::DbWeight::get().writes(30_u64))
    }
    /// Close: interest, buyback/repay, readd, book updates.
    fn close() -> Weight {
        Weight::from_parts(300_000_000, 36_000)
            .saturating_add(T::DbWeight::get().reads(40_u64))
            .saturating_add(T::DbWeight::get().writes(22_u64))
    }
    /// Partial repay shares the close path with pro-rata scaling.
    fn repay() -> Weight {
        Self::close()
    }
    fn set_enabled() -> Weight {
        Weight::from_parts(15_000_000, 1_500)
            .saturating_add(T::DbWeight::get().reads(1_u64))
            .saturating_add(T::DbWeight::get().writes(1_u64))
    }
    fn set_lift_params() -> Weight {
        Weight::from_parts(20_000_000, 2_000)
            .saturating_add(T::DbWeight::get().reads(1_u64))
            .saturating_add(T::DbWeight::get().writes(1_u64))
    }
    fn set_min_price_impact() -> Weight {
        Weight::from_parts(15_000_000, 1_500)
            .saturating_add(T::DbWeight::get().reads(1_u64))
            .saturating_add(T::DbWeight::get().writes(1_u64))
    }
    /// Permissionless reference + G_ema seed (idempotent).
    fn warm_reference() -> Weight {
        Weight::from_parts(40_000_000, 4_000)
            .saturating_add(T::DbWeight::get().reads(8_u64))
            .saturating_add(T::DbWeight::get().writes(5_u64))
    }
    /// Per-position interest collection (metered in on_idle).
    fn collect() -> Weight {
        Weight::from_parts(80_000_000, 8_000)
            .saturating_add(T::DbWeight::get().reads(8_u64))
            .saturating_add(T::DbWeight::get().writes(6_u64))
    }
    /// on_finalize reference / TaoEma walk for one subnet.
    fn update_reference() -> Weight {
        Weight::from_parts(50_000_000, 6_000)
            .saturating_add(T::DbWeight::get().reads(6_u64))
            .saturating_add(T::DbWeight::get().writes(3_u64))
    }
    /// Per-position dissolution settlement step.
    fn settle() -> Weight {
        Weight::from_parts(100_000_000, 10_000)
            .saturating_add(T::DbWeight::get().reads(10_u64))
            .saturating_add(T::DbWeight::get().writes(8_u64))
    }
}

/// Mock / test runtime: charge the same conservative schedule so idle meters
/// and extrinsic accounting exercise non-zero paths under test.
impl WeightInfo for () {
    fn open() -> Weight {
        Weight::from_parts(250_000_000, 32_000)
            .saturating_add(RocksDbWeight::get().reads(45_u64))
            .saturating_add(RocksDbWeight::get().writes(25_u64))
    }
    fn open_exact() -> Weight {
        Self::open()
    }
    fn open_long_and_sell() -> Weight {
        Weight::from_parts(350_000_000, 40_000)
            .saturating_add(RocksDbWeight::get().reads(55_u64))
            .saturating_add(RocksDbWeight::get().writes(30_u64))
    }
    fn close() -> Weight {
        Weight::from_parts(300_000_000, 36_000)
            .saturating_add(RocksDbWeight::get().reads(40_u64))
            .saturating_add(RocksDbWeight::get().writes(22_u64))
    }
    fn repay() -> Weight {
        Self::close()
    }
    fn set_enabled() -> Weight {
        Weight::from_parts(15_000_000, 1_500)
            .saturating_add(RocksDbWeight::get().reads(1_u64))
            .saturating_add(RocksDbWeight::get().writes(1_u64))
    }
    fn set_lift_params() -> Weight {
        Weight::from_parts(20_000_000, 2_000)
            .saturating_add(RocksDbWeight::get().reads(1_u64))
            .saturating_add(RocksDbWeight::get().writes(1_u64))
    }
    fn set_min_price_impact() -> Weight {
        Weight::from_parts(15_000_000, 1_500)
            .saturating_add(RocksDbWeight::get().reads(1_u64))
            .saturating_add(RocksDbWeight::get().writes(1_u64))
    }
    fn warm_reference() -> Weight {
        Weight::from_parts(40_000_000, 4_000)
            .saturating_add(RocksDbWeight::get().reads(8_u64))
            .saturating_add(RocksDbWeight::get().writes(5_u64))
    }
    fn collect() -> Weight {
        Weight::from_parts(80_000_000, 8_000)
            .saturating_add(RocksDbWeight::get().reads(8_u64))
            .saturating_add(RocksDbWeight::get().writes(6_u64))
    }
    fn update_reference() -> Weight {
        Weight::from_parts(50_000_000, 6_000)
            .saturating_add(RocksDbWeight::get().reads(6_u64))
            .saturating_add(RocksDbWeight::get().writes(3_u64))
    }
    fn settle() -> Weight {
        Weight::from_parts(100_000_000, 10_000)
            .saturating_add(RocksDbWeight::get().reads(10_u64))
            .saturating_add(RocksDbWeight::get().writes(8_u64))
    }
}
