//! One-shot storage migrations for the lending pallet.
mod migrate_lift_params_ema_band;
mod migrate_warm_references;
#[allow(unused_imports)]
pub use migrate_lift_params_ema_band::{
    Migration as LiftParamsEmaBandMigration, migrate_lift_params_ema_band,
};
#[allow(unused_imports)]
pub use migrate_warm_references::{Migration as WarmReferencesMigration, migrate_warm_references};
