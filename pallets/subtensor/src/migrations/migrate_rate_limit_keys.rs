use alloc::string::String;
use codec::{Decode, Encode};
use frame_support::traits::Get;
use frame_support::weights::Weight;
use sp_io::hashing::twox_128;
use sp_io::storage;
use sp_std::{collections::btree_set::BTreeSet, vec::Vec};
use subtensor_runtime_common::NetUid;

use crate::{
    ChildKeys, Config, Delegates, HasMigrationRun, LastRateLimitedBlock, ParentKeys,
    PendingChildKeys, RateLimitKey,
};

const MIGRATION_NAME: &[u8] = b"migrate_rate_limit_keys";

#[allow(dead_code)]
#[derive(Decode)]
enum RateLimitKeyV0<AccountId> {
    SetSNOwnerHotkey(NetUid),
    NetworkLastRegistered,
    LastTxBlock(AccountId),
    LastTxBlockChildKeyTake(AccountId),
    LastTxBlockDelegateTake(AccountId),
}

/// Rewrites `LastRateLimitedBlock` keys into the typed `RateLimitKey` encoding used by current rate-limit checks.
///
/// Idempotency key (frozen): `migrate_rate_limit_keys`.
pub fn migrate_rate_limit_keys<T: Config>() -> Weight
where
    T::AccountId: Ord + Clone,
{
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(MIGRATION_NAME) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(MIGRATION_NAME)
        );
        return weight;
    }

    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(MIGRATION_NAME)
    );

    let (child_accounts, child_weight) = collect_child_related_accounts::<T>();
    let (delegate_accounts, delegate_weight) = collect_delegate_accounts::<T>();
    weight = weight.saturating_add(child_weight);
    weight = weight.saturating_add(delegate_weight);

    let prefix = storage_prefix("SubtensorModule", "LastRateLimitedBlock");
    let mut cursor = prefix.clone();
    let mut entries = Vec::new();

    while let Some(next_key) = storage::next_key(&cursor) {
        if !next_key.starts_with(&prefix) {
            break;
        }
        if let Some(value) = storage::get(&next_key) {
            entries.push((next_key.clone(), value));
        }
        cursor = next_key;
    }

    weight = weight.saturating_add(T::DbWeight::get().reads(entries.len() as u64));

    let mut migrated_network = 0u64;
    let mut migrated_last_tx = 0u64;
    let mut migrated_child_take = 0u64;
    let mut migrated_delegate_take = 0u64;

    for (old_storage_key, value_bytes) in entries {
        if value_bytes.is_empty() {
            continue;
        }

        let Some(encoded_key) = old_storage_key.get(prefix.len()..) else {
            continue;
        };
        if encoded_key.is_empty() {
            continue;
        }

        let Some(decoded_legacy) = decode_legacy::<T>(encoded_key) else {
            // Unknown entry – skip to avoid clobbering valid data.
            continue;
        };

        let legacy_value = match decode_value(&value_bytes) {
            Some(v) => v,
            None => continue,
        };

        let Some(modern_key) =
            legacy_to_modern(decoded_legacy, &child_accounts, &delegate_accounts)
        else {
            continue;
        };
        let new_storage_key = LastRateLimitedBlock::<T>::hashed_key_for(&modern_key);
        weight = weight.saturating_add(T::DbWeight::get().reads(1));

        let merged_value = storage::get(&new_storage_key)
            .and_then(|data| decode_value(&data))
            .map_or(legacy_value, |current| {
                core::cmp::max(current, legacy_value)
            });

        storage::set(&new_storage_key, &merged_value.encode());
        if new_storage_key != old_storage_key {
            storage::clear(&old_storage_key);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }

        weight = weight.saturating_add(T::DbWeight::get().writes(1));
        match &modern_key {
            RateLimitKey::NetworkLastRegistered => {
                migrated_network = migrated_network.saturating_add(1);
            }
            RateLimitKey::LastTxBlock(_) => {
                migrated_last_tx = migrated_last_tx.saturating_add(1);
            }
            RateLimitKey::LastTxBlockChildKeyTake(_) => {
                migrated_child_take = migrated_child_take.saturating_add(1);
            }
            RateLimitKey::LastTxBlockDelegateTake(_) => {
                migrated_delegate_take = migrated_delegate_take.saturating_add(1);
            }
            _ => {}
        }
    }

    HasMigrationRun::<T>::insert(MIGRATION_NAME, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed. network={}, last_tx={}, child_take={}, delegate_take={}",
        String::from_utf8_lossy(MIGRATION_NAME),
        migrated_network,
        migrated_last_tx,
        migrated_child_take,
        migrated_delegate_take
    );

    weight
}

fn storage_prefix(pallet: &str, storage: &str) -> Vec<u8> {
    let pallet_hash = twox_128(pallet.as_bytes());
    let storage_hash = twox_128(storage.as_bytes());
    [pallet_hash, storage_hash].concat()
}

fn decode_legacy<T: Config>(bytes: &[u8]) -> Option<RateLimitKeyV0<T::AccountId>> {
    let mut slice = bytes;
    let decoded = RateLimitKeyV0::<T::AccountId>::decode(&mut slice).ok()?;
    if slice.is_empty() {
        Some(decoded)
    } else {
        None
    }
}

fn decode_value(bytes: &[u8]) -> Option<u64> {
    let mut slice = bytes;
    u64::decode(&mut slice).ok()
}

fn legacy_to_modern<AccountId: Ord + Clone>(
    legacy: RateLimitKeyV0<AccountId>,
    child_accounts: &BTreeSet<AccountId>,
    delegate_accounts: &BTreeSet<AccountId>,
) -> Option<RateLimitKey<AccountId>> {
    match legacy {
        RateLimitKeyV0::SetSNOwnerHotkey(_) => None,
        RateLimitKeyV0::NetworkLastRegistered => Some(RateLimitKey::NetworkLastRegistered),
        RateLimitKeyV0::LastTxBlock(account) => Some(RateLimitKey::LastTxBlock(account)),
        RateLimitKeyV0::LastTxBlockChildKeyTake(account) => {
            if child_accounts.contains(&account) {
                Some(RateLimitKey::LastTxBlockChildKeyTake(account))
            } else {
                None
            }
        }
        RateLimitKeyV0::LastTxBlockDelegateTake(account) => {
            if delegate_accounts.contains(&account) {
                Some(RateLimitKey::LastTxBlockDelegateTake(account))
            } else {
                None
            }
        }
    }
}

fn collect_child_related_accounts<T: Config>() -> (BTreeSet<T::AccountId>, Weight)
where
    T::AccountId: Ord + Clone,
{
    let mut accounts = BTreeSet::new();
    let mut reads = 0u64;

    for (parent, _, children) in ChildKeys::<T>::iter() {
        accounts.insert(parent.clone());
        for (_, child) in children {
            accounts.insert(child.clone());
        }
        reads = reads.saturating_add(1);
    }

    for (_, parent, (children, _)) in PendingChildKeys::<T>::iter() {
        accounts.insert(parent.clone());
        for (_, child) in children {
            accounts.insert(child.clone());
        }
        reads = reads.saturating_add(1);
    }

    for (child, _, parents) in ParentKeys::<T>::iter() {
        accounts.insert(child.clone());
        for (_, parent) in parents {
            accounts.insert(parent.clone());
        }
        reads = reads.saturating_add(1);
    }

    (accounts, T::DbWeight::get().reads(reads))
}

fn collect_delegate_accounts<T: Config>() -> (BTreeSet<T::AccountId>, Weight)
where
    T::AccountId: Ord + Clone,
{
    let mut accounts = BTreeSet::new();
    let mut reads = 0u64;

    for (account, _) in Delegates::<T>::iter() {
        accounts.insert(account.clone());
        reads = reads.saturating_add(1);
    }

    (accounts, T::DbWeight::get().reads(reads))
}
