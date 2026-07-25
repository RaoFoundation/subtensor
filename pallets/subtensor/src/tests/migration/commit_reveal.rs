#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! commit-reveal v2/v3, settings, disable, timelocked CR.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_commit_reveal_2() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entries
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_commit_reveal_2_v2";

        let pallet_prefix = twox_128("SubtensorModule".as_bytes());
        let storage_prefix_interval = twox_128("WeightCommitRevealInterval".as_bytes());
        let storage_prefix_commits = twox_128("WeightCommits".as_bytes());

        let netuid = NetUid::from(1);
        let interval_value: u64 = 50u64;

        // Construct the full key for WeightCommitRevealInterval
        let mut interval_key = Vec::new();
        interval_key.extend_from_slice(&pallet_prefix);
        interval_key.extend_from_slice(&storage_prefix_interval);
        interval_key.extend_from_slice(&netuid.encode());

        put_raw(&interval_key, &interval_value.encode());

        let test_account: U256 = U256::from(1);

        // Construct the full key for WeightCommits (DoubleMap)
        let mut commit_key = Vec::new();
        commit_key.extend_from_slice(&pallet_prefix);
        commit_key.extend_from_slice(&storage_prefix_commits);

        // First key (netuid) hashed with Twox64Concat
        let netuid_hashed = Twox64Concat::hash(&netuid.encode());
        commit_key.extend_from_slice(&netuid_hashed);

        // Second key (account) hashed with Twox64Concat
        let account_hashed = Twox64Concat::hash(&test_account.encode());
        commit_key.extend_from_slice(&account_hashed);

        let commit_value: (H256, u64) = (H256::from_low_u64_be(42), 100);
        put_raw(&commit_key, &commit_value.encode());

        let stored_interval = get_raw(&interval_key).expect("Expected to get a value");
        assert_eq!(
            u64::decode(&mut &stored_interval[..]).expect("Failed to decode interval value"),
            interval_value
        );

        let stored_commit = get_raw(&commit_key).expect("Expected to get a value");
        assert_eq!(
            <(H256, u64)>::decode(&mut &stored_commit[..]).expect("Failed to decode commit value"),
            commit_value
        );

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_commit_reveal_v2::migrate_commit_reveal_2::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------
        let stored_interval_after = get_raw(&interval_key);
        assert!(
            stored_interval_after.is_none(),
            "WeightCommitRevealInterval should be cleared"
        );

        let stored_commit_after = get_raw(&commit_key);
        assert!(
            stored_commit_after.is_none(),
            "WeightCommits entry should be cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

// Leaving in for reference. Will remove later.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::migration::test_migrate_rao --exact --show-output --nocapture
// #[test]
// fn test_migrate_rao() {
//     new_test_ext(1).execute_with(|| {
//         // Setup initial state
//         let netuid_0: u16 = 0;
//         let netuid_1: u16 = 1;
//         let netuid_2: u16 = 2;
//         let netuid_3: u16 = 3;
//         let hotkey1 = U256::from(1);
//         let hotkey2 = U256::from(2);
//         let coldkey1 = U256::from(3);
//         let coldkey2 = U256::from(4);
//         let coldkey3 = U256::from(5);
//         let stake_amount: u64 = 1_000_000_000;
//         let lock_amount: u64 = 500;
//         NetworkMinLockCost::<Test>::set(500);

//         // Add networks root and alpha
//         add_network(netuid_0, 1, 0);
//         add_network(netuid_1, 1, 0);
//         add_network(netuid_2, 1, 0);
//         add_network(netuid_3, 1, 0);

//         // Set subnet lock
//         SubnetLocked::<Test>::insert(netuid_1, lock_amount);

//         // Add some initial stake
//         EmissionValues::<Test>::insert(netuid_1, 1_000_000_000);
//         EmissionValues::<Test>::insert(netuid_2, 2_000_000_000);
//         EmissionValues::<Test>::insert(netuid_3, 3_000_000_000);

//         Owner::<Test>::insert(hotkey1, coldkey1);
//         Owner::<Test>::insert(hotkey2, coldkey2);
//         Stake::<Test>::insert(hotkey1, coldkey1, stake_amount);
//         Stake::<Test>::insert(hotkey1, coldkey2, stake_amount);
//         Stake::<Test>::insert(hotkey2, coldkey2, stake_amount);
//         Stake::<Test>::insert(hotkey2, coldkey3, stake_amount);

//         // Verify initial conditions
//         assert_eq!(SubnetTAO::<Test>::get(netuid_0), 0);
//         assert_eq!(SubnetTAO::<Test>::get(netuid_1), 0);
//         assert_eq!(SubnetAlphaOut::<Test>::get(netuid_0), 0);
//         assert_eq!(SubnetAlphaOut::<Test>::get(netuid_1), 0);
//         assert_eq!(SubnetAlphaIn::<Test>::get(netuid_0), 0);
//         assert_eq!(SubnetAlphaIn::<Test>::get(netuid_1), 0);
//         assert_eq!(TotalHotkeyShares::<Test>::get(hotkey1, netuid_0), 0);
//         assert_eq!(TotalHotkeyShares::<Test>::get(hotkey1, netuid_1), 0);
//         assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey1, netuid_0), 0);
//         assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey2, netuid_1), 0);

//         // Run migration
//         crate::migrations::migrate_rao::migrate_rao::<Test>();

//         // Verify root subnet (netuid 0) state after migration
//         assert_eq!(SubnetTAO::<Test>::get(netuid_0), 4 * stake_amount); // Root has everything
//         assert_eq!(SubnetTAO::<Test>::get(netuid_1), 1_000_000_000); // Always 1000000000
//         assert_eq!(SubnetAlphaIn::<Test>::get(netuid_0), 1_000_000_000); // Always 1_000_000_000
//         assert_eq!(SubnetAlphaIn::<Test>::get(netuid_1), 1_000_000_000); // Always 1_000_000_000
//         assert_eq!(SubnetAlphaOut::<Test>::get(netuid_0), 4 * stake_amount); // Root has everything.
//         assert_eq!(SubnetAlphaOut::<Test>::get(netuid_1), 0); // No stake outstanding.

//         // Assert share information for hotkey1 on netuid_0
//         assert_eq!(
//             TotalHotkeyShares::<Test>::get(hotkey1, netuid_0),
//             2 * stake_amount
//         ); // Shares
//         // Assert no shares for hotkey1 on netuid_1
//         assert_eq!(TotalHotkeyShares::<Test>::get(hotkey1, netuid_1), 0); // No shares
//         // Assert alpha for hotkey1 on netuid_0
//         assert_eq!(
//             TotalHotkeyAlpha::<Test>::get(hotkey1, netuid_0),
//             2 * stake_amount
//         ); // Alpha
//         // Assert no alpha for hotkey1 on netuid_1
//         assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey1, netuid_1), 0); // No alpha.
//         // Assert share information for hotkey2 on netuid_0
//         assert_eq!(
//             TotalHotkeyShares::<Test>::get(hotkey2, netuid_0),
//             2 * stake_amount
//         ); // Shares
//         // Assert no shares for hotkey2 on netuid_1
//         assert_eq!(TotalHotkeyShares::<Test>::get(hotkey2, netuid_1), 0); // No shares
//         // Assert alpha for hotkey2 on netuid_0
//         assert_eq!(
//             TotalHotkeyAlpha::<Test>::get(hotkey2, netuid_0),
//             2 * stake_amount
//         ); // Alpha
//         // Assert no alpha for hotkey2 on netuid_1
//         assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey2, netuid_1), 0); // No alpha.

//         // Assert stake balances for hotkey1 and coldkey1 on netuid_0
//         assert_eq!(
//             SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
//                 &hotkey1, &coldkey1, netuid_0
//             ),
//             stake_amount
//         );
//         // Assert stake balances for hotkey1 and coldkey2 on netuid_0
//         assert_eq!(
//             SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
//                 &hotkey1, &coldkey2, netuid_0
//             ),
//             stake_amount
//         );
//         // Assert stake balances for hotkey2 and coldkey2 on netuid_0
//         assert_eq!(
//             SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
//                 &hotkey2, &coldkey2, netuid_0
//             ),
//             stake_amount
//         );
//         // Assert stake balances for hotkey2 and coldkey3 on netuid_0
//         assert_eq!(
//             SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
//                 &hotkey2, &coldkey3, netuid_0
//             ),
//             stake_amount
//         );
//         // Assert total stake for hotkey1 on netuid_0
//         assert_eq!(
//             SubtensorModule::get_stake_for_hotkey_on_subnet(&hotkey1, netuid_0),
//             2 * stake_amount
//         );
//         // Assert total stake for hotkey2 on netuid_0
//         assert_eq!(
//             SubtensorModule::get_stake_for_hotkey_on_subnet(&hotkey2, netuid_0),
//             2 * stake_amount
//         );
//         // Increase stake for hotkey1 and coldkey1 on netuid_0
//         mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
//             &hotkey1,
//             &coldkey1,
//             netuid_0,
//             stake_amount,
//         );
//         // Assert updated stake for hotkey1 and coldkey1 on netuid_0
//         assert_eq!(
//             SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
//                 &hotkey1, &coldkey1, netuid_0
//             ),
//             2 * stake_amount
//         );
//         // Assert updated total stake for hotkey1 on netuid_0
//         assert_eq!(
//             SubtensorModule::get_stake_for_hotkey_on_subnet(&hotkey1, netuid_0),
//             3 * stake_amount
//         );
//         // Increase stake for hotkey1 and coldkey1 on netuid_1
//         mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
//             &hotkey1,
//             &coldkey1,
//             netuid_1,
//             stake_amount,
//         );
//         // Assert updated stake for hotkey1 and coldkey1 on netuid_1
//         assert_eq!(
//             SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
//                 &hotkey1, &coldkey1, netuid_1
//             ),
//             stake_amount
//         );
//         // Assert updated total stake for hotkey1 on netuid_1
//         assert_eq!(
//             SubtensorModule::get_stake_for_hotkey_on_subnet(&hotkey1, netuid_1),
//             stake_amount
//         );

//         // Run the coinbase
//         let emission: u64 = 1_000_000_000;
//         SubtensorModule::run_coinbase(I96F32::from_num(emission));
//         close(
//             SubnetTaoInEmission::<Test>::get(netuid_1),
//             emission / 6,
//             100,
//         );
//         close(
//             SubnetTaoInEmission::<Test>::get(netuid_2),
//             2 * (emission / 6),
//             100,
//         );
//         close(
//             SubnetTaoInEmission::<Test>::get(netuid_3),
//             3 * (emission / 6),
//             100,
//         );
//     });
// }

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::migration::test_migrate_subnet_volume --exact --show-output

#[test]
fn test_migrate_revealed_commitments() {
    new_test_ext(1).execute_with(|| {
        // --------------------------------
        // Step 1: Simulate Old Storage Entries
        // --------------------------------
        const MIGRATION_NAME: &str = "migrate_revealed_commitments_v2";

        // Pallet prefix == twox_128("Commitments")
        let pallet_prefix = twox_128("Commitments".as_bytes());
        // Storage item prefix == twox_128("RevealedCommitments")
        let storage_prefix = twox_128("RevealedCommitments".as_bytes());

        // Example keys for the DoubleMap:
        //   Key1 (netuid) uses Identity (no hash)
        //   Key2 (account) uses Twox64Concat
        let netuid = NetUid::from(123);
        let account_id: u64 = 999; // Or however your test `AccountId` is represented

        // Construct the full storage key for `RevealedCommitments(netuid, account_id)`
        let mut storage_key = Vec::new();
        storage_key.extend_from_slice(&pallet_prefix);
        storage_key.extend_from_slice(&storage_prefix);

        // Identity for netuid => no hashing, just raw encode
        storage_key.extend_from_slice(&netuid.encode());

        // Twox64Concat for account
        let account_hashed = Twox64Concat::hash(&account_id.encode());
        storage_key.extend_from_slice(&account_hashed);

        // Simulate an old value we might have stored:
        // For example, the old type was `RevealedData<Balance, ...>`
        // We'll just store a random encoded value for demonstration
        let old_value = (vec![1, 2, 3, 4], 42u64);
        put_raw(&storage_key, &old_value.encode());

        // Confirm the storage value is set
        let stored_value = get_raw(&storage_key).expect("Expected to get a value");
        let decoded_value = <(Vec<u8>, u64)>::decode(&mut &stored_value[..])
            .expect("Failed to decode the old revealed commitments");
        assert_eq!(decoded_value, old_value);

        // Also confirm that the migration has NOT run yet
        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // --------------------------------
        // Step 2: Run the Migration
        // --------------------------------
        let weight = crate::migrations::migrate_upgrade_revealed_commitments::migrate_upgrade_revealed_commitments::<Test>();

        // Migration should be marked as run
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should now be marked as run"
        );

        // --------------------------------
        // Step 3: Verify Migration Effects
        // --------------------------------
        // The old key/value should be removed
        let stored_value_after = get_raw(&storage_key);
        assert!(
            stored_value_after.is_none(),
            "Old storage entry should be cleared"
        );

        // Weight returned should be > 0 (some cost was incurred clearing storage)
        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[test]
fn test_migrate_crv3_commits_add_block() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // 0. Constants / helpers
        // ------------------------------
        const MIG_NAME: &[u8] = b"crv3_commits_add_block_v1";
        let netuid = NetUid::from(99);
        let epoch: u64 = 7;
        let tempo: u16 = 360;

        // ------------------------------
        // 1. Create a network so helper can compute first‑block
        // ------------------------------
        add_network(netuid, tempo, 0);

        // ------------------------------
        // 2. Simulate OLD storage (3‑tuple)
        // ------------------------------
        let who: U256 = U256::from(0xdeadbeef_u64);
        let ciphertext: BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>> =
            vec![1u8, 2, 3].try_into().unwrap();
        let round: RoundNumber = 42;

        let old_queue: VecDeque<_> = VecDeque::from(vec![(who, ciphertext.clone(), round)]);

        CRV3WeightCommits::<Test>::insert(
            NetUidStorageIndex::from(netuid),
            epoch,
            old_queue.clone(),
        );

        // Sanity: entry decodes under old alias
        assert_eq!(
            CRV3WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), epoch),
            old_queue
        );

        assert!(
            !HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should be false before run"
        );

        // ------------------------------
        // 3. Run migration
        // ------------------------------
        let w = crate::migrations::migrate_crv3_commits_add_block::migrate_crv3_commits_add_block::<
            Test,
        >();
        assert!(!w.is_zero(), "weight must be non-zero");

        // ------------------------------
        // 4. Verify results
        // ------------------------------
        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag not set"
        );

        // Old storage must be empty (drained)
        assert!(
            CRV3WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), epoch).is_empty(),
            "old queue should have been drained"
        );

        let new_q = CRV3WeightCommitsV2::<Test>::get(NetUidStorageIndex::from(netuid), epoch);
        assert_eq!(new_q.len(), 1, "exactly one migrated element expected");

        let (who2, commit_block, cipher2, round2) = new_q.front().cloned().unwrap();
        assert_eq!(who2, who);
        assert_eq!(cipher2, ciphertext);
        assert_eq!(round2, round);

        let expected_block = Pallet::<Test>::get_first_block_of_epoch(netuid, epoch);
        assert_eq!(
            commit_block, expected_block,
            "commit_block should equal first block of epoch key"
        );
    });
}

#[test]
fn test_migrate_disable_commit_reveal() {
    const MIG_NAME: &[u8] = b"disable_commit_reveal_v1";
    let netuids = [NetUid::from(1), NetUid::from(2), NetUid::from(42)];

    // ---------------------------------------------------------------------
    // 1. build initial state ─ all nets enabled
    // ---------------------------------------------------------------------
    new_test_ext(1).execute_with(|| {
        for (i, netuid) in netuids.iter().enumerate() {
            add_network(*netuid, 5u16 + i as u16, 0);
            CommitRevealWeightsEnabled::<Test>::insert(*netuid, true);
        }
        assert!(
            !HasMigrationRun::<Test>::get(MIG_NAME),
            "migration flag should be unset before run"
        );

        // -----------------------------------------------------------------
        // 2. run migration
        // -----------------------------------------------------------------
        let w = crate::migrations::migrate_disable_commit_reveal::migrate_disable_commit_reveal::<
            Test,
        >();

        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME),
            "migration flag not set"
        );

        // -----------------------------------------------------------------
        // 3. verify every netuid is now disabled and only one value exists
        // -----------------------------------------------------------------
        for netuid in netuids {
            assert!(
                !CommitRevealWeightsEnabled::<Test>::get(netuid),
                "commit-reveal should be disabled for netuid {netuid}"
            );
        }

        // There should be no stray keys
        let collected: Vec<_> = CommitRevealWeightsEnabled::<Test>::iter().collect();
        assert_eq!(collected.len(), netuids.len(), "unexpected key count");
        for (k, v) in collected {
            assert!(!v, "found an enabled flag after migration for netuid {k}");
        }

        // -----------------------------------------------------------------
        // 4. running again should be a no-op
        // -----------------------------------------------------------------
        let w2 = crate::migrations::migrate_disable_commit_reveal::migrate_disable_commit_reveal::<
            Test,
        >();
        assert_eq!(
            w2,
            <Test as Config>::DbWeight::get().reads(1),
            "second run should read the flag and do nothing else"
        );
    });
}

#[test]
fn test_migrate_commit_reveal_settings() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_commit_reveal_settings";

        // Set up some networks first
        let netuid1: u16 = 1;
        let netuid2: u16 = 2;
        // Add networks to simulate existing networks
        add_network(netuid1.into(), 1, 0);
        add_network(netuid2.into(), 1, 0);

        // Ensure the storage items use default values initially (but aren't explicitly set)
        // Since these are ValueQuery storage items, they return defaults even when not set
        assert_eq!(RevealPeriodEpochs::<Test>::get(NetUid::from(netuid1)), 1u64);
        assert_eq!(RevealPeriodEpochs::<Test>::get(NetUid::from(netuid2)), 1u64);
        assert!(CommitRevealWeightsEnabled::<Test>::get(NetUid::from(netuid1)));
        assert!(CommitRevealWeightsEnabled::<Test>::get(NetUid::from(netuid2)));

        // Check migration hasn't run
        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()));

        // Run migration
        let weight = crate::migrations::migrate_commit_reveal_settings::migrate_commit_reveal_settings::<Test>();

        // Check migration has been marked as run
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()));

        // Verify RevealPeriodEpochs was set correctly
        assert_eq!(RevealPeriodEpochs::<Test>::get(NetUid::from(netuid1)), 1u64);
        assert_eq!(RevealPeriodEpochs::<Test>::get(NetUid::from(netuid2)), 1u64);

        // Verify CommitRevealWeightsEnabled was set correctly
        assert!(CommitRevealWeightsEnabled::<Test>::get(NetUid::from(netuid1)));
        assert!(CommitRevealWeightsEnabled::<Test>::get(NetUid::from(netuid2)));
    });
}

#[test]
fn test_migrate_commit_reveal_settings_already_run() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_commit_reveal_settings";
        // Mark migration as already run
        HasMigrationRun::<Test>::insert(MIGRATION_NAME.as_bytes().to_vec(), true);

        // Run migration
        let weight = crate::migrations::migrate_commit_reveal_settings::migrate_commit_reveal_settings::<Test>();

        // Should only have read weight for checking migration status
        let expected_weight = <Test as frame_system::Config>::DbWeight::get().reads(1);
        assert_eq!(weight, expected_weight);
    });
}

#[test]
fn test_migrate_commit_reveal_settings_no_networks() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_commit_reveal_settings";

        // Check migration hasn't run
        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()));

        // Run migration
        let weight = crate::migrations::migrate_commit_reveal_settings::migrate_commit_reveal_settings::<Test>();

        // Check migration has been marked as run
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()));

        // Check that weight calculation is correct (no networks, so no additional reads/writes)
        // 1 read for migration check + 0 reads for networks + 0 writes for storage + 1 write for migration flag
        let expected_weight = <Test as frame_system::Config>::DbWeight::get().reads(1) + <Test as frame_system::Config>::DbWeight::get().writes(1);
        assert_eq!(weight, expected_weight);
    });
}

#[test]
fn test_migrate_commit_reveal_settings_multiple_networks() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_commit_reveal_settings";

        // Set up multiple networks
        let netuids = vec![1u16, 2u16, 3u16, 10u16, 42u16];
        for netuid in &netuids {
            add_network((*netuid).into(), 1, 0);
        }

        // Run migration
        let weight = crate::migrations::migrate_commit_reveal_settings::migrate_commit_reveal_settings::<Test>();

        // Verify all networks have correct settings
        for netuid in &netuids {
            assert_eq!(RevealPeriodEpochs::<Test>::get(NetUid::from(*netuid)), 1u64);
            assert!(CommitRevealWeightsEnabled::<Test>::get(NetUid::from(*netuid)));
        }

        // Check migration has been marked as run
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()));
    });
}

#[test]
fn test_migrate_commit_reveal_settings_values_access() {
    new_test_ext(1).execute_with(|| {
        let netuid: u16 = 1;
        add_network(netuid.into(), 1, 0);

        // Run migration
        crate::migrations::migrate_commit_reveal_settings::migrate_commit_reveal_settings::<Test>();

        // Test that we can access the values using the pallet functions
        assert_eq!(
            SubtensorModule::get_reveal_period(NetUid::from(netuid)),
            1u64
        );

        // Test direct storage access
        assert_eq!(RevealPeriodEpochs::<Test>::get(NetUid::from(netuid)), 1u64);
        assert!(CommitRevealWeightsEnabled::<Test>::get(NetUid::from(
            netuid
        )));
    });
}

#[test]
fn test_migrate_crv3_v2_to_timelocked() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // 0. Constants / helpers
        // ------------------------------
        const MIG_NAME: &[u8] = b"crv3_v2_to_timelocked_v1";
        let netuid = NetUid::from(99);
        let epoch: u64 = 7;

        // ------------------------------
        // 1. Simulate OLD storage (4‑tuple; V2 layout)
        // ------------------------------
        let who: U256 = U256::from(0xdeadbeef_u64);
        let commit_block: u64 = 12345;
        let ciphertext: BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>> =
            vec![1u8, 2, 3].try_into().unwrap();
        let round: RoundNumber = 9;

        let old_queue: VecDeque<_> =
            VecDeque::from(vec![(who, commit_block, ciphertext.clone(), round)]);

        // Insert under the deprecated alias
        CRV3WeightCommitsV2::<Test>::insert(
            NetUidStorageIndex::from(netuid),
            epoch,
            old_queue.clone(),
        );

        // Sanity: entry decodes under old alias
        assert_eq!(
            CRV3WeightCommitsV2::<Test>::get(NetUidStorageIndex::from(netuid), epoch),
            old_queue,
            "pre-migration: old queue should be present"
        );

        // Destination should be empty pre-migration
        assert!(
            TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), epoch)
                .is_empty(),
            "pre-migration: destination should be empty"
        );

        assert!(
            !HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should be false before run"
        );

        // ------------------------------
        // 2. Run migration
        // ------------------------------
        let w = crate::migrations::migrate_crv3_v2_to_timelocked::migrate_crv3_v2_to_timelocked::<
            Test,
        >();
        assert!(!w.is_zero(), "weight must be non-zero");

        // ------------------------------
        // 3. Verify results
        // ------------------------------
        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag not set"
        );

        // Old storage must be empty (drained)
        assert!(
            CRV3WeightCommitsV2::<Test>::get(NetUidStorageIndex::from(netuid), epoch).is_empty(),
            "old queue should have been drained"
        );

        // New storage must match exactly
        let new_q = TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), epoch);
        assert_eq!(
            new_q, old_queue,
            "migrated queue must exactly match the old queue"
        );

        // Verify the front element matches what we inserted
        let (who2, commit_block2, cipher2, round2) = new_q.front().cloned().unwrap();
        assert_eq!(who2, who);
        assert_eq!(commit_block2, commit_block);
        assert_eq!(cipher2, ciphertext);
        assert_eq!(round2, round);
    });
}
