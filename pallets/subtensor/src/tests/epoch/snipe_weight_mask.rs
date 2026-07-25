#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Commit-reveal sniped-UID incoming weight masking during epoch.

use frame_support::assert_ok;
use sp_core::U256;

use super::super::mock::*;
use crate::*;

#[test]
fn test_epoch_masks_incoming_to_sniped_uid_prevents_inheritance() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(40);
        let tempo: u16 = 10;
        let reveal: u64 = 2;

        add_network(netuid, tempo, 0);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, reveal));
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_max_allowed_uids(netuid, 3);
        SubtensorModule::set_target_registrations_per_interval(netuid, u16::MAX);

        /* Validator uid‑0 */
        let (val_hot, val_cold) = (U256::from(100), U256::from(200));
        register_ok_neuron(netuid, val_hot, val_cold, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &val_hot,
            &val_cold,
            netuid,
            10_000.into(),
        );
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);

        /* Miner uid‑1 (to be sniped later) */
        let (old_hot, old_cold) = (U256::from(101), U256::from(201));
        register_ok_neuron(netuid, old_hot, old_cold, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hot,
            &old_cold,
            netuid,
            100.into(),
        );

        /* filler uid‑2 */
        let (fill_hot, fill_cold) = (U256::from(102), U256::from(202));
        register_ok_neuron(netuid, fill_hot, fill_cold, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &fill_hot,
            &fill_cold,
            netuid,
            5_000.into(),
        );
        SubtensorModule::set_max_allowed_validators(netuid, 3);

        run_to_block(tempo as u64 * 2 + 1);

        /* commit, then move one block ahead so reg_block > commit_block */
        commit_dummy(val_hot, netuid);
        run_to_block(System::block_number() + 1);

        /* validator weights uid‑1 */
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(val_hot),
            netuid,
            vec![1],
            vec![u16::MAX],
            0
        ));
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::epoch(netuid, 1_000.into());

        /* register new miner (snipes) */
        let (new_hot, new_cold) = (U256::from(103), U256::from(203));
        register_ok_neuron(netuid, new_hot, new_cold, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &new_hot,
            &new_cold,
            netuid,
            10_000.into(),
        );
        let new_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &new_hot)
            .expect("new miner gets UID");

        run_to_block(System::block_number() + 1);

        /* validator refreshes vote (still inside window) */
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(val_hot),
            netuid,
            vec![0, new_uid],
            vec![u16::MAX / 2, u16::MAX / 2],
            0
        ));
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        SubtensorModule::epoch(netuid, 1_000.into());
        assert_eq!(SubtensorModule::get_rank_for_uid(netuid, new_uid), 0);
        assert_eq!(
            SubtensorModule::get_incentive_for_uid(netuid.into(), new_uid),
            0
        );
    });
}

#[test]
fn test_epoch_no_mask_when_commit_reveal_disabled() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(32);
        let tempo: u16 = 5;
        add_network(netuid, tempo, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);

        let (hot, cold) = (U256::from(1000), U256::from(1100));
        register_ok_neuron(netuid, hot, cold, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hot,
            &cold,
            netuid,
            1_000.into(),
        );
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);

        let (hot1, cold1) = (U256::from(1001), U256::from(1101));
        register_ok_neuron(netuid, hot1, cold1, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hot1,
            &cold1,
            netuid,
            1_000.into(),
        );

        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hot),
            netuid,
            vec![1],
            vec![u16::MAX],
            0
        ));

        for _ in 0..3 {
            SubtensorModule::epoch(netuid, 1.into());
            assert!(
                !SubtensorModule::unnormalized_weights_sparse(netuid.into())[0].is_empty(),
                "row visible when CR disabled"
            );
            run_to_block(System::block_number() + tempo as u64 + 1);
        }
    });
}

#[test]
fn test_epoch_does_not_mask_outside_window_but_masks_inside() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(50);
        let tempo: u16 = 8;
        let reveal: u16 = 2;

        add_network(netuid, tempo, 0);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, reveal as u64));
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_target_registrations_per_interval(netuid, u16::MAX);

        /* validator uid‑0 */
        let (v_hot, v_cold) = (U256::from(2000), U256::from(2100));
        register_ok_neuron(netuid, v_hot, v_cold, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &v_hot,
            &v_cold,
            netuid,
            10_000.into(),
        );
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_max_allowed_validators(netuid, 1);

        run_to_block(tempo as u64);

        /* first commit */
        commit_dummy(v_hot, netuid);

        /* UID‑1 — outside window */
        let (old_hot, old_cold) = (U256::from(2001), U256::from(2101));
        register_ok_neuron(netuid, old_hot, old_cold, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hot,
            &old_cold,
            netuid,
            1_000.into(),
        );

        /* let first commit expire for UID‑1 */
        for _ in 0..(reveal + 1) {
            run_to_block(System::block_number() + tempo as u64);
        }

        /* second commit — will mask UID‑2 & UID‑3 */
        commit_dummy(v_hot, netuid);

        /* ensure commit_block < reg_block for the new registrations */
        run_to_block(System::block_number() + 1);

        /* UID‑2, UID‑3 — inside window */
        let (mid_hot, mid_cold) = (U256::from(2002), U256::from(2102));
        register_ok_neuron(netuid, mid_hot, mid_cold, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &mid_hot,
            &mid_cold,
            netuid,
            1_000.into(),
        );

        let (new_hot, new_cold) = (U256::from(2003), U256::from(2103));
        register_ok_neuron(netuid, new_hot, new_cold, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &new_hot,
            &new_cold,
            netuid,
            1_000.into(),
        );

        run_to_block(System::block_number() + 1); // avoid out‑dated

        /* vote */
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(v_hot),
            netuid,
            vec![1, 2, 3],
            vec![u16::MAX / 3, u16::MAX / 3, u16::MAX / 3],
            0
        ));
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        SubtensorModule::epoch(netuid, 1_000.into());

        assert!(
            SubtensorModule::get_incentive_for_uid(netuid.into(), 1) > 0,
            "UID-1 (old) unmasked"
        );
        assert_eq!(
            SubtensorModule::get_incentive_for_uid(netuid.into(), 2),
            0,
            "UID-2 (inside window) masked"
        );
        assert_eq!(
            SubtensorModule::get_incentive_for_uid(netuid.into(), 3),
            0,
            "UID-3 (inside window) masked"
        );
    });
}
