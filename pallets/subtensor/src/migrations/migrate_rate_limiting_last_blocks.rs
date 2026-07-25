use crate::Vec;
use crate::{Config, HasMigrationRun, Pallet};
use alloc::string::String;
use codec::Decode;
use frame_support::traits::Get;
use frame_support::weights::Weight;
use sp_io::hashing::twox_128;
use sp_io::storage::{clear, get};

/// Entry point that migrates obsolete last-block rate-limit storage values into the current maps
/// (`NetworkLastRegistered`, `LastTxBlock`, childkey/delegate-take variants).
pub fn migrate_obsolete_rate_limiting_last_blocks_storage<T: Config>() -> Weight {
    migrate_network_last_registered::<T>()
        .saturating_add(migrate_last_tx_block::<T>())
        .saturating_add(migrate_last_tx_block_childkey_take::<T>())
        .saturating_add(migrate_last_tx_block_delegate_take::<T>())
}

/// Migrates `NetworkLastRegistered` last-block values into the current rate-limit storage shape.
///
/// Idempotency key (frozen): `migrate_network_last_registered`.
pub fn migrate_network_last_registered<T: Config>() -> Weight {
    let migration_name = b"migrate_network_last_registered".to_vec();
    let pallet_name = "SubtensorModule";
    let storage_name = "NetworkLastRegistered";

    migrate_value::<T, _>(migration_name, pallet_name, storage_name, |limit| {
        Pallet::<T>::set_network_last_lock_block(limit);
    })
}

#[allow(deprecated)]
/// Migrates per-account `LastTxBlock` rate-limit timestamps into the current storage shape.
///
/// Idempotency key (frozen): `migrate_last_tx_block`.
pub fn migrate_last_tx_block<T: Config>() -> Weight {
    let migration_name = b"migrate_last_tx_block".to_vec();

    migrate_last_block_map::<T, _, _>(
        migration_name,
        || crate::LastTxBlock::<T>::drain().collect::<Vec<_>>(),
        |account, block| {
            Pallet::<T>::set_last_tx_block(&account, block);
        },
    )
}

#[allow(deprecated)]
/// Migrates `LastTxBlockChildkeyTake` rate-limit timestamps into the current storage shape.
///
/// Idempotency key (frozen): `migrate_last_tx_block_childkey_take`.
pub fn migrate_last_tx_block_childkey_take<T: Config>() -> Weight {
    let migration_name = b"migrate_last_tx_block_childkey_take".to_vec();

    migrate_last_block_map::<T, _, _>(
        migration_name,
        || crate::LastTxBlockChildKeyTake::<T>::drain().collect::<Vec<_>>(),
        |account, block| {
            Pallet::<T>::set_last_tx_block_childkey(&account, block);
        },
    )
}

#[allow(deprecated)]
/// Migrates `LastTxBlockDelegateTake` rate-limit timestamps into the current storage shape.
///
/// Idempotency key (frozen): `migrate_last_tx_block_delegate_take`.
pub fn migrate_last_tx_block_delegate_take<T: Config>() -> Weight {
    let migration_name = b"migrate_last_tx_block_delegate_take".to_vec();

    migrate_last_block_map::<T, _, _>(
        migration_name,
        || crate::LastTxBlockDelegateTake::<T>::drain().collect::<Vec<_>>(),
        |account, block| {
            Pallet::<T>::set_last_tx_block_delegate_take(&account, block);
        },
    )
}

fn migrate_value<T, SetValueFunction>(
    migration_name: Vec<u8>,
    pallet_name: &str,
    storage_name: &str,
    set_value: SetValueFunction,
) -> Weight
where
    T: Config,
    SetValueFunction: Fn(u64 /*limit in blocks*/),
{
    // Initialize the weight with one read operation.
    let mut weight = T::DbWeight::get().reads(1);

    // Check if the migration has already run
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!("Migration '{migration_name:?}' has already run. Skipping.",);
        return weight;
    }
    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    let pallet_name_hash = twox_128(pallet_name.as_bytes());
    let storage_name_hash = twox_128(storage_name.as_bytes());
    let full_key = [pallet_name_hash, storage_name_hash].concat();

    if let Some(value_bytes) = get(&full_key) {
        if let Ok(rate_limit) = Decode::decode(&mut &value_bytes[..]) {
            set_value(rate_limit);
        }

        clear(&full_key);
    }

    weight = weight.saturating_add(T::DbWeight::get().writes(2));
    weight = weight.saturating_add(T::DbWeight::get().reads(1));

    // Mark the migration as completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed.",
        String::from_utf8_lossy(&migration_name)
    );

    // Return the migration weight.
    weight
}

fn migrate_last_block_map<T, GetValuesFunction, SetValueFunction>(
    migration_name: Vec<u8>,
    get_values: GetValuesFunction,
    set_value: SetValueFunction,
) -> Weight
where
    T: Config,
    GetValuesFunction: Fn() -> Vec<(T::AccountId, u64)>, // (account, limit in blocks)
    SetValueFunction: Fn(T::AccountId, u64),
{
    // Initialize the weight with one read operation.
    let mut weight = T::DbWeight::get().reads(1);

    // Check if the migration has already run
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!("Migration '{migration_name:?}' has already run. Skipping.",);
        return weight;
    }
    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    let key_values = get_values();
    weight = weight.saturating_add(T::DbWeight::get().reads(key_values.len() as u64));

    for (account, block) in key_values.into_iter() {
        set_value(account, block);

        weight = weight.saturating_add(T::DbWeight::get().writes(2));
        weight = weight.saturating_add(T::DbWeight::get().reads(1));
    }

    // Mark the migration as completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed.",
        String::from_utf8_lossy(&migration_name)
    );

    // Return the migration weight.
    weight
}
