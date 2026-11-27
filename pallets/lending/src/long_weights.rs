//! Placeholder weights of the long calls.
//!
// HOLD(longs): spec under review. The long paths are not benchmarked until the long legs of the
// pool adapter exist; these are storage-access counts priced with the runtime's `DbWeight`, as
// every call of the pallet was before `weights.rs` was measured.

use frame_support::{traits::Get, weights::Weight};

/// `open_long`. Worst case: the side and opposite-side checks, four parameter groups, the
/// price rings, both pool reserves, the three books, the flow counters, the position, the
/// balances, the custody legs, the Due entry with NextDue and both position counts, and the
/// holder's position list with its coldkey-swap marker.
// HOLD(longs): placeholder, not a benchmark.
pub fn open_long<T: frame_system::Config>() -> Weight {
    T::DbWeight::get().reads_writes(30, 18)
}

/// `increase_long`: as `open_long` without the opposite-side and count reads.
// HOLD(longs): placeholder, not a benchmark.
pub fn increase_long<T: frame_system::Config>() -> Weight {
    T::DbWeight::get().reads_writes(22, 10)
}

/// `close_long`. Worst case: the position, parameter groups, price rings, pool reserves,
/// books, the flow counters, the custody legs and the interest totals; a full close also
/// removes the Due entry and the retry count and lowers both position counts.
// HOLD(longs): placeholder, not a benchmark.
pub fn close_long<T: frame_system::Config>() -> Weight {
    T::DbWeight::get().reads_writes(25, 17)
}

/// `add_long_collateral`: the position, the parameter groups, price rings, pool reserves and
/// the custody legs.
// HOLD(longs): placeholder, not a benchmark.
pub fn add_long_collateral<T: frame_system::Config>() -> Weight {
    T::DbWeight::get().reads_writes(12, 5)
}

/// `withdraw_long_collateral`: as `add_long_collateral`, plus the coverage books.
// HOLD(longs): placeholder, not a benchmark.
pub fn withdraw_long_collateral<T: frame_system::Config>() -> Weight {
    T::DbWeight::get().reads_writes(14, 5)
}
