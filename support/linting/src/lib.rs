//! Custom workspace lints for Subtensor (run from repo-root `build.rs`).
//!
//! Each lint implements [`Lint`] and is invoked over parsed Rust sources. Lint type names
//! (`ForbidAsPrimitiveConversion`, `RequireFreezeStruct`, …) are referenced by string from
//! `build.rs` — rename them only with a matching update there.

pub mod lint;
pub use lint::*;

mod forbid_as_primitive;
mod forbid_keys_remove;
mod forbid_saturating_math;
mod pallet_index;
mod require_extrinsic_benchmarks;
mod require_freeze_struct;

pub use forbid_as_primitive::ForbidAsPrimitiveConversion;
pub use forbid_keys_remove::ForbidKeysRemoveCall;
pub use forbid_saturating_math::ForbidSaturatingMath;
pub use pallet_index::RequireExplicitPalletIndex;
pub use require_extrinsic_benchmarks::RequireExtrinsicBenchmarks;
pub use require_freeze_struct::RequireFreezeStruct;
