//! Placeholder weights for `pallet_lending`.
//!
//! These are not benchmark results. Each function counts the storage reads and writes of its
//! worst case and prices them with the runtime's `DbWeight`; the benchmark task replaces this
//! file with measured weights.

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::Weight};

/// Weight functions needed by `pallet_lending`.
pub trait WeightInfo {
    /// `set_enabled` with `s` live subnets, each re-seeded (SPEC section 9 item 3).
    fn set_enabled(s: u32) -> Weight;
    /// `set_longs_enabled`.
    fn set_longs_enabled() -> Weight;
    /// Any of the four parameter setters: validate and write the pending value.
    fn set_params() -> Weight;
    /// `on_initialize` with `s` live subnets: all four pending groups applied, both ring
    /// kinds re-seeded, and every subnet sampled with its day closing.
    fn on_initialize(s: u32) -> Weight;
    /// The v0 -> v1 migration with `s` live subnets.
    fn migrate_to_v1(s: u32) -> Weight;
    /// The interest queue of one block with `c` scheduled charges (SPEC P15): placeholder, not
    /// a benchmark.
    fn collect_due(c: u32) -> Weight;
    /// `open_short` or `open_long`: placeholder, not a benchmark (see the module docs).
    fn open_position() -> Weight;
    /// `increase_short` or `increase_long`: placeholder, not a benchmark.
    fn increase_position() -> Weight;
    /// `close_short` or `close_long`: placeholder, not a benchmark.
    fn close_position() -> Weight;
    /// `add_collateral` or `add_long_collateral`: placeholder, not a benchmark.
    fn add_collateral() -> Weight;
    /// `withdraw_collateral` or `withdraw_long_collateral`: placeholder, not a benchmark.
    fn withdraw_collateral() -> Weight;
}

/// Storage-access weights priced with the runtime's `DbWeight`.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
    fn set_enabled(s: u32) -> Weight {
        // Reads: Enabled, RankGeneration, DeregParamsStore, the live set; per subnet the
        // moving price. Writes: Enabled, RankGeneration; per subnet ring, generation, price.
        T::DbWeight::get().reads_writes(
            4u64.saturating_add(u64::from(s)),
            2u64.saturating_add(u64::from(s).saturating_mul(3)),
        )
    }
    fn set_longs_enabled() -> Weight {
        T::DbWeight::get().reads_writes(1, 1)
    }
    fn set_params() -> Weight {
        T::DbWeight::get().writes(1)
    }
    fn on_initialize(s: u32) -> Weight {
        // Reads: four pending and four live groups, RankGeneration, the live set; per subnet
        // pool TAO and its EMA, spot (two reserves), both rings twice (re-seed, then sample),
        // the published price and the ring generation.
        // Writes: four pending and four live groups, RankGeneration; per subnet the EMA, both
        // rings twice, the published price twice and the ring generation.
        T::DbWeight::get().reads_writes(
            10u64.saturating_add(u64::from(s).saturating_mul(10)),
            9u64.saturating_add(u64::from(s).saturating_mul(8)),
        )
    }
    fn migrate_to_v1(s: u32) -> Weight {
        // Reads: the storage version, RankGeneration, the live set; per subnet spot (two
        // reserves), pool TAO and the moving price.
        // Writes: the storage version and four groups; per subnet both rings, the published
        // price, the ring generation and the pool-TAO EMA.
        T::DbWeight::get().reads_writes(
            3u64.saturating_add(u64::from(s).saturating_mul(4)),
            5u64.saturating_add(u64::from(s).saturating_mul(5)),
        )
    }
    fn collect_due(c: u32) -> Weight {
        // Fixed: NextDue (read, write), LendingParamsStore, TotalPositions and a probe of each
        // of the `MAX_DUE_BUCKETS_PER_BLOCK` buckets.
        // Per charge. Reads: the Due key, the position, the TWAP ring, the rank generation
        // (live and of the ring), the rank price, both pool reserves (spot), the pool TAO
        // before and after, the custody balance, the pool reserve in the credit,
        // InterestTotals, InterestRetry, NextDue and the event topics.
        // Writes: the Due entry removed and added, the position, the custody balance, the pool
        // reserve, InterestTotals, InterestRetry, NextDue and the event.
        let buckets = u64::from(crate::interest::MAX_DUE_BUCKETS_PER_BLOCK);
        T::DbWeight::get().reads_writes(
            3u64.saturating_add(buckets)
                .saturating_add(u64::from(c).saturating_mul(16)),
            1u64.saturating_add(u64::from(c).saturating_mul(9)),
        )
    }
    fn open_position() -> Weight {
        // Worst case: the side and opposite-side checks, four parameter groups, the price
        // rings, both pool reserves, the three books, the flow counters, the position, the
        // balances, the custody legs, and the Due entry with NextDue and both position counts.
        T::DbWeight::get().reads_writes(28, 16)
    }
    fn increase_position() -> Weight {
        // As `open_position` without the opposite-side and count reads.
        T::DbWeight::get().reads_writes(22, 10)
    }
    fn close_position() -> Weight {
        // Worst case: the position, parameter groups, price rings, pool reserves, books, the
        // flow counters, the position, the custody legs and the interest totals; a full close
        // also removes the Due entry and the retry count and lowers both position counts.
        T::DbWeight::get().reads_writes(25, 17)
    }
    fn add_collateral() -> Weight {
        // The position, the parameter groups, price rings, pool reserves and the custody legs.
        T::DbWeight::get().reads_writes(12, 5)
    }
    fn withdraw_collateral() -> Weight {
        // As `add_collateral`, plus the coverage books.
        T::DbWeight::get().reads_writes(14, 5)
    }
}
