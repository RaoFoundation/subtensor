#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! Bad hotkey-swap repair — genesis-only cases.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_fix_bad_hk_swap_only_genesis() {
    new_test_ext(1).execute_with(|| {
        use crate::migrations::migrate_fix_bad_hk_swap::*;
        const MIGRATION_NAME: &[u8] = b"migrate_fix_bad_hk_swap";

        let coldkey = "5H1WgA7ET3FmEarJK6qc1vaTWbNd6g41mgvyLRkysrH4MDdo";
        let account_id32: AccountId32 =
            AccountId32::from_ss58check(coldkey).expect("Invalid coldkey");
        let mut account_id32_slice: &[u8] = account_id32.as_ref();
        let coldkey_account_id: <Test as Config>::AccountId =
            <Test as Config>::AccountId::decode(&mut account_id32_slice).expect("Invalid coldkey");
        let netuid = NetUid::from(59);
        // Setup
        // Add subnet 59
        add_network(netuid, 10, 0);
        SubtokenEnabled::<Test>::insert(netuid, true);
        SubnetMechanism::<Test>::insert(netuid, 1);

        // Add stake to hotkey matching
        let hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
        let account_id32: AccountId32 =
            AccountId32::from_ss58check(hotkey).expect("Invalid hotkey");
        let mut account_id32_slice: &[u8] = account_id32.as_ref();
        let hotkey_account_id: <Test as Config>::AccountId =
            <Test as Config>::AccountId::decode(&mut account_id32_slice).expect("Invalid hotkey");

        // Give balance to coldkey
        add_balance_to_coldkey_account(&coldkey_account_id, 100_000222.into());
        // Give stake to hotkey
        let stake_added = 222222.into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            stake_added,
        );

        // Check genesis hash
        let genesis_hash = frame_system::Pallet::<Test>::block_hash(0);
        let genesis_bytes = genesis_hash.as_ref();
        let mainnet_genesis =
            hex_literal::hex!("2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03");
        assert_ne!(genesis_bytes, mainnet_genesis);

        // Run migration
        let w = migrate_fix_bad_hk_swap::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Check stake did not change
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            ),
            stake_added
        );
    });
}

#[test]
fn test_migrate_fix_bad_hk_swap_runs_on_mainnet_genesis() {
    new_test_ext(1).execute_with(|| {
        use crate::migrations::migrate_fix_bad_hk_swap::*;
        const MIGRATION_NAME: &[u8] = b"migrate_fix_bad_hk_swap";

        let coldkey = "5H1WgA7ET3FmEarJK6qc1vaTWbNd6g41mgvyLRkysrH4MDdo";
        let account_id32: AccountId32 =
            AccountId32::from_ss58check(coldkey).expect("Invalid coldkey");
        let mut account_id32_slice: &[u8] = account_id32.as_ref();
        let coldkey_account_id: <Test as Config>::AccountId =
            <Test as Config>::AccountId::decode(&mut account_id32_slice).expect("Invalid coldkey");
        let netuid = NetUid::from(59);
        // Setup
        // Add subnet 59
        add_network(netuid, 10, 0);
        SubtokenEnabled::<Test>::insert(netuid, true);
        SubnetMechanism::<Test>::insert(netuid, 1);

        // Add stake to hotkey matching
        let hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
        let account_id32: AccountId32 =
            AccountId32::from_ss58check(hotkey).expect("Invalid hotkey");
        let mut account_id32_slice: &[u8] = account_id32.as_ref();
        let hotkey_account_id: <Test as Config>::AccountId =
            <Test as Config>::AccountId::decode(&mut account_id32_slice).expect("Invalid hotkey");

        // Give balance to coldkey
        add_balance_to_coldkey_account(&coldkey_account_id, 100_000222.into());
        // Give stake to hotkey
        let stake_added = 222222.into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            stake_added,
        );

        // Set genesis hash to mainnet genesis
        let mainnet_genesis =
            hex_literal::hex!("2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03");
        frame_system::BlockHash::<Test>::insert(0, H256::from_slice(&mainnet_genesis));
        // Check genesis hash
        let genesis_hash = frame_system::Pallet::<Test>::block_hash(0);
        let genesis_bytes = genesis_hash.as_ref();
        assert_eq!(genesis_bytes, mainnet_genesis);

        // Run migration
        let w = migrate_fix_bad_hk_swap::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Check stake DID change
        assert_ne!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            ),
            stake_added
        );
    });
}
