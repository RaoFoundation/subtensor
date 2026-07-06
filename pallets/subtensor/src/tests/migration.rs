#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use super::mock::*;
use crate::staking::lock::LockState;
use crate::*;
use alloc::collections::BTreeMap;
use approx::{assert_abs_diff_eq, assert_relative_eq};
use codec::{Decode, Encode};
use frame_support::{
    StorageHasher, Twox64Concat, assert_ok,
    storage::unhashed::{get, get_raw, put, put_raw},
    storage_alias,
    traits::{Currency, StorageInstance, StoredMap, fungible::Inspect},
    weights::Weight,
};
use safe_math::SafeDiv;

use crate::migrations::migrate_coldkey_swap_scheduled_to_announcements::deprecated as coldkey_swap_deprecated;
use crate::migrations::migrate_storage;
use frame_support::traits::Bounded;
use frame_system::Config;
use pallet_drand::types::RoundNumber;
use pallet_scheduler::ScheduledOf;
use scale_info::prelude::collections::VecDeque;
use sp_core::{H256, U256, crypto::Ss58Codec};
use sp_io::hashing::twox_128;
use sp_runtime::{
    AccountId32,
    traits::{Hash, Zero},
};
use sp_std::marker::PhantomData;
use substrate_fixed::types::{I96F32, U64F64};
use substrate_fixed::{traits::ToFixed, types::extra::U2};
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance};

#[allow(clippy::arithmetic_side_effects)]
fn close(value: u64, target: u64, eps: u64) {
    assert!(
        (value as i64 - target as i64).abs() < eps as i64,
        "Assertion failed: value = {value}, target = {target}, eps = {eps}"
    )
}

#[test]
fn test_migrate_tao_in_refund_deployment_block() {
    new_test_ext(1).execute_with(|| {
        let deployment_block: u64 = 42;
        let migration_name = b"migrate_tao_in_refund_deployment_block".to_vec();

        TaoInRefundDeploymentBlock::<Test>::put(0);
        HasMigrationRun::<Test>::remove(&migration_name);

        run_to_block(deployment_block);
        crate::migrations::migrate_tao_in_refund_deployment_block::migrate_tao_in_refund_deployment_block::<Test>();

        assert_eq!(TaoInRefundDeploymentBlock::<Test>::get(), deployment_block);
        assert!(HasMigrationRun::<Test>::get(&migration_name));

        run_to_block(deployment_block.saturating_add(1));
        crate::migrations::migrate_tao_in_refund_deployment_block::migrate_tao_in_refund_deployment_block::<Test>();

        assert_eq!(TaoInRefundDeploymentBlock::<Test>::get(), deployment_block);
    });
}

#[test]
fn test_migrate_fix_subnet_hotkey_lock_swaps_moves_or_discards_conflicts() {
    new_test_ext(1).execute_with(|| {
        let migration_name = b"migrate_fix_subnet_hotkey_lock_swaps".to_vec();
        let old_hotkey =
            decode_account_id32::<Test>("5Ca8L8PkbqXUtzohKtSM3i1naGQxANGLx51kJsEPNB14Admz")
                .expect("old hotkey should decode");
        let new_hotkey =
            decode_account_id32::<Test>("5Evgh9QTXJLxYLusVy3tcY5S6Z3GgRSNDb9AzXUchX5dco3P")
                .expect("new hotkey should decode");
        let netuid = NetUid::from(28);
        let coldkey_to_move = U256::from(1);
        let coldkey_with_conflict = U256::from(2);
        let chained_coldkey =
            decode_account_id32::<Test>("5EWUPMenvyvHdEGUHfUhSTeTDJDLzLkKZq74LFLRWtzcqZiS")
                .expect("chained coldkey should decode");
        let chained_first_hotkey =
            decode_account_id32::<Test>("5H3Kuy7L7DBSy7BS2c9EBayJYGkHV1pzWtnJm3iXvThT4VUJ")
                .expect("chained first hotkey should decode");
        let chained_middle_hotkey =
            decode_account_id32::<Test>("5CSiRF3sMKt1c3MT4KsRLBWENGkymVE7wA2zUDPsYy6JtpGE")
                .expect("chained middle hotkey should decode");
        let chained_final_hotkey =
            decode_account_id32::<Test>("5EsnHJK89FgF55EYwXtqhUwLu3c14xakyQ8PWoomcFwpxk5e")
                .expect("chained final hotkey should decode");
        let chained_netuid = NetUid::from(97);

        HasMigrationRun::<Test>::remove(&migration_name);

        let moved_lock = LockState {
            locked_mass: AlphaBalance::from(10_u64),
            conviction: U64F64::from_num(3),
            last_update: 11,
        };
        let discarded_lock = LockState {
            locked_mass: AlphaBalance::from(20_u64),
            conviction: U64F64::from_num(5),
            last_update: 12,
        };
        let existing_destination_lock = LockState {
            locked_mass: AlphaBalance::from(77_u64),
            conviction: U64F64::from_num(7),
            last_update: 10,
        };
        let chained_lock = LockState {
            locked_mass: AlphaBalance::from(33_u64),
            conviction: U64F64::from_num(4),
            last_update: 13,
        };

        Lock::<Test>::insert(
            (coldkey_to_move, netuid, old_hotkey),
            moved_lock.clone(),
        );
        LockingColdkeys::<Test>::insert((netuid, old_hotkey, coldkey_to_move), ());
        Lock::<Test>::insert(
            (coldkey_with_conflict, netuid, old_hotkey),
            discarded_lock.clone(),
        );
        LockingColdkeys::<Test>::insert((netuid, old_hotkey, coldkey_with_conflict), ());
        Lock::<Test>::insert(
            (coldkey_with_conflict, netuid, new_hotkey),
            existing_destination_lock.clone(),
        );
        LockingColdkeys::<Test>::insert((netuid, new_hotkey, coldkey_with_conflict), ());
        DecayingLock::<Test>::insert(coldkey_to_move, netuid, false);
        DecayingLock::<Test>::insert(coldkey_with_conflict, netuid, false);
        DecayingLock::<Test>::insert(chained_coldkey, chained_netuid, false);
        HotkeyLock::<Test>::insert(
            netuid,
            old_hotkey,
            LockState {
                locked_mass: AlphaBalance::from(30_u64),
                conviction: U64F64::from_num(8),
                last_update: 12,
            },
        );
        HotkeyLock::<Test>::insert(netuid, new_hotkey, existing_destination_lock.clone());
        Lock::<Test>::insert(
            (chained_coldkey, chained_netuid, chained_first_hotkey),
            chained_lock.clone(),
        );
        LockingColdkeys::<Test>::insert(
            (chained_netuid, chained_first_hotkey, chained_coldkey),
            (),
        );
        HotkeyLock::<Test>::insert(chained_netuid, chained_first_hotkey, chained_lock.clone());

        let weight =
            crate::migrations::migrate_fix_subnet_hotkey_lock_swaps::migrate_fix_subnet_hotkey_lock_swaps::<Test>();

        assert!(!weight.is_zero(), "migration weight should be non-zero");
        assert!(HasMigrationRun::<Test>::get(&migration_name));
        assert!(Lock::<Test>::get((coldkey_to_move, netuid, old_hotkey)).is_none());
        assert!(Lock::<Test>::get((coldkey_with_conflict, netuid, old_hotkey)).is_none());
        assert!(!LockingColdkeys::<Test>::contains_key((
            netuid,
            old_hotkey,
            coldkey_to_move
        )));
        assert!(!LockingColdkeys::<Test>::contains_key((
            netuid,
            old_hotkey,
            coldkey_with_conflict
        )));
        assert_eq!(
            Lock::<Test>::get((coldkey_to_move, netuid, new_hotkey)),
            Some(moved_lock.clone())
        );
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid,
            new_hotkey,
            coldkey_to_move
        )));
        assert_eq!(
            Lock::<Test>::get((coldkey_with_conflict, netuid, new_hotkey)),
            Some(existing_destination_lock.clone())
        );
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid,
            new_hotkey,
            coldkey_with_conflict
        )));
        assert!(HotkeyLock::<Test>::get(netuid, old_hotkey).is_none());

        let new_aggregate = HotkeyLock::<Test>::get(netuid, new_hotkey)
            .expect("new aggregate should exist");
        assert_eq!(
            new_aggregate.locked_mass,
            existing_destination_lock
                .locked_mass
                .saturating_add(moved_lock.locked_mass)
        );
        assert_eq!(
            new_aggregate.conviction,
            existing_destination_lock
                .conviction
                .saturating_add(moved_lock.conviction)
        );
        assert!(Lock::<Test>::get((
            chained_coldkey,
            chained_netuid,
            chained_first_hotkey
        ))
        .is_none());
        assert!(Lock::<Test>::get((
            chained_coldkey,
            chained_netuid,
            chained_middle_hotkey
        ))
        .is_none());
        assert!(!LockingColdkeys::<Test>::contains_key((
            chained_netuid,
            chained_first_hotkey,
            chained_coldkey
        )));
        assert!(!LockingColdkeys::<Test>::contains_key((
            chained_netuid,
            chained_middle_hotkey,
            chained_coldkey
        )));
        assert_eq!(
            Lock::<Test>::get((chained_coldkey, chained_netuid, chained_final_hotkey)),
            Some(chained_lock.clone())
        );
        assert!(LockingColdkeys::<Test>::contains_key((
            chained_netuid,
            chained_final_hotkey,
            chained_coldkey
        )));
        assert!(HotkeyLock::<Test>::get(chained_netuid, chained_first_hotkey).is_none());
        assert!(HotkeyLock::<Test>::get(chained_netuid, chained_middle_hotkey).is_none());
        assert_eq!(
            HotkeyLock::<Test>::get(chained_netuid, chained_final_hotkey),
            Some(chained_lock)
        );
    });
}
#[test]
fn test_migration_transfer_nets_to_foundation() {
    new_test_ext(1).execute_with(|| {
        // Create subnet 1
        add_network(1.into(), 1, 0);
        // Create subnet 11
        add_network(11.into(), 1, 0);

        log::info!("{:?}", SubtensorModule::get_subnet_owner(1.into()));
        //assert_eq!(SubtensorModule::<Test>::get_subnet_owner(1), );

        // Run the migration to transfer ownership
        let hex =
            hex_literal::hex!["feabaafee293d3b76dae304e2f9d885f77d2b17adab9e17e921b321eccd61c77"];
        crate::migrations::migrate_transfer_ownership_to_foundation::migrate_transfer_ownership_to_foundation::<Test>(hex);

        log::info!("new owner: {:?}", SubtensorModule::get_subnet_owner(1.into()));
    })
}

#[test]
fn test_migration_delete_subnet_3() {
    new_test_ext(1).execute_with(|| {
        // Create subnet 3
        add_network(3.into(), 1, 0);
        assert!(SubtensorModule::if_subnet_exist(3.into()));

        // Run the migration to transfer ownership
        crate::migrations::migrate_delete_subnet_3::migrate_delete_subnet_3::<Test>();

        assert!(!SubtensorModule::if_subnet_exist(3.into()));
    })
}

#[test]
fn test_migration_delete_subnet_21() {
    new_test_ext(1).execute_with(|| {
        // Create subnet 21
        add_network(21.into(), 1, 0);
        assert!(SubtensorModule::if_subnet_exist(21.into()));

        // Run the migration to transfer ownership
        crate::migrations::migrate_delete_subnet_21::migrate_delete_subnet_21::<Test>();

        assert!(!SubtensorModule::if_subnet_exist(21.into()));
    })
}

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
fn test_migrate_subnet_volume() {
    new_test_ext(1).execute_with(|| {
        // Setup initial state
        let netuid_1 = NetUid::from(1);
        add_network(netuid_1, 1, 0);

        // SubnetValue for netuid 1 key
        let old_key: [u8; 34] = hex_literal::hex!(
            "658faa385070e074c85bf6b568cf05553c3226e141696000b4b239c65bc2b2b40100"
        );

        // Old value in u64 format
        let old_value: u64 = 123_456_789_000_u64;
        put::<u64>(&old_key, &old_value); // Store as u64

        // Ensure it is stored as `u64`
        assert_eq!(get::<u64>(&old_key), Some(old_value));

        // Run migration
        crate::migrations::migrate_subnet_volume::migrate_subnet_volume::<Test>();

        // Verify the value is now stored as `u128`
        let new_value: Option<u128> = get(&old_key);
        let new_value_as_subnet_volume = SubnetVolume::<Test>::get(netuid_1);
        assert_eq!(new_value, Some(old_value as u128));
        assert_eq!(new_value_as_subnet_volume, old_value as u128);

        // Ensure migration does not break when running twice
        let weight_second_run =
            crate::migrations::migrate_subnet_volume::migrate_subnet_volume::<Test>();

        // Verify the value is still stored as `u128`
        let new_value: Option<u128> = get(&old_key);
        assert_eq!(new_value, Some(old_value as u128));
    });
}

#[test]
fn test_migrate_set_first_emission_block_number() {
    new_test_ext(1).execute_with(|| {
    let netuids: [NetUid; 3] = [1.into(), 2.into(), 3.into()];
    let block_number = 100;
    for netuid in netuids.iter() {
        add_network(*netuid, 1, 0);
    }
    run_to_block(block_number);
    let weight = crate::migrations::migrate_set_first_emission_block_number::migrate_set_first_emission_block_number::<Test>();

    let expected_weight: Weight = <Test as Config>::DbWeight::get().reads(3) + <Test as Config>::DbWeight::get().writes(netuids.len() as u64);
    assert_eq!(weight, expected_weight);

    assert_eq!(FirstEmissionBlockNumber::<Test>::get(NetUid::ROOT), None);
    for netuid in netuids.iter() {
        assert_eq!(FirstEmissionBlockNumber::<Test>::get(netuid), Some(block_number));
    }
});
}

#[test]
fn test_migrate_set_subtoken_enable() {
    new_test_ext(1).execute_with(|| {
        let netuids: [NetUid; 3] = [1.into(), 2.into(), 3.into()];
        let block_number = 100;
        for netuid in netuids.iter() {
            add_network(*netuid, 1, 0);
        }

        let new_netuid = NetUid::from(4);
        add_network_without_emission_block(new_netuid, 1, 0);

        let weight =
            crate::migrations::migrate_set_subtoken_enabled::migrate_set_subtoken_enabled::<Test>();

        let expected_weight: Weight = <Test as Config>::DbWeight::get().reads(1)
            + <Test as Config>::DbWeight::get().writes(netuids.len() as u64 + 2);
        assert_eq!(weight, expected_weight);

        for netuid in netuids.iter() {
            assert!(SubtokenEnabled::<Test>::get(netuid));
        }
        assert!(!SubtokenEnabled::<Test>::get(new_netuid));
    });
}

#[test]
fn test_migrate_remove_zero_total_hotkey_alpha() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_remove_zero_total_hotkey_alpha";
        let netuid = NetUid::from(1u16);

        let hotkey_zero = U256::from(100u64);
        let hotkey_nonzero = U256::from(101u64);

        // Insert one zero-alpha entry and one non-zero entry
        TotalHotkeyAlpha::<Test>::insert(hotkey_zero, netuid, AlphaBalance::ZERO);
        TotalHotkeyAlpha::<Test>::insert(hotkey_nonzero, netuid, AlphaBalance::from(123));

        assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey_zero, netuid), AlphaBalance::ZERO);
        assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey_nonzero, netuid), AlphaBalance::from(123));

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet."
        );

        let weight = crate::migrations::migrate_remove_zero_total_hotkey_alpha::migrate_remove_zero_total_hotkey_alpha::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run."
        );

        assert!(
            !TotalHotkeyAlpha::<Test>::contains_key(hotkey_zero, netuid),
            "Zero-alpha entry should have been removed."
        );

        assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey_nonzero, netuid), AlphaBalance::from(123));

        assert!(
            !weight.is_zero(),
            "Migration weight should be non-zero."
        );
    });
}

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
fn test_migrate_remove_total_hotkey_coldkey_stakes_this_interval() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_remove_total_hotkey_coldkey_stakes_this_interval";

        let pallet_name = twox_128(b"SubtensorModule");
        let storage_name = twox_128(b"TotalHotkeyColdkeyStakesThisInterval");
        let prefix = [pallet_name, storage_name].concat();

        // Set up 200 000 entries to be deleted.
        for i in 0..200_000{
            let hotkey = U256::from(i as u64);
            let coldkey = U256::from(i as u64);
            let key = [prefix.clone(), hotkey.encode(), coldkey.encode()].concat();
            let value = (100 + i, 200 + i);
            put_raw(&key, &value.encode());
        }

        assert!(frame_support::storage::unhashed::contains_prefixed_key(&prefix), "Entries should exist before migration.");
        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet."
        );

        // Run migration
        let weight = crate::migrations::migrate_remove_total_hotkey_coldkey_stakes_this_interval::migrate_remove_total_hotkey_coldkey_stakes_this_interval::<Test>();

        assert!(!frame_support::storage::unhashed::contains_prefixed_key(&prefix), "All entries should have been removed.");
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run."
        );
        assert!(!weight.is_zero(),"Migration weight should be non-zero.");
    });
}
fn test_migrate_remove_last_hotkey_coldkey_emission_on_netuid() {
    const MIGRATION_NAME: &str = "migrate_remove_last_hotkey_coldkey_emission_on_netuid";
    let pallet_name = "SubtensorModule";
    let storage_name = "LastHotkeyColdkeyEmissionOnNetuid";
    let migration =  crate::migrations::migrate_orphaned_storage_items::remove_last_hotkey_coldkey_emission_on_netuid::<Test>;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}
#[test]
fn test_migrate_remove_subnet_alpha_emission_sell() {
    const MIGRATION_NAME: &str = "migrate_remove_subnet_alpha_emission_sell";
    let pallet_name = "SubtensorModule";
    let storage_name = "SubnetAlphaEmissionSell";
    let migration =
        crate::migrations::migrate_orphaned_storage_items::remove_subnet_alpha_emission_sell::<Test>;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

#[test]
fn test_migrate_remove_neurons_to_prune_at_next_epoch() {
    const MIGRATION_NAME: &str = "migrate_remove_neurons_to_prune_at_next_epoch";
    let pallet_name = "SubtensorModule";
    let storage_name = "NeuronsToPruneAtNextEpoch";
    let migration =
        crate::migrations::migrate_orphaned_storage_items::remove_neurons_to_prune_at_next_epoch::<
            Test,
        >;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

#[test]
fn test_migrate_remove_total_stake_at_dynamic() {
    const MIGRATION_NAME: &str = "migrate_remove_total_stake_at_dynamic";
    let pallet_name = "SubtensorModule";
    let storage_name = "TotalStakeAtDynamic";
    let migration =
        crate::migrations::migrate_orphaned_storage_items::remove_total_stake_at_dynamic::<Test>;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

#[test]
fn test_migrate_remove_subnet_name() {
    const MIGRATION_NAME: &str = "migrate_remove_subnet_name";
    let pallet_name = "SubtensorModule";
    let storage_name = "SubnetName";
    let migration = crate::migrations::migrate_orphaned_storage_items::remove_subnet_name::<Test>;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

#[test]
fn test_migrate_remove_network_min_allowed_uids() {
    const MIGRATION_NAME: &str = "migrate_remove_network_min_allowed_uids";
    let pallet_name = "SubtensorModule";
    let storage_name = "NetworkMinAllowedUids";
    let migration =
        crate::migrations::migrate_orphaned_storage_items::remove_network_min_allowed_uids::<Test>;

    test_remove_storage_item(MIGRATION_NAME, pallet_name, storage_name, migration, 1);
}

#[test]
fn test_migrate_remove_dynamic_block() {
    const MIGRATION_NAME: &str = "migrate_remove_dynamic_block";
    let pallet_name = "SubtensorModule";
    let storage_name = "DynamicBlock";
    let migration = crate::migrations::migrate_orphaned_storage_items::remove_dynamic_block::<Test>;

    test_remove_storage_item(MIGRATION_NAME, pallet_name, storage_name, migration, 1);
}

#[allow(clippy::arithmetic_side_effects)]
fn test_remove_storage_item<F: FnOnce() -> Weight>(
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

#[test]
fn test_migrate_remove_commitments_rate_limit() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entry
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_remove_commitments_rate_limit";

        // Build the raw storage key: twox128("Commitments") ++ twox128("RateLimit")
        let pallet_prefix = twox_128("Commitments".as_bytes());
        let storage_prefix = twox_128("RateLimit".as_bytes());

        let mut key = Vec::new();
        key.extend_from_slice(&pallet_prefix);
        key.extend_from_slice(&storage_prefix);

        let original_value: u64 = 123;
        put_raw(&key, &original_value.encode());

        let stored_before = get_raw(&key).expect("Expected RateLimit to exist");
        assert_eq!(
            u64::decode(&mut &stored_before[..]).expect("Failed to decode RateLimit"),
            original_value
        );

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_remove_commitments_rate_limit::
            migrate_remove_commitments_rate_limit::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as completed"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------
        assert!(
            get_raw(&key).is_none(),
            "RateLimit storage should have been cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[test]
fn test_migrate_network_last_registered() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entry
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_network_last_registered";

        let pallet_name = "SubtensorModule";
        let storage_name = "NetworkLastRegistered";
        let pallet_name_hash = twox_128(pallet_name.as_bytes());
        let storage_name_hash = twox_128(storage_name.as_bytes());
        let prefix = [pallet_name_hash, storage_name_hash].concat();

        let mut full_key = prefix.clone();

        let original_value: u64 = 123;
        put_raw(&full_key, &original_value.encode());

        let stored_before = get_raw(&full_key).expect("Expected RateLimit to exist");
        assert_eq!(
            u64::decode(&mut &stored_before[..]).expect("Failed to decode RateLimit"),
            original_value
        );

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_rate_limiting_last_blocks::
        migrate_obsolete_rate_limiting_last_blocks_storage::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as completed"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------

        assert_eq!(
            SubtensorModule::get_network_last_lock_block(),
            original_value
        );
        assert_eq!(
            get_raw(&full_key),
            None,
            "RateLimit storage should have been cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[allow(deprecated)]
#[test]
fn test_migrate_last_block_tx() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entry
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_last_tx_block";

        let test_account: U256 = U256::from(1);
        let original_value: u64 = 123;

        LastTxBlock::<Test>::insert(test_account, original_value);

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_rate_limiting_last_blocks::
        migrate_obsolete_rate_limiting_last_blocks_storage::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as completed"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------

        assert_eq!(
            SubtensorModule::get_last_tx_block(&test_account),
            original_value
        );
        assert!(
            !LastTxBlock::<Test>::contains_key(test_account),
            "RateLimit storage should have been cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[allow(deprecated)]
#[test]
fn test_migrate_last_tx_block_childkey_take() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entry
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_last_tx_block_childkey_take";

        let test_account: U256 = U256::from(1);
        let original_value: u64 = 123;

        LastTxBlockChildKeyTake::<Test>::insert(test_account, original_value);

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_rate_limiting_last_blocks::
        migrate_obsolete_rate_limiting_last_blocks_storage::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as completed"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------

        assert_eq!(
            SubtensorModule::get_last_tx_block_childkey_take(&test_account),
            original_value
        );
        assert!(
            !LastTxBlockChildKeyTake::<Test>::contains_key(test_account),
            "RateLimit storage should have been cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[allow(deprecated)]
#[test]
fn test_migrate_last_tx_block_delegate_take() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entry
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_last_tx_block_delegate_take";

        let test_account: U256 = U256::from(1);
        let original_value: u64 = 123;

        LastTxBlockDelegateTake::<Test>::insert(test_account, original_value);

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_rate_limiting_last_blocks::
        migrate_last_tx_block_delegate_take::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as completed"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------

        assert_eq!(
            SubtensorModule::get_last_tx_block_delegate_take(&test_account),
            original_value
        );
        assert!(
            !LastTxBlockDelegateTake::<Test>::contains_key(test_account),
            "RateLimit storage should have been cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[test]
fn test_migrate_rate_limit_keys() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_rate_limit_keys";
        let prefix = {
            let pallet_prefix = twox_128("SubtensorModule".as_bytes());
            let storage_prefix = twox_128("LastRateLimitedBlock".as_bytes());
            [pallet_prefix, storage_prefix].concat()
        };

        // Seed new-format entries that must survive the migration untouched.
        let new_last_account = U256::from(10);
        SubtensorModule::set_last_tx_block(&new_last_account, 555);
        let new_child_account = U256::from(11);
        SubtensorModule::set_last_tx_block_childkey(&new_child_account, 777);
        let new_delegate_account = U256::from(12);
        SubtensorModule::set_last_tx_block_delegate_take(&new_delegate_account, 888);

        // Legacy NetworkLastRegistered entry (index 1)
        let mut legacy_network_key = prefix.clone();
        legacy_network_key.push(1u8);
        sp_io::storage::set(&legacy_network_key, &111u64.encode());

        // Legacy LastTxBlock entry (index 2) for an account that already has a new-format value.
        let mut legacy_last_key = prefix.clone();
        legacy_last_key.push(2u8);
        legacy_last_key.extend_from_slice(&new_last_account.encode());
        sp_io::storage::set(&legacy_last_key, &666u64.encode());

        // Legacy LastTxBlockChildKeyTake entry (index 3)
        let legacy_child_account = U256::from(3);
        ChildKeys::<Test>::insert(
            legacy_child_account,
            NetUid::from(0),
            vec![(0u64, U256::from(99))],
        );
        let mut legacy_child_key = prefix.clone();
        legacy_child_key.push(3u8);
        legacy_child_key.extend_from_slice(&legacy_child_account.encode());
        sp_io::storage::set(&legacy_child_key, &333u64.encode());

        // Legacy LastTxBlockDelegateTake entry (index 4)
        let legacy_delegate_account = U256::from(4);
        Delegates::<Test>::insert(legacy_delegate_account, 500u16);
        let mut legacy_delegate_key = prefix.clone();
        legacy_delegate_key.push(4u8);
        legacy_delegate_key.extend_from_slice(&legacy_delegate_account.encode());
        sp_io::storage::set(&legacy_delegate_key, &444u64.encode());

        let weight = crate::migrations::migrate_rate_limit_keys::migrate_rate_limit_keys::<Test>();
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should be marked as executed"
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero");

        // Legacy entries were migrated and cleared.
        assert_eq!(
            SubtensorModule::get_network_last_lock_block(),
            111u64,
            "Network last lock block should match migrated value"
        );
        assert!(
            sp_io::storage::get(&legacy_network_key).is_none(),
            "Legacy network entry should be cleared"
        );

        assert_eq!(
            SubtensorModule::get_last_tx_block(&new_last_account),
            666u64,
            "LastTxBlock should reflect the merged legacy value"
        );
        assert!(
            sp_io::storage::get(&legacy_last_key).is_none(),
            "Legacy LastTxBlock entry should be cleared"
        );

        assert_eq!(
            SubtensorModule::get_last_tx_block_childkey_take(&legacy_child_account),
            333u64,
            "Child key take block should be migrated"
        );
        assert!(
            sp_io::storage::get(&legacy_child_key).is_none(),
            "Legacy child take entry should be cleared"
        );

        assert_eq!(
            SubtensorModule::get_last_tx_block_delegate_take(&legacy_delegate_account),
            444u64,
            "Delegate take block should be migrated"
        );
        assert!(
            sp_io::storage::get(&legacy_delegate_key).is_none(),
            "Legacy delegate take entry should be cleared"
        );

        // New-format entries remain untouched.
        assert_eq!(
            SubtensorModule::get_last_tx_block_childkey_take(&new_child_account),
            777u64,
            "Existing child take entry should be preserved"
        );
        assert_eq!(
            SubtensorModule::get_last_tx_block_delegate_take(&new_delegate_account),
            888u64,
            "Existing delegate take entry should be preserved"
        );
    });
}

#[test]
fn test_migrate_remove_add_stake_burn_rate_limit() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_remove_add_stake_burn_rate_limit";
        let netuid = NetUid::from(1);
        let other_netuid = NetUid::from(2);
        let preserved_netuid = NetUid::from(3);
        let add_stake_burn_key = RateLimitKey::AddStakeBurn(netuid);
        let other_add_stake_burn_key = RateLimitKey::AddStakeBurn(other_netuid);
        let preserved_key = RateLimitKey::SetSNOwnerHotkey(preserved_netuid);

        SubtensorModule::set_rate_limited_last_block(&add_stake_burn_key, 100);
        SubtensorModule::set_rate_limited_last_block(&other_add_stake_burn_key, 200);
        SubtensorModule::set_rate_limited_last_block(&preserved_key, 300);

        let weight =
            crate::migrations::migrate_remove_add_stake_burn_rate_limit::migrate_remove_add_stake_burn_rate_limit::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should be marked as executed"
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero");

        assert_eq!(
            SubtensorModule::get_rate_limited_last_block(&add_stake_burn_key),
            0
        );
        assert_eq!(
            SubtensorModule::get_rate_limited_last_block(&other_add_stake_burn_key),
            0
        );
        assert_eq!(
            SubtensorModule::get_rate_limited_last_block(&preserved_key),
            300
        );
    });
}

#[test]
fn test_migrate_populate_locking_coldkeys() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_populate_locking_coldkeys";

        let netuid = NetUid::from(1);
        let coldkey_1 = U256::from(1001);
        let coldkey_2 = U256::from(1002);
        let hotkey = U256::from(2001);
        let expired_hotkey = U256::from(2002);

        Lock::<Test>::insert(
            (coldkey_1, netuid, hotkey),
            LockState {
                locked_mass: AlphaBalance::from(1_000_u64),
                conviction: U64F64::from_num(0),
                last_update: 1,
            },
        );
        Lock::<Test>::insert(
            (coldkey_2, netuid, hotkey),
            LockState {
                locked_mass: AlphaBalance::from(2_000_u64),
                conviction: U64F64::from_num(0),
                last_update: 1,
            },
        );
        Lock::<Test>::insert(
            (coldkey_1, netuid, expired_hotkey),
            LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction: U64F64::from_num(1),
                last_update: 1,
            },
        );

        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, hotkey)).count(),
            0
        );
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, expired_hotkey)).count(),
            0
        );
        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        let weight =
            crate::migrations::migrate_populate_locking_coldkeys::migrate_populate_locking_coldkeys::<Test>();

        assert!(!weight.is_zero(), "migration weight should be non-zero");
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey_1
        )));
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey_2
        )));
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, hotkey)).count(),
            2
        );
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, expired_hotkey)).count(),
            0
        );
        assert!(Lock::<Test>::get((coldkey_1, netuid, expired_hotkey)).is_none());
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        let _ = LockingColdkeys::<Test>::clear_prefix((netuid, hotkey), u32::MAX, None);
        let second_weight =
            crate::migrations::migrate_populate_locking_coldkeys::migrate_populate_locking_coldkeys::<Test>();

        assert_eq!(
            second_weight,
            <Test as frame_system::Config>::DbWeight::get().reads(1),
            "second run should only read the migration flag"
        );
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, hotkey)).count(),
            0
        );
    });
}

#[test]
fn test_migrate_populate_locking_coldkeys_removes_dust_from_aggregate() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let coldkey_1 = U256::from(1101);
        let coldkey_2 = U256::from(1102);
        let hotkey = U256::from(2101);
        let dust_lock = LockState {
            locked_mass: AlphaBalance::from(60_u64),
            conviction: U64F64::from_num(0),
            last_update: 1,
        };

        DecayingLock::<Test>::insert(coldkey_1, netuid, false);
        DecayingLock::<Test>::insert(coldkey_2, netuid, false);
        Lock::<Test>::insert((coldkey_1, netuid, hotkey), dust_lock.clone());
        Lock::<Test>::insert((coldkey_2, netuid, hotkey), dust_lock);
        HotkeyLock::<Test>::insert(
            netuid,
            hotkey,
            LockState {
                locked_mass: AlphaBalance::from(120_u64),
                conviction: U64F64::from_num(0),
                last_update: 1,
            },
        );

        crate::migrations::migrate_populate_locking_coldkeys::migrate_populate_locking_coldkeys::<
            Test,
        >();

        assert!(Lock::<Test>::get((coldkey_1, netuid, hotkey)).is_none());
        assert!(Lock::<Test>::get((coldkey_2, netuid, hotkey)).is_none());
        assert!(HotkeyLock::<Test>::get(netuid, hotkey).is_none());
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, hotkey)).count(),
            0
        );
    });
}

#[test]
fn test_migrate_fix_staking_hot_keys() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_fix_staking_hot_keys";

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should not have run yet"
        );

        // Add some data
        Alpha::<Test>::insert(
            (U256::from(1), U256::from(2), NetUid::ROOT),
            U64F64::from(1_u64),
        );
        // Run migration
        let weight =
            migrations::migrate_fix_staking_hot_keys::migrate_fix_staking_hot_keys::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should be marked as completed"
        );

        // Check migration has been marked as run
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        // Verify results
        assert_eq!(
            StakingHotkeys::<Test>::get(U256::from(2)),
            vec![U256::from(1)]
        );
    });
}

#[test]
fn test_migrate_fix_root_subnet_tao() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_fix_root_subnet_tao";

        let mut expected_total_stake = 0_u64;
        // Seed some hotkeys with some fake stake.
        for i in 0..100_000 {
            Owner::<Test>::insert(U256::from(U256::from(i)), U256::from(i + 1_000_000));
            let stake = i + 1_000_000;
            TotalHotkeyAlpha::<Test>::insert(
                U256::from(U256::from(i)),
                NetUid::ROOT,
                AlphaBalance::from(stake),
            );
            expected_total_stake += stake;
        }

        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);
        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // Run the migration
        let weight =
            crate::migrations::migrate_fix_root_subnet_tao::migrate_fix_root_subnet_tao::<Test>();

        // Verify the migration ran correctly
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run"
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero");
        assert_eq!(
            SubnetTAO::<Test>::get(NetUid::ROOT),
            expected_total_stake.into()
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::migration::test_migrate_fix_root_tao_and_alpha_in --exact --show-output
#[test]
fn test_migrate_fix_root_tao_and_alpha_in() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_fix_root_tao_and_alpha_in";

        // Set counters initially
        let initial_value = 1_000_000_000_000_u64;
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(initial_value));
        SubnetAlphaIn::<Test>::insert(NetUid::ROOT, AlphaBalance::from(initial_value));
        SubnetAlphaOut::<Test>::insert(NetUid::ROOT, AlphaBalance::from(initial_value));
        SubnetVolume::<Test>::insert(NetUid::ROOT, initial_value as u128);
        TotalStake::<Test>::set(TaoBalance::from(initial_value));

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // Run the migration
        let weight =
            crate::migrations::migrate_fix_root_tao_and_alpha_in::migrate_fix_root_tao_and_alpha_in::<Test>();

        // Verify the migration ran correctly
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run"
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero");

        // Verify counters have changed
        assert!(SubnetTAO::<Test>::get(NetUid::ROOT) != initial_value.into());
        assert!(SubnetAlphaIn::<Test>::get(NetUid::ROOT) != initial_value.into());
        assert!(SubnetAlphaOut::<Test>::get(NetUid::ROOT) != initial_value.into());
        assert!(SubnetVolume::<Test>::get(NetUid::ROOT) != initial_value as u128);
        assert!(TotalStake::<Test>::get() != initial_value.into());
    });
}

#[test]
fn test_migrate_subnet_symbols() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_subnet_symbols";

        // Create 100 subnets
        for i in 0..100 {
            add_network(i.into(), 1, 0);
        }

        // Shift some symbols
        TokenSymbol::<Test>::insert(
            NetUid::from(21),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(142)),
        );
        TokenSymbol::<Test>::insert(
            NetUid::from(42),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(184)),
        );
        TokenSymbol::<Test>::insert(
            NetUid::from(83),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(242)),
        );
        TokenSymbol::<Test>::insert(
            NetUid::from(99),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(284)),
        );

        // Run the migration
        let weight = crate::migrations::migrate_subnet_symbols::migrate_subnet_symbols::<Test>();

        // Check that the symbols have been corrected
        assert_eq!(
            TokenSymbol::<Test>::get(NetUid::from(21)),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(21))
        );
        assert_eq!(
            TokenSymbol::<Test>::get(NetUid::from(42)),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(42))
        );
        assert_eq!(
            TokenSymbol::<Test>::get(NetUid::from(83)),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(83))
        );
        assert_eq!(
            TokenSymbol::<Test>::get(NetUid::from(99)),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(99))
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[test]
fn test_migrate_set_registration_enable() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_set_registration_enable";

        // Create 3 subnets
        let netuids: [NetUid; 3] = [1.into(), 2.into(), 3.into()];
        for netuid in netuids.iter() {
            add_network(*netuid, 1, 0);
            // Set registration to false to simulate the need for migration
            SubtensorModule::set_network_registration_allowed(*netuid, false);
        }

        // Sanity check: registration is disabled before migration
        for netuid in netuids.iter() {
            assert!(!SubtensorModule::get_network_registration_allowed(*netuid));
        }

        // Run the migration
        let weight =
            crate::migrations::migrate_set_registration_enable::migrate_set_registration_enable::<
                Test,
            >();

        // After migration, regular registration should be enabled for all subnets except root
        for netuid in netuids.iter() {
            assert!(SubtensorModule::get_network_registration_allowed(*netuid));
        }

        // Migration should be marked as run
        assert!(HasMigrationRun::<Test>::get(
            MIGRATION_NAME.as_bytes().to_vec()
        ));

        // Weight should be non-zero
        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[test]
fn test_migrate_set_nominator_min_stake() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_set_nominator_min_stake";

        let min_nomination_initial = 100_000_000;
        let min_nomination_migrated = 10_000_000;
        NominatorMinRequiredStake::<Test>::set(min_nomination_initial);

        assert_eq!(
            NominatorMinRequiredStake::<Test>::get(),
            min_nomination_initial
        );
        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // Run the migration
        let weight =
            crate::migrations::migrate_set_nominator_min_stake::migrate_set_nominator_min_stake::<
                Test,
            >();

        // Verify the migration ran correctly
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run"
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero");
        assert_eq!(
            NominatorMinRequiredStake::<Test>::get(),
            min_nomination_migrated
        );
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
fn test_migrate_auto_stake_destination() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entries
        // ------------------------------
        const MIGRATION_NAME: &[u8] = b"migrate_auto_stake_destination";
		let netuids = [NetUid::ROOT, NetUid::from(1), NetUid::from(2), NetUid::from(42)];
		for netuid in &netuids {
			NetworksAdded::<Test>::insert(*netuid, true);
		}

        let pallet_prefix = twox_128("SubtensorModule".as_bytes());
        let storage_prefix = twox_128("AutoStakeDestination".as_bytes());

        // Create test accounts
        let coldkey1: U256 = U256::from(1);
        let coldkey2: U256 = U256::from(2);
        let hotkey1: U256 = U256::from(100);
        let hotkey2: U256 = U256::from(200);

        // Construct storage keys for old format (StorageMap)
        let mut key1 = Vec::new();
        key1.extend_from_slice(&pallet_prefix);
        key1.extend_from_slice(&storage_prefix);
        key1.extend_from_slice(&Blake2_128Concat::hash(&coldkey1.encode()));

        let mut key2 = Vec::new();
        key2.extend_from_slice(&pallet_prefix);
        key2.extend_from_slice(&storage_prefix);
        key2.extend_from_slice(&Blake2_128Concat::hash(&coldkey2.encode()));

        // Store old format entries
        put_raw(&key1, &hotkey1.encode());
        put_raw(&key2, &hotkey2.encode());

        // Verify old entries are stored
        assert_eq!(get_raw(&key1), Some(hotkey1.encode()));
        assert_eq!(get_raw(&key2), Some(hotkey2.encode()));

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_auto_stake_destination::migrate_auto_stake_destination::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should be marked as run"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------

        // Verify new format entries exist
		for netuid in &netuids {
			if *netuid == NetUid::ROOT {
				assert_eq!(
					AutoStakeDestination::<Test>::get(coldkey1, NetUid::ROOT),
					None
				);
				assert_eq!(
					AutoStakeDestination::<Test>::get(coldkey2, NetUid::ROOT),
					None
				);
			} else {
				assert_eq!(
					AutoStakeDestination::<Test>::get(coldkey1, *netuid),
					Some(hotkey1)
				);
				assert_eq!(
					AutoStakeDestination::<Test>::get(coldkey2, *netuid),
					Some(hotkey2)
				);

				// Verify entry for AutoStakeDestinationColdkeys
				assert_eq!(
					AutoStakeDestinationColdkeys::<Test>::get(hotkey1, *netuid),
					vec![coldkey1]
				);
				assert_eq!(
					AutoStakeDestinationColdkeys::<Test>::get(hotkey2, *netuid),
					vec![coldkey2]
				);
			}
		}

        // Verify old format entries are cleared
        assert_eq!(get_raw(&key1), None, "Old storage entry 1 should be cleared");
        assert_eq!(get_raw(&key2), None, "Old storage entry 2 should be cleared");

        // Verify weight calculation
        assert!(!weight.is_zero(), "Migration weight should be non-zero");

        // ------------------------------
        // Step 4: Test Migration Idempotency
        // ------------------------------
        let weight_second_run = crate::migrations::migrate_auto_stake_destination::migrate_auto_stake_destination::<Test>();

        // Second run should only read the migration flag
        assert_eq!(
            weight_second_run,
            <Test as Config>::DbWeight::get().reads(1),
            "Second run should only read the migration flag"
        );
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

#[test]
fn test_migrate_remove_network_modality() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // 0. Constants / helpers
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_remove_network_modality";

        // Create multiple networks to test
        let netuids: [NetUid; 3] = [1.into(), 2.into(), 3.into()];
        for netuid in netuids.iter() {
            add_network(*netuid, 1, 0);
        }

        // Set initial storage version to 7 (below target)
        StorageVersion::new(7).put::<Pallet<Test>>();
        assert_eq!(
            Pallet::<Test>::on_chain_storage_version(),
            StorageVersion::new(7)
        );

        // ------------------------------
        // 1. Simulate NetworkModality entries using deprecated storage alias
        // ------------------------------
        // We need to manually create storage entries that would exist for NetworkModality
        // Since NetworkModality was a StorageMap<_, Identity, NetUid, u16>, we simulate this
        let pallet_prefix = twox_128("SubtensorModule".as_bytes());
        let storage_prefix = twox_128("NetworkModality".as_bytes());

        // Create NetworkModality entries for each network
        for (i, netuid) in netuids.iter().enumerate() {
            let mut key = Vec::new();
            key.extend_from_slice(&pallet_prefix);
            key.extend_from_slice(&storage_prefix);
            // Identity encoding for netuid
            key.extend_from_slice(&netuid.encode());

            let modality_value: u16 = (i as u16) + 1; // Different values for testing
            put_raw(&key, &modality_value.encode());

            // Verify the entry was created
            let stored_value = get_raw(&key).expect("NetworkModality entry should exist");
            assert_eq!(
                u16::decode(&mut &stored_value[..]).expect("Failed to decode modality"),
                modality_value
            );
        }

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // 2. Run migration
        // ------------------------------
        let weight =
            crate::migrations::migrate_remove_network_modality::migrate_remove_network_modality::<
                Test,
            >();

        // ------------------------------
        // 3. Verify migration effects
        // ------------------------------
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run"
        );

        // Verify weight is non-zero
        assert!(!weight.is_zero(), "Migration weight should be non-zero");

        // Verify weight calculation: 1 read (version check) + 1 read (total networks) + N writes (removal) + 1 write (version update)
        let expected_weight = <Test as Config>::DbWeight::get().reads(2)
            + <Test as Config>::DbWeight::get().writes(netuids.len() as u64 + 1);
        assert_eq!(
            weight, expected_weight,
            "Weight calculation should be correct"
        );
    });
}

#[test]
fn test_migrate_remove_network_modality_already_run() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_remove_network_modality";

        // Mark migration as already run
        HasMigrationRun::<Test>::insert(MIGRATION_NAME.as_bytes().to_vec(), true);

        // Set storage version to 8 (target version)
        StorageVersion::new(8).put::<Pallet<Test>>();
        assert_eq!(
            Pallet::<Test>::on_chain_storage_version(),
            StorageVersion::new(8)
        );

        // Run migration
        let weight =
            crate::migrations::migrate_remove_network_modality::migrate_remove_network_modality::<
                Test,
            >();

        // Should only have read weight for checking migration status
        let expected_weight = <Test as Config>::DbWeight::get().reads(1);
        assert_eq!(
            weight, expected_weight,
            "Second run should only read the migration flag"
        );

        // Verify migration is still marked as run
        assert!(HasMigrationRun::<Test>::get(
            MIGRATION_NAME.as_bytes().to_vec()
        ));
    });
}

#[test]
fn test_migrate_subnet_limit_to_default() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // 0. Constants / helpers
        // ------------------------------
        const MIG_NAME: &[u8] = b"subnet_limit_to_default";

        // Compute a non-default value safely
        let default: u16 = DefaultSubnetLimit::<Test>::get();
        let not_default: u16 = default.wrapping_add(1);

        // ------------------------------
        // 1. Pre-state: ensure a non-default value is stored
        // ------------------------------
        SubnetLimit::<Test>::put(not_default);
        assert_eq!(
            SubnetLimit::<Test>::get(),
            not_default,
            "precondition failed: SubnetLimit should be non-default before migration"
        );

        assert!(
            !HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should be false before run"
        );

        // ------------------------------
        // 2. Run migration
        // ------------------------------
        let w = crate::migrations::migrate_subnet_limit_to_default::migrate_subnet_limit_to_default::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // ------------------------------
        // 3. Verify results
        // ------------------------------
        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag not set"
        );

        assert_eq!(
            SubnetLimit::<Test>::get(),
            default,
            "SubnetLimit should be reset to the configured default"
        );
    });
}

#[test]
fn test_migrate_network_lock_reduction_interval_and_decay() {
    new_test_ext(0).execute_with(|| {
        const FOUR_DAYS: u64 = 28_800;
        const EIGHT_DAYS: u64 = 57_600;
        const ONE_WEEK_BLOCKS: u64 = 50_400;

        // ── pre ──────────────────────────────────────────────────────────────
        assert!(
            !HasMigrationRun::<Test>::get(b"migrate_network_lock_reduction_interval".to_vec()),
            "HasMigrationRun should be false before migration"
        );

        // ensure current_block > 0
        step_block(1);
        let current_block_before = Pallet::<Test>::get_current_block_as_u64();

        // ── run migration ────────────────────────────────────────────────────
        let weight = crate::migrations::migrate_network_lock_reduction_interval::migrate_network_lock_reduction_interval::<Test>();
        assert!(!weight.is_zero(), "migration weight should be > 0");

        // ── params & flags ───────────────────────────────────────────────────
        assert_eq!(NetworkLockReductionInterval::<Test>::get(), EIGHT_DAYS);
        assert_eq!(NetworkRateLimit::<Test>::get(), FOUR_DAYS);
        assert_eq!(
            Pallet::<Test>::get_network_last_lock(),
            1_000_000_000_000u64.into(), // 1000 TAO in rao
            "last_lock should be 1_000_000_000_000 rao"
        );

        // last_lock_block should be set one week in the future
        let last_lock_block = Pallet::<Test>::get_network_last_lock_block();
        let expected_block = current_block_before + ONE_WEEK_BLOCKS;
        assert_eq!(
            last_lock_block,
            expected_block,
            "last_lock_block should be current + ONE_WEEK_BLOCKS"
        );

        // registration start block should match the same future block
        assert_eq!(
            NetworkRegistrationStartBlock::<Test>::get(),
            expected_block,
            "NetworkRegistrationStartBlock should equal last_lock_block"
        );

        // lock cost should be 2000 TAO immediately after migration
        let lock_cost_now = Pallet::<Test>::get_network_lock_cost();
        assert_eq!(
            lock_cost_now,
            2_000_000_000_000u64.into(),
            "lock cost should be 2000 TAO right after migration"
        );

        assert!(
            HasMigrationRun::<Test>::get(b"migrate_network_lock_reduction_interval".to_vec()),
            "HasMigrationRun should be true after migration"
        );
    });
}

#[test]
fn test_migrate_restore_subnet_locked_65_128() {
    use sp_runtime::traits::SaturatedConversion;
    new_test_ext(0).execute_with(|| {
        let name = b"migrate_restore_subnet_locked".to_vec();
        assert!(
            !HasMigrationRun::<Test>::get(name.clone()),
            "HasMigrationRun should be false before migration"
        );

        // Expected snapshot for netuids 65..128.
        const EXPECTED: &[(u16, u64)] = &[
            (65, 37_274_536_408),
            (66, 65_230_444_016),
            (67, 114_153_284_032),
            (68, 199_768_252_064),
            (69, 349_594_445_728),
            (70, 349_412_366_216),
            (71, 213_408_488_702),
            (72, 191_341_473_067),
            (73, 246_711_333_592),
            (74, 291_874_466_228),
            (75, 247_485_227_056),
            (76, 291_241_991_316),
            (77, 303_154_601_714),
            (78, 287_407_417_932),
            (79, 254_935_051_664),
            (80, 255_413_055_349),
            (81, 249_790_431_509),
            (82, 261_343_249_180),
            (83, 261_361_408_796),
            (84, 201_938_003_214),
            (85, 264_805_234_604),
            (86, 223_171_973_880),
            (87, 180_397_358_280),
            (88, 270_596_039_760),
            (89, 286_399_608_951),
            (90, 267_684_201_301),
            (91, 284_637_542_762),
            (92, 288_373_410_868),
            (93, 290_836_604_849),
            (94, 270_861_792_144),
            (95, 210_595_055_304),
            (96, 315_263_727_200),
            (97, 158_244_884_792),
            (98, 168_102_223_900),
            (99, 252_153_339_800),
            (100, 378_230_014_000),
            (101, 205_977_765_866),
            (102, 149_434_017_849),
            (103, 135_476_471_008),
            (104, 147_970_415_680),
            (105, 122_003_668_139),
            (106, 133_585_556_570),
            (107, 200_137_144_216),
            (108, 106_767_623_816),
            (109, 124_280_483_748),
            (110, 186_420_726_696),
            (111, 249_855_564_892),
            (112, 196_761_272_984),
            (113, 147_120_048_727),
            (114, 84_021_895_534),
            (115, 98_002_215_656),
            (116, 89_944_262_256),
            (117, 107_183_582_952),
            (118, 110_644_724_664),
            (119, 99_380_483_902),
            (120, 138_829_019_156),
            (121, 111_988_743_976),
            (122, 130_264_686_152),
            (123, 118_034_291_488),
            (124, 79_312_501_676),
            (125, 43_214_310_704),
            (126, 64_755_449_962),
            (127, 97_101_698_382),
            (128, 145_645_807_991),
        ];

        // Run migration
        let weight =
            crate::migrations::migrate_subnet_locked::migrate_restore_subnet_locked::<Test>();
        assert!(!weight.is_zero(), "migration weight should be > 0");

        // Read back storage as (u16 -> u64)
        let actual: BTreeMap<u16, u64> = SubnetLocked::<Test>::iter()
            .map(|(k, v)| (k.saturated_into::<u16>(), u64::from(v)))
            .collect();

        let expected: BTreeMap<u16, u64> = EXPECTED.iter().copied().collect();

        // 1) exact content
        assert_eq!(
            actual, expected,
            "SubnetLocked map mismatch for 65..128 snapshot"
        );

        // 2) count and total
        let expected_len = expected.len();
        let expected_sum: u128 = expected.values().map(|v| *v as u128).sum();

        let count_after = actual.len();
        let sum_after: u128 = actual.values().map(|v| *v as u128).sum();

        assert_eq!(count_after, expected_len, "entry count mismatch");
        assert_eq!(sum_after, expected_sum, "total RAO sum mismatch");

        // 3) migration flag set
        assert!(
            HasMigrationRun::<Test>::get(name.clone()),
            "HasMigrationRun should be true after migration"
        );

        // 4) idempotence
        let before = actual.clone();
        let _again =
            crate::migrations::migrate_subnet_locked::migrate_restore_subnet_locked::<Test>();
        let after: BTreeMap<u16, u64> = SubnetLocked::<Test>::iter()
            .map(|(k, v)| (k.saturated_into::<u16>(), u64::from(v)))
            .collect();
        assert_eq!(
            before, after,
            "re-running the migration should not change storage"
        );
    });
}

#[test]
fn test_migrate_network_lock_cost_2500_sets_price_and_decay() {
    new_test_ext(0).execute_with(|| {
        // ── constants ───────────────────────────────────────────────────────
        const RAO_PER_TAO: u64 = 1_000_000_000;
        const TARGET_COST_TAO: u64 = 2_500;
        const TARGET_COST_RAO: u64 = TARGET_COST_TAO * RAO_PER_TAO;
        const NEW_LAST_LOCK_RAO: u64 = (TARGET_COST_TAO / 2) * RAO_PER_TAO;

        let migration_key = b"migrate_network_lock_cost_2500".to_vec();

        // ── pre ──────────────────────────────────────────────────────────────
        assert!(
            !HasMigrationRun::<Test>::get(migration_key.clone()),
            "HasMigrationRun should be false before migration"
        );

        // Ensure current_block > 0 so mult == 2 in get_network_lock_cost()
        step_block(1);
        let current_block_before = Pallet::<Test>::get_current_block_as_u64();

        // Snapshot interval to ensure migration doesn't change it
        let interval_before = NetworkLockReductionInterval::<Test>::get();

        // ── run migration ────────────────────────────────────────────────────
        let weight = crate::migrations::migrate_network_lock_cost_2500::migrate_network_lock_cost_2500::<Test>();
        assert!(!weight.is_zero(), "migration weight should be > 0");

        // ── asserts: params & flags ─────────────────────────────────────────
        assert_eq!(
            Pallet::<Test>::get_network_last_lock(),
            NEW_LAST_LOCK_RAO.into(),
            "last_lock should be set to 1,250 TAO (in rao)"
        );
        assert_eq!(
            Pallet::<Test>::get_network_last_lock_block(),
            current_block_before,
            "last_lock_block should be set to the current block"
        );

        // Lock cost should be exactly 2,500 TAO immediately after migration
        let lock_cost_now = Pallet::<Test>::get_network_lock_cost();
        assert_eq!(
            lock_cost_now,
            TARGET_COST_RAO.into(),
            "lock cost should be 2,500 TAO right after migration"
        );

        // Interval should be unchanged by this migration
        assert_eq!(
            NetworkLockReductionInterval::<Test>::get(),
            interval_before,
            "lock reduction interval should not be modified by this migration"
        );

        assert!(
            HasMigrationRun::<Test>::get(migration_key.clone()),
            "HasMigrationRun should be true after migration"
        );

        // ── decay check (1 block later) ─────────────────────────────────────
        // Expected: cost = max(min_lock, 2*L - floor(L / eff_interval) * delta_blocks)
        let eff_interval = Pallet::<Test>::get_lock_reduction_interval();
        let per_block_decrement: u64 = if eff_interval == 0 {
            0
        } else {
            NEW_LAST_LOCK_RAO / eff_interval
        };

        let min_lock_rao: u64 = Pallet::<Test>::get_network_min_lock().to_u64();

        step_block(1);
        let expected_after_1: u64 =
            core::cmp::max(min_lock_rao, TARGET_COST_RAO - per_block_decrement);
        let lock_cost_after_1 = Pallet::<Test>::get_network_lock_cost();
        assert_eq!(
            lock_cost_after_1,
            expected_after_1.into(),
            "lock cost should decay by one per-block step after 1 block"
        );

        // ── idempotency: running the migration again should do nothing ──────
        let last_lock_before_rerun = Pallet::<Test>::get_network_last_lock();
        let last_lock_block_before_rerun = Pallet::<Test>::get_network_last_lock_block();
        let cost_before_rerun = Pallet::<Test>::get_network_lock_cost();

        let _weight2 = crate::migrations::migrate_network_lock_cost_2500::migrate_network_lock_cost_2500::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(migration_key.clone()),
            "HasMigrationRun remains true on second run"
        );
        assert_eq!(
            Pallet::<Test>::get_network_last_lock(),
            last_lock_before_rerun,
            "second run should not modify last_lock"
        );
        assert_eq!(
            Pallet::<Test>::get_network_last_lock_block(),
            last_lock_block_before_rerun,
            "second run should not modify last_lock_block"
        );
        assert_eq!(
            Pallet::<Test>::get_network_lock_cost(),
            cost_before_rerun,
            "second run should not change current lock cost"
        );
    });
}

#[test]
fn test_migrate_kappa_map_to_default() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // 0. Constants / helpers
        // ------------------------------
        const MIG_NAME: &[u8] = b"kappa_map_to_default";
        let default: u16 = DefaultKappa::<Test>::get();

        let not_default: u16 = if default == u16::MAX {
            default - 1
        } else {
            default + 1
        };

        // ------------------------------
        // 1. Pre-state: seed using the correct key type (NetUid)
        // ------------------------------
        let n0: NetUid = 0u16.into();
        let n1: NetUid = 1u16.into();
        let n2: NetUid = 42u16.into();

        Kappa::<Test>::insert(n0, not_default);
        Kappa::<Test>::insert(n1, default);
        Kappa::<Test>::insert(n2, not_default);

        assert_eq!(
            Kappa::<Test>::get(n0),
            not_default,
            "precondition failed: Kappa[n0] should be non-default before migration"
        );
        assert_eq!(
            Kappa::<Test>::get(n1),
            default,
            "precondition failed: Kappa[n1] should be default before migration"
        );
        assert_eq!(
            Kappa::<Test>::get(n2),
            not_default,
            "precondition failed: Kappa[n2] should be non-default before migration"
        );

        assert!(
            !HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should be false before run"
        );

        // ------------------------------
        // 2. Run migration
        // ------------------------------
        let w =
            crate::migrations::migrate_kappa_map_to_default::migrate_kappa_map_to_default::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // ------------------------------
        // 3. Verify results
        // ------------------------------
        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag not set"
        );

        assert_eq!(
            Kappa::<Test>::get(n0),
            default,
            "Kappa[n0] should be reset to the configured default"
        );
        assert_eq!(
            Kappa::<Test>::get(n1),
            default,
            "Kappa[n1] should remain at the configured default"
        );
        assert_eq!(
            Kappa::<Test>::get(n2),
            default,
            "Kappa[n2] should be reset to the configured default"
        );
    });
}

#[test]
fn test_migrate_remove_tao_dividends() {
    const MIGRATION_NAME: &str = "migrate_remove_tao_dividends";
    let pallet_name = "SubtensorModule";
    let storage_name = "TaoDividendsPerSubnet";
    let migration =
        crate::migrations::migrate_remove_tao_dividends::migrate_remove_tao_dividends::<Test>;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );

    let storage_name = "PendingAlphaSwapped";
    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );

    let storage_name = "PendingRootDivs";
    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

fn do_setup_unactive_sn() -> (Vec<NetUid>, Vec<NetUid>) {
    // Register some subnets
    let netuid0 = add_dynamic_network_without_emission_block(&U256::from(0), &U256::from(0));
    let netuid1 = add_dynamic_network_without_emission_block(&U256::from(1), &U256::from(1));
    let netuid2 = add_dynamic_network_without_emission_block(&U256::from(2), &U256::from(2));
    let inactive_netuids = vec![netuid0, netuid1, netuid2];
    // Add active subnets
    let netuid3 = add_dynamic_network_without_emission_block(&U256::from(3), &U256::from(3));
    let netuid4 = add_dynamic_network_without_emission_block(&U256::from(4), &U256::from(4));
    let netuid5 = add_dynamic_network_without_emission_block(&U256::from(5), &U256::from(5));
    let active_netuids = vec![netuid3, netuid4, netuid5];
    let netuids: Vec<NetUid> = inactive_netuids
        .iter()
        .chain(active_netuids.iter())
        .copied()
        .collect();

    let initial_tao = Pallet::<Test>::get_network_min_lock();
    let initial_alpha: AlphaBalance = initial_tao.to_u64().into();

    const EXTRA_POOL_TAO: u64 = 123_123_u64;
    const EXTRA_POOL_ALPHA: u64 = 123_123_u64;

    // Add stake to the subnet pools
    for netuid in &netuids {
        let extra_for_pool = TaoBalance::from(EXTRA_POOL_TAO);
        let stake_in_pool = TaoBalance::from(
            u64::from(initial_tao)
                .checked_add(EXTRA_POOL_TAO)
                .expect("initial_tao + extra_for_pool overflow"),
        );
        SubnetTAO::<Test>::insert(netuid, stake_in_pool);
        TotalStake::<Test>::mutate(|total_stake| {
            let updated_total = u64::from(*total_stake)
                .checked_add(EXTRA_POOL_TAO)
                .expect("total stake overflow");
            *total_stake = updated_total.into();
        });
        TotalIssuance::<Test>::mutate(|total_issuance| {
            let updated_total = u64::from(*total_issuance)
                .checked_add(EXTRA_POOL_TAO)
                .expect("total issuance overflow");
            *total_issuance = updated_total.into();
        });

        let subnet_alpha_in = AlphaBalance::from(
            u64::from(initial_alpha)
                .checked_add(EXTRA_POOL_ALPHA)
                .expect("initial alpha + extra alpha overflow"),
        );
        SubnetAlphaIn::<Test>::insert(netuid, subnet_alpha_in);
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(EXTRA_POOL_ALPHA));
        SubnetVolume::<Test>::insert(netuid, 123123_u128);

        // Try registering on the subnet to simulate a real network
        // give balance to the coldkey
        let coldkey_account_id = U256::from(1111);
        let hotkey_account_id = U256::from(1111);
        let burn_cost = SubtensorModule::get_burn(*netuid);
        add_balance_to_coldkey_account(&coldkey_account_id, burn_cost.into());
        TotalIssuance::<Test>::mutate(|total_issuance| {
            let updated_total = u64::from(*total_issuance)
                .checked_add(u64::from(burn_cost))
                .expect("total issuance overflow (burn)");
            *total_issuance = updated_total.into();
        });

        // register the neuron
        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            *netuid,
            hotkey_account_id
        ));
    }

    for netuid in &active_netuids {
        // Set the FirstEmissionBlockNumber for the active subnet
        FirstEmissionBlockNumber::<Test>::insert(netuid, 100);
        // Also set SubtokenEnabled to true
        SubtokenEnabled::<Test>::insert(netuid, true);
    }

    let alpha_amt = AlphaBalance::from(123123_u64);
    // Create some Stake entries
    for netuid in &netuids {
        for hotkey in 0..10 {
            let hk = U256::from(hotkey);
            TotalHotkeyAlpha::<Test>::insert(hk, netuid, alpha_amt);
            TotalHotkeyShares::<Test>::insert(hk, netuid, U64F64::from(123123_u64));
            TotalHotkeyAlphaLastEpoch::<Test>::insert(hk, netuid, alpha_amt);

            RootClaimable::<Test>::mutate(hk, |claimable| {
                claimable.insert(*netuid, I96F32::from(alpha_amt.to_u64()));
            });
            for coldkey in 0..10 {
                let ck = U256::from(coldkey);
                Alpha::<Test>::insert((hk, ck, netuid), U64F64::from(123_u64));
                RootClaimed::<Test>::insert((netuid, hk, ck), 222_u128);
            }
        }
    }
    // Add some pending emissions
    let alpha_em_amt = AlphaBalance::from(355555_u64);
    for netuid in &netuids {
        PendingServerEmission::<Test>::insert(netuid, alpha_em_amt);
        PendingValidatorEmission::<Test>::insert(netuid, alpha_em_amt);
        PendingRootAlphaDivs::<Test>::insert(netuid, alpha_em_amt);
        PendingOwnerCut::<Test>::insert(netuid, alpha_em_amt);

        SubnetTaoInEmission::<Test>::insert(netuid, TaoBalance::from(12345678_u64));
        SubnetAlphaInEmission::<Test>::insert(netuid, AlphaBalance::from(12345678_u64));
        SubnetAlphaOutEmission::<Test>::insert(netuid, AlphaBalance::from(12345678_u64));
    }

    (active_netuids, inactive_netuids)
}

#[test]
fn test_migrate_reset_unactive_sn_get_unactive_netuids() {
    new_test_ext(1).execute_with(|| {
        let (active_netuids, inactive_netuids) = do_setup_unactive_sn();

        let initial_tao = Pallet::<Test>::get_network_min_lock();
        let initial_alpha: AlphaBalance = initial_tao.to_u64().into();

        let (unactive_netuids, w) =
            crate::migrations::migrate_reset_unactive_sn::get_unactive_sn_netuids::<Test>(
                initial_alpha,
            );
        // Make sure ALL the inactive subnets are in the unactive netuids
        assert!(
            inactive_netuids
                .iter()
                .all(|netuid| unactive_netuids.contains(netuid))
        );
        // Make sure the active subnets are not in the unactive netuids
        assert!(
            active_netuids
                .iter()
                .all(|netuid| !unactive_netuids.contains(netuid))
        );
    });
}

#[test]
fn test_migrate_reset_unactive_sn() {
    new_test_ext(1).execute_with(|| {
        use sp_std::collections::btree_map::BTreeMap;

        let (active_netuids, inactive_netuids) = do_setup_unactive_sn();

        let initial_tao = Pallet::<Test>::get_network_min_lock();
        let initial_alpha: AlphaBalance = initial_tao.to_u64().into();

        let mut locked_before: BTreeMap<NetUid, TaoBalance> = BTreeMap::new();
        let mut rao_recycled_before: BTreeMap<NetUid, TaoBalance> = BTreeMap::new();

        for netuid in active_netuids.iter().chain(inactive_netuids.iter()) {
            locked_before.insert(*netuid, SubnetLocked::<Test>::get(*netuid));
            rao_recycled_before.insert(*netuid, RAORecycledForRegistration::<Test>::get(netuid));
        }

        // Run the migration
        let w = crate::migrations::migrate_reset_unactive_sn::migrate_reset_unactive_sn::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Verify the results
        for netuid in &inactive_netuids {
            let netuid = *netuid;

            assert_eq!(
                SubnetLocked::<Test>::get(netuid),
                *locked_before.get(&netuid).unwrap(),
                "SubnetLocked unexpectedly changed for inactive subnet {netuid:?}"
            );
            assert_eq!(
                RAORecycledForRegistration::<Test>::get(netuid),
                *rao_recycled_before.get(&netuid).unwrap(),
                "RAORecycledForRegistration unexpectedly changed for inactive subnet {netuid:?}"
            );

            assert_eq!(
                PendingServerEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_eq!(
                PendingValidatorEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_eq!(
                PendingRootAlphaDivs::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_eq!(
                // not modified
                RAORecycledForRegistration::<Test>::get(netuid),
                *rao_recycled_before.get(&netuid).unwrap()
            );
            assert_eq!(PendingOwnerCut::<Test>::get(netuid), AlphaBalance::ZERO);
            assert_ne!(SubnetTAO::<Test>::get(netuid), initial_tao);
            assert_ne!(SubnetAlphaIn::<Test>::get(netuid), initial_alpha);
            assert_ne!(SubnetAlphaOut::<Test>::get(netuid), AlphaBalance::ZERO);
            assert_eq!(SubnetTaoInEmission::<Test>::get(netuid), TaoBalance::ZERO);
            assert_eq!(
                SubnetAlphaInEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_eq!(
                SubnetAlphaOutEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_ne!(SubnetVolume::<Test>::get(netuid), 0u128);
            for hotkey in 0..10 {
                let hk = U256::from(hotkey);
                assert_ne!(
                    TotalHotkeyAlpha::<Test>::get(hk, netuid),
                    AlphaBalance::ZERO
                );
                assert_ne!(
                    TotalHotkeyShares::<Test>::get(hk, netuid),
                    U64F64::from_num(0.0)
                );
                assert_ne!(
                    TotalHotkeyAlphaLastEpoch::<Test>::get(hk, netuid),
                    AlphaBalance::ZERO
                );
                assert_ne!(RootClaimable::<Test>::get(hk).get(&netuid), None);
                for coldkey in 0..10 {
                    let ck = U256::from(coldkey);
                    assert_ne!(Alpha::<Test>::get((hk, ck, netuid)), U64F64::from_num(0.0));
                    assert_ne!(RootClaimed::<Test>::get((netuid, hk, ck)), 0u128);
                }
            }

            // Don't touch SubnetLocked
            assert_ne!(SubnetLocked::<Test>::get(netuid), TaoBalance::ZERO);
        }

        // !!! Make sure the active subnets were not reset
        for netuid in &active_netuids {
            let netuid = *netuid;

            assert_eq!(
                SubnetLocked::<Test>::get(netuid),
                *locked_before.get(&netuid).unwrap(),
                "SubnetLocked unexpectedly changed for active subnet {netuid:?}"
            );
            assert_eq!(
                RAORecycledForRegistration::<Test>::get(netuid),
                *rao_recycled_before.get(&netuid).unwrap(),
                "RAORecycledForRegistration unexpectedly changed for active subnet {netuid:?}"
            );

            assert_ne!(
                PendingServerEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_ne!(
                PendingValidatorEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_ne!(
                PendingRootAlphaDivs::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_eq!(
                // unchanged (already asserted above via snapshot)
                RAORecycledForRegistration::<Test>::get(netuid),
                *rao_recycled_before.get(&netuid).unwrap()
            );
            assert_ne!(SubnetTaoInEmission::<Test>::get(netuid), TaoBalance::ZERO);
            assert_ne!(
                SubnetAlphaInEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_ne!(
                SubnetAlphaOutEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_ne!(PendingOwnerCut::<Test>::get(netuid), AlphaBalance::ZERO);
            assert_ne!(SubnetTAO::<Test>::get(netuid), initial_tao);
            assert_ne!(SubnetAlphaIn::<Test>::get(netuid), initial_alpha);
            assert_ne!(SubnetAlphaOut::<Test>::get(netuid), AlphaBalance::ZERO);
            assert_ne!(SubnetVolume::<Test>::get(netuid), 0u128);
            for hotkey in 0..10 {
                let hk = U256::from(hotkey);
                assert_ne!(
                    TotalHotkeyAlpha::<Test>::get(hk, netuid),
                    AlphaBalance::ZERO
                );
                assert_ne!(
                    TotalHotkeyShares::<Test>::get(hk, netuid),
                    U64F64::from_num(0.0)
                );
                assert_ne!(
                    TotalHotkeyAlphaLastEpoch::<Test>::get(hk, netuid),
                    AlphaBalance::ZERO
                );
                assert!(RootClaimable::<Test>::get(hk).contains_key(&netuid));
                for coldkey in 0..10 {
                    let ck = U256::from(coldkey);
                    assert_ne!(Alpha::<Test>::get((hk, ck, netuid)), U64F64::from_num(0.0));
                    assert_ne!(RootClaimed::<Test>::get((netuid, hk, ck)), 0u128);
                }
            }
            // Don't touch SubnetLocked
            assert_ne!(SubnetLocked::<Test>::get(netuid), TaoBalance::ZERO);
        }
    });
}

#[test]
fn test_migrate_reset_unactive_sn_idempotence() {
    new_test_ext(1).execute_with(|| {
        let (active_netuids, inactive_netuids) = do_setup_unactive_sn();
        let netuids = inactive_netuids
            .iter()
            .chain(active_netuids.iter())
            .copied()
            .collect::<Vec<_>>();

        // Run total issuance migration *before* running the migration.
        crate::migrations::migrate_init_total_issuance::migrate_init_total_issuance::<Test>();

        // Run the migration
        let w = crate::migrations::migrate_reset_unactive_sn::migrate_reset_unactive_sn::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Store the values after running the migration
        let mut subnet_tao_before = BTreeMap::new();
        for netuid in &netuids {
            subnet_tao_before.insert(netuid, SubnetTAO::<Test>::get(netuid));
        }
        let total_stake_before = TotalStake::<Test>::get();
        let total_issuance_before = TotalIssuance::<Test>::get();

        // Run total issuance migration again, to make sure no changes happen from it.
        crate::migrations::migrate_init_total_issuance::migrate_init_total_issuance::<Test>();

        // Verify that none of the values are different
        for netuid in &netuids {
            assert_eq!(
                SubnetTAO::<Test>::get(netuid),
                *subnet_tao_before.get(netuid).unwrap_or(&TaoBalance::ZERO)
            );
        }
        assert_eq!(TotalStake::<Test>::get(), total_stake_before);
        assert_eq!(TotalIssuance::<Test>::get(), total_issuance_before);
    });
}

fn test_migrate_remove_old_identity_maps() {
    let migration =
        crate::migrations::migrate_remove_old_identity_maps::migrate_remove_old_identity_maps::<Test>;

    const MIGRATION_NAME: &str = "migrate_remove_old_identity_maps";

    let pallet_name = "SubtensorModule";

    test_remove_storage_item(MIGRATION_NAME, pallet_name, "Identities", migration, 100);

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        "SubnetIdentities",
        migration,
        100,
    );

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        "SubnetIdentitiesV2",
        migration,
        100,
    );
}

#[test]
fn test_migrate_remove_unknown_neuron_axon_cert_prom() {
    use crate::migrations::migrate_remove_unknown_neuron_axon_cert_prom::*;
    const MIGRATION_NAME: &[u8] = b"migrate_remove_neuron_axon_cert_prom";

    new_test_ext(1).execute_with(|| {
        setup_for(NetUid::from(2), 64, 1231);
        setup_for(NetUid::from(42), 256, 15151);
        setup_for(NetUid::from(99), 1024, 32323);
        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME));

        let w = migrate_remove_unknown_neuron_axon_cert_prom::<Test>();
        assert!(!w.is_zero(), "Weight must be non-zero");

        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME));
        assert_for(NetUid::from(2), 64, 1231);
        assert_for(NetUid::from(42), 256, 15151);
        assert_for(NetUid::from(99), 1024, 32323);
    });

    fn setup_for(netuid: NetUid, uids: u32, items: u32) {
        NetworksAdded::<Test>::insert(netuid, true);

        for i in 1u32..=uids {
            let hk = U256::from(netuid.inner() as u32 * 1000 + i);
            Uids::<Test>::insert(netuid, hk, i as u16);
        }

        for i in 1u32..=items {
            let hk = U256::from(netuid.inner() as u32 * 1000 + i);
            Axons::<Test>::insert(netuid, hk, AxonInfo::default());
            NeuronCertificates::<Test>::insert(netuid, hk, NeuronCertificate::default());
            Prometheus::<Test>::insert(netuid, hk, PrometheusInfo::default());
        }
    }

    fn assert_for(netuid: NetUid, uids: u32, items: u32) {
        assert_eq!(
            Axons::<Test>::iter_key_prefix(netuid).count(),
            uids as usize
        );
        assert_eq!(
            NeuronCertificates::<Test>::iter_key_prefix(netuid).count(),
            uids as usize
        );
        assert_eq!(
            Prometheus::<Test>::iter_key_prefix(netuid).count(),
            uids as usize
        );

        for i in 1u32..=uids {
            let hk = U256::from(netuid.inner() as u32 * 1000 + i);
            assert!(Axons::<Test>::contains_key(netuid, hk));
            assert!(NeuronCertificates::<Test>::contains_key(netuid, hk));
            assert!(Prometheus::<Test>::contains_key(netuid, hk));
        }

        for i in uids + 1u32..=items {
            let hk = U256::from(netuid.inner() as u32 * 1000 + i);
            assert!(!Axons::<Test>::contains_key(netuid, hk));
            assert!(!NeuronCertificates::<Test>::contains_key(netuid, hk));
            assert!(!Prometheus::<Test>::contains_key(netuid, hk));
        }
    }
}

// cargo test --package pallet-subtensor --lib -- tests::migration::test_migrate_cleanup_swap_v3 --exact --nocapture
#[test]
fn test_migrate_cleanup_swap_v3() {
    use crate::migrations::migrate_cleanup_swap_v3::deprecated_swap_maps;
    use substrate_fixed::types::U64F64;

    new_test_ext(1).execute_with(|| {
        let migration = crate::migrations::migrate_cleanup_swap_v3::migrate_cleanup_swap_v3::<Test>;

        const MIGRATION_NAME: &str = "migrate_cleanup_swap_v3";

        let provided: u64 = 9876;
        let reserves: u64 = 1_000_000;

        SubnetTAO::<Test>::insert(NetUid::from(1), TaoBalance::from(reserves));
        SubnetAlphaIn::<Test>::insert(NetUid::from(1), AlphaBalance::from(reserves));

        // Insert deprecated maps values
        deprecated_swap_maps::SubnetTaoProvided::<Test>::insert(
            NetUid::from(1),
            TaoBalance::from(provided),
        );
        deprecated_swap_maps::SubnetAlphaInProvided::<Test>::insert(
            NetUid::from(1),
            AlphaBalance::from(provided),
        );

        // Run migration
        let weight = migration();

        // Test that values are removed from state
        assert!(!deprecated_swap_maps::SubnetTaoProvided::<Test>::contains_key(NetUid::from(1)),);
        assert!(
            !deprecated_swap_maps::SubnetAlphaInProvided::<Test>::contains_key(NetUid::from(1)),
        );

        // Provided got added to reserves
        assert_eq!(
            u64::from(SubnetTAO::<Test>::get(NetUid::from(1))),
            reserves + provided
        );
        assert_eq!(
            u64::from(SubnetAlphaIn::<Test>::get(NetUid::from(1))),
            reserves + provided
        );
    });
}

#[test]
fn test_migrate_coldkey_swap_scheduled_to_announcements() {
    new_test_ext(1000).execute_with(|| {
        use crate::migrations::migrate_coldkey_swap_scheduled_to_announcements::*;
        use coldkey_swap_deprecated as deprecated;

        const MIGRATION_NAME: &[u8] = b"migrate_coldkey_swap_scheduled_to_announcements";
        let now = frame_system::Pallet::<Test>::block_number();

        // Set the schedule duration and reschedule duration
        deprecated::ColdkeySwapScheduleDuration::<Test>::set(Some(now + 100));
        deprecated::ColdkeySwapRescheduleDuration::<Test>::set(Some(now + 200));

        let make_swap_task = |who: U256, new_coldkey: U256| -> ScheduledOf<Test> {
            let call_bytes = deprecated::RuntimeCall::<Test>::SubtensorCall(
                deprecated::SubtensorCall::SwapColdkey {
                    old_coldkey: who,
                    new_coldkey,
                    swap_cost: 1000.into(),
                },
            )
            .encode();
            pallet_scheduler::Scheduled {
                maybe_id: None,
                priority: 63,
                call: Bounded::Inline(BoundedVec::truncate_from(call_bytes)),
                maybe_periodic: None,
                origin: OriginCaller::system(frame_system::RawOrigin::Root),
                _phantom: PhantomData,
            }
        };

        let make_other_task = || -> ScheduledOf<Test> {
            let call_bytes = RuntimeCall::SubtensorModule(crate::Call::burned_register {
                netuid: 1u16.into(),
                hotkey: U256::from(999),
            })
            .encode();
            pallet_scheduler::Scheduled {
                maybe_id: None,
                priority: 63,
                call: Bounded::Inline(BoundedVec::truncate_from(call_bytes)),
                maybe_periodic: None,
                origin: OriginCaller::system(frame_system::RawOrigin::Root),
                _phantom: PhantomData,
            }
        };

        deprecated::ColdkeySwapScheduled::<Test>::insert(
            U256::from(1),
            (now + 100, U256::from(10)),
        );
        pallet_scheduler::Agenda::<Test>::insert(
            now + 100,
            BoundedVec::truncate_from(vec![
                Some(make_swap_task(U256::from(1), U256::from(10))),
                Some(make_other_task()),
            ]),
        );

        deprecated::ColdkeySwapScheduled::<Test>::insert(
            U256::from(2),
            (now - 200, U256::from(20)),
        );

        deprecated::ColdkeySwapScheduled::<Test>::insert(
            U256::from(3),
            (now + 200, U256::from(30)),
        );
        pallet_scheduler::Agenda::<Test>::insert(
            now + 200,
            BoundedVec::truncate_from(vec![Some(make_swap_task(U256::from(3), U256::from(30)))]),
        );

        deprecated::ColdkeySwapScheduled::<Test>::insert(
            U256::from(4),
            (now - 400, U256::from(40)),
        );

        deprecated::ColdkeySwapScheduled::<Test>::insert(
            U256::from(5),
            (now + 300, U256::from(50)),
        );
        pallet_scheduler::Agenda::<Test>::insert(
            now + 300,
            BoundedVec::truncate_from(vec![
                Some(make_other_task()),
                Some(make_swap_task(U256::from(5), U256::from(50))),
            ]),
        );

        let w = migrate_coldkey_swap_scheduled_to_announcements::<Test>();

        assert!(!w.is_zero(), "weight must be non-zero");
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME));

        // Ensure the deprecated storage is cleared
        assert!(!deprecated::ColdkeySwapScheduleDuration::<Test>::exists());
        assert!(!deprecated::ColdkeySwapRescheduleDuration::<Test>::exists());
        assert_eq!(deprecated::ColdkeySwapScheduled::<Test>::iter().count(), 0);

        assert_eq!(
            pallet_scheduler::Agenda::<Test>::get(now + 100),
            vec![None, Some(make_other_task())],
            "swap task for who=1 should be cancelled"
        );

        assert_eq!(
            pallet_scheduler::Agenda::<Test>::get(now + 200),
            vec![None],
            "swap task for who=3 should be cancelled"
        );

        assert_eq!(
            pallet_scheduler::Agenda::<Test>::get(now + 300),
            vec![Some(make_other_task()), None],
            "swap task for who=5 should be cancelled"
        );

        let delay = ColdkeySwapAnnouncementDelay::<Test>::get();
        assert_eq!(ColdkeySwapAnnouncements::<Test>::iter().count(), 3);
        assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(U256::from(
            2
        )));
        assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(U256::from(
            4
        )));
        assert_eq!(
            ColdkeySwapAnnouncements::<Test>::get(U256::from(1)),
            Some((
                now + 100 - delay,
                <Test as frame_system::Config>::Hashing::hash_of(&U256::from(10))
            ))
        );
        assert_eq!(
            ColdkeySwapAnnouncements::<Test>::get(U256::from(3)),
            Some((
                now + 200 - delay,
                <Test as frame_system::Config>::Hashing::hash_of(&U256::from(30))
            ))
        );
        assert_eq!(
            ColdkeySwapAnnouncements::<Test>::get(U256::from(5)),
            Some((
                now + 300 - delay,
                <Test as frame_system::Config>::Hashing::hash_of(&U256::from(50))
            ))
        );
    });
}

#[test]
fn test_migrate_clear_deprecated_registration_maps() {
    new_test_ext(1).execute_with(|| {
        const MIG_NAME: &[u8] = b"migrate_clear_deprecated_registration_maps_v1";

        let netuid0: NetUid = 0u16.into();
        let netuid1: NetUid = 1u16.into();

        // --------------------------------------------------------------------
        // 0) Pre-state
        // --------------------------------------------------------------------
        assert!(
            !HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should be false before run"
        );

        // New-model storage must remain untouched by this migration.
        crate::BurnHalfLife::<Test>::insert(netuid0, 777u16);
        crate::BurnIncreaseMult::<Test>::insert(netuid0, U64F64::from_num(9));

        crate::BurnHalfLife::<Test>::insert(netuid1, 888u16);
        crate::BurnIncreaseMult::<Test>::insert(netuid1, U64F64::from_num(11));

        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid0), 777u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid0), 9u64);

        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid1), 888u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid1), 11u64);

        // Seed deprecated storage items that the migration is expected to clear.
        crate::NetworkPowRegistrationAllowed::<Test>::insert(netuid0, true);

        crate::POWRegistrationsThisInterval::<Test>::insert(netuid0, 7u16);
        crate::BurnRegistrationsThisInterval::<Test>::insert(netuid0, 8u16);

        crate::NetworkPowRegistrationAllowed::<Test>::insert(netuid1, false);

        crate::POWRegistrationsThisInterval::<Test>::insert(netuid1, 17u16);
        crate::BurnRegistrationsThisInterval::<Test>::insert(netuid1, 18u16);

        assert!(crate::NetworkPowRegistrationAllowed::<Test>::contains_key(netuid0));
        assert!(crate::POWRegistrationsThisInterval::<Test>::contains_key(netuid0));
        assert!(crate::BurnRegistrationsThisInterval::<Test>::contains_key(netuid0));

        assert!(crate::NetworkPowRegistrationAllowed::<Test>::contains_key(netuid1));
        assert!(crate::POWRegistrationsThisInterval::<Test>::contains_key(netuid1));
        assert!(crate::BurnRegistrationsThisInterval::<Test>::contains_key(netuid1));

        // --------------------------------------------------------------------
        // 1) Run migration
        // --------------------------------------------------------------------
        let w = crate::migrations::migrate_clear_deprecated_registration_maps::migrate_clear_deprecated_registration_maps::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // --------------------------------------------------------------------
        // 2) Post-state: deprecated storage cleared
        // --------------------------------------------------------------------
        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should be true after run"
        );

        assert!(!crate::NetworkPowRegistrationAllowed::<Test>::contains_key(netuid0));
        assert!(!crate::POWRegistrationsThisInterval::<Test>::contains_key(netuid0));
        assert!(!crate::BurnRegistrationsThisInterval::<Test>::contains_key(netuid0));

        assert!(!crate::NetworkPowRegistrationAllowed::<Test>::contains_key(netuid1));
        assert!(!crate::POWRegistrationsThisInterval::<Test>::contains_key(netuid1));
        assert!(!crate::BurnRegistrationsThisInterval::<Test>::contains_key(netuid1));

        // --------------------------------------------------------------------
        // 3) Post-state: new-model storage unchanged
        // --------------------------------------------------------------------
        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid0), 777u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid0), 9u64);

        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid1), 888u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid1), 11u64);

        // --------------------------------------------------------------------
        // 4) Idempotency
        // --------------------------------------------------------------------
        let w2 = crate::migrations::migrate_clear_deprecated_registration_maps::migrate_clear_deprecated_registration_maps::<Test>();
        assert!(!w2.is_zero(), "second call should still return non-zero read weight");

        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should remain true after second run"
        );

        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid0), 777u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid0), 9u64);

        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid1), 888u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid1), 11u64);
    });
}

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

fn decode_account_id32<T: Config>(ss58_string: &str) -> Option<T::AccountId> {
    let account_id32: AccountId32 = AccountId32::from_ss58check(ss58_string).ok()?;
    let mut account_id32_slice: &[u8] = account_id32.as_ref();
    T::AccountId::decode(&mut account_id32_slice).ok()
}

#[test]
fn test_migrate_fix_bad_hk_swap_mainnet() {
    new_test_ext(1).execute_with(|| {
        use crate::migrations::migrate_fix_bad_hk_swap::*;

        let netuid = NetUid::from(59);
        // Add subnet 59
        add_network(netuid, 10, 0);
        SubtokenEnabled::<Test>::insert(netuid, true);
        SubnetMechanism::<Test>::insert(netuid, 1);

        let hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
        let hotkey_account_id = decode_account_id32::<Test>(hotkey).expect("Invalid hotkey");

        #[rustfmt::skip]
        let diffs: [(&str, i64); 112] = [
            ("5Fn9SqQhx5bhDua7AGgkKxxk3gfZ75WWBGCMPeKH1WBgPaMQ", -2375685930981_i64),
            ("5Fnhtm7cpxEbZaChnRZ8yWoF8MXVxmobkmLRehh5bkYtyZA9", -4090996138227),
            ("5C7j3w2zz1SVejRuFrb2zFWHXT7UfG7eWA87KXL1WyV5KLVR", -607494031),
            ("5DthZ1rvnXBb9oXVNtrMaMsDAnRxBPZCjD6fdRdeqC3fg1ca", -17022477949),
            ("5F7BkPL3EVjKTYMbBkEmPAtTZQSGeyNzFPaf1DtebPFmJsJ7", -4016510),
            ("5EefisctzgWdVGFQaL4LjFFacTE7dM4YJVNy3ogGBQoapTU1", -13106893093),
            ("5CwkvpBxHCaRK9xBC2n6WdhpF5zg9t5WLkGorASaoErdynFQ", 439139249152),
            ("5FU7ErUtmi22xuqeeCYVpNZp6WVSSL98hqDi5iyeZbkXtkbe", -35958768555),
            ("5D7HL8T95qkHQTPFjgSFCjRoeM7oE3vQBYjiR1kAPbPxcMKu", -201914811997),
            ("5HL3pPdDFY94Qdf8VnbfT4W6LXFkpd68Y5GSGzNJfntMdGZX", -235660917467),
            ("5EcYAz8SBKWsogA6meJmVXcwVp4tjCvw3ZnJE6UXTyWNUdF2", -500070769668),
            ("5EoE3c7XMf8TN3yudAaFjv4yvjtWYRviHcXi73EXkLHmWTCB", -86442928436),
            ("5CMDjL7t2biHGREBwrmd8renD74FLEhjCVqfJG2MXckWBwDu", 1039317),
            ("5CVGKimL4cLgyTqvYKQbPKYFZfiztsdczU7HrwNdSFKbbn5D", 4224201),
            ("5HmZnEcW4eHbXmUEFWJbc4GHnBBYEK8ZPsFa25PuEmP5iuwM", -13156128),
            ("5FNa4J4fTKh555CEyXHgR29RicSm8nTEHx36utTa4MJepJyX", -9519954),
            ("5Gun93uQgffYpxqMKSmfG18AHiQW7Z2GR2dfPPR8W188vJYc", -1127662),
            ("5HW1C4js4RyjQqNwALSUZC8NJ2WinD5Si2X2XkstXrMW2uYo", -34457336758),
            ("5EqMhjdLY9h64ui2mizRZyBp1mEPJ7s4TsfAxQSQkAFmMfzE", -9346443829744),
            ("5H3XwzydgE2XUGoJCR4dSj7tkd7uxZDJqik69hux2DBcruom", -1215347774),
            ("5DjkmYpCUX6dBTvGoyN9j4QZhtMPhdcywDE8cJ8Qq1vg4X6e", -3603984447),
            ("5E7Z7Btjz74XpZLH5fRzfZqiHCo4j9PXKfqi88kQ5MFrds34", -823907380854),
            ("5DSBWN4hN9413C6o6A2hR9tYUbHjWsQqPRV74GrnCrMkCGJx", -309708781),
            ("5FMLRmKPqsTsMbakpVUwoYro1P64QXVNTWyzNDugaNwSKRzF", -137525398263),
            ("5EFZf5pnTqLegv6gxCrb6TKBQBGz9xLJNK8x9eR273cSons6", -1521760918),
            ("5CGAGEuMLaidBDk8bDZKJb23dxRSP1wLenLALGLw8BTG1E3W", -544739696),
            ("5HSzRtcQjD5KP6Nh2GVSS16aLDe6q9R33Wpu6s2eEeeo3AYS", -2309184790),
            ("5DALvFDcfANQJcWz6AXMfDqabnoZhdDMoH6FxqYUibug1ja7", -369405632507),
            ("5Fy8iWkpcbsskmEN1nYZDdS9zKh167Em9RRYisoss7jaYXxi", 15257429),
            ("5CQCVTRyqJgZKBDmtHzpoF8su6BScLcNGbX8t3WMm5qYbbJH", -10721968),
            ("5DXZByh2NS4MU61a1aaLrcLYpyzpJgHe95TEBdcEN2cF1SA5", -655946136),
            ("5EX5yAYiABFzKDQJDe1kRVwFm3XRRY4HyLMe4Vu9A5U2VEVT", -325581360246),
            ("5FvabwjtyW887gtc7vUnUc47KVhy17UeaNLRjzTg5nkVACMP", -77588524213),
            ("5HTbYi5cmgWJxvyTy9JeYdtnjoDzjXnEXTGFsPEVx9iRPmVF", -53542953784),
            ("5CWzmvA17MAMQ9mnAecLxFXS2N8846rz6T7m4QNHyVtJVq4j", 2672295922502),
            ("5DSYntgHZY4krYUtkkQZyyoffVtu5e8rYWhXuhs832zY6YKy", -2680205688),
            ("5EYyTFyLDqXscaa5VtXTvUc3x2ow2TeT8G12ZDMZwE6uFWPQ", -39165843935),
            ("5CohfM1qdyNwdeJEex1Zyht3S2WS48rV993DmVbyKs2mEEd6", -4004685632),
            ("5Gx6Y7UQD39Latgxigr6mHbnh1herpwNPau2PjvzwLWEjXL3", -559504),
            ("5Hh4Efq5WDwe8URjjUqUNX8KxtMwLHLViwoRvXfEpXQCZakh", -32541090531),
            ("5GWRHC7Nd8njqTPsdJkp6ngniCCBu9UjGhLfxp2jF1fPrfZ4", -5394093031),
            ("5GNAB64UN32krzr3Xxu5LW6naeu2P3XULcdBCR9VZ5Libyit", -24884230),
            ("5EEz25th1nYNM5xR1UsyFFAUaXMjdHqLxZ3wUjyHokYbXHku", -12525171),
            ("5HKJq4JCS9xoKdYhcRnsRp1bodovba7ncd5KTYVwfReKaxHT", -408133990236),
            ("5DXs7x664RL5NdSW77DTseLiu84unstuHGuqvmY61UtJwzRN", -3095078614148),
            ("5CDfdDaA2p9sK1ia5yMVYfzgtFs2e1TrSAxuQqXoS28Lcrxf", -1032856892),
            ("5Ecg4vD2zKXHDFhQqogWq1dZdijPsDty8rGsZu3raeoJSiXb", -995678),
            ("5C5Yg63TNLb68Tu819qXd3Bt4giG8mAPzLmAFSqa2HC1R5Rm", -40818739830910),
            ("5HHH25Wuf9rmVuk9cMKU1hCCPJ1qbHBd1SyHj91R3fMT36yb", -391416057906),
            ("5GKGGE5YLHoDciYJ6Ec2YnUP3SykSQPA47hqmwBP63EtVrd9", -413944553000),
            ("5CSi9ZLyiXfLeYtEFaZSBuTofNMRnXEJEJE9CS4gGaT6CkWt", -17811605275),
            ("5CSoA7QVdFHHBZz53bbRV2mC5vhL64ehhWa8ibtLppmt2n3J", -65701320107),
            ("5GpA5BtfMMX52rXztrha79YqfwR4YaSfTuAcb48Yt73U4h71", -2194562),
            ("5Euz5wpb4xiDWfV1A6AKK6i6ca3WoZQD5hCVyf1fws8GXh4z", -6143407839874),
            ("5DZzmhCG7SMK3LwrkmHZ8ZBwaAByMjfBpEid14nNQdxHipCE", -386645),
            ("5Fc9Vo3hkbr6bPxJpjQo5sQ43L5Hc2G8R5BdqRYF8psvB5pw", 55668553),
            ("5GuSHC3iowySHLDW4pEyEZE6PKxKP62YpJYJyBy5tijzAnYz", -159317636526),
            ("5HVVZrUBPvjYHiwaSvtvaN9GZogoznM49m2AEmVW6RXnYCka", -1995572213),
            ("5EcGpeV2wjkCVsBjsBifSWbdcqH98b6oEY8beDY59c4fXkhw", -177096614584),
            ("5GnCjvWJEESwVNFZzy85zbBzw26etuEt87WiqsE3ee2Ws1wm", -1961445),
            ("5GWuPUpTuChAqKxvU22TRLvRkBFiyWWZnq9cLpJN6SSvkho1", -94157569391),
            ("5FXHf7q5rvBXnzQgmsa31Db9rjcRy6ZHKMiyDSb8Vs5p2msN", -688433531658),
            ("5GbxkzytnvbRuNQ7qxPpfPuWMoeitS8V4KDY9jSshE5fDegD", -19085313),
            ("5Gus1B7c9uWkky7Yawh2tKR1V6AMh5DbqUBPq881JHqeqVqY", -16101671818),
            ("5DLhRdbvWkYYScDmwx4QgJfieSN4apBWbZ2yno3MfgbR8hBP", -21062025),
            ("5Cg5kVyNEs7MWWRHU8X5MHwX5cN3aegvC4RBt2JK19w2GiR8", -2593737050),
            ("5Dkushsxtc8AdCf287MtTYHQv9DoZeBRpttUpBtmyFhGy3uR", -48672832345630),
            ("5EqNqVsHj9bQVyEujcm62zjMYUFhTLY7rTP854txSrJzyoco", -3828526),
            ("5Dea6d6nKErEbRQ4MBGuCALn8NZ2xo4kaa51hB5KMriPBkEM", -1560192853875),
            ("5DNt2XDWdeMd4H92FLnfUvkqyXzmavezHvzLboP3VgT1xLZV", -831964576998),
            ("5FKtFoTeK8aaG6HZTrDgvoYHVQ5NY4S9VyV7W5K74cWcwLYA", -60823501166),
            ("5GEBanZKUU7Hrf8K2VNi33HxyJRstgQ3WD3odHgvMj2nPbhi", -98946626902),
            ("5CUtw7LYB2n2bzgXt6YnmKDHt6PsB3kKAyD9azYJNcRG8TNg", -9779588557490),
            ("5EynbF72b12fbgMvEeL1vJSY342rCryNbuwxFivU1Xevtmv3", -17314385200455),
            ("5CapiZRuULed8ConS1gbjMVgnwcT5JnQah7tx6sZnK7sJJuJ", -5810972),
            ("5DnaxLaNduf41WM6WWZ4fkzcGzWNWx6eLJyQpSaMueUGCsaU", -12668760),
            ("5Cqz9SChYPxTFZ2623rE2aQQ5ttQoLwZ8yfwYgiZyQDANqZn", -683549),
            ("5C8ZcLzF23GrXKdH4Pg3ZXC3vKQsF5PM8VvhzzxzTQksgj8e", -44720570590),
            ("5GuNsmoswrP6hTKZkKcpTpZftTMKrmnCHvTL2V3NHJy2fpen", -5042891812715),
            ("5F1TYDkLnP36HHY5btigxyKUPzBraxdrU1aX1bqFfPfcfnzU", -1189104279832),
            ("5Dc384z9HuTGF6oratZs1fLciCHtPZaLhrHfCVw82a5AikWZ", -616163196988),
            ("5DhcaEUsRKhZQ31qRffJqjtLmFkbVaCebn8nVjYhvB4KJtX5", -17746006723),
            ("5HYE7z3xTcrN1rqz54NyZRAkehFfRMcaEcdoMq5g5ATET5wQ", 212509751245),
            ("5F97DdEVTy9gPCtN6jkJJENDJuQiRGiwbMVSL74qRq8FCq5W", 2225287736222),
            ("5FUVN133rSvuKXgsXKMR2ZEaysxZjkRUFUWS1UMyNGre9xFV", -73216740161),
            ("5CZeimtfpRqQgPxVwr1MzfG2Sok8E1AMERHo6vUmEdRS5JiU", -3937802),
            ("5Eqq2JwGh7qbtnjPiFEPmmnHxs3S4J4Ahg8fr4sybZV1tPdY", -173406860562),
            ("5ERfDw6K3GmQqwqsEG6foFtu7VsYGifPi556UJKQsBnfbHKN", 96022588728),
            ("5Ek8RkU6KMv5Fx7yivRVoQkuJYAKhULWiLWDpbGG4hvR9HFD", 968139369093),
            ("5HpCpGALzqgnDTP1HXFiuhzD5MFaDTRHjXBCvaMY9LNNRkT9", 104943979521),
            ("5FYqS77gxW9gHG8id1YYPS7Cd4TQmNUMhF8h3S77Fq2VvvRQ", 729199757977),
            ("5FtBqMg13pNf1N6TwfG6BmwyaDM77mkeQ16UTHsGasrVDedX", 131457064336),
            ("5GbnWR2XhWrRMt123SdrLbR9G2a4N5dtzA3TSu3Czkzoeu7x", 2295599153),
            ("5GgiowcCG4kLpwkCTGxxQJQv8WwKFyBQ6McPRmqKtWPy8EaK", 113838605389),
            ("5FL5YtYozpUAGaiVWonpbwEYdEMij3obJHSH3ACY4vgWmDgy", 8689039),
            ("5Egq58bxRv7boM2s3rnDxx1udnkzxPQ23HuoqohVxjh9RenC", 216373234348),
            ("5FRGeeEgRNR8U33FDKvN7yUgts8zR3qRJH4yKKWoR9GswBRb", 2196574958718),
            ("5FnhSy79BPYyrmmFsbckinQw1fLiLqqPkQL2vgZwPxbRfu3k", 42319631507),
            ("5Hj8jMhqAv7cfyRh5STfbZefMhv17QxZ1RxWq9jNcLAEsRRo", 132216702183491),
            ("5EXYTGMqumAH6RLQgHwkMEMnSvHcpHc89R6U8krfNJTYWm9J", 504320264499),
            ("5EFh8ctzmytXURqrCTUBWHTs87f7TMWB6XKUzdqxKXVUtvS2", 2209599669432),
            ("5CqVqEcRBkw7Gm2reJ33cj7puR9W2Tq7qsLxSruV1BgnMqKN", 1033387458788),
            ("5D79enmLSGimsruoraGagofhaSeYJZvGUqFCCrr83ZfZs1HS", 7591184215233),
            ("5HbpyjsvyXLWtf1QT1CyNUdyut6scM5dM7ytm8hoxFvRtU1i", 129833188275),
            ("5CnxCi7CdEriWSdw4LcXdbtjodxA6uTat4gBm4wuT9QToMdo", 3132978),
            ("5G48fiQjhAd8hc4rYc6GituCuAPKznL28jyyyq1auMyZiG4t", 514913328178),
            ("5FFGjW2hJ7tQ41qghSsLP4cVmA8j9pZVSrr2CrLG7fQAsLHJ", 346794972723),
            ("5FWjnxeRMtMFxRc9kvZKCG5iJAyyz2kmXV8u3kqyiXizZtiz", 225939835005),
            ("5CUw3sB4oxd3dVSHUr3kxsB591VEjaPzr444KkfjwVFnLRfJ", 208250614494),
            ("5EaBhxNUwMRyKsaeA2BEjDCrvwE5J8FDSpfCHK9gGmnmbhCa", 278083207003),
            ("5GHJ5HxFxYQyVoNFUxR3JCqqCKRumaFCY7N5zMxwF4CpRUWr", 1381466224829),
            ("5H1WgA7ET3FmEarJK6qc1vaTWbNd6g41mgvyLRkysrH4MDdo", 774889),
        ];

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            if diff > 0 {
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                    diff.unsigned_abs().into(),
                );
            }
        }

        let w = try_restore_shares::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Check stake is near 0 for all positive entires and near diff for all negative
        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");

            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff > 0 {
                log::debug!("diff: {} for ck: {}", diff, coldkey);
                assert_relative_eq!(stake_float, 0_f64, max_relative = 0.001_f64);
            } else {
                let diff_float: f64 =
                    num_traits::ToPrimitive::to_f64(&diff.unsigned_abs()).expect("float conv fail");
                assert_relative_eq!(stake_float, diff_float, max_relative = 0.001_f64);
            }
        }
    });
}

#[test]
fn test_migrate_fix_bad_hk_swap_mainnet_some_exits() {
    // test with some of the gainers have exited fully or partially before the migration
    // i.e. balance is less than they owe
    new_test_ext(1).execute_with(|| {
        use crate::migrations::migrate_fix_bad_hk_swap::*;

        let netuid = NetUid::from(59);
        // Add subnet 59
        add_network(netuid, 10, 0);
        SubtokenEnabled::<Test>::insert(netuid, true);
        SubnetMechanism::<Test>::insert(netuid, 1);

        let hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
        let hotkey_account_id = decode_account_id32::<Test>(hotkey).expect("Invalid hotkey");

        #[rustfmt::skip]
        let diffs: [(&str, i64); 112] = [
            ("5Fn9SqQhx5bhDua7AGgkKxxk3gfZ75WWBGCMPeKH1WBgPaMQ", -2375685930981_i64),
            ("5Fnhtm7cpxEbZaChnRZ8yWoF8MXVxmobkmLRehh5bkYtyZA9", -4090996138227),
            ("5C7j3w2zz1SVejRuFrb2zFWHXT7UfG7eWA87KXL1WyV5KLVR", -607494031),
            ("5DthZ1rvnXBb9oXVNtrMaMsDAnRxBPZCjD6fdRdeqC3fg1ca", -17022477949),
            ("5F7BkPL3EVjKTYMbBkEmPAtTZQSGeyNzFPaf1DtebPFmJsJ7", -4016510),
            ("5EefisctzgWdVGFQaL4LjFFacTE7dM4YJVNy3ogGBQoapTU1", -13106893093),
            ("5CwkvpBxHCaRK9xBC2n6WdhpF5zg9t5WLkGorASaoErdynFQ", 439139249152),
            ("5FU7ErUtmi22xuqeeCYVpNZp6WVSSL98hqDi5iyeZbkXtkbe", -35958768555),
            ("5D7HL8T95qkHQTPFjgSFCjRoeM7oE3vQBYjiR1kAPbPxcMKu", -201914811997),
            ("5HL3pPdDFY94Qdf8VnbfT4W6LXFkpd68Y5GSGzNJfntMdGZX", -235660917467),
            ("5EcYAz8SBKWsogA6meJmVXcwVp4tjCvw3ZnJE6UXTyWNUdF2", -500070769668),
            ("5EoE3c7XMf8TN3yudAaFjv4yvjtWYRviHcXi73EXkLHmWTCB", -86442928436),
            ("5CMDjL7t2biHGREBwrmd8renD74FLEhjCVqfJG2MXckWBwDu", 1039317),
            ("5CVGKimL4cLgyTqvYKQbPKYFZfiztsdczU7HrwNdSFKbbn5D", 4224201),
            ("5HmZnEcW4eHbXmUEFWJbc4GHnBBYEK8ZPsFa25PuEmP5iuwM", -13156128),
            ("5FNa4J4fTKh555CEyXHgR29RicSm8nTEHx36utTa4MJepJyX", -9519954),
            ("5Gun93uQgffYpxqMKSmfG18AHiQW7Z2GR2dfPPR8W188vJYc", -1127662),
            ("5HW1C4js4RyjQqNwALSUZC8NJ2WinD5Si2X2XkstXrMW2uYo", -34457336758),
            ("5EqMhjdLY9h64ui2mizRZyBp1mEPJ7s4TsfAxQSQkAFmMfzE", -9346443829744),
            ("5H3XwzydgE2XUGoJCR4dSj7tkd7uxZDJqik69hux2DBcruom", -1215347774),
            ("5DjkmYpCUX6dBTvGoyN9j4QZhtMPhdcywDE8cJ8Qq1vg4X6e", -3603984447),
            ("5E7Z7Btjz74XpZLH5fRzfZqiHCo4j9PXKfqi88kQ5MFrds34", -823907380854),
            ("5DSBWN4hN9413C6o6A2hR9tYUbHjWsQqPRV74GrnCrMkCGJx", -309708781),
            ("5FMLRmKPqsTsMbakpVUwoYro1P64QXVNTWyzNDugaNwSKRzF", -137525398263),
            ("5EFZf5pnTqLegv6gxCrb6TKBQBGz9xLJNK8x9eR273cSons6", -1521760918),
            ("5CGAGEuMLaidBDk8bDZKJb23dxRSP1wLenLALGLw8BTG1E3W", -544739696),
            ("5HSzRtcQjD5KP6Nh2GVSS16aLDe6q9R33Wpu6s2eEeeo3AYS", -2309184790),
            ("5DALvFDcfANQJcWz6AXMfDqabnoZhdDMoH6FxqYUibug1ja7", -369405632507),
            ("5Fy8iWkpcbsskmEN1nYZDdS9zKh167Em9RRYisoss7jaYXxi", 15257429),
            ("5CQCVTRyqJgZKBDmtHzpoF8su6BScLcNGbX8t3WMm5qYbbJH", -10721968),
            ("5DXZByh2NS4MU61a1aaLrcLYpyzpJgHe95TEBdcEN2cF1SA5", -655946136),
            ("5EX5yAYiABFzKDQJDe1kRVwFm3XRRY4HyLMe4Vu9A5U2VEVT", -325581360246),
            ("5FvabwjtyW887gtc7vUnUc47KVhy17UeaNLRjzTg5nkVACMP", -77588524213),
            ("5HTbYi5cmgWJxvyTy9JeYdtnjoDzjXnEXTGFsPEVx9iRPmVF", -53542953784),
            ("5CWzmvA17MAMQ9mnAecLxFXS2N8846rz6T7m4QNHyVtJVq4j", 2672295922502),
            ("5DSYntgHZY4krYUtkkQZyyoffVtu5e8rYWhXuhs832zY6YKy", -2680205688),
            ("5EYyTFyLDqXscaa5VtXTvUc3x2ow2TeT8G12ZDMZwE6uFWPQ", -39165843935),
            ("5CohfM1qdyNwdeJEex1Zyht3S2WS48rV993DmVbyKs2mEEd6", -4004685632),
            ("5Gx6Y7UQD39Latgxigr6mHbnh1herpwNPau2PjvzwLWEjXL3", -559504),
            ("5Hh4Efq5WDwe8URjjUqUNX8KxtMwLHLViwoRvXfEpXQCZakh", -32541090531),
            ("5GWRHC7Nd8njqTPsdJkp6ngniCCBu9UjGhLfxp2jF1fPrfZ4", -5394093031),
            ("5GNAB64UN32krzr3Xxu5LW6naeu2P3XULcdBCR9VZ5Libyit", -24884230),
            ("5EEz25th1nYNM5xR1UsyFFAUaXMjdHqLxZ3wUjyHokYbXHku", -12525171),
            ("5HKJq4JCS9xoKdYhcRnsRp1bodovba7ncd5KTYVwfReKaxHT", -408133990236),
            ("5DXs7x664RL5NdSW77DTseLiu84unstuHGuqvmY61UtJwzRN", -3095078614148),
            ("5CDfdDaA2p9sK1ia5yMVYfzgtFs2e1TrSAxuQqXoS28Lcrxf", -1032856892),
            ("5Ecg4vD2zKXHDFhQqogWq1dZdijPsDty8rGsZu3raeoJSiXb", -995678),
            ("5C5Yg63TNLb68Tu819qXd3Bt4giG8mAPzLmAFSqa2HC1R5Rm", -40818739830910),
            ("5HHH25Wuf9rmVuk9cMKU1hCCPJ1qbHBd1SyHj91R3fMT36yb", -391416057906),
            ("5GKGGE5YLHoDciYJ6Ec2YnUP3SykSQPA47hqmwBP63EtVrd9", -413944553000),
            ("5CSi9ZLyiXfLeYtEFaZSBuTofNMRnXEJEJE9CS4gGaT6CkWt", -17811605275),
            ("5CSoA7QVdFHHBZz53bbRV2mC5vhL64ehhWa8ibtLppmt2n3J", -65701320107),
            ("5GpA5BtfMMX52rXztrha79YqfwR4YaSfTuAcb48Yt73U4h71", -2194562),
            ("5Euz5wpb4xiDWfV1A6AKK6i6ca3WoZQD5hCVyf1fws8GXh4z", -6143407839874),
            ("5DZzmhCG7SMK3LwrkmHZ8ZBwaAByMjfBpEid14nNQdxHipCE", -386645),
            ("5Fc9Vo3hkbr6bPxJpjQo5sQ43L5Hc2G8R5BdqRYF8psvB5pw", 55668553),
            ("5GuSHC3iowySHLDW4pEyEZE6PKxKP62YpJYJyBy5tijzAnYz", -159317636526),
            ("5HVVZrUBPvjYHiwaSvtvaN9GZogoznM49m2AEmVW6RXnYCka", -1995572213),
            ("5EcGpeV2wjkCVsBjsBifSWbdcqH98b6oEY8beDY59c4fXkhw", -177096614584),
            ("5GnCjvWJEESwVNFZzy85zbBzw26etuEt87WiqsE3ee2Ws1wm", -1961445),
            ("5GWuPUpTuChAqKxvU22TRLvRkBFiyWWZnq9cLpJN6SSvkho1", -94157569391),
            ("5FXHf7q5rvBXnzQgmsa31Db9rjcRy6ZHKMiyDSb8Vs5p2msN", -688433531658),
            ("5GbxkzytnvbRuNQ7qxPpfPuWMoeitS8V4KDY9jSshE5fDegD", -19085313),
            ("5Gus1B7c9uWkky7Yawh2tKR1V6AMh5DbqUBPq881JHqeqVqY", -16101671818),
            ("5DLhRdbvWkYYScDmwx4QgJfieSN4apBWbZ2yno3MfgbR8hBP", -21062025),
            ("5Cg5kVyNEs7MWWRHU8X5MHwX5cN3aegvC4RBt2JK19w2GiR8", -2593737050),
            ("5Dkushsxtc8AdCf287MtTYHQv9DoZeBRpttUpBtmyFhGy3uR", -48672832345630),
            ("5EqNqVsHj9bQVyEujcm62zjMYUFhTLY7rTP854txSrJzyoco", -3828526),
            ("5Dea6d6nKErEbRQ4MBGuCALn8NZ2xo4kaa51hB5KMriPBkEM", -1560192853875),
            ("5DNt2XDWdeMd4H92FLnfUvkqyXzmavezHvzLboP3VgT1xLZV", -831964576998),
            ("5FKtFoTeK8aaG6HZTrDgvoYHVQ5NY4S9VyV7W5K74cWcwLYA", -60823501166),
            ("5GEBanZKUU7Hrf8K2VNi33HxyJRstgQ3WD3odHgvMj2nPbhi", -98946626902),
            ("5CUtw7LYB2n2bzgXt6YnmKDHt6PsB3kKAyD9azYJNcRG8TNg", -9779588557490),
            ("5EynbF72b12fbgMvEeL1vJSY342rCryNbuwxFivU1Xevtmv3", -17314385200455),
            ("5CapiZRuULed8ConS1gbjMVgnwcT5JnQah7tx6sZnK7sJJuJ", -5810972),
            ("5DnaxLaNduf41WM6WWZ4fkzcGzWNWx6eLJyQpSaMueUGCsaU", -12668760),
            ("5Cqz9SChYPxTFZ2623rE2aQQ5ttQoLwZ8yfwYgiZyQDANqZn", -683549),
            ("5C8ZcLzF23GrXKdH4Pg3ZXC3vKQsF5PM8VvhzzxzTQksgj8e", -44720570590),
            ("5GuNsmoswrP6hTKZkKcpTpZftTMKrmnCHvTL2V3NHJy2fpen", -5042891812715),
            ("5F1TYDkLnP36HHY5btigxyKUPzBraxdrU1aX1bqFfPfcfnzU", -1189104279832),
            ("5Dc384z9HuTGF6oratZs1fLciCHtPZaLhrHfCVw82a5AikWZ", -616163196988),
            ("5DhcaEUsRKhZQ31qRffJqjtLmFkbVaCebn8nVjYhvB4KJtX5", -17746006723),
            ("5HYE7z3xTcrN1rqz54NyZRAkehFfRMcaEcdoMq5g5ATET5wQ", 212509751245),
            ("5F97DdEVTy9gPCtN6jkJJENDJuQiRGiwbMVSL74qRq8FCq5W", 2225287736222),
            ("5FUVN133rSvuKXgsXKMR2ZEaysxZjkRUFUWS1UMyNGre9xFV", -73216740161),
            ("5CZeimtfpRqQgPxVwr1MzfG2Sok8E1AMERHo6vUmEdRS5JiU", -3937802),
            ("5Eqq2JwGh7qbtnjPiFEPmmnHxs3S4J4Ahg8fr4sybZV1tPdY", -173406860562),
            ("5ERfDw6K3GmQqwqsEG6foFtu7VsYGifPi556UJKQsBnfbHKN", 96022588728),
            ("5Ek8RkU6KMv5Fx7yivRVoQkuJYAKhULWiLWDpbGG4hvR9HFD", 968139369093),
            ("5HpCpGALzqgnDTP1HXFiuhzD5MFaDTRHjXBCvaMY9LNNRkT9", 104943979521),
            ("5FYqS77gxW9gHG8id1YYPS7Cd4TQmNUMhF8h3S77Fq2VvvRQ", 729199757977),
            ("5FtBqMg13pNf1N6TwfG6BmwyaDM77mkeQ16UTHsGasrVDedX", 131457064336),
            ("5GbnWR2XhWrRMt123SdrLbR9G2a4N5dtzA3TSu3Czkzoeu7x", 2295599153),
            ("5GgiowcCG4kLpwkCTGxxQJQv8WwKFyBQ6McPRmqKtWPy8EaK", 113838605389),
            ("5FL5YtYozpUAGaiVWonpbwEYdEMij3obJHSH3ACY4vgWmDgy", 8689039),
            ("5Egq58bxRv7boM2s3rnDxx1udnkzxPQ23HuoqohVxjh9RenC", 216373234348),
            ("5FRGeeEgRNR8U33FDKvN7yUgts8zR3qRJH4yKKWoR9GswBRb", 2196574958718),
            ("5FnhSy79BPYyrmmFsbckinQw1fLiLqqPkQL2vgZwPxbRfu3k", 42319631507),
            ("5Hj8jMhqAv7cfyRh5STfbZefMhv17QxZ1RxWq9jNcLAEsRRo", 132216702183491),
            ("5EXYTGMqumAH6RLQgHwkMEMnSvHcpHc89R6U8krfNJTYWm9J", 504320264499),
            ("5EFh8ctzmytXURqrCTUBWHTs87f7TMWB6XKUzdqxKXVUtvS2", 2209599669432),
            ("5CqVqEcRBkw7Gm2reJ33cj7puR9W2Tq7qsLxSruV1BgnMqKN", 1033387458788),
            ("5D79enmLSGimsruoraGagofhaSeYJZvGUqFCCrr83ZfZs1HS", 7591184215233),
            ("5HbpyjsvyXLWtf1QT1CyNUdyut6scM5dM7ytm8hoxFvRtU1i", 129833188275),
            ("5CnxCi7CdEriWSdw4LcXdbtjodxA6uTat4gBm4wuT9QToMdo", 3132978),
            ("5G48fiQjhAd8hc4rYc6GituCuAPKznL28jyyyq1auMyZiG4t", 514913328178),
            ("5FFGjW2hJ7tQ41qghSsLP4cVmA8j9pZVSrr2CrLG7fQAsLHJ", 346794972723),
            ("5FWjnxeRMtMFxRc9kvZKCG5iJAyyz2kmXV8u3kqyiXizZtiz", 225939835005),
            ("5CUw3sB4oxd3dVSHUr3kxsB591VEjaPzr444KkfjwVFnLRfJ", 208250614494),
            ("5EaBhxNUwMRyKsaeA2BEjDCrvwE5J8FDSpfCHK9gGmnmbhCa", 278083207003),
            ("5GHJ5HxFxYQyVoNFUxR3JCqqCKRumaFCY7N5zMxwF4CpRUWr", 1381466224829),
            ("5H1WgA7ET3FmEarJK6qc1vaTWbNd6g41mgvyLRkysrH4MDdo", 774889),
        ];

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            if diff > 0 {
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                    diff.unsigned_abs().into(),
                );
            }
        }

        // For one of the gainers, remove some of the stake
        let idx = 6;
        let gained_ck = diffs[idx].0;
        let coldkey_account_id = decode_account_id32::<Test>(gained_ck).expect("Invalid coldkey");
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            num_traits::ToPrimitive::to_u64(
                &(num_traits::ToPrimitive::to_f64(&diffs[idx].1).expect("float conv fail")
                    * 0.9_f64)
                    .abs(),
            )
            .expect("u64 conv fail")
            .into(),
        );

        let w = try_restore_shares::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Check stake is near 0 for all positive entires except the one we removed
        // Check the stake for all negative entries is proportional to the amount they lost
        let total_lost: f64 = diffs
            .iter()
            .map(|(_, diff)| {
                if diff.is_negative() {
                    num_traits::ToPrimitive::to_f64(&diff.saturating_abs())
                        .expect("float conv fail")
                } else {
                    0_f64
                }
            })
            .sum::<f64>();
        let mut total_returned = 0_f64;
        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");

            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff > 0 {
                log::debug!("diff: {} for ck: {}", diff, coldkey);
                assert_relative_eq!(stake_float, 0_f64, max_relative = 0.001_f64);
            } else {
                total_returned += stake_float;
            }
        }

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff < 0 {
                // Should get a return proportional to the amount they lost
                // versus the amount that was able to be recovered
                let prop_returned: f64 = num_traits::ToPrimitive::to_f64(&diff.abs())
                    .expect("float conv fail")
                    / total_lost
                    * total_returned;
                assert_relative_eq!(stake_float, prop_returned, max_relative = 0.001_f64);
            }
        }
    });
}

#[test]
fn test_migrate_fix_bad_hk_swap_mainnet_has_more() {
    // test with some of the gainers have a balance higher than the gain before the migration
    new_test_ext(1).execute_with(|| {
        use crate::migrations::migrate_fix_bad_hk_swap::*;

        let netuid = NetUid::from(59);
        // Add subnet 59
        add_network(netuid, 10, 0);
        SubtokenEnabled::<Test>::insert(netuid, true);
        SubnetMechanism::<Test>::insert(netuid, 1);

        let hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
        let hotkey_account_id = decode_account_id32::<Test>(hotkey).expect("Invalid hotkey");

        #[rustfmt::skip]
        let diffs: [(&str, i64); 112] = [
            ("5Fn9SqQhx5bhDua7AGgkKxxk3gfZ75WWBGCMPeKH1WBgPaMQ", -2375685930981_i64),
            ("5Fnhtm7cpxEbZaChnRZ8yWoF8MXVxmobkmLRehh5bkYtyZA9", -4090996138227),
            ("5C7j3w2zz1SVejRuFrb2zFWHXT7UfG7eWA87KXL1WyV5KLVR", -607494031),
            ("5DthZ1rvnXBb9oXVNtrMaMsDAnRxBPZCjD6fdRdeqC3fg1ca", -17022477949),
            ("5F7BkPL3EVjKTYMbBkEmPAtTZQSGeyNzFPaf1DtebPFmJsJ7", -4016510),
            ("5EefisctzgWdVGFQaL4LjFFacTE7dM4YJVNy3ogGBQoapTU1", -13106893093),
            ("5CwkvpBxHCaRK9xBC2n6WdhpF5zg9t5WLkGorASaoErdynFQ", 439139249152),
            ("5FU7ErUtmi22xuqeeCYVpNZp6WVSSL98hqDi5iyeZbkXtkbe", -35958768555),
            ("5D7HL8T95qkHQTPFjgSFCjRoeM7oE3vQBYjiR1kAPbPxcMKu", -201914811997),
            ("5HL3pPdDFY94Qdf8VnbfT4W6LXFkpd68Y5GSGzNJfntMdGZX", -235660917467),
            ("5EcYAz8SBKWsogA6meJmVXcwVp4tjCvw3ZnJE6UXTyWNUdF2", -500070769668),
            ("5EoE3c7XMf8TN3yudAaFjv4yvjtWYRviHcXi73EXkLHmWTCB", -86442928436),
            ("5CMDjL7t2biHGREBwrmd8renD74FLEhjCVqfJG2MXckWBwDu", 1039317),
            ("5CVGKimL4cLgyTqvYKQbPKYFZfiztsdczU7HrwNdSFKbbn5D", 4224201),
            ("5HmZnEcW4eHbXmUEFWJbc4GHnBBYEK8ZPsFa25PuEmP5iuwM", -13156128),
            ("5FNa4J4fTKh555CEyXHgR29RicSm8nTEHx36utTa4MJepJyX", -9519954),
            ("5Gun93uQgffYpxqMKSmfG18AHiQW7Z2GR2dfPPR8W188vJYc", -1127662),
            ("5HW1C4js4RyjQqNwALSUZC8NJ2WinD5Si2X2XkstXrMW2uYo", -34457336758),
            ("5EqMhjdLY9h64ui2mizRZyBp1mEPJ7s4TsfAxQSQkAFmMfzE", -9346443829744),
            ("5H3XwzydgE2XUGoJCR4dSj7tkd7uxZDJqik69hux2DBcruom", -1215347774),
            ("5DjkmYpCUX6dBTvGoyN9j4QZhtMPhdcywDE8cJ8Qq1vg4X6e", -3603984447),
            ("5E7Z7Btjz74XpZLH5fRzfZqiHCo4j9PXKfqi88kQ5MFrds34", -823907380854),
            ("5DSBWN4hN9413C6o6A2hR9tYUbHjWsQqPRV74GrnCrMkCGJx", -309708781),
            ("5FMLRmKPqsTsMbakpVUwoYro1P64QXVNTWyzNDugaNwSKRzF", -137525398263),
            ("5EFZf5pnTqLegv6gxCrb6TKBQBGz9xLJNK8x9eR273cSons6", -1521760918),
            ("5CGAGEuMLaidBDk8bDZKJb23dxRSP1wLenLALGLw8BTG1E3W", -544739696),
            ("5HSzRtcQjD5KP6Nh2GVSS16aLDe6q9R33Wpu6s2eEeeo3AYS", -2309184790),
            ("5DALvFDcfANQJcWz6AXMfDqabnoZhdDMoH6FxqYUibug1ja7", -369405632507),
            ("5Fy8iWkpcbsskmEN1nYZDdS9zKh167Em9RRYisoss7jaYXxi", 15257429),
            ("5CQCVTRyqJgZKBDmtHzpoF8su6BScLcNGbX8t3WMm5qYbbJH", -10721968),
            ("5DXZByh2NS4MU61a1aaLrcLYpyzpJgHe95TEBdcEN2cF1SA5", -655946136),
            ("5EX5yAYiABFzKDQJDe1kRVwFm3XRRY4HyLMe4Vu9A5U2VEVT", -325581360246),
            ("5FvabwjtyW887gtc7vUnUc47KVhy17UeaNLRjzTg5nkVACMP", -77588524213),
            ("5HTbYi5cmgWJxvyTy9JeYdtnjoDzjXnEXTGFsPEVx9iRPmVF", -53542953784),
            ("5CWzmvA17MAMQ9mnAecLxFXS2N8846rz6T7m4QNHyVtJVq4j", 2672295922502),
            ("5DSYntgHZY4krYUtkkQZyyoffVtu5e8rYWhXuhs832zY6YKy", -2680205688),
            ("5EYyTFyLDqXscaa5VtXTvUc3x2ow2TeT8G12ZDMZwE6uFWPQ", -39165843935),
            ("5CohfM1qdyNwdeJEex1Zyht3S2WS48rV993DmVbyKs2mEEd6", -4004685632),
            ("5Gx6Y7UQD39Latgxigr6mHbnh1herpwNPau2PjvzwLWEjXL3", -559504),
            ("5Hh4Efq5WDwe8URjjUqUNX8KxtMwLHLViwoRvXfEpXQCZakh", -32541090531),
            ("5GWRHC7Nd8njqTPsdJkp6ngniCCBu9UjGhLfxp2jF1fPrfZ4", -5394093031),
            ("5GNAB64UN32krzr3Xxu5LW6naeu2P3XULcdBCR9VZ5Libyit", -24884230),
            ("5EEz25th1nYNM5xR1UsyFFAUaXMjdHqLxZ3wUjyHokYbXHku", -12525171),
            ("5HKJq4JCS9xoKdYhcRnsRp1bodovba7ncd5KTYVwfReKaxHT", -408133990236),
            ("5DXs7x664RL5NdSW77DTseLiu84unstuHGuqvmY61UtJwzRN", -3095078614148),
            ("5CDfdDaA2p9sK1ia5yMVYfzgtFs2e1TrSAxuQqXoS28Lcrxf", -1032856892),
            ("5Ecg4vD2zKXHDFhQqogWq1dZdijPsDty8rGsZu3raeoJSiXb", -995678),
            ("5C5Yg63TNLb68Tu819qXd3Bt4giG8mAPzLmAFSqa2HC1R5Rm", -40818739830910),
            ("5HHH25Wuf9rmVuk9cMKU1hCCPJ1qbHBd1SyHj91R3fMT36yb", -391416057906),
            ("5GKGGE5YLHoDciYJ6Ec2YnUP3SykSQPA47hqmwBP63EtVrd9", -413944553000),
            ("5CSi9ZLyiXfLeYtEFaZSBuTofNMRnXEJEJE9CS4gGaT6CkWt", -17811605275),
            ("5CSoA7QVdFHHBZz53bbRV2mC5vhL64ehhWa8ibtLppmt2n3J", -65701320107),
            ("5GpA5BtfMMX52rXztrha79YqfwR4YaSfTuAcb48Yt73U4h71", -2194562),
            ("5Euz5wpb4xiDWfV1A6AKK6i6ca3WoZQD5hCVyf1fws8GXh4z", -6143407839874),
            ("5DZzmhCG7SMK3LwrkmHZ8ZBwaAByMjfBpEid14nNQdxHipCE", -386645),
            ("5Fc9Vo3hkbr6bPxJpjQo5sQ43L5Hc2G8R5BdqRYF8psvB5pw", 55668553),
            ("5GuSHC3iowySHLDW4pEyEZE6PKxKP62YpJYJyBy5tijzAnYz", -159317636526),
            ("5HVVZrUBPvjYHiwaSvtvaN9GZogoznM49m2AEmVW6RXnYCka", -1995572213),
            ("5EcGpeV2wjkCVsBjsBifSWbdcqH98b6oEY8beDY59c4fXkhw", -177096614584),
            ("5GnCjvWJEESwVNFZzy85zbBzw26etuEt87WiqsE3ee2Ws1wm", -1961445),
            ("5GWuPUpTuChAqKxvU22TRLvRkBFiyWWZnq9cLpJN6SSvkho1", -94157569391),
            ("5FXHf7q5rvBXnzQgmsa31Db9rjcRy6ZHKMiyDSb8Vs5p2msN", -688433531658),
            ("5GbxkzytnvbRuNQ7qxPpfPuWMoeitS8V4KDY9jSshE5fDegD", -19085313),
            ("5Gus1B7c9uWkky7Yawh2tKR1V6AMh5DbqUBPq881JHqeqVqY", -16101671818),
            ("5DLhRdbvWkYYScDmwx4QgJfieSN4apBWbZ2yno3MfgbR8hBP", -21062025),
            ("5Cg5kVyNEs7MWWRHU8X5MHwX5cN3aegvC4RBt2JK19w2GiR8", -2593737050),
            ("5Dkushsxtc8AdCf287MtTYHQv9DoZeBRpttUpBtmyFhGy3uR", -48672832345630),
            ("5EqNqVsHj9bQVyEujcm62zjMYUFhTLY7rTP854txSrJzyoco", -3828526),
            ("5Dea6d6nKErEbRQ4MBGuCALn8NZ2xo4kaa51hB5KMriPBkEM", -1560192853875),
            ("5DNt2XDWdeMd4H92FLnfUvkqyXzmavezHvzLboP3VgT1xLZV", -831964576998),
            ("5FKtFoTeK8aaG6HZTrDgvoYHVQ5NY4S9VyV7W5K74cWcwLYA", -60823501166),
            ("5GEBanZKUU7Hrf8K2VNi33HxyJRstgQ3WD3odHgvMj2nPbhi", -98946626902),
            ("5CUtw7LYB2n2bzgXt6YnmKDHt6PsB3kKAyD9azYJNcRG8TNg", -9779588557490),
            ("5EynbF72b12fbgMvEeL1vJSY342rCryNbuwxFivU1Xevtmv3", -17314385200455),
            ("5CapiZRuULed8ConS1gbjMVgnwcT5JnQah7tx6sZnK7sJJuJ", -5810972),
            ("5DnaxLaNduf41WM6WWZ4fkzcGzWNWx6eLJyQpSaMueUGCsaU", -12668760),
            ("5Cqz9SChYPxTFZ2623rE2aQQ5ttQoLwZ8yfwYgiZyQDANqZn", -683549),
            ("5C8ZcLzF23GrXKdH4Pg3ZXC3vKQsF5PM8VvhzzxzTQksgj8e", -44720570590),
            ("5GuNsmoswrP6hTKZkKcpTpZftTMKrmnCHvTL2V3NHJy2fpen", -5042891812715),
            ("5F1TYDkLnP36HHY5btigxyKUPzBraxdrU1aX1bqFfPfcfnzU", -1189104279832),
            ("5Dc384z9HuTGF6oratZs1fLciCHtPZaLhrHfCVw82a5AikWZ", -616163196988),
            ("5DhcaEUsRKhZQ31qRffJqjtLmFkbVaCebn8nVjYhvB4KJtX5", -17746006723),
            ("5HYE7z3xTcrN1rqz54NyZRAkehFfRMcaEcdoMq5g5ATET5wQ", 212509751245),
            ("5F97DdEVTy9gPCtN6jkJJENDJuQiRGiwbMVSL74qRq8FCq5W", 2225287736222),
            ("5FUVN133rSvuKXgsXKMR2ZEaysxZjkRUFUWS1UMyNGre9xFV", -73216740161),
            ("5CZeimtfpRqQgPxVwr1MzfG2Sok8E1AMERHo6vUmEdRS5JiU", -3937802),
            ("5Eqq2JwGh7qbtnjPiFEPmmnHxs3S4J4Ahg8fr4sybZV1tPdY", -173406860562),
            ("5ERfDw6K3GmQqwqsEG6foFtu7VsYGifPi556UJKQsBnfbHKN", 96022588728),
            ("5Ek8RkU6KMv5Fx7yivRVoQkuJYAKhULWiLWDpbGG4hvR9HFD", 968139369093),
            ("5HpCpGALzqgnDTP1HXFiuhzD5MFaDTRHjXBCvaMY9LNNRkT9", 104943979521),
            ("5FYqS77gxW9gHG8id1YYPS7Cd4TQmNUMhF8h3S77Fq2VvvRQ", 729199757977),
            ("5FtBqMg13pNf1N6TwfG6BmwyaDM77mkeQ16UTHsGasrVDedX", 131457064336),
            ("5GbnWR2XhWrRMt123SdrLbR9G2a4N5dtzA3TSu3Czkzoeu7x", 2295599153),
            ("5GgiowcCG4kLpwkCTGxxQJQv8WwKFyBQ6McPRmqKtWPy8EaK", 113838605389),
            ("5FL5YtYozpUAGaiVWonpbwEYdEMij3obJHSH3ACY4vgWmDgy", 8689039),
            ("5Egq58bxRv7boM2s3rnDxx1udnkzxPQ23HuoqohVxjh9RenC", 216373234348),
            ("5FRGeeEgRNR8U33FDKvN7yUgts8zR3qRJH4yKKWoR9GswBRb", 2196574958718),
            ("5FnhSy79BPYyrmmFsbckinQw1fLiLqqPkQL2vgZwPxbRfu3k", 42319631507),
            ("5Hj8jMhqAv7cfyRh5STfbZefMhv17QxZ1RxWq9jNcLAEsRRo", 132216702183491),
            ("5EXYTGMqumAH6RLQgHwkMEMnSvHcpHc89R6U8krfNJTYWm9J", 504320264499),
            ("5EFh8ctzmytXURqrCTUBWHTs87f7TMWB6XKUzdqxKXVUtvS2", 2209599669432),
            ("5CqVqEcRBkw7Gm2reJ33cj7puR9W2Tq7qsLxSruV1BgnMqKN", 1033387458788),
            ("5D79enmLSGimsruoraGagofhaSeYJZvGUqFCCrr83ZfZs1HS", 7591184215233),
            ("5HbpyjsvyXLWtf1QT1CyNUdyut6scM5dM7ytm8hoxFvRtU1i", 129833188275),
            ("5CnxCi7CdEriWSdw4LcXdbtjodxA6uTat4gBm4wuT9QToMdo", 3132978),
            ("5G48fiQjhAd8hc4rYc6GituCuAPKznL28jyyyq1auMyZiG4t", 514913328178),
            ("5FFGjW2hJ7tQ41qghSsLP4cVmA8j9pZVSrr2CrLG7fQAsLHJ", 346794972723),
            ("5FWjnxeRMtMFxRc9kvZKCG5iJAyyz2kmXV8u3kqyiXizZtiz", 225939835005),
            ("5CUw3sB4oxd3dVSHUr3kxsB591VEjaPzr444KkfjwVFnLRfJ", 208250614494),
            ("5EaBhxNUwMRyKsaeA2BEjDCrvwE5J8FDSpfCHK9gGmnmbhCa", 278083207003),
            ("5GHJ5HxFxYQyVoNFUxR3JCqqCKRumaFCY7N5zMxwF4CpRUWr", 1381466224829),
            ("5H1WgA7ET3FmEarJK6qc1vaTWbNd6g41mgvyLRkysrH4MDdo", 774889),
        ];

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            if diff > 0 {
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                    diff.unsigned_abs().into(),
                );
            }
        }

        // For one of the gainers, add some extra stake
        let idx = 6;
        let gained_ck = diffs[idx].0;
        let coldkey_account_id = decode_account_id32::<Test>(gained_ck).expect("Invalid coldkey");
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            num_traits::ToPrimitive::to_u64(
                &((num_traits::ToPrimitive::to_f64(&diffs[idx].1).expect("float conv fail")
                    * 0.9_f64)
                    .abs()),
            )
            .expect("u64 conv fail")
            .into(),
        );
        let extra_balance = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        )
        .saturating_sub(
            num_traits::ToPrimitive::to_u64(&diffs[idx].1.abs())
                .expect("float conv fail")
                .into(),
        );
        assert!(
            extra_balance.to_u64() > 0_u64,
            "extra balance must be positive"
        );

        let w = try_restore_shares::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Check stake is near 0 for all positive entires except the one we removed
        // Check the stake for all negative entries is proportional to the amount they lost
        let total_lost: f64 = diffs
            .iter()
            .map(|(_, diff)| {
                if diff.is_negative() {
                    num_traits::ToPrimitive::to_f64(&diff.saturating_abs())
                        .expect("float conv fail")
                } else {
                    0_f64
                }
            })
            .sum::<f64>();
        let mut total_returned = 0_f64;
        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");

            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff > 0 {
                if coldkey == gained_ck {
                    // this CK should retain the extra balance
                    assert_relative_eq!(
                        stake_float,
                        num_traits::ToPrimitive::to_f64(&extra_balance.to_u64())
                            .expect("float conv fail"),
                        max_relative = 0.001_f64
                    );
                } else {
                    assert_relative_eq!(stake_float, 0_f64, max_relative = 0.001_f64);
                }
            } else {
                total_returned += stake_float;
            }
        }

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff < 0 {
                // Should get a return proportional to the amount they lost
                // versus the amount that was able to be recovered
                let prop_returned: f64 = num_traits::ToPrimitive::to_f64(&diff.abs())
                    .expect("float conv fail")
                    / total_lost
                    * total_returned;
                assert_relative_eq!(stake_float, prop_returned, max_relative = 0.001_f64);
            }
        }
    });
}

#[test]
fn test_migrate_fix_bad_hk_swap_mainnet_some_entries() {
    // test with some of the losers have an existing balance before the migration
    new_test_ext(1).execute_with(|| {
        use crate::migrations::migrate_fix_bad_hk_swap::*;

        let netuid = NetUid::from(59);
        // Add subnet 59
        add_network(netuid, 10, 0);
        SubtokenEnabled::<Test>::insert(netuid, true);
        SubnetMechanism::<Test>::insert(netuid, 1);

        let hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
        let hotkey_account_id = decode_account_id32::<Test>(hotkey).expect("Invalid hotkey");

        #[rustfmt::skip]
        let diffs: [(&str, i64); 112] = [
            ("5Fn9SqQhx5bhDua7AGgkKxxk3gfZ75WWBGCMPeKH1WBgPaMQ", -2375685930981_i64),
            ("5Fnhtm7cpxEbZaChnRZ8yWoF8MXVxmobkmLRehh5bkYtyZA9", -4090996138227),
            ("5C7j3w2zz1SVejRuFrb2zFWHXT7UfG7eWA87KXL1WyV5KLVR", -607494031),
            ("5DthZ1rvnXBb9oXVNtrMaMsDAnRxBPZCjD6fdRdeqC3fg1ca", -17022477949),
            ("5F7BkPL3EVjKTYMbBkEmPAtTZQSGeyNzFPaf1DtebPFmJsJ7", -4016510),
            ("5EefisctzgWdVGFQaL4LjFFacTE7dM4YJVNy3ogGBQoapTU1", -13106893093),
            ("5CwkvpBxHCaRK9xBC2n6WdhpF5zg9t5WLkGorASaoErdynFQ", 439139249152),
            ("5FU7ErUtmi22xuqeeCYVpNZp6WVSSL98hqDi5iyeZbkXtkbe", -35958768555),
            ("5D7HL8T95qkHQTPFjgSFCjRoeM7oE3vQBYjiR1kAPbPxcMKu", -201914811997),
            ("5HL3pPdDFY94Qdf8VnbfT4W6LXFkpd68Y5GSGzNJfntMdGZX", -235660917467),
            ("5EcYAz8SBKWsogA6meJmVXcwVp4tjCvw3ZnJE6UXTyWNUdF2", -500070769668),
            ("5EoE3c7XMf8TN3yudAaFjv4yvjtWYRviHcXi73EXkLHmWTCB", -86442928436),
            ("5CMDjL7t2biHGREBwrmd8renD74FLEhjCVqfJG2MXckWBwDu", 1039317),
            ("5CVGKimL4cLgyTqvYKQbPKYFZfiztsdczU7HrwNdSFKbbn5D", 4224201),
            ("5HmZnEcW4eHbXmUEFWJbc4GHnBBYEK8ZPsFa25PuEmP5iuwM", -13156128),
            ("5FNa4J4fTKh555CEyXHgR29RicSm8nTEHx36utTa4MJepJyX", -9519954),
            ("5Gun93uQgffYpxqMKSmfG18AHiQW7Z2GR2dfPPR8W188vJYc", -1127662),
            ("5HW1C4js4RyjQqNwALSUZC8NJ2WinD5Si2X2XkstXrMW2uYo", -34457336758),
            ("5EqMhjdLY9h64ui2mizRZyBp1mEPJ7s4TsfAxQSQkAFmMfzE", -9346443829744),
            ("5H3XwzydgE2XUGoJCR4dSj7tkd7uxZDJqik69hux2DBcruom", -1215347774),
            ("5DjkmYpCUX6dBTvGoyN9j4QZhtMPhdcywDE8cJ8Qq1vg4X6e", -3603984447),
            ("5E7Z7Btjz74XpZLH5fRzfZqiHCo4j9PXKfqi88kQ5MFrds34", -823907380854),
            ("5DSBWN4hN9413C6o6A2hR9tYUbHjWsQqPRV74GrnCrMkCGJx", -309708781),
            ("5FMLRmKPqsTsMbakpVUwoYro1P64QXVNTWyzNDugaNwSKRzF", -137525398263),
            ("5EFZf5pnTqLegv6gxCrb6TKBQBGz9xLJNK8x9eR273cSons6", -1521760918),
            ("5CGAGEuMLaidBDk8bDZKJb23dxRSP1wLenLALGLw8BTG1E3W", -544739696),
            ("5HSzRtcQjD5KP6Nh2GVSS16aLDe6q9R33Wpu6s2eEeeo3AYS", -2309184790),
            ("5DALvFDcfANQJcWz6AXMfDqabnoZhdDMoH6FxqYUibug1ja7", -369405632507),
            ("5Fy8iWkpcbsskmEN1nYZDdS9zKh167Em9RRYisoss7jaYXxi", 15257429),
            ("5CQCVTRyqJgZKBDmtHzpoF8su6BScLcNGbX8t3WMm5qYbbJH", -10721968),
            ("5DXZByh2NS4MU61a1aaLrcLYpyzpJgHe95TEBdcEN2cF1SA5", -655946136),
            ("5EX5yAYiABFzKDQJDe1kRVwFm3XRRY4HyLMe4Vu9A5U2VEVT", -325581360246),
            ("5FvabwjtyW887gtc7vUnUc47KVhy17UeaNLRjzTg5nkVACMP", -77588524213),
            ("5HTbYi5cmgWJxvyTy9JeYdtnjoDzjXnEXTGFsPEVx9iRPmVF", -53542953784),
            ("5CWzmvA17MAMQ9mnAecLxFXS2N8846rz6T7m4QNHyVtJVq4j", 2672295922502),
            ("5DSYntgHZY4krYUtkkQZyyoffVtu5e8rYWhXuhs832zY6YKy", -2680205688),
            ("5EYyTFyLDqXscaa5VtXTvUc3x2ow2TeT8G12ZDMZwE6uFWPQ", -39165843935),
            ("5CohfM1qdyNwdeJEex1Zyht3S2WS48rV993DmVbyKs2mEEd6", -4004685632),
            ("5Gx6Y7UQD39Latgxigr6mHbnh1herpwNPau2PjvzwLWEjXL3", -559504),
            ("5Hh4Efq5WDwe8URjjUqUNX8KxtMwLHLViwoRvXfEpXQCZakh", -32541090531),
            ("5GWRHC7Nd8njqTPsdJkp6ngniCCBu9UjGhLfxp2jF1fPrfZ4", -5394093031),
            ("5GNAB64UN32krzr3Xxu5LW6naeu2P3XULcdBCR9VZ5Libyit", -24884230),
            ("5EEz25th1nYNM5xR1UsyFFAUaXMjdHqLxZ3wUjyHokYbXHku", -12525171),
            ("5HKJq4JCS9xoKdYhcRnsRp1bodovba7ncd5KTYVwfReKaxHT", -408133990236),
            ("5DXs7x664RL5NdSW77DTseLiu84unstuHGuqvmY61UtJwzRN", -3095078614148),
            ("5CDfdDaA2p9sK1ia5yMVYfzgtFs2e1TrSAxuQqXoS28Lcrxf", -1032856892),
            ("5Ecg4vD2zKXHDFhQqogWq1dZdijPsDty8rGsZu3raeoJSiXb", -995678),
            ("5C5Yg63TNLb68Tu819qXd3Bt4giG8mAPzLmAFSqa2HC1R5Rm", -40818739830910),
            ("5HHH25Wuf9rmVuk9cMKU1hCCPJ1qbHBd1SyHj91R3fMT36yb", -391416057906),
            ("5GKGGE5YLHoDciYJ6Ec2YnUP3SykSQPA47hqmwBP63EtVrd9", -413944553000),
            ("5CSi9ZLyiXfLeYtEFaZSBuTofNMRnXEJEJE9CS4gGaT6CkWt", -17811605275),
            ("5CSoA7QVdFHHBZz53bbRV2mC5vhL64ehhWa8ibtLppmt2n3J", -65701320107),
            ("5GpA5BtfMMX52rXztrha79YqfwR4YaSfTuAcb48Yt73U4h71", -2194562),
            ("5Euz5wpb4xiDWfV1A6AKK6i6ca3WoZQD5hCVyf1fws8GXh4z", -6143407839874),
            ("5DZzmhCG7SMK3LwrkmHZ8ZBwaAByMjfBpEid14nNQdxHipCE", -386645),
            ("5Fc9Vo3hkbr6bPxJpjQo5sQ43L5Hc2G8R5BdqRYF8psvB5pw", 55668553),
            ("5GuSHC3iowySHLDW4pEyEZE6PKxKP62YpJYJyBy5tijzAnYz", -159317636526),
            ("5HVVZrUBPvjYHiwaSvtvaN9GZogoznM49m2AEmVW6RXnYCka", -1995572213),
            ("5EcGpeV2wjkCVsBjsBifSWbdcqH98b6oEY8beDY59c4fXkhw", -177096614584),
            ("5GnCjvWJEESwVNFZzy85zbBzw26etuEt87WiqsE3ee2Ws1wm", -1961445),
            ("5GWuPUpTuChAqKxvU22TRLvRkBFiyWWZnq9cLpJN6SSvkho1", -94157569391),
            ("5FXHf7q5rvBXnzQgmsa31Db9rjcRy6ZHKMiyDSb8Vs5p2msN", -688433531658),
            ("5GbxkzytnvbRuNQ7qxPpfPuWMoeitS8V4KDY9jSshE5fDegD", -19085313),
            ("5Gus1B7c9uWkky7Yawh2tKR1V6AMh5DbqUBPq881JHqeqVqY", -16101671818),
            ("5DLhRdbvWkYYScDmwx4QgJfieSN4apBWbZ2yno3MfgbR8hBP", -21062025),
            ("5Cg5kVyNEs7MWWRHU8X5MHwX5cN3aegvC4RBt2JK19w2GiR8", -2593737050),
            ("5Dkushsxtc8AdCf287MtTYHQv9DoZeBRpttUpBtmyFhGy3uR", -48672832345630),
            ("5EqNqVsHj9bQVyEujcm62zjMYUFhTLY7rTP854txSrJzyoco", -3828526),
            ("5Dea6d6nKErEbRQ4MBGuCALn8NZ2xo4kaa51hB5KMriPBkEM", -1560192853875),
            ("5DNt2XDWdeMd4H92FLnfUvkqyXzmavezHvzLboP3VgT1xLZV", -831964576998),
            ("5FKtFoTeK8aaG6HZTrDgvoYHVQ5NY4S9VyV7W5K74cWcwLYA", -60823501166),
            ("5GEBanZKUU7Hrf8K2VNi33HxyJRstgQ3WD3odHgvMj2nPbhi", -98946626902),
            ("5CUtw7LYB2n2bzgXt6YnmKDHt6PsB3kKAyD9azYJNcRG8TNg", -9779588557490),
            ("5EynbF72b12fbgMvEeL1vJSY342rCryNbuwxFivU1Xevtmv3", -17314385200455),
            ("5CapiZRuULed8ConS1gbjMVgnwcT5JnQah7tx6sZnK7sJJuJ", -5810972),
            ("5DnaxLaNduf41WM6WWZ4fkzcGzWNWx6eLJyQpSaMueUGCsaU", -12668760),
            ("5Cqz9SChYPxTFZ2623rE2aQQ5ttQoLwZ8yfwYgiZyQDANqZn", -683549),
            ("5C8ZcLzF23GrXKdH4Pg3ZXC3vKQsF5PM8VvhzzxzTQksgj8e", -44720570590),
            ("5GuNsmoswrP6hTKZkKcpTpZftTMKrmnCHvTL2V3NHJy2fpen", -5042891812715),
            ("5F1TYDkLnP36HHY5btigxyKUPzBraxdrU1aX1bqFfPfcfnzU", -1189104279832),
            ("5Dc384z9HuTGF6oratZs1fLciCHtPZaLhrHfCVw82a5AikWZ", -616163196988),
            ("5DhcaEUsRKhZQ31qRffJqjtLmFkbVaCebn8nVjYhvB4KJtX5", -17746006723),
            ("5HYE7z3xTcrN1rqz54NyZRAkehFfRMcaEcdoMq5g5ATET5wQ", 212509751245),
            ("5F97DdEVTy9gPCtN6jkJJENDJuQiRGiwbMVSL74qRq8FCq5W", 2225287736222),
            ("5FUVN133rSvuKXgsXKMR2ZEaysxZjkRUFUWS1UMyNGre9xFV", -73216740161),
            ("5CZeimtfpRqQgPxVwr1MzfG2Sok8E1AMERHo6vUmEdRS5JiU", -3937802),
            ("5Eqq2JwGh7qbtnjPiFEPmmnHxs3S4J4Ahg8fr4sybZV1tPdY", -173406860562),
            ("5ERfDw6K3GmQqwqsEG6foFtu7VsYGifPi556UJKQsBnfbHKN", 96022588728),
            ("5Ek8RkU6KMv5Fx7yivRVoQkuJYAKhULWiLWDpbGG4hvR9HFD", 968139369093),
            ("5HpCpGALzqgnDTP1HXFiuhzD5MFaDTRHjXBCvaMY9LNNRkT9", 104943979521),
            ("5FYqS77gxW9gHG8id1YYPS7Cd4TQmNUMhF8h3S77Fq2VvvRQ", 729199757977),
            ("5FtBqMg13pNf1N6TwfG6BmwyaDM77mkeQ16UTHsGasrVDedX", 131457064336),
            ("5GbnWR2XhWrRMt123SdrLbR9G2a4N5dtzA3TSu3Czkzoeu7x", 2295599153),
            ("5GgiowcCG4kLpwkCTGxxQJQv8WwKFyBQ6McPRmqKtWPy8EaK", 113838605389),
            ("5FL5YtYozpUAGaiVWonpbwEYdEMij3obJHSH3ACY4vgWmDgy", 8689039),
            ("5Egq58bxRv7boM2s3rnDxx1udnkzxPQ23HuoqohVxjh9RenC", 216373234348),
            ("5FRGeeEgRNR8U33FDKvN7yUgts8zR3qRJH4yKKWoR9GswBRb", 2196574958718),
            ("5FnhSy79BPYyrmmFsbckinQw1fLiLqqPkQL2vgZwPxbRfu3k", 42319631507),
            ("5Hj8jMhqAv7cfyRh5STfbZefMhv17QxZ1RxWq9jNcLAEsRRo", 132216702183491),
            ("5EXYTGMqumAH6RLQgHwkMEMnSvHcpHc89R6U8krfNJTYWm9J", 504320264499),
            ("5EFh8ctzmytXURqrCTUBWHTs87f7TMWB6XKUzdqxKXVUtvS2", 2209599669432),
            ("5CqVqEcRBkw7Gm2reJ33cj7puR9W2Tq7qsLxSruV1BgnMqKN", 1033387458788),
            ("5D79enmLSGimsruoraGagofhaSeYJZvGUqFCCrr83ZfZs1HS", 7591184215233),
            ("5HbpyjsvyXLWtf1QT1CyNUdyut6scM5dM7ytm8hoxFvRtU1i", 129833188275),
            ("5CnxCi7CdEriWSdw4LcXdbtjodxA6uTat4gBm4wuT9QToMdo", 3132978),
            ("5G48fiQjhAd8hc4rYc6GituCuAPKznL28jyyyq1auMyZiG4t", 514913328178),
            ("5FFGjW2hJ7tQ41qghSsLP4cVmA8j9pZVSrr2CrLG7fQAsLHJ", 346794972723),
            ("5FWjnxeRMtMFxRc9kvZKCG5iJAyyz2kmXV8u3kqyiXizZtiz", 225939835005),
            ("5CUw3sB4oxd3dVSHUr3kxsB591VEjaPzr444KkfjwVFnLRfJ", 208250614494),
            ("5EaBhxNUwMRyKsaeA2BEjDCrvwE5J8FDSpfCHK9gGmnmbhCa", 278083207003),
            ("5GHJ5HxFxYQyVoNFUxR3JCqqCKRumaFCY7N5zMxwF4CpRUWr", 1381466224829),
            ("5H1WgA7ET3FmEarJK6qc1vaTWbNd6g41mgvyLRkysrH4MDdo", 774889),
        ];

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            if diff > 0 {
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                    diff.unsigned_abs().into(),
                );
            }
        }

        // For one of the losers, add some extra stake
        let idx = 0;
        let lost_ck = diffs[idx].0;
        let coldkey_account_id = decode_account_id32::<Test>(lost_ck).expect("Invalid coldkey");
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            num_traits::ToPrimitive::to_u64(
                &((num_traits::ToPrimitive::to_f64(&diffs[idx].1).expect("float conv fail")
                    * 0.9_f64)
                    .abs()),
            )
            .expect("u64 conv fail")
            .into(),
        );
        let extra_balance = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );
        assert!(!extra_balance.is_zero(), "extra balance must be non-zero");

        let w = try_restore_shares::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Check stake is near 0 for all positive entires except the one we removed
        // Check the stake for all negative entries is proportional to the amount they lost
        let total_lost: f64 = diffs
            .iter()
            .map(|(_, diff)| {
                if diff.is_negative() {
                    num_traits::ToPrimitive::to_f64(&diff.saturating_abs())
                        .expect("float conv fail")
                } else {
                    0_f64
                }
            })
            .sum::<f64>();
        let mut total_returned = 0_f64;
        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");

            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff > 0 {
                assert_relative_eq!(stake_float, 0_f64, max_relative = 0.001_f64);
            } else if coldkey == lost_ck {
                total_returned += stake_float
                    - num_traits::ToPrimitive::to_f64(&extra_balance.to_u64())
                        .expect("float conv fail");
            } else {
                total_returned += stake_float;
            }
        }

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff < 0 {
                // Should get a return proportional to the amount they lost
                // versus the amount that was able to be recovered
                let prop_returned: f64 = num_traits::ToPrimitive::to_f64(&diff.abs())
                    .expect("float conv fail")
                    / total_lost
                    * total_returned;

                let mut expected_stake: f64 = prop_returned;
                if coldkey == lost_ck {
                    // this CK should retain the extra balance
                    expected_stake = prop_returned
                        + num_traits::ToPrimitive::to_f64(&extra_balance.to_u64())
                            .expect("float conv fail");
                }

                assert_relative_eq!(stake_float, expected_stake, max_relative = 0.001_f64);
            }
        }
    });
}

fn decode_account_id32_test(ss58_string: &str) -> U256 {
    let account_id32: AccountId32 = AccountId32::from_ss58check(ss58_string).unwrap();
    let mut account_id32_slice: &[u8] = account_id32.as_ref();
    U256::decode(&mut account_id32_slice).unwrap()
}

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

#[test]
fn test_migrate_subnet_balances() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);

        // Add network locks
        let lock1 = TaoBalance::from(123_000_000_000_u64);
        let lock2 = TaoBalance::from(321_000_000_000_u64);
        SubnetLocked::<Test>::insert(netuid1, lock1);
        SubnetLocked::<Test>::insert(netuid2, lock2);

        // Add SubnetTAO
        let reserve1 = TaoBalance::from(456_000_000_000_u64);
        let reserve2 = TaoBalance::from(654_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid1, reserve1);
        SubnetTAO::<Test>::insert(netuid2, reserve2);

        // Run migration
        crate::migrations::migrate_subnet_balances::migrate_subnet_balances::<Test>();

        // Test that subnet balances got updated
        let subnet_account_1 = SubtensorModule::get_subnet_account_id(netuid1).unwrap();
        let subnet_account_2 = SubtensorModule::get_subnet_account_id(netuid2).unwrap();
        let balance1 = SubtensorModule::get_coldkey_balance(&subnet_account_1);
        let balance2 = SubtensorModule::get_coldkey_balance(&subnet_account_2);
        let initial_pool_tao = NetworkMinLockCost::<Test>::get();
        assert_eq!(balance1, lock1 + reserve1 - initial_pool_tao);
        assert_eq!(balance2, lock2 + reserve2 - initial_pool_tao);

        // Check migration has been marked as run
        const MIGRATION_NAME: &[u8] = b"migrate_subnet_balances";
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
    });
}

#[test]
fn test_migrate_fix_total_issuance_evm_fees() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_fix_total_issuance_evm_fees";

        let account = U256::from(42);
        let balances_total_issuance = TaoBalance::from(123_456_789_u64);
        Balances::make_free_balance_be(&account, balances_total_issuance);

        let broken_subtensor_total_issuance = TaoBalance::from(987_654_321_u64);
        TotalIssuance::<Test>::put(broken_subtensor_total_issuance);

        assert_eq!(Balances::total_issuance(), balances_total_issuance);
        assert_eq!(
            TotalIssuance::<Test>::get(),
            broken_subtensor_total_issuance
        );
        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        let weight = crate::migrations::migrate_fix_total_issuance_evm_fees::migrate_fix_total_issuance_evm_fees::<Test>();

        assert!(!weight.is_zero(), "weight must be non-zero");
        assert_eq!(TotalIssuance::<Test>::get(), balances_total_issuance);
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        let second_wrong_value = TaoBalance::from(555_u64);
        TotalIssuance::<Test>::put(second_wrong_value);

        crate::migrations::migrate_fix_total_issuance_evm_fees::migrate_fix_total_issuance_evm_fees::<Test>();

        assert_eq!(
            TotalIssuance::<Test>::get(),
            second_wrong_value,
            "migration must not run more than once"
        );
    });
}

#[test]
fn test_migrate_reset_tnet_conviction_locks() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_reset_tnet_conviction_locks";

        let netuid = NetUid::from(1);
        let other_netuid = NetUid::from(2);
        let coldkey_1 = U256::from(1001);
        let coldkey_2 = U256::from(1002);
        let hotkey_1 = U256::from(2001);
        let hotkey_2 = U256::from(2002);

        let lock_1 = LockState {
            locked_mass: AlphaBalance::from(10_u64),
            conviction: U64F64::from_num(1.5),
            last_update: 11,
        };
        let lock_2 = LockState {
            locked_mass: AlphaBalance::from(20_u64),
            conviction: U64F64::from_num(2.5),
            last_update: 22,
        };

        Lock::<Test>::insert((coldkey_1, netuid, hotkey_1), lock_1.clone());
        Lock::<Test>::insert((coldkey_2, other_netuid, hotkey_2), lock_2.clone());
        HotkeyLock::<Test>::insert(netuid, hotkey_1, lock_1.clone());
        DecayingHotkeyLock::<Test>::insert(other_netuid, hotkey_2, lock_2.clone());
        OwnerLock::<Test>::insert(netuid, lock_1.clone());
        DecayingOwnerLock::<Test>::insert(other_netuid, lock_2.clone());
        DecayingLock::<Test>::insert(coldkey_1, netuid, false);
        DecayingLock::<Test>::insert(coldkey_2, other_netuid, false);

        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
        assert_eq!(Lock::<Test>::iter().count(), 2);
        assert_eq!(HotkeyLock::<Test>::iter().count(), 1);
        assert_eq!(DecayingHotkeyLock::<Test>::iter().count(), 1);
        assert_eq!(OwnerLock::<Test>::iter().count(), 1);
        assert_eq!(DecayingOwnerLock::<Test>::iter().count(), 1);
        assert_eq!(DecayingLock::<Test>::iter().count(), 2);

        let raw_owner_lock_key = {
            let mut key = Vec::new();
            key.extend_from_slice(&twox_128("SubtensorModule".as_bytes()));
            key.extend_from_slice(&twox_128("OwnerLock".as_bytes()));
            key.extend_from_slice(&NetUid::from(99).encode());
            key
        };
        let raw_decaying_hotkey_lock_key = {
            let mut key = Vec::new();
            key.extend_from_slice(&twox_128("SubtensorModule".as_bytes()));
            key.extend_from_slice(&twox_128("DecayingHotkeyLock".as_bytes()));
            key.extend_from_slice(&NetUid::from(100).encode());
            key.extend_from_slice(&Blake2_128Concat::hash(&U256::from(3003).encode()));
            key
        };

        // Simulate deprecated aggregate entries with bytes that the current
        // `LockState` type should never need to decode during this reset.
        put_raw(&raw_owner_lock_key, &123_u32.encode());
        put_raw(&raw_decaying_hotkey_lock_key, &(456_u32, 789_u32).encode());
        assert!(get_raw(&raw_owner_lock_key).is_some());
        assert!(get_raw(&raw_decaying_hotkey_lock_key).is_some());

        let weight =
            crate::migrations::migrate_reset_tnet_conviction_locks::migrate_reset_tnet_conviction_locks::<Test>();

        assert!(!weight.is_zero(), "migration weight should be non-zero");
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
        assert!(get_raw(&raw_owner_lock_key).is_none());
        assert!(get_raw(&raw_decaying_hotkey_lock_key).is_none());
        assert_eq!(Lock::<Test>::iter().count(), 0);
        assert_eq!(HotkeyLock::<Test>::iter().count(), 0);
        assert_eq!(DecayingHotkeyLock::<Test>::iter().count(), 0);
        assert_eq!(OwnerLock::<Test>::iter().count(), 0);
        assert_eq!(DecayingOwnerLock::<Test>::iter().count(), 0);
        assert_eq!(DecayingLock::<Test>::iter().count(), 0);

        Lock::<Test>::insert((coldkey_1, netuid, hotkey_1), lock_1);
        let second_weight =
            crate::migrations::migrate_reset_tnet_conviction_locks::migrate_reset_tnet_conviction_locks::<Test>();

        assert_eq!(
            second_weight,
            <Test as frame_system::Config>::DbWeight::get().reads(1),
            "second run should only read the migration flag"
        );
        assert_eq!(
            Lock::<Test>::iter().count(),
            1,
            "migration must not run more than once"
        );
    });
}

#[test]
fn test_migrate_dynamic_tempo_aligns_first_post_upgrade_fire() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "dynamic_tempo_v1";
        let netuid = NetUid::from(7u16);
        let tempo: u16 = 360;

        add_network(netuid, tempo, 0);
        let current_block = 1234u64;
        run_to_block(current_block);

        // Compute next-fire block
        let netuid_plus_one = (u16::from(netuid) as u64) + 1;
        let tempo_plus_one = (tempo as u64) + 1;
        let adjusted = current_block + netuid_plus_one;
        let remainder = adjusted % tempo_plus_one;
        let legacy_blocks_until_next = (tempo as u64) - remainder;
        let expected_next_fire = current_block + legacy_blocks_until_next;

        crate::migrations::migrate_dynamic_tempo::migrate_dynamic_tempo::<Test>();

        // New formula: next fire = LastEpochBlock + tempo.
        let last_epoch = LastEpochBlock::<Test>::get(netuid);
        assert_eq!(
            last_epoch + tempo as u64,
            expected_next_fire,
            "back-fill should make new scheduler fire at the same block as legacy modulo"
        );
        assert!(HasMigrationRun::<Test>::get(
            MIGRATION_NAME.as_bytes().to_vec()
        ));
    });
}

#[test]
fn test_migrate_dynamic_tempo_preserves_non_standard_tempo() {
    new_test_ext(1).execute_with(|| {
        // Three subnets — one standard, two with non-standard tempo
        // (simulates the 2 mainnet subnets root configured outside MIN/MAX bounds).
        let standard = NetUid::from(1u16);
        let small = NetUid::from(2u16);
        let large = NetUid::from(3u16);

        add_network(standard, 360, 0);
        add_network(small, 10, 0); // < MIN_TEMPO (360)
        add_network(large, 60_000, 0); // > MAX_TEMPO (50_400)

        crate::migrations::migrate_dynamic_tempo::migrate_dynamic_tempo::<Test>();

        // Tempo values preserved as-is — no clamp.
        assert_eq!(Tempo::<Test>::get(standard), 360);
        assert_eq!(Tempo::<Test>::get(small), 10);
        assert_eq!(Tempo::<Test>::get(large), 60_000);

        // All non-zero tempos got LastEpochBlock seeded.
        assert!(LastEpochBlock::<Test>::contains_key(standard));
        assert!(LastEpochBlock::<Test>::contains_key(small));
        assert!(LastEpochBlock::<Test>::contains_key(large));
    });
}

#[test]
fn test_migrate_dynamic_tempo_activity_cutoff_round_trips_production_values() {
    new_test_ext(1).execute_with(|| {
        // (cutoff_blocks, tempo) combinations from production data.
        let cases: [(u16, u16); 6] = [
            (5000, 360),
            (6000, 360),
            (7200, 360),
            (12000, 360),
            (1000, 360),
            (360, 360),
        ];

        for (i, &(cutoff, tempo)) in cases.iter().enumerate() {
            let netuid = NetUid::from((i + 1) as u16);
            add_network(netuid, tempo, 0);
            ActivityCutoff::<Test>::insert(netuid, cutoff);
        }

        crate::migrations::migrate_dynamic_tempo::migrate_dynamic_tempo::<Test>();

        for (i, &(cutoff, _)) in cases.iter().enumerate() {
            let netuid = NetUid::from((i + 1) as u16);
            // get_activity_cutoff_blocks = factor * tempo / 1000 must equal original cutoff exactly.
            assert_eq!(
                crate::Pallet::<Test>::get_activity_cutoff_blocks(netuid),
                cutoff as u64,
                "ceiling division must round-trip cutoff exactly for netuid {}",
                u16::from(netuid)
            );
        }
    });
}

#[test]
fn test_migrate_dynamic_tempo_idempotent() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        add_network(netuid, 360, 0);

        crate::migrations::migrate_dynamic_tempo::migrate_dynamic_tempo::<Test>();
        let last_epoch_first = LastEpochBlock::<Test>::get(netuid);

        // Mutate state to verify second run is a no-op.
        run_to_block(crate::Pallet::<Test>::get_current_block_as_u64() + 100);
        crate::migrations::migrate_dynamic_tempo::migrate_dynamic_tempo::<Test>();

        assert_eq!(
            LastEpochBlock::<Test>::get(netuid),
            last_epoch_first,
            "second migration call must be a no-op"
        );
    });
}
