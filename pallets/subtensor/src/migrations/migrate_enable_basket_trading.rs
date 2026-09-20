use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use sp_std::vec::Vec;

/// `HasMigrationRun` key for this migration.
pub const MIGRATION_NAME: &[u8] = b"enable_basket_trading_v1";

/// Turns validator-directed basket trading on: sets [`crate::BasketTradingEnabled`] to
/// `true` once, stamped in `HasMigrationRun`.
///
/// A one-shot stamped migration rather than a changed storage default, deliberately:
/// * it is deterministic under try-runtime (the first pass stamps and flips, every later
///   pass is one read), and
/// * `AdminUtils::sudo_set_basket_trading_enabled(false)` stays a durable emergency off —
///   a later runtime upgrade never re-enables trading behind governance's back, which a
///   `true` default would do the moment the explicit `false` were killed.
///
/// Chains where trading is already on (devnet, testnet) only receive the stamp.
pub fn migrate_enable_basket_trading<T: Config>() -> Weight {
    let mig_name: Vec<u8> = MIGRATION_NAME.to_vec();
    let mig_name_str = String::from_utf8_lossy(&mig_name);

    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!("Migration '{mig_name_str}' already executed - skipping");
        return total_weight;
    }

    log::info!("Running migration '{mig_name_str}'");

    total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
    if BasketTradingEnabled::<T>::get() {
        log::info!("Migration '{mig_name_str}': basket trading already enabled");
    } else {
        BasketTradingEnabled::<T>::put(true);
        total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
        log::info!("Migration '{mig_name_str}': basket trading enabled");
    }

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!("Migration '{mig_name_str}' completed");

    total_weight
}

/// [`OnRuntimeUpgrade`] wrapper for [`migrate_enable_basket_trading`], registered in the
/// runtime `Migrations` tuple so try-runtime validates the flip against real network state.
pub mod enable_basket_trading {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use codec::{Decode, Encode};
    #[cfg(feature = "try-runtime")]
    use frame_support::ensure;
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;

    /// State carried from `pre_upgrade` to `post_upgrade`: whether the migration had
    /// already run and whether trading was enabled.
    #[cfg(feature = "try-runtime")]
    type PreUpgradeState = (bool, bool);

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_enable_basket_trading::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            let already_run = HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec());
            let enabled = BasketTradingEnabled::<T>::get();
            Ok((already_run, enabled).encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let (already_run, enabled_before): PreUpgradeState =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
            ensure!(
                HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                "the migration must be stamped"
            );
            if already_run {
                // A re-run touches nothing: a governance emergency-off after the first run
                // must survive every later upgrade.
                ensure!(
                    BasketTradingEnabled::<T>::get() == enabled_before,
                    "an already-run migration must not change the trading switch"
                );
            } else {
                ensure!(
                    BasketTradingEnabled::<T>::get(),
                    "basket trading must be enabled by the first run"
                );
            }
            Ok(())
        }
    }
}
