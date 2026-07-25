#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! root claimed overclaim repair.

use super::helpers::*;
use super::prelude::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::migration::test_migrate_fix_root_claimed_overclaim --exact --nocapture
#[test]
fn test_migrate_fix_root_claimed_overclaim() {
    use crate::migrations::migrate_fix_root_claimed_overclaim::*;

    let new_hotkey = decode_account_id32_test("5H6BqkzjYvViiqp7rQLXjpnaEmW7U9CoKxXhQ4efMqtX1mQw");
    let untouched_hotkey = U256::from(7777_u64);
    let coldkey_a = U256::from(42_u64);
    let coldkey_b = U256::from(43_u64);

    let root_netuid = NetUid::from(0_u16);
    let netuid_a = NetUid::from(27_u16);
    let netuid_b = NetUid::from(1_u16);

    let mainnet_genesis =
        hex_literal::hex!("2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03");
    const MIGRATION_NAME: &[u8] = b"migrate_fix_root_claimed_overclaim";

    // CASE 1: new hotkey has no root stake → RootClaimable is cleared
    new_test_ext(1).execute_with(|| {
        frame_system::BlockHash::<Test>::insert(0u64, H256::from_slice(&mainnet_genesis));

        RootClaimable::<Test>::mutate(new_hotkey, |map| {
            map.insert(netuid_a, I96F32::from_num(500_000_u64));
            map.insert(netuid_b, I96F32::from_num(300_000_u64));
        });
        RootClaimed::<Test>::insert((netuid_a, new_hotkey, coldkey_a), 999u128);
        RootClaimed::<Test>::insert((netuid_b, new_hotkey, coldkey_b), 111u128);

        // Unrelated hotkey's claimed entry must stay intact
        RootClaimable::<Test>::mutate(untouched_hotkey, |map| {
            map.insert(netuid_a, I96F32::from_num(42_u64));
        });
        RootClaimed::<Test>::insert((netuid_a, untouched_hotkey, coldkey_a), 555u128);

        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        let w = migrate_fix_root_claimed_overclaim::<Test>();
        assert!(!w.is_zero());
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        assert!(
            RootClaimable::<Test>::get(new_hotkey).is_empty(),
            "new hotkey RootClaimable must be cleared"
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid_a, new_hotkey, coldkey_a)),
            999u128,
            "RootClaimed entries must be left intact"
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid_b, new_hotkey, coldkey_b)),
            111u128,
            "RootClaimed entries must be left intact"
        );

        assert_eq!(
            RootClaimable::<Test>::get(untouched_hotkey)
                .get(&netuid_a)
                .copied(),
            Some(I96F32::from_num(42_u64))
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid_a, untouched_hotkey, coldkey_a)),
            555u128
        );
    });

    // CASE 2: new hotkey has root stake → state is preserved
    new_test_ext(1).execute_with(|| {
        frame_system::BlockHash::<Test>::insert(0u64, H256::from_slice(&mainnet_genesis));

        RootClaimable::<Test>::mutate(new_hotkey, |map| {
            map.insert(netuid_a, I96F32::from_num(500_000_u64));
        });
        RootClaimed::<Test>::insert((netuid_a, new_hotkey, coldkey_a), 999u128);

        TotalHotkeyAlpha::<Test>::insert(new_hotkey, root_netuid, AlphaBalance::from(1_000u64));

        let w = migrate_fix_root_claimed_overclaim::<Test>();
        assert!(!w.is_zero());
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        assert_eq!(
            RootClaimable::<Test>::get(new_hotkey)
                .get(&netuid_a)
                .copied(),
            Some(I96F32::from_num(500_000_u64)),
            "must not clear when new hotkey still holds root stake"
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid_a, new_hotkey, coldkey_a)),
            999u128
        );
    });

    // CASE 3: idempotency — second run is a no-op
    new_test_ext(1).execute_with(|| {
        frame_system::BlockHash::<Test>::insert(0u64, H256::from_slice(&mainnet_genesis));
        HasMigrationRun::<Test>::insert(MIGRATION_NAME.to_vec(), true);

        RootClaimable::<Test>::mutate(new_hotkey, |map| {
            map.insert(netuid_a, I96F32::from_num(777_u64));
        });

        let w = migrate_fix_root_claimed_overclaim::<Test>();
        assert_eq!(
            w,
            <Test as frame_system::Config>::DbWeight::get().reads(1),
            "second run should only read the migration flag"
        );
        assert_eq!(
            RootClaimable::<Test>::get(new_hotkey)
                .get(&netuid_a)
                .copied(),
            Some(I96F32::from_num(777_u64))
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::migration::test_migrate_fix_root_claimed_incorrect_genesis --exact --nocapture
#[test]
fn test_migrate_fix_root_claimed_incorrect_genesis() {
    use crate::migrations::migrate_fix_root_claimed_overclaim::*;

    let old_hotkey = decode_account_id32_test("5GmvyePN9aYErXBBhBnxZKGoGk4LKZApE4NkaSzW62CYCYNA");
    let new_hotkey = decode_account_id32_test("5H6BqkzjYvViiqp7rQLXjpnaEmW7U9CoKxXhQ4efMqtX1mQw");
    let coldkey = U256::from(42_u64);

    let netuid_target = NetUid::from(27_u16);
    let netuid_other = NetUid::from(1_u16);

    let mainnet_genesis =
        hex_literal::hex!("2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03");
    const MIGRATION_NAME: &[u8] = b"migrate_fix_root_claimed_overclaim";

    // CASE 2: non-mainnet genesis — full no-op
    new_test_ext(1).execute_with(|| {
        frame_system::BlockHash::<Test>::insert(0u64, H256::from_low_u64_be(0xdeadbeef));

        RootClaimable::<Test>::mutate(new_hotkey, |map| {
            map.insert(netuid_target, I96F32::from_num(123_u64));
        });
        Alpha::<Test>::insert(
            (new_hotkey, coldkey, netuid_target),
            U64F64::from_num(1_000_u64),
        );

        let w = migrate_fix_root_claimed_overclaim::<Test>();
        assert!(
            !w.is_zero(),
            "weight must be non-zero (writes migration flag)"
        );
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        assert!(
            RootClaimable::<Test>::get(old_hotkey).is_empty(),
            "migration must not touch storage on non-mainnet"
        );
        assert!(
            RootClaimable::<Test>::get(new_hotkey).contains_key(&netuid_target),
            "new_hotkey data must remain untouched on non-mainnet"
        );
    });
}
