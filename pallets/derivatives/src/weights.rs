//! Weights for `pallet_derivatives`.
//!
//! These are hand-written placeholders sized from storage reads and writes so the pallet can be
//! wired up. CI's reference benchmark run replaces them with measured values.

#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use core::marker::PhantomData;
use frame_support::{
    traits::Get,
    weights::{Weight, constants::RocksDbWeight},
};

/// Weight functions needed for `pallet_derivatives`.
pub trait WeightInfo {
    fn add() -> Weight;
    fn close() -> Weight;
    fn sudo_set_params() -> Weight;
    fn sudo_set_subnet_override() -> Weight;
}

/// Weights for `pallet_derivatives` using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
    /// Worst case is a flip: a full settlement followed by a fresh tranche.
    fn add() -> Weight {
        Self::close().saturating_add(
            Weight::from_parts(600_000_000, 12_000)
                .saturating_add(T::DbWeight::get().reads(30_u64))
                .saturating_add(T::DbWeight::get().writes(20_u64)),
        )
    }
    /// Up to three pool swaps plus stake, reserve and position bookkeeping.
    fn close() -> Weight {
        Weight::from_parts(900_000_000, 12_000)
            .saturating_add(T::DbWeight::get().reads(35_u64))
            .saturating_add(T::DbWeight::get().writes(25_u64))
    }
    fn sudo_set_params() -> Weight {
        Weight::from_parts(6_000_000, 0).saturating_add(T::DbWeight::get().writes(1_u64))
    }
    fn sudo_set_subnet_override() -> Weight {
        Weight::from_parts(6_000_000, 0).saturating_add(T::DbWeight::get().writes(1_u64))
    }
}

// For backwards compatibility and tests.
impl WeightInfo for () {
    fn add() -> Weight {
        Self::close().saturating_add(
            Weight::from_parts(600_000_000, 12_000)
                .saturating_add(RocksDbWeight::get().reads(30_u64))
                .saturating_add(RocksDbWeight::get().writes(20_u64)),
        )
    }
    fn close() -> Weight {
        Weight::from_parts(900_000_000, 12_000)
            .saturating_add(RocksDbWeight::get().reads(35_u64))
            .saturating_add(RocksDbWeight::get().writes(25_u64))
    }
    fn sudo_set_params() -> Weight {
        Weight::from_parts(6_000_000, 0).saturating_add(RocksDbWeight::get().writes(1_u64))
    }
    fn sudo_set_subnet_override() -> Weight {
        Weight::from_parts(6_000_000, 0).saturating_add(RocksDbWeight::get().writes(1_u64))
    }
}
