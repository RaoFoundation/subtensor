#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Small and large bipartite graph epoch runs (`epoch` / `epoch_dense`).

use frame_support::assert_ok;
use sp_core::U256;
use substrate_fixed::types::I32F32;
use subtensor_runtime_common::{AlphaBalance, TaoBalance};

use super::super::mock::*;
use super::helpers::{distribute_nodes, init_run_epochs};
use crate::*;

// Test an epoch on a graph with a single item.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::epoch::graph_epochs::test_1_graph --exact --show-output --nocapture
#[test]
fn test_1_graph() {
    new_test_ext(1).execute_with(|| {
        log::info!("test_1_graph:");
        let netuid = NetUid::from(1);
        let coldkey = U256::from(0);
        let hotkey = U256::from(0);
        let uid: u16 = 0;
        let stake_amount: TaoBalance = 1_000_000_000.into();
        add_network_disable_commit_reveal(netuid, u16::MAX - 1, 0); // set higher tempo to avoid built-in epoch, then manual epoch instead
        SubtensorModule::set_max_allowed_uids(netuid, 1);
        add_balance_to_coldkey_account(
            &coldkey,
            stake_amount + ExistentialDeposit::get() + SubtensorModule::get_network_min_lock(),
        );
        register_ok_neuron(netuid, hotkey, coldkey, 1);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            stake_amount.into()
        ));

        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 1);
        run_to_block(1); // run to next block to ensure weights are set on nodes after their registration block
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(U256::from(uid)),
            netuid,
            vec![uid],
            vec![u16::MAX],
            0
        ));
        // SubtensorModule::set_weights_for_testing( netuid, i as u16, vec![ ( 0, u16::MAX )]); // doesn't set update status
        // SubtensorModule::set_bonds_for_testing( netuid, uid, vec![ ( 0, u16::MAX )]); // rather, bonds are calculated in epoch
        SubtensorModule::epoch(netuid, 1_000_000_000.into());
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey),
            stake_amount.into()
        );
        assert_eq!(SubtensorModule::get_rank_for_uid(netuid, uid), 0);
        assert_eq!(SubtensorModule::get_trust_for_uid(netuid, uid), 0);
        assert_eq!(SubtensorModule::get_consensus_for_uid(netuid, uid), 0);
        assert_eq!(
            SubtensorModule::get_incentive_for_uid(netuid.into(), uid),
            0
        );
        assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, uid), 0);
    });
}
// Test an epoch on a graph with two items.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::epoch::graph_epochs::test_10_graph --exact --show-output --nocapture
#[test]
fn test_10_graph() {
    new_test_ext(1).execute_with(|| {
        log::info!("test_10_graph");
        // Function for adding a nodes to the graph.
        pub fn add_node(netuid: NetUid, coldkey: U256, hotkey: U256, uid: u16, stake_amount: u64) {
            log::info!(
                "+Add net:{:?} coldkey:{:?} hotkey:{:?} uid:{:?} stake_amount: {:?} subn: {:?}",
                netuid,
                coldkey,
                hotkey,
                uid,
                stake_amount,
                SubtensorModule::get_subnetwork_n(netuid),
            );
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                netuid,
                stake_amount.into(),
            );
            SubtensorModule::append_neuron(netuid, &hotkey, 0);
            assert_eq!(SubtensorModule::get_subnetwork_n(netuid) - 1, uid);
        }
        // Build the graph with 10 items
        // each with 1 stake and self weights.
        let n: usize = 10;
        let netuid = NetUid::from(1);
        add_network_disable_commit_reveal(netuid, u16::MAX - 1, 0); // set higher tempo to avoid built-in epoch, then manual epoch instead
        SubtensorModule::set_max_allowed_uids(netuid, n as u16);
        for i in 0..10 {
            add_node(netuid, U256::from(i), U256::from(i), i as u16, 1)
        }
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 10);
        run_to_block(1); // run to next block to ensure weights are set on nodes after their registration block
        for i in 0..10 {
            assert_ok!(SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::from(i)),
                netuid,
                vec![i as u16],
                vec![u16::MAX],
                0
            ));
        }
        // Run the epoch.
        SubtensorModule::epoch(netuid, 1_000_000_000.into());
        // Check return values.
        for i in 0..n {
            assert_eq!(
                SubtensorModule::get_total_stake_for_hotkey(&(U256::from(i))),
                TaoBalance::from(1)
            );
            assert_eq!(SubtensorModule::get_rank_for_uid(netuid, i as u16), 0);
            assert_eq!(SubtensorModule::get_trust_for_uid(netuid, i as u16), 0);
            assert_eq!(SubtensorModule::get_consensus_for_uid(netuid, i as u16), 0);
            assert_eq!(
                SubtensorModule::get_incentive_for_uid(netuid.into(), i as u16),
                0
            );
            assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, i as u16), 0);
            assert_eq!(
                SubtensorModule::get_emission_for_uid(netuid, i as u16),
                99999999.into()
            );
        }
    });
}

// Test an epoch on a graph with 512 nodes, of which the first 64 are validators setting non-self weights, and the rest servers setting only self-weights.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::epoch::graph_epochs::test_512_graph --exact --show-output --nocapture
#[test]
fn test_512_graph() {
    let netuid = NetUid::from(1);
    let network_n: u16 = 512;
    let validators_n: u16 = 64;
    let max_stake_per_validator: u64 = 328_125_000_000_000; // 21_000_000_000_000_000 / 64
    let epochs: u16 = 3;
    log::info!("test_{network_n:?}_graph ({validators_n:?} validators)");
    for interleave in 0..3 {
        for server_self in [false, true] {
            // server-self weight off/on
            let (validators, servers) = distribute_nodes(
                validators_n as usize,
                network_n as usize,
                interleave as usize,
            );
            let server: usize = servers[0] as usize;
            let validator: usize = validators[0] as usize;
            new_test_ext(1).execute_with(|| {
                init_run_epochs(
                    netuid,
                    network_n,
                    &validators,
                    &servers,
                    epochs,
                    max_stake_per_validator,
                    server_self,
                    &[],
                    false,
                    &[],
                    false,
                    false,
                    0,
                    false,
                    u16::MAX,
                );
                let bonds = SubtensorModule::get_bonds(netuid.into());
                for uid in validators {
                    assert_eq!(
                        SubtensorModule::get_total_stake_for_hotkey(&(U256::from(uid))),
                        max_stake_per_validator.into()
                    );
                    assert_eq!(SubtensorModule::get_consensus_for_uid(netuid, uid), 0);
                    assert_eq!(
                        SubtensorModule::get_incentive_for_uid(netuid.into(), uid),
                        0
                    );
                    assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, uid), 1023); // floor(1 / 64 * 65_535)
                    assert_eq!(
                        SubtensorModule::get_emission_for_uid(netuid, uid),
                        7812500.into()
                    ); // 0.5 / 200 * 1_000_000_000
                    assert_eq!(bonds[uid as usize][validator], 0.0);
                    assert_eq!(bonds[uid as usize][server], I32F32::from_num(65_535));
                }
                for uid in servers {
                    assert_eq!(
                        SubtensorModule::get_total_stake_for_hotkey(&(U256::from(uid))),
                        TaoBalance::ZERO
                    );
                    assert_eq!(SubtensorModule::get_consensus_for_uid(netuid, uid), 146);
                    assert_eq!(
                        SubtensorModule::get_incentive_for_uid(netuid.into(), uid),
                        146
                    ); // floor(1 / (512 - 64) * 65_535)
                    assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, uid), 0);
                    assert_eq!(
                        SubtensorModule::get_emission_for_uid(netuid, uid),
                        1116071.into()
                    ); // floor(0.5 / (512 - 64) * 1_000_000_000)
                    assert_eq!(bonds[uid as usize][validator], 0.0);
                    assert_eq!(bonds[uid as usize][server], 0.0);
                }
            });
        }
    }
}

// Test an epoch on a graph with 4096 nodes, of which the first 256 are validators setting random non-self weights, and the rest servers setting only self-weights.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::epoch::graph_epochs::test_512_graph_random_weights --exact --show-output --nocapture
#[test]
fn test_512_graph_random_weights() {
    let netuid = NetUid::from(1);
    let network_n: u16 = 512;
    let validators_n: u16 = 64;
    let epochs: u16 = 1;
    log::info!("test_{network_n:?}_graph_random_weights ({validators_n:?} validators)");
    for interleave in 0..3 {
        // server-self weight off/on
        for server_self in [false, true] {
            for bonds_penalty in [0, u16::MAX / 2, u16::MAX] {
                let (validators, servers) = distribute_nodes(
                    validators_n as usize,
                    network_n as usize,
                    interleave as usize,
                );
                let server: usize = servers[0] as usize;
                let validator: usize = validators[0] as usize;
                let (mut rank, mut incentive, mut dividend, mut emission, mut bondv, mut bonds): (
                    Vec<u16>,
                    Vec<u16>,
                    Vec<u16>,
                    Vec<AlphaBalance>,
                    Vec<I32F32>,
                    Vec<I32F32>,
                ) = (vec![], vec![], vec![], vec![], vec![], vec![]);

                // Dense epoch
                new_test_ext(1).execute_with(|| {
                    init_run_epochs(
                        netuid,
                        network_n,
                        &validators,
                        &servers,
                        epochs,
                        1,
                        server_self,
                        &[],
                        false,
                        &[],
                        false,
                        true,
                        interleave as u64,
                        false,
                        bonds_penalty,
                    );

                    let bond = SubtensorModule::get_bonds(netuid.into());
                    for uid in 0..network_n {
                        rank.push(SubtensorModule::get_rank_for_uid(netuid, uid));
                        incentive.push(SubtensorModule::get_incentive_for_uid(netuid.into(), uid));
                        dividend.push(SubtensorModule::get_dividends_for_uid(netuid, uid));
                        emission.push(SubtensorModule::get_emission_for_uid(netuid, uid));
                        bondv.push(bond[uid as usize][validator]);
                        bonds.push(bond[uid as usize][server]);
                    }
                });

                // Sparse epoch (same random seed as dense)
                new_test_ext(1).execute_with(|| {
                    init_run_epochs(
                        netuid,
                        network_n,
                        &validators,
                        &servers,
                        epochs,
                        1,
                        server_self,
                        &[],
                        false,
                        &[],
                        false,
                        true,
                        interleave as u64,
                        true,
                        bonds_penalty,
                    );
                    // Assert that dense and sparse epoch results are equal
                    let bond = SubtensorModule::get_bonds(netuid.into());
                    for uid in 0..network_n {
                        assert_eq!(
                            SubtensorModule::get_rank_for_uid(netuid, uid),
                            rank[uid as usize]
                        );
                        assert_eq!(
                            SubtensorModule::get_incentive_for_uid(netuid.into(), uid),
                            incentive[uid as usize]
                        );
                        assert_eq!(
                            SubtensorModule::get_dividends_for_uid(netuid, uid),
                            dividend[uid as usize]
                        );
                        assert_eq!(
                            SubtensorModule::get_emission_for_uid(netuid, uid),
                            emission[uid as usize]
                        );
                        assert_eq!(bond[uid as usize][validator], bondv[uid as usize]);
                        assert_eq!(bond[uid as usize][server], bonds[uid as usize]);
                    }
                });
            }
        }
    }
}

// Test an epoch on a graph with 4096 nodes, of which the first 256 are validators setting non-self weights, and the rest servers setting only self-weights.
// #[test]
// fn test_4096_graph() {
//     let netuid = NetUid::from(1);
//     let network_n: u16 = 4096;
//     let validators_n: u16 = 256;
//     let epochs: u16 = 1;
//     let max_stake_per_validator: u64 = 82_031_250_000_000; // 21_000_000_000_000_000 / 256
//     log::info!("test_{network_n:?}_graph ({validators_n:?} validators)");
//     for interleave in 0..3 {
//         let (validators, servers) = distribute_nodes(
//             validators_n as usize,
//             network_n as usize,
//             interleave as usize,
//         );
//         let server: usize = servers[0] as usize;
//         let validator: usize = validators[0] as usize;
//         for server_self in [false, true] {
//             // server-self weight off/on
//             new_test_ext(1).execute_with(|| {
//                 init_run_epochs(
//                     netuid,
//                     network_n,
//                     &validators,
//                     &servers,
//                     epochs,
//                     max_stake_per_validator,
//                     server_self,
//                     &[],
//                     false,
//                     &[],
//                     false,
//                     false,
//                     0,
//                     true,
//                     u16::MAX,
//                 );
//                 let (total_stake, _, _) = SubtensorModule::get_stake_weights_for_network(netuid);
//                 assert_eq!(total_stake.iter().map(|s| s.to_num::<u64>()).sum::<u64>(), 21_000_000_000_000_000);
//                 let bonds = SubtensorModule::get_bonds(netuid);
//                 for uid in &validators {
//                     assert_eq!(
//                         SubtensorModule::get_total_stake_for_hotkey(&(U256::from(*uid as u64))),
//                         max_stake_per_validator
//                     );
//                     assert_eq!(SubtensorModule::get_rank_for_uid(netuid, *uid), 0);
//                     assert_eq!(SubtensorModule::get_trust_for_uid(netuid, *uid), 0);
//                     assert_eq!(SubtensorModule::get_consensus_for_uid(netuid, *uid), 0);
//                     assert_eq!(SubtensorModule::get_incentive_for_uid(netuid, *uid), 0);
//                     assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, *uid), 255); // Note D = floor(1 / 256 * 65_535)
//                     assert_eq!(SubtensorModule::get_emission_for_uid(netuid, *uid), 1953125); // Note E = 0.5 / 256 * 1_000_000_000 = 1953125
//                     assert_eq!(bonds[*uid as usize][validator], 0.0);
//                     assert_eq!(
//                         bonds[*uid as usize][server],
//                         I32F32::from_num(255) / I32F32::from_num(65_535)
//                     ); // Note B_ij = floor(1 / 256 * 65_535) / 65_535
//                 }
//                 for uid in &servers {
//                     assert_eq!(
//                         SubtensorModule::get_total_stake_for_hotkey(&(U256::from(*uid as u64))),
//                         0
//                     );
//                     assert_eq!(SubtensorModule::get_rank_for_uid(netuid, *uid), 17); // Note R = floor(1 / (4096 - 256) * 65_535) = 17
//                     assert_eq!(SubtensorModule::get_trust_for_uid(netuid, *uid), 65535);
//                     assert_eq!(SubtensorModule::get_consensus_for_uid(netuid, *uid), 17); // Note C = floor(1 / (4096 - 256) * 65_535) = 17
//                     assert_eq!(SubtensorModule::get_incentive_for_uid(netuid, *uid), 17); // Note I = floor(1 / (4096 - 256) * 65_535) = 17
//                     assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, *uid), 0);
//                     assert_eq!(SubtensorModule::get_emission_for_uid(netuid, *uid), 130208); // Note E = floor(0.5 / (4096 - 256) * 1_000_000_000) = 130208
//                     assert_eq!(bonds[*uid as usize][validator], 0.0);
//                     assert_eq!(bonds[*uid as usize][server], 0.0);
//                 }
//             });
//         }
//     }
// }

// Test an epoch_sparse on a graph with 16384 nodes, of which the first 512 are validators setting non-self weights, and the rest servers setting only self-weights.
// #[test]
// fn test_16384_graph_sparse() {
//     new_test_ext(1).execute_with(|| {
//         let netuid = NetUid::from(1);
//         let n: u16 = 16384;
//         let validators_n: u16 = 512;
//         let validators: Vec<u16> = (0..validators_n).collect();
//         let servers: Vec<u16> = (validators_n..n).collect();
//         let server: u16 = servers[0];
//         let epochs: u16 = 1;
//         log::info!("test_{n:?}_graph ({validators_n:?} validators)");
//         init_run_epochs(
//             netuid,
//             n,
//             &validators,
//             &servers,
//             epochs,
//             1,
//             false,
//             &[],
//             false,
//             &[],
//             false,
//             false,
//             0,
//             true,
//             u16::MAX,
//         );
//         let bonds = SubtensorModule::get_bonds(netuid);
//         for uid in validators {
//             assert_eq!(
//                 SubtensorModule::get_total_stake_for_hotkey(&(U256::from(uid))),
//                 1
//             );
//             assert_eq!(SubtensorModule::get_rank_for_uid(netuid, uid), 0);
//             assert_eq!(SubtensorModule::get_trust_for_uid(netuid, uid), 0);
//             assert_eq!(SubtensorModule::get_consensus_for_uid(netuid, uid), 438); // Note C = 0.0066928507 = (0.0066928507*65_535) = floor( 438.6159706245 )
//             assert_eq!(SubtensorModule::get_incentive_for_uid(netuid, uid), 0);
//             assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, uid), 127); // Note D = floor(1 / 512 * 65_535) = 127
//             assert_eq!(SubtensorModule::get_emission_for_uid(netuid, uid), 976085); // Note E = 0.5 / 512 * 1_000_000_000 = 976_562 (discrepancy)
//             assert_eq!(bonds[uid as usize][0], 0.0);
//             assert_eq!(
//                 bonds[uid as usize][server as usize],
//                 I32F32::from_num(127) / I32F32::from_num(65_535)
//             ); // Note B_ij = floor(1 / 512 * 65_535) / 65_535 = 127 / 65_535
//         }
//         for uid in servers {
//             assert_eq!(
//                 SubtensorModule::get_total_stake_for_hotkey(&(U256::from(uid))),
//                 0
//             );
//             assert_eq!(SubtensorModule::get_rank_for_uid(netuid, uid), 4); // Note R = floor(1 / (16384 - 512) * 65_535) = 4
//             assert_eq!(SubtensorModule::get_trust_for_uid(netuid, uid), 65535);
//             assert_eq!(SubtensorModule::get_consensus_for_uid(netuid, uid), 4); // Note C = floor(1 / (16384 - 512) * 65_535) = 4
//             assert_eq!(SubtensorModule::get_incentive_for_uid(netuid, uid), 4); // Note I = floor(1 / (16384 - 512) * 65_535) = 4
//             assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, uid), 0);
//             assert_eq!(SubtensorModule::get_emission_for_uid(netuid, uid), 31517); // Note E = floor(0.5 / (16384 - 512) * 1_000_000_000) = 31502 (discrepancy)
//             assert_eq!(bonds[uid as usize][0], 0.0);
//             assert_eq!(bonds[uid as usize][server as usize], 0.0);
//         }
//     });
// }
