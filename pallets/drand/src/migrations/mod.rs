//! Storage migrations for `pallet-drand` (pulse window repair / prune watermarks).

pub mod migrate_prune_old_pulses;
pub use migrate_prune_old_pulses::*;
pub mod migrate_set_oldest_round;
pub use migrate_set_oldest_round::*;
