//! Tests for `migrate_clear_v1_storage` clearing removed MevShield v1 items.

use crate::migrations::migrate_clear_v1_storage::migrate_clear_v1_storage;
use crate::mock::*;
use crate::{AuthorKeys, CurrentKey, HasMigrationRun, NextKey};
use frame_support::BoundedVec;
use sp_io::hashing::twox_128;

#[test]
fn migrate_clear_v1_storage_works() {
    new_test_ext().execute_with(|| {
        // Seed legacy storage that should be cleared.
        seed_legacy_map("Submissions", 5);
        seed_legacy_map("KeyHashByBlock", 3);
        CurrentKey::<Test>::put(valid_shield_enc_key());

        // Current storage that must survive.
        NextKey::<Test>::put(valid_shield_enc_key());
        AuthorKeys::<Test>::insert(author(1), valid_shield_enc_key_b());

        // Sanity: legacy values exist.
        assert_eq!(count_keys("Submissions"), 5);
        assert_eq!(count_keys("KeyHashByBlock"), 3);
        assert!(CurrentKey::<Test>::get().is_some());

        migrate_clear_v1_storage::<Test>();

        // Legacy storage cleared.
        assert_eq!(count_keys("Submissions"), 0);
        assert_eq!(count_keys("KeyHashByBlock"), 0);
        assert!(CurrentKey::<Test>::get().is_none());

        // Current storage untouched.
        assert_eq!(NextKey::<Test>::get(), Some(valid_shield_enc_key()));
        assert_eq!(
            AuthorKeys::<Test>::get(author(1)),
            Some(valid_shield_enc_key_b())
        );

        // Migration was recorded.
        let mig_key = BoundedVec::truncate_from(b"migrate_clear_v1_storage".to_vec());
        assert!(HasMigrationRun::<Test>::get(&mig_key));

        // Idempotent: re-run doesn't touch new data.
        CurrentKey::<Test>::put(valid_shield_enc_key_b());
        migrate_clear_v1_storage::<Test>();
        assert_eq!(CurrentKey::<Test>::get(), Some(valid_shield_enc_key_b()));
    });
}

fn seed_legacy_map(storage_name: &str, count: u32) {
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&twox_128(b"MevShield"));
    prefix.extend_from_slice(&twox_128(storage_name.as_bytes()));

    for i in 0..count {
        let mut key = prefix.clone();
        key.extend_from_slice(&i.to_le_bytes());
        sp_io::storage::set(&key, &[1u8; 32]);
    }
}

fn count_keys(storage_name: &str) -> u32 {
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&twox_128(b"MevShield"));
    prefix.extend_from_slice(&twox_128(storage_name.as_bytes()));

    let mut count = 0u32;
    let mut next_key = sp_io::storage::next_key(&prefix);
    while let Some(key) = next_key {
        if !key.starts_with(&prefix) {
            break;
        }
        count += 1;
        next_key = sp_io::storage::next_key(&key);
    }
    count
}
