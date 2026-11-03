//! One-shot storage migrations for the lending pallet.
mod migrate_drop_position_hotkey;
mod migrate_lift_params_ema_band;
mod migrate_position_terms_version;
mod migrate_warm_references;
#[allow(unused_imports)]
pub use migrate_drop_position_hotkey::{
    Migration as DropPositionHotkeyMigration, migrate_drop_position_hotkey,
};
#[cfg(test)]
pub(crate) use migrate_drop_position_hotkey::{PositionBeforeHotkeyDrop, RETIRED_HOTKEY_INDEXES};
#[allow(unused_imports)]
pub use migrate_lift_params_ema_band::{
    Migration as LiftParamsEmaBandMigration, migrate_lift_params_ema_band,
};
#[cfg(any(test, feature = "try-runtime"))]
pub(crate) use migrate_position_terms_version::PositionBeforeTermsVersion;
#[allow(unused_imports)]
pub use migrate_position_terms_version::{
    Migration as PositionTermsVersionMigration, migrate_position_terms_version,
};
#[allow(unused_imports)]
pub use migrate_warm_references::{Migration as WarmReferencesMigration, migrate_warm_references};
