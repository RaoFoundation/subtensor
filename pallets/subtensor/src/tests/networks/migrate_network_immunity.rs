#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! `migrate_network_immunity_period` storage migration coverage.

use super::prelude::*;

#[test]
fn test_migrate_network_immunity_period() {
    new_test_ext(0).execute_with(|| {
        // --------------------------------------------------------------------
        // ‼️ PRE-CONDITIONS
        // --------------------------------------------------------------------
        assert_ne!(NetworkImmunityPeriod::<Test>::get(), 864_000);
        assert!(
            !HasMigrationRun::<Test>::get(b"migrate_network_immunity_period".to_vec()),
            "HasMigrationRun should be false before migration"
        );

        // --------------------------------------------------------------------
        // ▶️  RUN MIGRATION
        // --------------------------------------------------------------------
        let weight = migrate_network_immunity_period::migrate_network_immunity_period::<Test>();

        // --------------------------------------------------------------------
        // ✅ POST-CONDITIONS
        // --------------------------------------------------------------------
        assert_eq!(
            NetworkImmunityPeriod::<Test>::get(),
            864_000,
            "NetworkImmunityPeriod should now be 864_000"
        );

        assert!(
            HasMigrationRun::<Test>::get(b"migrate_network_immunity_period".to_vec()),
            "HasMigrationRun should be true after migration"
        );

        assert!(weight != Weight::zero(), "migration weight should be > 0");
    });
}

// #[test]
// fn test_schedule_dissolve_network_execution() {
//     new_test_ext(1).execute_with(|| {
//         let block_number: u64 = 0;
//         let netuid = NetUid::from(2);
//         let tempo: u16 = 13;
//         let hotkey_account_id: U256 = U256::from(1);
//         let coldkey_account_id = U256::from(0); // Neighbour of the beast, har har
//         let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
//             netuid,
//             block_number,
//             129123813,
//             &hotkey_account_id,
//         );

//         //add network
//         add_network(netuid, tempo, 0);

//         assert_ok!(SubtensorModule::register(
//             <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
//             netuid,
//             block_number,
//             nonce,
//             work.clone(),
//             hotkey_account_id,
//             coldkey_account_id
//         ));

//         assert!(SubtensorModule::if_subnet_exist(netuid));

//         assert_ok!(SubtensorModule::schedule_dissolve_network(
//             <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
//             netuid
//         ));

//         let current_block = System::block_number();
//         let execution_block = current_block + DissolveNetworkScheduleDuration::<Test>::get();

//         System::assert_last_event(
//             Event::DissolveNetworkScheduled {
//                 account: coldkey_account_id,
//                 netuid,
//                 execution_block,
//             }
//             .into(),
//         );

//         run_to_block(execution_block);
//         assert!(!SubtensorModule::if_subnet_exist(netuid));
//     })
// }

// #[test]
// fn test_non_owner_schedule_dissolve_network_execution() {
//     new_test_ext(1).execute_with(|| {
//         let block_number: u64 = 0;
//         let netuid = NetUid::from(2);
//         let tempo: u16 = 13;
//         let hotkey_account_id: U256 = U256::from(1);
//         let coldkey_account_id = U256::from(0); // Neighbour of the beast, har har
//         let non_network_owner_account_id = U256::from(2); //
//         let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
//             netuid,
//             block_number,
//             129123813,
//             &hotkey_account_id,
//         );

//         //add network
//         add_network(netuid, tempo, 0);

//         assert_ok!(SubtensorModule::register(
//             <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
//             netuid,
//             block_number,
//             nonce,
//             work.clone(),
//             hotkey_account_id,
//             coldkey_account_id
//         ));

//         assert!(SubtensorModule::if_subnet_exist(netuid));

//         assert_ok!(SubtensorModule::schedule_dissolve_network(
//             <<Test as Config>::RuntimeOrigin>::signed(non_network_owner_account_id),
//             netuid
//         ));

//         let current_block = System::block_number();
//         let execution_block = current_block + DissolveNetworkScheduleDuration::<Test>::get();

//         System::assert_last_event(
//             Event::DissolveNetworkScheduled {
//                 account: non_network_owner_account_id,
//                 netuid,
//                 execution_block,
//             }
//             .into(),
//         );

//         run_to_block(execution_block);
//         // network exists since the caller is no the network owner
//         assert!(SubtensorModule::if_subnet_exist(netuid));
//     })
// }

// #[test]
// fn test_new_owner_schedule_dissolve_network_execution() {
//     new_test_ext(1).execute_with(|| {
//         let block_number: u64 = 0;
//         let netuid = NetUid::from(2);
//         let tempo: u16 = 13;
//         let hotkey_account_id: U256 = U256::from(1);
//         let coldkey_account_id = U256::from(0); // Neighbour of the beast, har har
//         let new_network_owner_account_id = U256::from(2); //
//         let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
//             netuid,
//             block_number,
//             129123813,
//             &hotkey_account_id,
//         );

//         //add network
//         add_network(netuid, tempo, 0);

//         assert_ok!(SubtensorModule::register(
//             <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
//             netuid,
//             block_number,
//             nonce,
//             work.clone(),
//             hotkey_account_id,
//             coldkey_account_id
//         ));

//         assert!(SubtensorModule::if_subnet_exist(netuid));

//         // the account is not network owner when schedule the call
//         assert_ok!(SubtensorModule::schedule_dissolve_network(
//             <<Test as Config>::RuntimeOrigin>::signed(new_network_owner_account_id),
//             netuid
//         ));

//         let current_block = System::block_number();
//         let execution_block = current_block + DissolveNetworkScheduleDuration::<Test>::get();

//         System::assert_last_event(
//             Event::DissolveNetworkScheduled {
//                 account: new_network_owner_account_id,
//                 netuid,
//                 execution_block,
//             }
//             .into(),
//         );
//         run_to_block(current_block + 1);
//         // become network owner after call scheduled
//         crate::SubnetOwner::<Test>::insert(netuid, new_network_owner_account_id);

//         run_to_block(execution_block);
//         // network exists since the caller is no the network owner
//         assert!(!SubtensorModule::if_subnet_exist(netuid));
//     })
// }

// #[test]
// fn test_schedule_dissolve_network_execution_with_coldkey_swap() {
//     new_test_ext(1).execute_with(|| {
//         let block_number: u64 = 0;
//         let netuid = NetUid::from(2);
//         let tempo: u16 = 13;
//         let hotkey_account_id: U256 = U256::from(1);
//         let coldkey_account_id = U256::from(0); // Neighbour of the beast, har har
//         let new_network_owner_account_id = U256::from(2); //

//         add_balance_to_coldkey_account(&coldkey_account_id, 1000000000000000);

//         let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
//             netuid,
//             block_number,
//             129123813,
//             &hotkey_account_id,
//         );

//         //add network
//         add_network(netuid, tempo, 0);

//         assert_ok!(SubtensorModule::register(
//             <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
//             netuid,
//             block_number,
//             nonce,
//             work.clone(),
//             hotkey_account_id,
//             coldkey_account_id
//         ));

//         assert!(SubtensorModule::if_subnet_exist(netuid));

//         // the account is not network owner when schedule the call
//         assert_ok!(SubtensorModule::schedule_swap_coldkey(
//             <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
//             new_network_owner_account_id
//         ));

//         let current_block = System::block_number();
//         let execution_block = current_block + ColdkeySwapScheduleDuration::<Test>::get();

//         run_to_block(execution_block - 1);

//         // the account is not network owner when schedule the call
//         assert_ok!(SubtensorModule::schedule_dissolve_network(
//             <<Test as Config>::RuntimeOrigin>::signed(new_network_owner_account_id),
//             netuid
//         ));

//         System::assert_last_event(
//             Event::DissolveNetworkScheduled {
//                 account: new_network_owner_account_id,
//                 netuid,
//                 execution_block: DissolveNetworkScheduleDuration::<Test>::get() + execution_block
//                     - 1,
//             }
//             .into(),
//         );

//         run_to_block(execution_block);
//         assert_eq!(
//             crate::SubnetOwner::<Test>::get(netuid),
//             new_network_owner_account_id
//         );

//         let current_block = System::block_number();
//         let execution_block = current_block + DissolveNetworkScheduleDuration::<Test>::get();

//         run_to_block(execution_block);
//         // network exists since the caller is no the network owner
//         assert!(!SubtensorModule::if_subnet_exist(netuid));
//     })
// }

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::networks::test_register_subnet_low_lock_cost --exact --show-output --nocapture
