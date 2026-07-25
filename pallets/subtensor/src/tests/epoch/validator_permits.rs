#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Max-allowed-validators / permit issuance via epoch.

use frame_support::assert_ok;
use sp_core::U256;
use subtensor_runtime_common::TaoBalance;

use super::super::mock::*;
use super::helpers::distribute_nodes;
use crate::*;

// Test that epoch assigns validator permits to highest stake uids that are over the stake threshold, varies uid interleaving and stake values.
#[test]
fn test_validator_permits() {
    let netuid = NetUid::from(1);
    let tempo: u16 = u16::MAX - 1; // high tempo to skip automatic epochs in on_initialize, use manual epochs instead
    for interleave in 0..3 {
        for (network_n, validators_n) in [(2, 1), (4, 2), (8, 4)] {
            let min_stake = validators_n as u64;
            for assignment in 0..=1 {
                let (validators, servers) =
                    distribute_nodes(validators_n as usize, network_n, interleave as usize);
                let correct: bool = true;
                let mut stake: Vec<TaoBalance> = vec![0.into(); network_n];
                for validator in &validators {
                    stake[*validator as usize] = match assignment {
                        1 => TaoBalance::from(*validator) + network_n.into(),
                        _ => 1.into(),
                    };
                }
                for server in &servers {
                    stake[*server as usize] = match assignment {
                        1 => TaoBalance::from(*server),
                        _ => 0.into(),
                    };
                }
                new_test_ext(1).execute_with(|| {
                    let block_number: u64 = 0;
                    add_network(netuid, tempo, 0);
                    SubtensorModule::set_max_allowed_uids(netuid, network_n as u16);
                    assert_eq!(
                        SubtensorModule::get_max_allowed_uids(netuid),
                        network_n as u16
                    );
                    SubtensorModule::set_max_registrations_per_block(netuid, network_n as u16);
                    SubtensorModule::set_target_registrations_per_interval(
                        netuid,
                        network_n as u16,
                    );
                    SubtensorModule::set_stake_threshold(min_stake);

                    // === Register [validator1, validator2, server1, server2]
                    for key in 0..network_n as u64 {
                        add_balance_to_coldkey_account(
                            &U256::from(key),
                            stake[key as usize]
                                + ExistentialDeposit::get()
                                + SubtensorModule::get_network_min_lock(),
                        );
                        let (nonce, work): (u64, Vec<u8>) =
                            SubtensorModule::create_work_for_block_number(
                                netuid,
                                block_number,
                                key * 1_000_000,
                                &U256::from(key),
                            );
                        assert_ok!(SubtensorModule::register(
                            RuntimeOrigin::signed(U256::from(key)),
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
                            stake[key as usize].to_u64().into(),
                        );
                    }
                    assert_eq!(SubtensorModule::get_subnetwork_n(netuid), network_n as u16);

                    // === Issue validator permits
                    SubtensorModule::set_max_allowed_validators(netuid, validators_n as u16);
                    assert_eq!(
                        SubtensorModule::get_max_allowed_validators(netuid),
                        validators_n as u16
                    );
                    SubtensorModule::epoch(netuid, 1_000_000_000.into()); // run first epoch to set allowed validators
                    for validator in &validators {
                        assert_eq!(
                            stake[*validator as usize] >= TaoBalance::from(min_stake),
                            SubtensorModule::get_validator_permit_for_uid(netuid, *validator)
                        );
                    }
                    for server in &servers {
                        assert_eq!(
                            !correct,
                            SubtensorModule::get_validator_permit_for_uid(netuid, *server)
                        );
                    }

                    // === Increase server stake above validators
                    for server in &servers {
                        add_balance_to_coldkey_account(
                            &(U256::from(*server as u64)),
                            (2 * network_n as u64).into(),
                        );
                        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                            &(U256::from(*server as u64)),
                            &(U256::from(*server as u64)),
                            netuid,
                            (2 * network_n as u64).into(),
                        );
                    }

                    // === Update validator permits
                    run_to_block(1);
                    SubtensorModule::epoch(netuid, 1_000_000_000.into());

                    // === Check that servers now own permits instead of the validator uids
                    for validator in &validators {
                        assert_eq!(
                            !correct,
                            SubtensorModule::get_validator_permit_for_uid(netuid, *validator)
                        );
                    }
                    for server in &servers {
                        assert_eq!(
                            (stake[*server as usize]
                                + (TaoBalance::from(2) * TaoBalance::from(network_n)))
                                >= TaoBalance::from(min_stake),
                            SubtensorModule::get_validator_permit_for_uid(netuid, *server)
                        );
                    }
                });
            }
        }
    }
}
