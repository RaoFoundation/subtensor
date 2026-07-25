#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Yuma3 / liquid-alpha bond and dividend trajectories across epochs.

use frame_support::assert_ok;
use sp_core::U256;
use substrate_fixed::types::I32F32;
use subtensor_runtime_common::TaoBalance;

use super::super::mock::*;
use crate::epoch::math::{fixed, u16_proportion_to_fixed};
use crate::*;

/// Asserts that two I32F32 values are approximately equal within a given epsilon.
///
/// # Arguments
/// * `left` - The first value to compare.
/// * `right` - The second value to compare.
/// * `epsilon` - The maximum allowed difference between the two values.
pub(super) fn assert_approx_eq(left: I32F32, right: I32F32, epsilon: I32F32) {
    if (left - right).abs() > epsilon {
        panic!(
            "assertion failed: `(left ≈ right)`\n  left: `{left:?}`,\n right: `{right:?}`,\n epsilon: `{epsilon:?}`"
        );
    }
}

// test Yuma 3 scenarios over a sequence of epochs.
fn setup_yuma_3_scenario(netuid: NetUid, n: u16, sparse: bool, max_stake: u64, stakes: Vec<u64>) {
    let block_number = System::block_number();
    let tempo: u16 = 1; // high tempo to skip automatic epochs in on_initialize, use manual epochs instead
    add_network_disable_commit_reveal(netuid, tempo, 0);

    SubtensorModule::set_max_allowed_uids(netuid, n);
    assert_eq!(SubtensorModule::get_max_allowed_uids(netuid), n);
    SubtensorModule::set_max_registrations_per_block(netuid, n);
    SubtensorModule::set_target_registrations_per_interval(netuid, n);
    SubtensorModule::set_weights_set_rate_limit(netuid, 0);
    SubtensorModule::set_min_allowed_weights(netuid, 1);
    SubtensorModule::set_bonds_penalty(netuid, 0);
    SubtensorModule::set_alpha_sigmoid_steepness(netuid, 1000);
    SubtensorModule::set_bonds_moving_average(netuid, 975_000);

    // === Register
    for key in 0..n as u64 {
        add_balance_to_coldkey_account(
            &U256::from(key),
            TaoBalance::from(max_stake)
                + ExistentialDeposit::get()
                + SubtensorModule::get_network_min_lock(),
        );
        let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
            netuid,
            block_number,
            key * 1_000_000,
            &U256::from(key),
        );
        assert_ok!(SubtensorModule::register(
            <<Test as frame_system::Config>::RuntimeOrigin>::signed(U256::from(key)),
            netuid,
            block_number,
            nonce,
            work,
            U256::from(key),
            U256::from(key)
        ));
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &U256::from(key),
            &U256::from(key),
            netuid,
            stakes[key as usize].into(),
        );
    }
    assert_eq!(SubtensorModule::get_max_allowed_uids(netuid), n);
    assert_eq!(SubtensorModule::get_subnetwork_n(netuid), n);

    // Enable Liquid Alpha
    SubtensorModule::set_kappa(netuid, u16::MAX / 2);
    SubtensorModule::set_liquid_alpha_enabled(netuid, true);
    SubtensorModule::set_alpha_values_32(netuid, I32F32::from_num(0.1), I32F32::from_num(0.3));

    // Enable Yuma3
    SubtensorModule::set_yuma3_enabled(netuid, true);

    // === Issue validator permits
    SubtensorModule::set_max_allowed_validators(netuid, 3);

    // run first epoch to set allowed validators
    // run to next block to ensure weights are set on nodes after their registration block
    run_epoch(netuid, sparse);
}

fn run_epoch(netuid: NetUid, sparse: bool) {
    next_block_no_epoch(netuid);
    if sparse {
        SubtensorModule::epoch(netuid, 1_000_000_000.into());
    } else {
        SubtensorModule::epoch_dense(netuid, 1_000_000_000.into());
    }
}

fn run_epoch_and_check_bonds_dividends(
    netuid: NetUid,
    sparse: bool,
    target_bonds: &[Vec<f32>],
    target_dividends: &[f32],
) {
    run_epoch(netuid, sparse);
    let bonds = SubtensorModule::get_bonds_fixed_proportion(netuid.into());
    let dividends = SubtensorModule::get_dividends(netuid);

    let epsilon = I32F32::from_num(1e-3);
    // Check the bonds
    for (bond, target_bond) in bonds.iter().zip(target_bonds.iter()) {
        // skip the 3 validators
        for (b, t) in bond.iter().zip(target_bond.iter().skip(3)) {
            assert_approx_eq(*b, fixed(*t), epsilon);
        }
    }
    // Check the dividends
    for (dividend, target_dividend) in dividends.iter().zip(target_dividends.iter()) {
        assert_approx_eq(
            u16_proportion_to_fixed(*dividend),
            fixed(*target_dividend),
            epsilon,
        );
    }
}

fn set_yuma_3_weights(netuid: NetUid, weights: Vec<Vec<u16>>, indices: Vec<u16>) {
    for (uid, weight) in weights.iter().enumerate() {
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(U256::from(uid as u64)),
            netuid,
            indices.clone(),
            weight.to_vec(),
            0
        ));
    }
}

#[test]
fn test_yuma_3_kappa_moves_first() {
    for sparse in [true, false].iter() {
        new_test_ext(1).execute_with(|| {
            let n: u16 = 5; // 3 validators, 2 servers
            let netuid = NetUid::from(1);
            let max_stake: u64 = 8;

            // Validator A: kappa / Big validator (0.8) - moves first
            // Validator B: Small eager validator (0.1) - moves second
            // Validator C: Small lazy validator (0.1) - moves last
            let stakes: Vec<u64> = vec![8, 1, 1, 0, 0];

            setup_yuma_3_scenario(netuid, n, *sparse, max_stake, stakes);
            let targets_bonds = [
                vec![
                    vec![0.1013, 0.0000],
                    vec![0.1013, 0.0000],
                    vec![0.1013, 0.0000],
                ],
                vec![
                    vec![0.0908, 0.1013],
                    vec![0.3697, 0.0000],
                    vec![0.3697, 0.0000],
                ],
                vec![
                    vec![0.0815, 0.1924],
                    vec![0.3170, 0.1013],
                    vec![0.5580, 0.0000],
                ],
                vec![
                    vec![0.0731, 0.2742],
                    vec![0.2765, 0.1924],
                    vec![0.4306, 0.1013],
                ],
                vec![
                    vec![0.0656, 0.3478],
                    vec![0.2435, 0.2742],
                    vec![0.3589, 0.1924],
                ],
                vec![
                    vec![0.0588, 0.4139],
                    vec![0.2157, 0.3478],
                    vec![0.3089, 0.2742],
                ],
            ];

            let targets_dividends = [
                vec![0.8000, 0.1000, 0.1000, 0.0000, 0.0000],
                vec![1.0000, 0.0000, 0.0000, 0.0000, 0.0000],
                vec![0.9382, 0.0618, 0.0000, 0.0000, 0.0000],
                vec![0.8819, 0.0773, 0.0407, 0.0000, 0.0000],
                vec![0.8564, 0.0844, 0.0592, 0.0000, 0.0000],
                vec![0.8418, 0.0884, 0.0697, 0.0000, 0.0000],
            ];

            for (epoch, (target_bonds, target_dividends)) in targets_bonds
                .iter()
                .zip(targets_dividends.iter())
                .enumerate()
            {
                match epoch {
                    0 => {
                        // Initially, consensus is achieved by all Validators
                        set_yuma_3_weights(netuid, vec![vec![u16::MAX, 0]; 3], vec![3, 4]);
                    }
                    1 => {
                        // Validator A -> Server 2
                        // Validator B -> Server 1
                        // Validator C -> Server 1
                        set_yuma_3_weights(
                            netuid,
                            vec![vec![0, u16::MAX], vec![u16::MAX, 0], vec![u16::MAX, 0]],
                            vec![3, 4],
                        );
                    }
                    2 => {
                        // Validator A -> Server 2
                        // Validator B -> Server 2
                        // Validator C -> Server 1
                        set_yuma_3_weights(
                            netuid,
                            vec![vec![0, u16::MAX], vec![0, u16::MAX], vec![u16::MAX, 0]],
                            vec![3, 4],
                        );
                    }
                    3 => {
                        // Subsequent epochs All validators -> Server 2
                        set_yuma_3_weights(netuid, vec![vec![0, u16::MAX]; 3], vec![3, 4]);
                    }
                    _ => {}
                };
                run_epoch_and_check_bonds_dividends(
                    netuid,
                    *sparse,
                    target_bonds,
                    target_dividends,
                );
            }
        })
    }
}

#[test]
fn test_yuma_3_kappa_moves_second() {
    for sparse in [true, false].iter() {
        new_test_ext(1).execute_with(|| {
            let n: u16 = 5; // 3 validators, 2 servers
            let netuid = NetUid::from(1);
            let max_stake: u64 = 8;

            // Validator A: kappa / Big validator (0.8) - moves second
            // Validator B: Small eager validator (0.1) - moves first
            // Validator C: Small lazy validator (0.1) - moves last
            let stakes: Vec<u64> = vec![8, 1, 1, 0, 0];

            setup_yuma_3_scenario(netuid, n, *sparse, max_stake, stakes);
            let targets_bonds = [
                vec![
                    vec![0.1013, 0.0000],
                    vec![0.1013, 0.0000],
                    vec![0.1013, 0.0000],
                ],
                vec![
                    vec![0.1924, 0.0000],
                    vec![0.0908, 0.2987],
                    vec![0.1924, 0.0000],
                ],
                vec![
                    vec![0.1715, 0.1013],
                    vec![0.0815, 0.3697],
                    vec![0.4336, 0.0000],
                ],
                vec![
                    vec![0.1531, 0.1924],
                    vec![0.0731, 0.4336],
                    vec![0.3608, 0.1013],
                ],
                vec![
                    vec![0.1369, 0.2742],
                    vec![0.0656, 0.4910],
                    vec![0.3103, 0.1924],
                ],
                vec![
                    vec![0.1225, 0.3478],
                    vec![0.0588, 0.5426],
                    vec![0.2712, 0.2742],
                ],
            ];
            let targets_dividends = [
                vec![0.8000, 0.1000, 0.1000, 0.0000, 0.0000],
                vec![0.8446, 0.0498, 0.1056, 0.0000, 0.0000],
                vec![0.6868, 0.3132, 0.0000, 0.0000, 0.0000],
                vec![0.7421, 0.2090, 0.0489, 0.0000, 0.0000],
                vec![0.7625, 0.1706, 0.0669, 0.0000, 0.0000],
                vec![0.7730, 0.1508, 0.0762, 0.0000, 0.0000],
            ];

            for (epoch, (target_bonds, target_dividends)) in targets_bonds
                .iter()
                .zip(targets_dividends.iter())
                .enumerate()
            {
                match epoch {
                    0 => {
                        // Initially, consensus is achieved by all Validators
                        set_yuma_3_weights(netuid, vec![vec![u16::MAX, 0]; 3], vec![3, 4]);
                    }
                    1 => {
                        // Validator A -> Server 1
                        // Validator B -> Server 2
                        // Validator C -> Server 1
                        set_yuma_3_weights(
                            netuid,
                            vec![vec![u16::MAX, 0], vec![0, u16::MAX], vec![u16::MAX, 0]],
                            vec![3, 4],
                        );
                    }
                    2 => {
                        // Validator A -> Server 2
                        // Validator B -> Server 2
                        // Validator C -> Server 1
                        set_yuma_3_weights(
                            netuid,
                            vec![vec![0, u16::MAX], vec![0, u16::MAX], vec![u16::MAX, 0]],
                            vec![3, 4],
                        );
                    }
                    3 => {
                        // Subsequent epochs All validators -> Server 2
                        set_yuma_3_weights(netuid, vec![vec![0, u16::MAX]; 3], vec![3, 4]);
                    }
                    _ => {}
                };
                run_epoch_and_check_bonds_dividends(
                    netuid,
                    *sparse,
                    target_bonds,
                    target_dividends,
                );
            }
        })
    }
}

#[test]
fn test_yuma_3_kappa_moves_last() {
    for sparse in [true, false].iter() {
        new_test_ext(1).execute_with(|| {
            let n: u16 = 5; // 3 validators, 2 servers
            let netuid = NetUid::from(1);
            let max_stake: u64 = 8;

            // Validator A: kappa / Big validator (0.8) - moves last
            // Validator B: Small eager validator (0.1) - moves first
            // Validator C: Small lazy validator (0.1) - moves second
            let stakes: Vec<u64> = vec![8, 1, 1, 0, 0];

            setup_yuma_3_scenario(netuid, n, *sparse, max_stake, stakes);
            let targets_bonds = [
                vec![
                    vec![0.1013, 0.0000],
                    vec![0.1013, 0.0000],
                    vec![0.1013, 0.0000],
                ],
                vec![
                    vec![0.1924, 0.0000],
                    vec![0.0908, 0.2987],
                    vec![0.1924, 0.0000],
                ],
                vec![
                    vec![0.2742, 0.0000],
                    vec![0.0815, 0.5081],
                    vec![0.1715, 0.2987],
                ],
                vec![
                    vec![0.2416, 0.1013],
                    vec![0.0731, 0.5580],
                    vec![0.1531, 0.3697],
                ],
                vec![
                    vec![0.2141, 0.1924],
                    vec![0.0656, 0.6028],
                    vec![0.1369, 0.4336],
                ],
                vec![
                    vec![0.1903, 0.2742],
                    vec![0.0588, 0.6430],
                    vec![0.1225, 0.4910],
                ],
            ];
            let targets_dividends = [
                vec![0.8000, 0.1000, 0.1000, 0.0000, 0.0000],
                vec![0.8446, 0.0498, 0.1056, 0.0000, 0.0000],
                vec![0.8966, 0.0333, 0.0701, 0.0000, 0.0000],
                vec![0.4663, 0.3210, 0.2127, 0.0000, 0.0000],
                vec![0.5976, 0.2340, 0.1683, 0.0000, 0.0000],
                vec![0.6592, 0.1932, 0.1475, 0.0000, 0.0000],
            ];

            for (epoch, (target_bonds, target_dividends)) in targets_bonds
                .iter()
                .zip(targets_dividends.iter())
                .enumerate()
            {
                match epoch {
                    0 => {
                        // Initially, consensus is achieved by all Validators
                        set_yuma_3_weights(netuid, vec![vec![u16::MAX, 0]; 3], vec![3, 4]);
                    }
                    1 => {
                        // Validator A -> Server 1
                        // Validator B -> Server 2
                        // Validator C -> Server 1
                        set_yuma_3_weights(
                            netuid,
                            vec![vec![u16::MAX, 0], vec![0, u16::MAX], vec![u16::MAX, 0]],
                            vec![3, 4],
                        );
                    }
                    2 => {
                        // Validator A -> Server 1
                        // Validator B -> Server 2
                        // Validator C -> Server 2
                        set_yuma_3_weights(
                            netuid,
                            vec![vec![u16::MAX, 0], vec![0, u16::MAX], vec![0, u16::MAX]],
                            vec![3, 4],
                        );
                    }
                    3 => {
                        // Subsequent epochs All validators -> Server 2
                        set_yuma_3_weights(netuid, vec![vec![0, u16::MAX]; 3], vec![3, 4]);
                    }
                    _ => {}
                };
                run_epoch_and_check_bonds_dividends(
                    netuid,
                    *sparse,
                    target_bonds,
                    target_dividends,
                );
            }
        })
    }
}

#[test]
fn test_yuma_3_one_epoch_switch() {
    for sparse in [true, false].iter() {
        new_test_ext(1).execute_with(|| {
            let n: u16 = 5; // 3 validators, 2 servers
            let netuid = NetUid::from(1);
            let max_stake: u64 = 8;

            // Equal stake validators
            let stakes: Vec<u64> = vec![33, 33, 34, 0, 0];

            setup_yuma_3_scenario(netuid, n, *sparse, max_stake, stakes);

            let targets_bonds = [
                vec![
                    vec![0.1013, 0.0000],
                    vec![0.1013, 0.0000],
                    vec![0.1013, 0.0000],
                ],
                vec![
                    vec![0.1924, 0.0000],
                    vec![0.1924, 0.0000],
                    vec![0.1924, 0.0000],
                ],
                vec![
                    vec![0.2742, 0.0000],
                    vec![0.2742, 0.0000],
                    vec![0.1715, 0.2987],
                ],
                vec![
                    vec![0.3478, 0.0000],
                    vec![0.3478, 0.0000],
                    vec![0.2554, 0.2618],
                ],
                vec![
                    vec![0.4139, 0.0000],
                    vec![0.4139, 0.0000],
                    vec![0.3309, 0.2312],
                ],
                vec![
                    vec![0.4733, 0.0000],
                    vec![0.4733, 0.0000],
                    vec![0.3987, 0.2051],
                ],
            ];
            let targets_dividends = [
                vec![0.3300, 0.3300, 0.3400, 0.0000, 0.0000],
                vec![0.3300, 0.3300, 0.3400, 0.0000, 0.0000],
                vec![0.3782, 0.3782, 0.2436, 0.0000, 0.0000],
                vec![0.3628, 0.3628, 0.2745, 0.0000, 0.0000],
                vec![0.3541, 0.3541, 0.2917, 0.0000, 0.0000],
                vec![0.3487, 0.3487, 0.3026, 0.0000, 0.0000],
            ];

            for (epoch, (target_bonds, target_dividends)) in targets_bonds
                .iter()
                .zip(targets_dividends.iter())
                .enumerate()
            {
                match epoch {
                    2 => {
                        // Validator A -> Server 1
                        // Validator B -> Server 1
                        // Validator C -> Server 2
                        set_yuma_3_weights(
                            netuid,
                            vec![vec![u16::MAX, 0], vec![u16::MAX, 0], vec![0, u16::MAX]],
                            vec![3, 4],
                        );
                    }
                    _ => {
                        // All validators -> Server 1
                        set_yuma_3_weights(netuid, vec![vec![u16::MAX, 0]; 3], vec![3, 4]);
                    }
                };
                run_epoch_and_check_bonds_dividends(
                    netuid,
                    *sparse,
                    target_bonds,
                    target_dividends,
                );
            }
        })
    }
}

#[test]
fn test_yuma_3_liquid_alpha_disabled() {
    for sparse in [true, false].iter() {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(1);
            let n: u16 = 5; // 3 validators, 2 servers
            let max_stake: u64 = 8;

            // Equal stake validators
            let stakes: Vec<u64> = vec![33, 33, 34, 0, 0];

            setup_yuma_3_scenario(netuid, n, *sparse, max_stake, stakes);

            // disable liquid alpha
            SubtensorModule::set_liquid_alpha_enabled(netuid, false);

            let targets_bonds = [
                vec![
                    vec![0.0000, 0.0250, 0.0000],
                    vec![0.0000, 0.0250, 0.0000],
                    vec![0.0000, 0.0250, 0.0000],
                ],
                vec![
                    vec![0.0000, 0.0494, 0.0000],
                    vec![0.0000, 0.0494, 0.0000],
                    vec![0.0000, 0.0494, 0.0000],
                ],
                vec![
                    vec![0.0000, 0.0731, 0.0000],
                    vec![0.0000, 0.0731, 0.0000],
                    vec![0.0000, 0.0481, 0.0250],
                ],
                vec![
                    vec![0.0000, 0.0963, 0.0000],
                    vec![0.0000, 0.0963, 0.0000],
                    vec![0.0000, 0.0719, 0.0244],
                ],
                vec![
                    vec![0.0000, 0.1189, 0.0000],
                    vec![0.0000, 0.1189, 0.0000],
                    vec![0.0000, 0.0951, 0.0238],
                ],
                vec![
                    vec![0.0000, 0.1409, 0.0000],
                    vec![0.0000, 0.1409, 0.0000],
                    vec![0.0000, 0.1178, 0.0232],
                ],
            ];
            let targets_dividends = [
                vec![0.3300, 0.3300, 0.3400, 0.0000, 0.0000],
                vec![0.3300, 0.3300, 0.3400, 0.0000, 0.0000],
                vec![0.3734, 0.3734, 0.2532, 0.0000, 0.0000],
                vec![0.3611, 0.3611, 0.2779, 0.0000, 0.0000],
                vec![0.3541, 0.3541, 0.2919, 0.0000, 0.0000],
                vec![0.3495, 0.3495, 0.3009, 0.0000, 0.0000],
            ];

            for (epoch, (target_bonds, target_dividends)) in targets_bonds
                .iter()
                .zip(targets_dividends.iter())
                .enumerate()
            {
                match epoch {
                    2 => {
                        // Validator A -> Server 1
                        // Validator B -> Server 1
                        // Validator C -> Server 2
                        set_yuma_3_weights(
                            netuid,
                            vec![vec![u16::MAX, 0], vec![u16::MAX, 0], vec![0, u16::MAX]],
                            vec![3, 4],
                        );
                    }
                    _ => {
                        // All validators -> Server 1
                        set_yuma_3_weights(netuid, vec![vec![u16::MAX, 0]; 3], vec![3, 4]);
                    }
                };
                run_epoch_and_check_bonds_dividends(
                    netuid,
                    *sparse,
                    target_bonds,
                    target_dividends,
                );
            }
        })
    }
}

#[test]
fn test_yuma_3_stable_miner() {
    for sparse in [true, false].iter() {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(1);
            let n: u16 = 6; // 3 validators, 3 servers
            let max_stake: u64 = 8;

            // Validator A: kappa / Big validator (0.8)
            // Validator B: Small eager validator (0.1)
            // Validator C: Small lazy validator (0.1)
            let stakes: Vec<u64> = vec![8, 1, 1, 0, 0, 0];

            setup_yuma_3_scenario(netuid, n, *sparse, max_stake, stakes);
            let targets_bonds = [
                vec![
                    vec![0.0507, 0.0000, 0.0507],
                    vec![0.0507, 0.0000, 0.0507],
                    vec![0.0507, 0.0000, 0.0507],
                ],
                vec![
                    vec![0.0962, 0.0000, 0.0962],
                    vec![0.0455, 0.1000, 0.0962],
                    vec![0.0962, 0.0000, 0.0962],
                ],
                vec![
                    vec![0.0863, 0.0507, 0.1371],
                    vec![0.0408, 0.1405, 0.1371],
                    vec![0.1770, 0.0000, 0.1371],
                ],
                vec![
                    vec![0.0774, 0.0962, 0.1739],
                    vec![0.0367, 0.1770, 0.1739],
                    vec![0.1579, 0.0507, 0.1739],
                ],
                vec![
                    vec![0.0694, 0.1371, 0.2069],
                    vec![0.0329, 0.2097, 0.2069],
                    vec![0.1411, 0.0962, 0.2069],
                ],
                vec![
                    vec![0.0623, 0.1739, 0.2366],
                    vec![0.0296, 0.2391, 0.2366],
                    vec![0.1263, 0.1371, 0.2366],
                ],
            ];
            let targets_dividends = [
                vec![0.8000, 0.1000, 0.1000, 0.0000, 0.0000, 0.0000],
                vec![0.8226, 0.0745, 0.1028, 0.0000, 0.0000, 0.0000],
                vec![0.7750, 0.1685, 0.0565, 0.0000, 0.0000, 0.0000],
                vec![0.7864, 0.1372, 0.0764, 0.0000, 0.0000, 0.0000],
                vec![0.7912, 0.1241, 0.0847, 0.0000, 0.0000, 0.0000],
                vec![0.7937, 0.1173, 0.0890, 0.0000, 0.0000, 0.0000],
            ];

            for (epoch, (target_bonds, target_dividends)) in targets_bonds
                .iter()
                .zip(targets_dividends.iter())
                .enumerate()
            {
                match epoch {
                    0 => {
                        // all validators 0.5 for first and third server
                        set_yuma_3_weights(
                            netuid,
                            vec![vec![u16::MAX / 2, 0, u16::MAX / 2]; 3],
                            vec![3, 4, 5],
                        );
                    }
                    1 => {
                        // one of small validators moves 0.5 to seconds server
                        set_yuma_3_weights(
                            netuid,
                            vec![
                                vec![u16::MAX / 2, 0, u16::MAX / 2],
                                vec![0, u16::MAX / 2, u16::MAX / 2],
                                vec![u16::MAX / 2, 0, u16::MAX / 2],
                            ],
                            vec![3, 4, 5],
                        );
                    }
                    2 => {
                        // big validator follows
                        set_yuma_3_weights(
                            netuid,
                            vec![
                                vec![0, u16::MAX / 2, u16::MAX / 2],
                                vec![0, u16::MAX / 2, u16::MAX / 2],
                                vec![u16::MAX / 2, 0, u16::MAX / 2],
                            ],
                            vec![3, 4, 5],
                        );
                    }
                    3 => {
                        // Subsequent epochs all validators have moves
                        set_yuma_3_weights(
                            netuid,
                            vec![vec![0, u16::MAX / 2, u16::MAX / 2]; 3],
                            vec![3, 4, 5],
                        );
                    }
                    _ => {}
                };
                run_epoch_and_check_bonds_dividends(
                    netuid,
                    *sparse,
                    target_bonds,
                    target_dividends,
                );
            }
        })
    }
}

#[test]
fn test_yuma_3_bonds_reset() {
    new_test_ext(1).execute_with(|| {
        let sparse: bool = true;
        let n: u16 = 5; // 3 validators, 2 servers
        let netuid = NetUid::from(1);
        let max_stake: u64 = 8;

        // "Case 8 - big vali moves late, then late"
        // Big dishonest lazy vali. (0.8)
        // Small eager-eager vali. (0.1)
        // Small eager-eager vali 2. (0.1)
        let stakes: Vec<u64> = vec![8, 1, 1, 0, 0];

        setup_yuma_3_scenario(netuid, n, sparse, max_stake, stakes);
        SubtensorModule::set_bonds_reset(netuid, true);

        // target bonds and dividends for specific epoch
        let targets_dividends: std::collections::HashMap<_, _> = [
            (0, vec![0.8000, 0.1000, 0.1000, 0.0000, 0.0000]),
            (1, vec![0.8944, 0.0528, 0.0528, 0.0000, 0.0000]),
            (2, vec![0.5230, 0.2385, 0.2385, 0.0000, 0.0000]),
            (19, vec![0.7919, 0.1040, 0.1040, 0.0000, 0.0000]),
            (20, vec![0.7928, 0.1036, 0.1036, 0.0000, 0.0000]),
            (21, vec![0.8467, 0.0766, 0.0766, 0.0000, 0.0000]),
            (40, vec![0.7928, 0.1036, 0.1036, 0.0000, 0.0000]),
        ]
        .into_iter()
        .collect();
        let targets_bonds: std::collections::HashMap<_, _> = [
            (
                0,
                vec![
                    vec![0.1013, 0.0000],
                    vec![0.1013, 0.0000],
                    vec![0.1013, 0.0000],
                ],
            ),
            (
                1,
                vec![
                    vec![0.1924, 0.0000],
                    vec![0.0908, 0.2987],
                    vec![0.0908, 0.2987],
                ],
            ),
            (
                2,
                vec![
                    vec![0.1715, 0.1013],
                    vec![0.0815, 0.3697],
                    vec![0.0815, 0.3697],
                ],
            ),
            (
                19,
                vec![
                    vec![0.0269, 0.8539],
                    vec![0.0131, 0.8975],
                    vec![0.0131, 0.8975],
                ],
            ),
            (
                20,
                vec![
                    vec![0.0000, 0.8687],
                    vec![0.0000, 0.9079],
                    vec![0.0000, 0.9079],
                ],
            ),
            (
                21,
                vec![
                    vec![0.0000, 0.8820],
                    vec![0.2987, 0.6386],
                    vec![0.2987, 0.6386],
                ],
            ),
            (
                40,
                vec![
                    vec![0.8687, 0.0578],
                    vec![0.9079, 0.0523],
                    vec![0.9079, 0.0523],
                ],
            ),
        ]
        .into_iter()
        .collect();

        for epoch in 0..=40 {
            match epoch {
                0 => {
                    // All validators -> Server 1
                    set_yuma_3_weights(netuid, vec![vec![u16::MAX, 0]; 3], vec![3, 4]);
                }
                1 => {
                    // validators B, C switch
                    // Validator A -> Server 1
                    // Validator B -> Server 2
                    // Validator C -> Server 2
                    set_yuma_3_weights(
                        netuid,
                        vec![vec![u16::MAX, 0], vec![0, u16::MAX], vec![0, u16::MAX]],
                        vec![3, 4],
                    );
                }
                (2..=20) => {
                    // validator A copies weights
                    // All validators -> Server 2
                    set_yuma_3_weights(netuid, vec![vec![0, u16::MAX]; 3], vec![3, 4]);
                    if epoch == 20 {
                        let hotkey = SubtensorModule::get_hotkey_for_net_and_uid(netuid, 3)
                            .expect("Hotkey not found");
                        let _ = SubtensorModule::reset_bonds_column_for_hotkey(netuid.into(), &hotkey);
                    }
                }
                21 => {
                    // validators B, C switch back
                    // Validator A -> Server 2
                    // Validator B -> Server 1
                    // Validator C -> Server 1
                    set_yuma_3_weights(
                        netuid,
                        vec![vec![0, u16::MAX], vec![u16::MAX, 0], vec![u16::MAX, 0]],
                        vec![3, 4],
                    );
                }
                _ => {
                    // validator A copies weights
                    // All validators -> Server 1
                    set_yuma_3_weights(netuid, vec![vec![u16::MAX, 0]; 3], vec![3, 4]);
                }
            };

            if let Some((target_dividend, target_bond)) =
                targets_dividends.get(&epoch).zip(targets_bonds.get(&epoch))
            {
                run_epoch_and_check_bonds_dividends(netuid, sparse, target_bond, target_dividend);
            } else {
                run_epoch(netuid, sparse);
            }
        }
    })
}
