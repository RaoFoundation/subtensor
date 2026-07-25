//! Storage migrations for `pallet-crowdloan`.
//!
//! Each migration records completion in [`crate::HasMigrationRun`] under a fixed
//! byte-string name (must stay stable for idempotency).

mod migrate_add_contributors_count;
pub use migrate_add_contributors_count::*;
