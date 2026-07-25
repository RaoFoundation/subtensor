#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Active-stake filtering during epoch (validators without recent weights).

use frame_support::assert_ok;
use sp_core::U256;
use substrate_fixed::types::I32F32;
use subtensor_runtime_common::{AlphaBalance, TaoBalance};

use super::super::mock::*;
use crate::*;

// Test that epoch masks out inactive stake of validators with outdated weights beyond activity cutoff.
#[test]
fn test_active_stake() {
    new_test_ext(1).execute_with(|| {
        System::set_block_number(0);
        let sparse: bool = true;
        let n: u16 = 4;
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;
        let block_number: u64 = System::block_number();
        let stake: TaoBalance = 1.into();
        add_network_disable_commit_reveal(netuid, tempo, 0);
        SubtensorModule::set_max_allowed_uids(netuid, n);
        assert_eq!(SubtensorModule::get_max_allowed_uids(netuid), n);
        SubtensorModule::set_max_registrations_per_block(netuid, n);
        SubtensorModule::set_target_registrations_per_interval(netuid, n);
        SubtensorModule::set_min_allowed_weights(netuid, 0);

        // === Register [validator1, validator2, server1, server2]
        for key in 0..n as u64 {
            add_balance_to_coldkey_account(
                &U256::from(key),
                stake + ExistentialDeposit::get() + SubtensorModule::get_network_min_lock(),
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
        assert_eq!(SubtensorModule::get_max_allowed_uids(netuid), n);
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), n);

        // === Issue validator permits
        SubtensorModule::set_max_allowed_validators(netuid, n);
        assert_eq!(SubtensorModule::get_max_allowed_validators(netuid), n);
        SubtensorModule::epoch(netuid, 1_000_000_000.into()); // run first epoch to set allowed validators
        next_block_no_epoch(netuid); // run to next block to ensure weights are set on nodes after their registration block

        // === Set weights [val1->srv1: 0.5, val1->srv2: 0.5, val2->srv1: 0.5, val2->srv2: 0.5]
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
        let bonds = SubtensorModule::get_bonds(netuid.into());
        for uid in 0..n {
            // log::info!("\n{uid}" );
            // uid_stats(netuid, uid);
            // log::info!("bonds: {:?}", bonds[uid as usize]);
            if uid < n / 2 {
                assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, uid), 32767);
                // Note D = floor(0.5 * 65_535)
            }
            assert_eq!(
                SubtensorModule::get_emission_for_uid(netuid, uid),
                250000000.into()
            ); // Note E = 0.5 / (n/2) * 1_000_000_000 = 250_000_000
        }
        for bond in bonds.iter().take((n / 2) as usize) {
            // for on_validator in 0..(n / 2) as usize {
            for i in bond.iter().take((n / 2) as usize) {
                assert_eq!(*i, 0);
            }
            for i in bond.iter().take(n as usize).skip((n / 2) as usize) {
                assert_eq!(*i, I32F32::from_num(65_535)); // floor(0.5*(2^16-1))/(2^16-1), then max-upscale to 65_535
            }
        }
        let activity_cutoff: u64 = SubtensorModule::get_activity_cutoff(netuid) as u64;
        run_to_block_no_epoch(netuid, activity_cutoff + 2); // run to block where validator (uid 0, 1) weights become outdated

        // === Update uid 0 weights
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(U256::from(0)),
            netuid,
            ((n / 2)..n).collect(),
            vec![u16::MAX / (n / 2); (n / 2) as usize],
            0
        ));
        if sparse {
            SubtensorModule::epoch(netuid, 1_000_000_000.into());
        } else {
            SubtensorModule::epoch_dense(netuid, 1_000_000_000.into());
        }
        /*  current_block: 5002; activity_cutoff: 5000
        Last update: [5002, 1, 0, 0]; Inactive: [false, true, true, true]; Block at registration: [0, 0, 0, 0]
        S: [0.25, 0.25, 0.25, 0.25]; S (mask): [0.25, 0, 0, 0]; S (mask+norm): [1, 0, 0, 0]
        validator_permits: [true, true, true, true]; max_allowed_validators: 4; new_validator_permits: [true, true, true, true]
        W: [[(2, 0.4999923704), (3, 0.4999923704)], [(2, 0.4999923704), (3, 0.4999923704)], [], []]
        W (permit): [[(2, 0.4999923704), (3, 0.4999923704)], [(2, 0.4999923704), (3, 0.4999923704)], [], []]
        W (permit+diag): [[(2, 0.4999923704), (3, 0.4999923704)], [(2, 0.4999923704), (3, 0.4999923704)], [], []]
        W (permit+diag+outdate): [[(2, 0.4999923704), (3, 0.4999923704)], [(2, 0.4999923704), (3, 0.4999923704)], [], []]
        W (mask+norm): [[(2, 0.5), (3, 0.5)], [(2, 0.5), (3, 0.5)], [], []]
        R: [0, 0, 0.5, 0.5]
        W (threshold): [[(2, 1), (3, 1)], [(2, 1), (3, 1)], [], []]
        T: [0, 0, 1, 1]
        C: [0.006693358, 0.006693358, 0.9933076561, 0.9933076561]
        I: [0, 0, 0.5, 0.5]
        B: [[(2, 0.4999923704), (3, 0.4999923704)], [(2, 0.4999923704), (3, 0.4999923704)], [], []]
        B (outdatedmask): [[(2, 0.4999923704), (3, 0.4999923704)], [(2, 0.4999923704), (3, 0.4999923704)], [], []]
        B (mask+norm): [[(2, 0.5), (3, 0.5)], [(2, 0.5), (3, 0.5)], [], []]
        ΔB: [[(2, 0.5), (3, 0.5)], [(2, 0), (3, 0)], [], []]
        ΔB (norm): [[(2, 1), (3, 1)], [(2, 0), (3, 0)], [], []]
        emaB: [[(2, 0.55), (3, 0.55)], [(2, 0.45), (3, 0.45)], [], []]
        emaB (max-upscale): [[(2, 1), (3, 1)], [(2, 1), (3, 1)], [], []]
        D: [0.55, 0.4499999997, 0, 0]
        nE: [0.275, 0.2249999999, 0.25, 0.25]
        E: [274999999, 224999999, 250000000, 250000000]
        P: [0.275, 0.2249999999, 0.25, 0.25]
        P (u16): [65535, 53619, 59577, 59577] */
        let bonds = SubtensorModule::get_bonds(netuid.into());
        assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, 0), 36044); // Note D = floor((0.5 * 0.9 + 0.1) * 65_535)
        assert_eq!(
            SubtensorModule::get_emission_for_uid(netuid, 0),
            274999999.into()
        ); // Note E = 0.5 * 0.55 * 1_000_000_000 = 275_000_000 (discrepancy)
        for server in ((n / 2) as usize)..n as usize {
            assert_eq!(bonds[0][server], I32F32::from_num(65_535)); // floor(0.55*(2^16-1))/(2^16-1), then max-upscale
        }
        for validator in 1..(n / 2) {
            assert_eq!(
                SubtensorModule::get_dividends_for_uid(netuid, validator),
                29490
            ); // Note D = floor((0.5 * 0.9) * 65_535)
            assert_eq!(
                SubtensorModule::get_emission_for_uid(netuid, validator),
                224999999.into()
            ); // Note E = 0.5 * 0.45 * 1_000_000_000 = 225_000_000 (discrepancy)
            for server in ((n / 2) as usize)..n as usize {
                assert_eq!(bonds[validator as usize][server], I32F32::from_num(53619));
                // floor(0.45*(2^16-1))/(2^16-1), then max-upscale
            }
        }

        // === Update uid 1 weights as well
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(U256::from(1)),
            netuid,
            ((n / 2)..n).collect(),
            vec![u16::MAX / (n / 2); (n / 2) as usize],
            0
        ));
        run_to_block_no_epoch(netuid, activity_cutoff + 3); // run to block where validator (uid 0, 1) weights become outdated
        if sparse {
            SubtensorModule::epoch(netuid, 1_000_000_000.into());
        } else {
            SubtensorModule::epoch_dense(netuid, 1_000_000_000.into());
        }
        /*  current_block: 5003; activity_cutoff: 5000
        Last update: [5002, 5002, 0, 0]; Inactive: [false, false, true, true]; Block at registration: [0, 0, 0, 0]
        S: [0.25, 0.25, 0.25, 0.25]; S (mask): [0.25, 0.25, 0, 0]; S (mask+norm): [0.5, 0.5, 0, 0]
        validator_permits: [true, true, true, true]; max_allowed_validators: 4; new_validator_permits: [true, true, true, true]
        W: [[(2, 0.4999923704), (3, 0.4999923704)], [(2, 0.4999923704), (3, 0.4999923704)], [], []]
        W (permit): [[(2, 0.4999923704), (3, 0.4999923704)], [(2, 0.4999923704), (3, 0.4999923704)], [], []]
        W (permit+diag): [[(2, 0.4999923704), (3, 0.4999923704)], [(2, 0.4999923704), (3, 0.4999923704)], [], []]
        W (permit+diag+outdate): [[(2, 0.4999923704), (3, 0.4999923704)], [(2, 0.4999923704), (3, 0.4999923704)], [], []]
        W (mask+norm): [[(2, 0.5), (3, 0.5)], [(2, 0.5), (3, 0.5)], [], []]
        R: [0, 0, 0.5, 0.5]
        W (threshold): [[(2, 1), (3, 1)], [(2, 1), (3, 1)], [], []]
        T: [0, 0, 1, 1]
        C: [0.006693358, 0.006693358, 0.9933076561, 0.9933076561]
        I: [0, 0, 0.5, 0.5]
        B: [[(2, 65535), (3, 65535)], [(2, 53619), (3, 53619)], [], []]
        B (outdatedmask): [[(2, 65535), (3, 65535)], [(2, 53619), (3, 53619)], [], []]
        B (mask+norm): [[(2, 0.5500025176), (3, 0.5500025176)], [(2, 0.4499974821), (3, 0.4499974821)], [], []]
        ΔB: [[(2, 0.25), (3, 0.25)], [(2, 0.25), (3, 0.25)], [], []]
        ΔB (norm): [[(2, 0.5), (3, 0.5)], [(2, 0.5), (3, 0.5)], [], []]
        emaB: [[(2, 0.545002266), (3, 0.545002266)], [(2, 0.4549977337), (3, 0.4549977337)], [], []]
        emaB (max-upscale): [[(2, 1), (3, 1)], [(2, 0.8348547556), (3, 0.8348547556)], [], []]
        D: [0.545002266, 0.4549977337, 0, 0]
        nE: [0.272501133, 0.2274988669, 0.25, 0.25]
        E: [272501132, 227498866, 250000000, 250000000]
        P: [0.272501133, 0.2274988669, 0.25, 0.25]
        P (u16): [65535, 54711, 60123, 60123] */
        let bonds = SubtensorModule::get_bonds(netuid.into());
        assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, 0), 35716); // Note D = floor((0.55 * 0.9 + 0.5 * 0.1) * 65_535)
        assert_eq!(
            SubtensorModule::get_emission_for_uid(netuid, 0),
            272501132.into()
        ); // Note E = 0.5 * (0.55 * 0.9 + 0.5 * 0.1) * 1_000_000_000 = 272_500_000 (discrepancy)
        for server in ((n / 2) as usize)..n as usize {
            assert_eq!(bonds[0][server], I32F32::from_num(65_535)); // floor((0.55 * 0.9 + 0.5 * 0.1)*(2^16-1))/(2^16-1), then max-upscale
        }
        assert_eq!(SubtensorModule::get_dividends_for_uid(netuid, 1), 29818); // Note D = floor((0.45 * 0.9 + 0.5 * 0.1) * 65_535)
        assert_eq!(
            SubtensorModule::get_emission_for_uid(netuid, 1),
            227498866.into()
        ); // Note E = 0.5 * (0.45 * 0.9 + 0.5 * 0.1) * 1_000_000_000 = 227_500_000 (discrepancy)
        for server in ((n / 2) as usize)..n as usize {
            assert_eq!(bonds[1][server], I32F32::from_num(54712)); // floor((0.45 * 0.9 + 0.5 * 0.1)/(0.55 * 0.9 + 0.5 * 0.1)*(2^16-1))
        }
    });
}

// Test that epoch masks out outdated weights and bonds of validators on deregistered servers.
//
