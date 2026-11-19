use super::*;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use subtensor_runtime_common::GammaBalance;

/// Drops the spender from every gamma deposit and service: a deposit's hotkey spends it, at the
/// services listed on its subnet.
pub fn migrate_drop_gamma_spenders<T: Config>() -> Weight {
    let migration_name = b"migrate_drop_gamma_spenders".to_vec();
    let mut weight = T::DbWeight::get().reads(1);
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }
    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    let mut translated: u64 = 0;
    GammaDeposits::<T>::translate::<
        (
            T::AccountId,
            T::AccountId,
            GammaBalance,
            Option<GammaDailySpendCap>,
        ),
        _,
    >(|_, _, (coldkey, _spender, balance, daily_spend_cap)| {
        translated = translated.saturating_add(1);
        Some(GammaDeposit {
            coldkey,
            balance,
            daily_spend_cap,
        })
    });
    Services::<T>::translate::<(BoundedVec<u8, ConstU32<MAX_SERVICE_URL_LEN>>, T::AccountId), _>(
        |_, _, (url, _spender)| {
            translated = translated.saturating_add(1);
            Some(ServiceInfo { url })
        },
    );
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(translated, translated));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(&migration_name)
    );
    weight
}
