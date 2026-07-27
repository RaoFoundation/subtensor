#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! Shared fixtures for migration unit tests.

use super::prelude::*;

#[allow(clippy::arithmetic_side_effects)]
pub(super) fn close(value: u64, target: u64, eps: u64) {
    assert!(
        (value as i64 - target as i64).abs() < eps as i64,
        "Assertion failed: value = {value}, target = {target}, eps = {eps}"
    )
}

#[allow(clippy::arithmetic_side_effects)]
pub(super) fn test_remove_storage_item<F: FnOnce() -> Weight>(
    migration_name: &'static str,
    pallet_name: &'static str,
    storage_name: &'static str,
    migration: F,
    test_entries_number: i32,
) {
    new_test_ext(1).execute_with(|| {
        let pallet_name = twox_128(pallet_name.as_bytes());
        let storage_name = twox_128(storage_name.as_bytes());
        let prefix = [pallet_name, storage_name].concat();

        // Set up entries to be deleted.
        for i in 0..test_entries_number {
            let hotkey = U256::from(i as u64);
            let coldkey = U256::from(i as u64);
            let key = [prefix.clone(), hotkey.encode(), coldkey.encode()].concat();
            let value = (100 + i, 200 + i);
            put_raw(&key, &value.encode());
        }

        assert!(
            frame_support::storage::unhashed::contains_prefixed_key(&prefix),
            "Entries should exist before migration."
        );
        assert!(
            !HasMigrationRun::<Test>::get(migration_name.as_bytes().to_vec()),
            "Migration should not have run yet."
        );

        // Run migration
        let weight = migration();

        assert!(
            !frame_support::storage::unhashed::contains_prefixed_key(&prefix),
            "All entries should have been removed."
        );
        assert!(
            HasMigrationRun::<Test>::get(migration_name.as_bytes().to_vec()),
            "Migration should be marked as run."
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero.");
    });
}

pub(super) fn decode_account_id32<T: Config>(ss58_string: &str) -> Option<T::AccountId> {
    let account_id32: AccountId32 = AccountId32::from_ss58check(ss58_string).ok()?;
    let mut account_id32_slice: &[u8] = account_id32.as_ref();
    T::AccountId::decode(&mut account_id32_slice).ok()
}

pub(super) fn decode_account_id32_test(ss58_string: &str) -> U256 {
    let account_id32: AccountId32 = AccountId32::from_ss58check(ss58_string).unwrap();
    let mut account_id32_slice: &[u8] = account_id32.as_ref();
    U256::decode(&mut account_id32_slice).unwrap()
}
