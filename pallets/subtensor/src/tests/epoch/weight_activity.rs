#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Outdated and zero weight rows and their effect on epoch outputs.

use frame_support::assert_ok;
use sp_core::U256;
use substrate_fixed::types::I32F32;
use subtensor_runtime_common::{AlphaBalance, TaoBalance};

use super::super::mock::*;
use crate::*;

#[test]
fn test_outdated_weights() {
    new_test_ext(1).execute_with(|| {
        let sparse: bool = true;
        let n: u16 = 4;
        let netuid = NetUid::from(1);
        let tempo: u16 = 0;
        let mut block_number: u64 = System::block_number();
        let stake: TaoBalance = 1.into();
        add_network_disable_commit_reveal(netuid, tempo, 0);
        SubtensorModule::set_max_allowed_uids(netuid, n);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_max_registrations_per_block(netuid, n);
        SubtensorModule::set_target_registrations_per_interval(netuid, n);
        SubtensorModule::set_min_allowed_weights(netuid, 0);
        SubtensorModule::set_bonds_penalty(netuid, u16::MAX);
        assert_eq!(SubtensorModule::get_registrations_this_block(netuid), 0);

        // === Register [validator1, validator2, server1, server2]
        for key in 0..n as u64 {
            add_balance_to_coldkey_account(
                &U256::from(key),
                stake
                    + ExistentialDeposit::get()
                    + (SubtensorModule::get_network_min_lock() * 2.into()),
            );
            let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
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
                AlphaBalance::from(stake.to_u64()),
            );
        }
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), n);
        assert_eq!(SubtensorModule::get_registrations_this_block(netuid), 4);

        // === Issue validator permits
        SubtensorModule::set_max_allowed_validators(netuid, n);
        assert_eq!(SubtensorModule::get_max_allowed_validators(netuid), n);
        SubtensorModule::epoch(netuid, 1_000_000_000.into()); // run first epoch to set allowed validators
        assert_eq!(SubtensorModule::get_registrations_this_block(netuid), 4);
        block_number = next_block_no_epoch(netuid); // run to next block to ensure weights are set on nodes after their registration block
        assert_eq!(SubtensorModule::get_registrations_this_block(netuid), 0);

        // === Set weights [val1->srv1: 2/3, val1->srv2: 1/3, val2->srv1: 2/3, val2->srv2: 1/3, srv1->srv1: 1, srv2->srv2: 1]
        for uid in 0..(n / 2) as u64 {
            assert_ok!(SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::from(uid)),
                netuid,
                ((n / 2)..n).collect(),
                vec![2 * (u16::MAX / 3), u16::MAX / 3],
                0
            ));
        }
        for uid in ((n / 2) as u64)..n as u64 {
            assert_ok!(SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::from(uid)),
                netuid,
                vec![uid as u16],
                vec![u16::MAX],
                0
            )); // server self-weight
        }
        if sparse {
            SubtensorModule::epoch(netuid, 1_000_000_000.into());
        } else {
            SubtensorModule::epoch_dense(netuid, 1_000_000_000.into());
        }
        /*  current_block: 1; activity_cutoff: 5000
        Last update: [1, 1, 1, 1]; Inactive: [false, false, false, false]; Block at registration: [0, 0, 0, 0]
        S: [0.25, 0.25, 0.25, 0.25]; S (mask): [0.25, 0.25, 0.25, 0.25]; S (mask+norm): [0.25, 0.25, 0.25, 0.25]
        validator_permits: [true, true, true, true]; max_allowed_validators: 4; new_validator_permits: [true, true, true, true]
        W: [[(2, 65535), (3, 32768)], [(2, 65535), (3, 32768)], [(2, 65535)], [(3, 65535)]]
        W (permit): [[(2, 65535), (3, 32768)], [(2, 65535), (3, 32768)], [(2, 65535)], [(3, 65535)]]
        W (permit+diag): [[(2, 65535), (3, 32768)], [(2, 65535), (3, 32768)], [], []]
        W (permit+diag+outdate): [[(2, 65535), (3, 32768)], [(2, 65535), (3, 32768)], [], []]
        W (mask+norm): [[(2, 0.6666632756), (3, 0.3333367242)], [(2, 0.6666632756), (3, 0.3333367242)], [], []]
        R (before): [0, 0, 0.3333316376, 0.166668362]
        C: [0, 0, 0.6666632756, 0.3333367242]
        W: [[(2, 0.6666632756), (3, 0.3333367242)], [(2, 0.6666632756), (3, 0.3333367242)], [], []]
        Tv: [0.9999999998, 0.9999999998, 0, 0]
        R (after): [0, 0, 0.3333316376, 0.166668362]
        T: [0, 0, 1, 1]
        I (=R): [0, 0, 0.6666632756, 0.3333367242]
        B: [[], [], [], []]
        B (outdatedmask): [[], [], [], []]
        B (mask+norm): [[], [], [], []]
        ΔB: [[(2, 0.1666658188), (3, 0.083334181)], [(2, 0.1666658188), (3, 0.083334181)], [], []]
        ΔB (norm): [[(2, 0.5), (3, 0.5)], [(2, 0.5), (3, 0.5)], [], []]
        emaB: [[(2, 0.5), (3, 0.5)], [(2, 0.5), (3, 0.5)], [], []]
        D: [0.5, 0.5, 0, 0]
        nE: [0.25, 0.25, 0.3333316378, 0.166668362]
        E: [250000000, 250000000, 333331637, 166668361]
        P: [0.25, 0.25, 0.3333316378, 0.166668362]
        P (u16): [49151, 49151, 65535, 32767] */

        // === Dereg server2 at uid3 (least emission) + register new key over uid3
        let new_key: u64 = n as u64; // register a new key while at max capacity, which means the least incentive uid will be deregistered
        add_balance_to_coldkey_account(
            &U256::from(new_key),
            stake
                + ExistentialDeposit::get()
                + (SubtensorModule::get_network_min_lock() * 2.into()),
        );
        let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
            netuid,
            block_number,
            0,
            &U256::from(new_key),
        );
        assert_eq!(System::block_number(), block_number);
        assert_eq!(SubtensorModule::get_max_registrations_per_block(netuid), n);
        assert_eq!(SubtensorModule::get_registrations_this_block(netuid), 0);
        assert_ok!(SubtensorModule::register(
            RuntimeOrigin::signed(U256::from(new_key)),
            netuid,
            block_number,
            nonce,
            work,
            U256::from(new_key),
            U256::from(new_key)
        ));
        let deregistered_uid: u16 = n - 1; // since uid=n-1 only recieved 1/3 of weight, it will get pruned first
        assert_eq!(
            U256::from(new_key),
            SubtensorModule::get_hotkey_for_net_and_uid(netuid, deregistered_uid)
                .expect("Not registered")
        );
        next_block_no_epoch(netuid); // run to next block to outdate weights and bonds set on deregistered uid

        // === Update weights from only uid=0
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(U256::from(0)),
            netuid,
            ((n / 2)..n).collect(),
            vec![2 * (u16::MAX / 3), u16::MAX / 3],
            0
        ));
        if sparse {
            SubtensorModule::epoch(netuid, 1_000_000_000.into());
        } else {
            SubtensorModule::epoch_dense(netuid, 1_000_000_000.into());
        }
        /*  current_block: 2; activity_cutoff: 5000
        Last update: [2, 1, 1, 1]; Inactive: [false, false, false, false]; Block at registration: [0, 0, 0, 1]
        S: [0.3333333333, 0.3333333333, 0.3333333333, 0]
        S (mask): [0.3333333333, 0.3333333333, 0.3333333333, 0]
        S (mask+norm): [0.3333333333, 0.3333333333, 0.3333333333, 0]
        validator_permits: [true, true, true, false]; max_allowed_validators: 4; new_validator_permits: [true, true, true, true]
        W: [[(2, 65535), (3, 32768)], [(2, 65535), (3, 32768)], [(2, 65535)], [(3, 65535)]]
        W (permit): [[(2, 65535), (3, 32768)], [(2, 65535), (3, 32768)], [(2, 65535)], [(3, 65535)]]
        W (permit+diag): [[(2, 65535), (3, 32768)], [(2, 65535), (3, 32768)], [], []]
        W (permit+diag+outdate): [[(2, 65535), (3, 32768)], [(2, 65535)], [], []]
        W (mask+norm): [[(2, 0.6666632756), (3, 0.3333367242)], [(2, 1)], [], []]
        R (before): [0, 0, 0.5555544249, 0.1111122412]
        C: [0, 0, 0.6666632756, 0]
        W: [[(2, 0.6666632756)], [(2, 0.6666632756)], [], []]
        Tv: [0.6666632756, 0.6666632756, 0, 0]
        R (after): [0, 0, 0.4444421832, 0]
        T: [0, 0, 0.799997558, 0]
        I (=R): [0, 0, 1, 0]
        B: [[(2, 65535), (3, 65535)], [(2, 65535), (3, 65535)], [], []]
        B (outdatedmask): [[(2, 65535), (3, 65535)], [(2, 65535)], [], []]
        B (mask+norm): [[(2, 0.5), (3, 1)], [(2, 0.5)], [], []]
        ΔB: [[(2, 0.2222210916)], [(2, 0.2222210916)], [], []]
        ΔB (norm): [[(2, 0.5)], [(2, 0.5)], [], []]
        emaB: [[(2, 0.5), (3, 1)], [(2, 0.5)], [], []]
        emaB (max-upscale): [[(2, 1), (3, 1)], [(2, 1)], [], []]
        D: [0.5, 0.5, 0, 0]
        nE: [0.25, 0.25, 0.5, 0]
        E: [250000000, 250000000, 500000000, 0]
        P: [0.25, 0.25, 0.5, 0]
        P (u16): [32767, 32767, 65535, 0] */
        let bonds = SubtensorModule::get_bonds(netuid.into());
        assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, 0), 32767); // Note D = floor(0.5 * 65_535)
        assert_eq!(
            SubtensorModule::get_emission_for_uid(netuid, 0),
            250000000.into()
        ); // Note E = 0.5 * 0.5 * 1_000_000_000 = 249311245
        assert_eq!(bonds[0][2], I32F32::from_num(65_535)); // floor(0.5*(2^16-1))/(2^16-1), then max-upscale
        assert_eq!(bonds[0][3], I32F32::from_num(65_535)); // only uid0 has updated weights for new reg
    });
}

/// Test the zero emission handling and fallback under zero effective weight conditions, to ensure non-zero effective emission.
#[test]
fn test_zero_weights() {
    new_test_ext(1).execute_with(|| {
        let sparse: bool = true;
        let n: u16 = 2;
        let netuid = NetUid::from(1);
        let tempo: u16 = u16::MAX - 1; // high tempo to skip automatic epochs in on_initialize, use manual epochs instead
        let mut block_number: u64 = 0;
        let stake: u64 = 1;
        add_network_disable_commit_reveal(netuid, tempo, 0);
        SubtensorModule::set_max_allowed_uids(netuid, n);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_max_registrations_per_block(netuid, n);
        SubtensorModule::set_target_registrations_per_interval(netuid, n);
        SubtensorModule::set_min_allowed_weights(netuid, 0);

        // === Register [validator, server]
        for key in 0..n as u64 {
            add_balance_to_coldkey_account(
                &U256::from(key),
                ExistentialDeposit::get() + (SubtensorModule::get_network_min_lock() * 2.into()),
            );
            let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
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
        }
        for validator in 0..(n / 2) as u64 {
            add_balance_to_coldkey_account(&U256::from(validator), stake.into());
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &U256::from(validator),
                &U256::from(validator),
                netuid,
                stake.into(),
            );
        }
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), n);

        // === No weights
        if sparse {
            SubtensorModule::epoch(netuid, 1_000_000_000.into());
        } else {
            SubtensorModule::epoch_dense(netuid, 1_000_000_000.into());
        }
        /*	current_block: 0; activity_cutoff: 5000; Last update: [0, 0]; Inactive: [false, false]
        S: [1, 0]; S (mask): [1, 0]; S (mask+norm): [1, 0]; Block at registration: [0, 0]
        W: [[], []]; W (diagmask): [[], []]; W (diag+outdatemask): [[], []]; W (mask+norm): [[], []]
        R: [0, 0]; W (threshold): [[], []]; T: [0, 0]; C: [0.006693358, 0.006693358]; I: [0, 0]
        B: [[], []]; B (mask+norm): [[], []];
        ΔB: [[], []]; ΔB (norm): [[], []]; emaB: [[], []]; D: [0, 0]
        E: [1000000000, 0]; P: [1, 0] */
        for validator in 0..(n / 2) {
            assert_eq!(
                SubtensorModule::get_emission_for_uid(netuid, validator),
                1000000000.into()
            ); // Note E = 1 * 1_000_000_000
        }
        for server in (n / 2)..n {
            assert_eq!(
                SubtensorModule::get_emission_for_uid(netuid, server),
                0.into()
            );
            // no stake
        }
        run_to_block(1);
        block_number += 1; // run to next block to ensure weights are set on nodes after their registration block

        // === Self-weights only: set weights [srv->srv: 1]
        for uid in ((n / 2) as u64)..n as u64 {
            assert_ok!(SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::from(uid)),
                netuid,
                vec![uid as u16],
                vec![u16::MAX],
                0
            )); // server self-weight
        }
        if sparse {
            SubtensorModule::epoch(netuid, 1_000_000_000.into());
        } else {
            SubtensorModule::epoch_dense(netuid, 1_000_000_000.into());
        }
        /*	current_block: 1; activity_cutoff: 5000; Last update: [0, 1]; Inactive: [false, false]
        S: [1, 0]; S (mask): [1, 0]; S (mask+norm): [1, 0]; Block at registration: [0, 0]
        W: [[], [(1, 1)]]
        W (diagmask): [[], []]; W (diag+outdatemask): [[], []]; W (mask+norm): [[], []]
        R: [0, 0]; W (threshold): [[], []]; T: [0, 0]; C: [0.006693358, 0.006693358]; I: [0, 0]
        B: [[], []]: B (mask+norm): [[], []]
        ΔB: [[], []]; ΔB (norm): [[], []]; emaB: [[], []]; D: [0, 0]
        E: [1000000000, 0]; P: [1, 0] */
        for validator in 0..(n / 2) {
            assert_eq!(
                SubtensorModule::get_emission_for_uid(netuid, validator),
                1000000000.into()
            ); // Note E = 1 * 1_000_000_000
        }
        for server in (n / 2)..n {
            assert_eq!(
                SubtensorModule::get_emission_for_uid(netuid, server),
                0.into()
            );
            // no stake
        }
        run_to_block(2);
        block_number += 1;

        // === Set weights [val->srv: 1/(n/2)]
        for uid in 0..(n / 2) as u64 {
            assert_ok!(SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::from(uid)),
                netuid,
                ((n / 2)..n).collect(),
                vec![u16::MAX / (n / 2); (n / 2) as usize],
                0
            ));
        }

        // === Outdate weights by reregistering servers
        for new_key in n..n + (n / 2) {
            // register a new key while at max capacity, which means the least emission uid will be deregistered
            add_balance_to_coldkey_account(
                &U256::from(new_key),
                ExistentialDeposit::get() + (SubtensorModule::get_network_min_lock() * 2.into()),
            );
            let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
                netuid,
                block_number,
                new_key as u64 * 1_000_000,
                &(U256::from(new_key)),
            );
            assert_ok!(SubtensorModule::register(
                RuntimeOrigin::signed(U256::from(new_key)),
                netuid,
                block_number,
                nonce,
                work,
                U256::from(new_key),
                U256::from(new_key)
            ));
        }
        if sparse {
            SubtensorModule::epoch(netuid, 1_000_000_000.into());
        } else {
            SubtensorModule::epoch_dense(netuid, 1_000_000_000.into());
        }
        /*	current_block: 2; activity_cutoff: 5000; Last update: [2, 1]; Inactive: [false, false];
        S: [1, 0]; S (mask): [1, 0]; S (mask+norm): [1, 0]; Block at registration: [0, 2];
        W: [[(1, 1)], []]; W (diagmask): [[(1, 1)], []]; W (diag+outdatemask): [[], []]; W (mask+norm): [[], []];
        R: [0, 0]; W (threshold): [[], []]; T: [0, 0]; C: [0.006693358, 0.006693358]; I: [0, 0];
        B: [[], []]; B (mask+norm): [[], []];
        ΔB: [[], []]; ΔB (norm): [[], []]; emaB: [[], []]; D: [0, 0];
        E: [1000000000, 0]; P: [1, 0] */
        for validator in 0..(n / 2) {
            assert_eq!(
                SubtensorModule::get_emission_for_uid(netuid, validator),
                1000000000.into()
            ); // Note E = 1 * 1_000_000_000
        }
        for server in (n / 2)..n {
            assert_eq!(
                SubtensorModule::get_emission_for_uid(netuid, server),
                0.into()
            );
            // no stake
        }
        run_to_block(3);

        // === Set new weights [val->srv: 1/(n/2)] to check that updated weights would produce non-zero incentive
        for uid in 0..(n / 2) as u64 {
            assert_ok!(SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::from(uid)),
                netuid,
                ((n / 2)..n).collect(),
                vec![u16::MAX / (n / 2); (n / 2) as usize],
                0
            ));
        }
        if sparse {
            SubtensorModule::epoch(netuid, 1_000_000_000.into());
        } else {
            SubtensorModule::epoch_dense(netuid, 1_000_000_000.into());
        }
        /*	current_block: 3; activity_cutoff: 5000; Last update: [3, 1]; Inactive: [false, false];
        S: [1, 0]; S (mask): [1, 0]; S (mask+norm): [1, 0]; Block at registration: [0, 2];
        W: [[(1, 1)], []]; W (diagmask): [[(1, 1)], []]; W (diag+outdatemask): [[(1, 1)], []]; W (mask+norm): [[(1, 1)], []];
        R: [0, 1]; W (threshold): [[(1, 1)], []]; T: [0, 1]; C: [0.006693358, 0.9933076561]; I: [0, 1];
        B: [[], []]; B (mask+norm): [[], []];
        ΔB: [[(1, 1)], []]; ΔB (norm): [[(1, 1)], []]; emaB: [[(1, 1)], []]; D: [1, 0]; emaB (max-upscale): [[(1, 1)], []]
        E: [500000000, 500000000]; P: [0.5, 0.5] */
        for validator in 0..n {
            assert_eq!(
                SubtensorModule::get_emission_for_uid(netuid, validator),
                (1000000000 / (n as u64)).into()
            ); // Note E = 1/2 * 1_000_000_000
        }
    });
}
