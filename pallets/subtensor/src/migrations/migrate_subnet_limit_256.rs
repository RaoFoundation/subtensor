use super::*;
use codec::{Decode, Encode};
use frame_support::{
    ensure,
    traits::{Get, OnRuntimeUpgrade},
    weights::Weight,
};
use log;
use scale_info::prelude::string::String;
use sp_std::marker::PhantomData;

/// Migration id stored in `HasMigrationRun`.
pub const MIGRATION_NAME: &[u8] = b"subnet_limit_256";
/// Target subnet slot count for v440.
pub const TARGET: u16 = 256;

/// Pre-upgrade snapshot used by try-runtime / unit tests.
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug)]
pub struct PreUpgradeState {
    pub previous_limit: u16,
    pub already_run: bool,
}

/// Captures pre-migration state for try-runtime validation.
pub fn pre_migrate_subnet_limit_256<T: Config>() -> PreUpgradeState {
    PreUpgradeState {
        previous_limit: SubnetLimit::<T>::get(),
        already_run: HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
    }
}

/// Validates post-migration invariants against the captured pre-state.
///
/// - Values below 256 become 256
/// - Values already at/above 256 are preserved
/// - A previously-run migration is a no-op (limit unchanged, flag stays set)
pub fn post_migrate_subnet_limit_256<T: Config>(
    state: PreUpgradeState,
) -> Result<(), &'static str> {
    let current = SubnetLimit::<T>::get();
    ensure!(
        HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
        "subnet_limit_256: HasMigrationRun not set after migration"
    );

    if state.already_run {
        ensure!(
            current == state.previous_limit,
            "subnet_limit_256: re-run changed SubnetLimit"
        );
    } else if state.previous_limit < TARGET {
        ensure!(
            current == TARGET,
            "subnet_limit_256: SubnetLimit was not raised to 256"
        );
    } else {
        ensure!(
            current == state.previous_limit,
            "subnet_limit_256: SubnetLimit above 256 was not preserved"
        );
    }
    Ok(())
}

/// v440: doubles the subnet slot count to 256. Safe alongside the emission
/// gate: the gate bar is a property of the demand distribution, so the new
/// empty slots neither dilute the head nor lower the bar.
pub fn migrate_subnet_limit_256<T: Config>() -> Weight {
    let mig_name: Vec<u8> = MIGRATION_NAME.to_vec();

    #[cfg(feature = "try-runtime")]
    let pre_state = pre_migrate_subnet_limit_256::<T>();

    // 1 read: HasMigrationRun flag
    let mut total_weight = T::DbWeight::get().reads(1);

    // Run once guard
    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(&mig_name)
        );

        #[cfg(feature = "try-runtime")]
        post_migrate_subnet_limit_256::<T>(pre_state)
            .expect("subnet_limit_256 try-runtime post-check (noop path)");

        return total_weight;
    }
    log::info!("Running migration '{}'", String::from_utf8_lossy(&mig_name));

    let current: u16 = SubnetLimit::<T>::get();
    total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));

    if current < TARGET {
        SubnetLimit::<T>::put(TARGET);
        total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
        log::info!("SubnetLimit updated: {current} -> {TARGET}");
    } else {
        log::info!("SubnetLimit already at or above {TARGET} ({current}), no update performed.");
    }

    // Mark as done
    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(&mig_name)
    );

    #[cfg(feature = "try-runtime")]
    post_migrate_subnet_limit_256::<T>(pre_state)
        .expect("subnet_limit_256 try-runtime post-check");

    total_weight
}

/// `OnRuntimeUpgrade` wrapper so try-runtime can exercise pre/post checks when
/// this migration is invoked through the trait (and mirrors the free function
/// used from pallet hooks).
pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_subnet_limit_256::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
        let state = pre_migrate_subnet_limit_256::<T>();
        log::info!(
            target: "runtime",
            "try-runtime::pre_upgrade subnet_limit_256: previous_limit={}, already_run={}",
            state.previous_limit,
            state.already_run,
        );
        Ok(state.encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
        let state = PreUpgradeState::decode(&mut &state[..])
            .map_err(|_| "subnet_limit_256: failed to decode pre_upgrade state")?;
        post_migrate_subnet_limit_256::<T>(state).map_err(Into::into)
    }
}
