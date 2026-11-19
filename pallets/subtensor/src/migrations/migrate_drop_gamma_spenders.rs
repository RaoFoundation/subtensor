use super::*;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use subtensor_runtime_common::GammaBalance;

/// Drops the spender from every gamma deposit and service: a deposit's hotkey spends it, at the
/// services listed on its subnet. Two kinds of deposit pause, so their hotkey spends only once the
/// coldkey sets a cap: the subnet owner's uncapped deposit at the subnet owner hotkey, as gamma
/// emission opens it, and one whose cap is below the least a spend takes.
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
    let mut owner_reads: u64 = 0;
    GammaDeposits::<T>::translate::<
        (
            T::AccountId,
            T::AccountId,
            GammaBalance,
            Option<GammaDailySpendCap>,
        ),
        _,
    >(
        |netuid, hotkey, (coldkey, _spender, balance, daily_spend_cap)| {
            translated = translated.saturating_add(1);
            let pauses = match daily_spend_cap {
                None => {
                    owner_reads = owner_reads.saturating_add(2);
                    SubnetOwnerHotkey::<T>::get(netuid) == hotkey
                        && SubnetOwner::<T>::get(netuid) == coldkey
                }
                Some(GammaDailySpendCap { cap, .. }) => {
                    !cap.is_zero() && cap.to_u64() < crate::MIN_GAMMA_AMOUNT
                }
            };
            Some(GammaDeposit {
                coldkey,
                balance,
                daily_spend_cap: if pauses {
                    Pallet::<T>::full_gamma_cap(Some(GammaBalance::ZERO))
                } else {
                    daily_spend_cap
                },
            })
        },
    );
    Services::<T>::translate::<(BoundedVec<u8, ConstU32<MAX_SERVICE_URL_LEN>>, T::AccountId), _>(
        |_, _, (url, _spender)| {
            translated = translated.saturating_add(1);
            Some(ServiceInfo { url })
        },
    );
    weight = weight.saturating_add(
        T::DbWeight::get().reads_writes(translated.saturating_add(owner_reads), translated),
    );

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(&migration_name)
    );
    weight
}
