#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Epoch input consistency (`is_epoch_input_state_consistent`) and LastUpdate size mismatch.

use frame_support::assert_ok;
use sp_core::U256;
use subtensor_runtime_common::{NetUidStorageIndex, TaoBalance};

use super::super::mock::*;
use crate::*;

// Test an epoch doesn't panic when LastUpdate size doesn't match to Weights size.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::epoch::epoch_input_state::test_last_update_size_mismatch --exact --show-output --nocapture
#[test]
fn test_last_update_size_mismatch() {
    new_test_ext(1).execute_with(|| {
        log::info!("test_1_graph:");
        let netuid = NetUid::from(1);
        let coldkey = U256::from(0);
        let hotkey = U256::from(0);
        let uid: u16 = 0;
        let stake_amount: u64 = 1_000_000_000;
        add_network_disable_commit_reveal(netuid, u16::MAX - 1, 0);
        SubtensorModule::set_max_allowed_uids(netuid, 1);
        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(stake_amount)
                + ExistentialDeposit::get()
                + (SubtensorModule::get_network_min_lock() * 2.into()),
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

        // Set mismatching LastUpdate vector
        LastUpdate::<Test>::insert(NetUidStorageIndex::from(netuid), vec![1, 1, 1]);

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

#[test]
fn empty_ok() {
    new_test_ext(1).execute_with(|| {
        let netuid: NetUid = 155.into();
        assert!(Pallet::<Test>::is_epoch_input_state_consistent(netuid));
    });
}

#[test]
fn unique_hotkeys_and_uids_ok() {
    new_test_ext(1).execute_with(|| {
        let netuid: NetUid = 155.into();

        // (netuid, uid) -> hotkey (AccountId = U256)
        Keys::<Test>::insert(netuid, 0u16, U256::from(1u64));
        Keys::<Test>::insert(netuid, 1u16, U256::from(2u64));
        Keys::<Test>::insert(netuid, 2u16, U256::from(3u64));

        assert!(Pallet::<Test>::is_epoch_input_state_consistent(netuid));
    });
}

#[test]
fn duplicate_hotkey_within_same_netuid_fails() {
    new_test_ext(1).execute_with(|| {
        let netuid: NetUid = 155.into();

        // Same hotkey mapped from two different UIDs in the SAME netuid
        let hk = U256::from(42u64);
        Keys::<Test>::insert(netuid, 0u16, hk);
        Keys::<Test>::insert(netuid, 1u16, U256::from(42u64)); // duplicate hotkey

        assert!(!Pallet::<Test>::is_epoch_input_state_consistent(netuid));
    });
}

#[test]
fn same_hotkey_across_different_netuids_is_ok() {
    new_test_ext(1).execute_with(|| {
        let net_a: NetUid = 10.into();
        let net_b: NetUid = 11.into();

        // Same hotkey appears once in each netuid — each net checks independently.
        let hk = U256::from(777u64);
        Keys::<Test>::insert(net_a, 0u16, hk);
        Keys::<Test>::insert(net_b, 0u16, hk);

        assert!(Pallet::<Test>::is_epoch_input_state_consistent(net_a));
        assert!(Pallet::<Test>::is_epoch_input_state_consistent(net_b));
    });
}
