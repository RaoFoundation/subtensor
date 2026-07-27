#![allow(unused, clippy::indexing_slicing, clippy::panic, clippy::unwrap_used)]

use approx::assert_abs_diff_eq;
use codec::Encode;
use frame_support::weights::Weight;
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::{Config, RawOrigin};
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance, Token};

use super::super::mock::*;
use crate::*;
use share_pool::SafeFloat;
use sp_core::{Get, H160, H256, U256};
use sp_runtime::{PerU16, SaturatedConversion};
use std::collections::BTreeSet;
use substrate_fixed::types::{I96F32, U64F64};

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_stake_is_not_lost --exact --nocapture
#[test]
fn test_revert_hotkey_swap_stake_is_not_lost() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let tempo: u16 = 13;
        let hk1 = U256::from(1);
        let hk2 = U256::from(2);
        let coldkey = U256::from(3);
        let swap_cost = 1_000_000_000u64 * 2;
        let stake2 = 1_000_000_000u64;

        // Setup
        add_network(netuid, tempo, 0);
        add_network(netuid2, tempo, 0);
        register_ok_neuron(netuid, hk1, coldkey, 0);
        register_ok_neuron(netuid2, hk1, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, swap_cost.into());

        let hk1_stake_before_increase =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid);
        assert!(
            hk1_stake_before_increase == 0.into(),
            "hk1 should have empty stake"
        );

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey,
            netuid,
            1_000_000_000u64.into(),
        );

        let hk1_stake_before_swap =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid);
        assert!(
            hk1_stake_before_swap == 1_000_000_000.into(),
            "hk1 should have stake before swap"
        );

        step_block(20);

        assert_ok!(SubtensorModule::perform_hotkey_swap(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey,
            netuid,
            stake2.into(),
        );

        step_block(20);

        let hk2_stake_before_revert =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk2, &coldkey, netuid);
        let hk1_stake_before_revert =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid);

        assert_eq!(hk1_stake_before_revert, stake2.into());

        // Revert: hk2 -> hk1
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        let hk1_stake_after_revert =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid);
        let hk2_stake_after_revert =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk2, &coldkey, netuid);

        assert_eq!(
            hk1_stake_after_revert,
            hk2_stake_before_revert + stake2.into(),
        );

        // hk2 should be empty
        assert_eq!(
            hk2_stake_after_revert,
            0.into(),
            "hk2 should have no stake after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap --exact --nocapture
// This test confirms, that the old hotkey can be reverted after the hotkey swap
#[test]
fn test_revert_hotkey_swap() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let tempo: u16 = 13;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let swap_cost = 1_000_000_000u64 * 2;

        // Setup initial state
        add_network(netuid, tempo, 0);
        add_network(netuid2, tempo, 0);
        register_ok_neuron(netuid, old_hotkey, coldkey, 0);
        register_ok_neuron(netuid2, old_hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, swap_cost.into());
        step_block(20);

        // Perform the first swap (only on netuid)
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert!(SubtensorModule::is_hotkey_registered_on_any_network(
            &old_hotkey
        ));

        step_block(20);

        assert_ok!(SubtensorModule::perform_hotkey_swap(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &new_hotkey,
            &old_hotkey,
            Some(netuid),
            false
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_parent_hotkey_childkey_maps --exact --nocapture
#[test]
fn test_revert_hotkey_swap_parent_hotkey_childkey_maps() {
    new_test_ext(1).execute_with(|| {
        let hk1 = U256::from(1);
        let coldkey = U256::from(2);
        let child = U256::from(3);
        let child_other = U256::from(4);
        let hk2 = U256::from(5);

        let netuid = add_dynamic_network(&hk1, &coldkey);
        let netuid2 = add_dynamic_network(&hk1, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        SubtensorModule::create_account_if_non_existent(&coldkey, &hk1);

        mock_set_children(&coldkey, &hk1, netuid, &[(u64::MAX, child)]);
        step_rate_limit(&TransactionType::SetChildren, netuid);
        mock_schedule_children(&coldkey, &hk1, netuid, &[(u64::MAX, child_other)]);

        assert_eq!(
            ParentKeys::<Test>::get(child, netuid),
            vec![(u64::MAX, hk1)]
        );
        assert_eq!(ChildKeys::<Test>::get(hk1, netuid), vec![(u64::MAX, child)]);
        let existing_pending_child_keys = PendingChildKeys::<Test>::get(netuid, hk1);
        assert_eq!(existing_pending_child_keys.0, vec![(u64::MAX, child_other)]);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(
            ParentKeys::<Test>::get(child, netuid),
            vec![(u64::MAX, hk2)]
        );
        assert_eq!(ChildKeys::<Test>::get(hk2, netuid), vec![(u64::MAX, child)]);
        assert_eq!(
            PendingChildKeys::<Test>::get(netuid, hk2),
            existing_pending_child_keys
        );
        assert!(ChildKeys::<Test>::get(hk1, netuid).is_empty());
        assert!(PendingChildKeys::<Test>::get(netuid, hk1).0.is_empty());

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            ParentKeys::<Test>::get(child, netuid),
            vec![(u64::MAX, hk1)],
            "ParentKeys must point back to hk1 after revert"
        );
        assert_eq!(
            ChildKeys::<Test>::get(hk1, netuid),
            vec![(u64::MAX, child)],
            "ChildKeys must be restored to hk1 after revert"
        );
        assert_eq!(
            PendingChildKeys::<Test>::get(netuid, hk1),
            existing_pending_child_keys,
            "PendingChildKeys must be restored to hk1 after revert"
        );

        assert!(
            ChildKeys::<Test>::get(hk2, netuid).is_empty(),
            "hk2 must have no ChildKeys after revert"
        );
        assert!(
            PendingChildKeys::<Test>::get(netuid, hk2).0.is_empty(),
            "hk2 must have no PendingChildKeys after revert"
        );
    })
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_uids_and_keys --exact --nocapture
#[test]
fn test_revert_hotkey_swap_uids_and_keys() {
    new_test_ext(1).execute_with(|| {
        let uid = 5u16;
        let hk1 = U256::from(1);
        let hk2 = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&hk1, &coldkey);
        let netuid2 = add_dynamic_network(&hk1, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        IsNetworkMember::<Test>::insert(hk1, netuid, true);
        Uids::<Test>::insert(netuid, hk1, uid);
        Keys::<Test>::insert(netuid, uid, hk1);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(Uids::<Test>::get(netuid, hk1), None);
        assert_eq!(Uids::<Test>::get(netuid, hk2), Some(uid));
        assert_eq!(Keys::<Test>::get(netuid, uid), hk2);

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            Uids::<Test>::get(netuid, hk2),
            None,
            "hk2 must have no uid after revert"
        );
        assert_eq!(
            Uids::<Test>::get(netuid, hk1),
            Some(uid),
            "hk1 must have its uid restored after revert"
        );
        assert_eq!(
            Keys::<Test>::get(netuid, uid),
            hk1,
            "Keys must point back to hk1 after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_auto_stake_destination --exact --nocapture
#[test]
fn test_revert_hotkey_swap_auto_stake_destination() {
    new_test_ext(1).execute_with(|| {
        let hk1 = U256::from(1);
        let hk2 = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(2u16);
        let netuid2 = NetUid::from(3u16);
        let staker1 = U256::from(4);
        let staker2 = U256::from(5);
        let coldkeys = vec![staker1, staker2, coldkey];

        add_network(netuid, 1, 0);
        add_network(netuid2, 1, 0);
        register_ok_neuron(netuid, hk1, coldkey, 0);
        register_ok_neuron(netuid2, hk1, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        AutoStakeDestinationColdkeys::<Test>::insert(hk1, netuid, coldkeys.clone());
        AutoStakeDestination::<Test>::insert(coldkey, netuid, hk1);
        AutoStakeDestination::<Test>::insert(staker1, netuid, hk1);
        AutoStakeDestination::<Test>::insert(staker2, netuid, hk1);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(hk2, netuid),
            coldkeys
        );
        assert!(AutoStakeDestinationColdkeys::<Test>::get(hk1, netuid).is_empty());
        assert_eq!(
            AutoStakeDestination::<Test>::get(coldkey, netuid),
            Some(hk2)
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get(staker1, netuid),
            Some(hk2)
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get(staker2, netuid),
            Some(hk2)
        );

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(hk1, netuid),
            coldkeys,
            "AutoStakeDestinationColdkeys must be restored to hk1 after revert"
        );
        assert!(
            AutoStakeDestinationColdkeys::<Test>::get(hk2, netuid).is_empty(),
            "hk2 must have no AutoStakeDestinationColdkeys after revert"
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get(coldkey, netuid),
            Some(hk1),
            "coldkey AutoStakeDestination must point back to hk1 after revert"
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get(staker1, netuid),
            Some(hk1),
            "staker1 AutoStakeDestination must point back to hk1 after revert"
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get(staker2, netuid),
            Some(hk1),
            "staker2 AutoStakeDestination must point back to hk1 after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_subnet_owner --exact --nocapture
#[test]
fn test_revert_hotkey_swap_subnet_owner() {
    new_test_ext(1).execute_with(|| {
        let hk1 = U256::from(1);
        let hk2 = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&hk1, &coldkey);
        let netuid2 = add_dynamic_network(&hk1, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), hk1);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(
            SubnetOwnerHotkey::<Test>::get(netuid),
            hk2,
            "hk2 must be subnet owner after swap"
        );

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            SubnetOwnerHotkey::<Test>::get(netuid),
            hk1,
            "hk1 must be restored as subnet owner after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_dividends --exact --nocapture
#[test]
fn test_revert_hotkey_swap_dividends() {
    new_test_ext(1).execute_with(|| {
        let hk1 = U256::from(1);
        let hk2 = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&hk1, &coldkey);
        remove_owner_registration_stake(netuid);
        let netuid2 = add_dynamic_network(&hk1, &coldkey);
        remove_owner_registration_stake(netuid2);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        let amount = 10_000;
        let shares = U64F64::from_num(10_000);

        TotalHotkeyAlpha::<Test>::insert(hk1, netuid, AlphaBalance::from(amount));
        TotalHotkeyAlphaLastEpoch::<Test>::insert(hk1, netuid, AlphaBalance::from(amount * 2));
        TotalHotkeyShares::<Test>::insert(hk1, netuid, U64F64::from_num(shares));
        Alpha::<Test>::insert((hk1, coldkey, netuid), U64F64::from_num(amount));
        AlphaDividendsPerSubnet::<Test>::insert(netuid, hk1, AlphaBalance::from(amount));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(hk1, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(hk2, netuid),
            AlphaBalance::from(amount)
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hk1, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hk2, netuid),
            AlphaBalance::from(amount * 2)
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(hk1, netuid),
            U64F64::from_num(0)
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(hk2, netuid),
            U64F64::from_num(0)
        );
        assert_eq!(TotalHotkeySharesV2::<Test>::get(hk2, netuid), shares.into());
        assert_eq!(
            Alpha::<Test>::get((hk1, coldkey, netuid)),
            U64F64::from_num(0)
        );
        assert_eq!(
            Alpha::<Test>::get((hk2, coldkey, netuid)),
            U64F64::from_num(0)
        );
        assert_eq!(AlphaV2::<Test>::get((hk2, coldkey, netuid)), amount.into());
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hk1),
            AlphaBalance::ZERO
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hk2),
            AlphaBalance::from(amount)
        );

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(hk2, netuid),
            AlphaBalance::ZERO,
            "hk2 TotalHotkeyAlpha must be zero after revert"
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(hk1, netuid),
            AlphaBalance::from(amount),
            "hk1 TotalHotkeyAlpha must be restored after revert"
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hk2, netuid),
            AlphaBalance::ZERO,
            "hk2 TotalHotkeyAlphaLastEpoch must be zero after revert"
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hk1, netuid),
            AlphaBalance::from(amount * 2),
            "hk1 TotalHotkeyAlphaLastEpoch must be restored after revert"
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(hk2, netuid),
            U64F64::from_num(0),
            "hk2 TotalHotkeyShares must be zero after revert"
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(hk1, netuid),
            U64F64::from_num(0),
            "hk1 TotalHotkeyShares must be migrated to v2"
        );
        assert_eq!(
            TotalHotkeySharesV2::<Test>::get(hk1, netuid),
            shares.into(),
            "hk1 TotalHotkeyShares must be restored to v2 after revert"
        );
        assert_eq!(
            Alpha::<Test>::get((hk2, coldkey, netuid)),
            U64F64::from_num(0),
            "hk2 Alpha must be zero after revert"
        );
        assert_eq!(
            Alpha::<Test>::get((hk1, coldkey, netuid)),
            U64F64::from_num(0),
            "hk1 Alpha must be migrated to v2"
        );
        assert_eq!(
            AlphaV2::<Test>::get((hk1, coldkey, netuid)),
            amount.into(),
            "hk1 Alpha must be restored to v2 after revert"
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hk2),
            AlphaBalance::ZERO,
            "hk2 AlphaDividendsPerSubnet must be zero after revert"
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hk1),
            AlphaBalance::from(amount),
            "hk1 AlphaDividendsPerSubnet must be restored after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_voting_power_transfers_on_hotkey_swap --exact --nocapture
#[test]
fn test_revert_voting_power_transfers_on_hotkey_swap() {
    new_test_ext(1).execute_with(|| {
        let hk1 = U256::from(1);
        let hk2 = U256::from(99);
        let coldkey = U256::from(2);
        let netuid = add_dynamic_network(&hk1, &coldkey);
        let voting_power_value = 5_000_000_000_000_u64;

        VotingPower::<Test>::insert(netuid, hk1, voting_power_value);
        assert_eq!(
            SubtensorModule::get_voting_power(netuid, &hk1),
            voting_power_value
        );
        assert_eq!(SubtensorModule::get_voting_power(netuid, &hk2), 0);

        SubtensorModule::swap_voting_power_for_hotkey(&hk1, &hk2, netuid);

        assert_eq!(SubtensorModule::get_voting_power(netuid, &hk1), 0);
        assert_eq!(
            SubtensorModule::get_voting_power(netuid, &hk2),
            voting_power_value
        );

        // Revert: hk2 -> hk1
        SubtensorModule::swap_voting_power_for_hotkey(&hk2, &hk1, netuid);

        assert_eq!(
            SubtensorModule::get_voting_power(netuid, &hk1),
            voting_power_value,
            "hk1 voting power must be fully restored after revert"
        );
        assert_eq!(
            SubtensorModule::get_voting_power(netuid, &hk2),
            0,
            "hk2 must have no voting power after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_claim_root_with_swap_hotkey --exact --nocapture
#[test]
fn test_revert_claim_root_with_swap_hotkey() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hk1 = U256::from(1002);
        let hk2 = U256::from(1003);
        let coldkey = U256::from(1004);

        let netuid = add_dynamic_network(&hk1, &owner_coldkey);
        let netuid2 = add_dynamic_network(&hk1, &owner_coldkey);

        add_balance_to_coldkey_account(&owner_coldkey, 1_000_000_000_000_u64.into());
        SubtensorModule::set_tao_weight(u64::MAX);

        let root_stake = 2_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );

        let initial_total_hotkey_alpha = 10_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &owner_coldkey,
            netuid,
            initial_total_hotkey_alpha.into(),
        );

        let pending_root_alpha = 1_000_000u64;
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );

        assert_ok!(SubtensorModule::set_root_claim_type(
            RuntimeOrigin::signed(coldkey),
            RootClaimTypeEnum::Keep
        ));
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(coldkey),
            BTreeSet::from([netuid])
        ));

        let stake_after_claim: u64 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid)
                .into();

        let hk1_root_claimed = RootClaimed::<Test>::get((netuid, &hk1, &coldkey));
        let hk1_claimable = *RootClaimable::<Test>::get(hk1).get(&netuid).unwrap();

        assert_eq!(u128::from(stake_after_claim), hk1_root_claimed);
        assert!(!RootClaimable::<Test>::get(hk2).contains_key(&netuid));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(owner_coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(
            RootClaimed::<Test>::get((netuid, &hk2, &coldkey)),
            0u128,
            "hk2 RootClaimed must be zero after swap"
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid, &hk1, &coldkey)),
            hk1_root_claimed,
            "hk2 must have hk1's RootClaimed after swap"
        );
        assert!(RootClaimable::<Test>::get(hk1).contains_key(&netuid));
        assert!(!RootClaimable::<Test>::get(hk2).contains_key(&netuid));

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(owner_coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            RootClaimed::<Test>::get((netuid, &hk2, &coldkey)),
            0u128,
            "hk2 RootClaimed must be zero after revert"
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid, &hk1, &coldkey)),
            hk1_root_claimed,
            "hk1 RootClaimed must be restored after revert"
        );

        assert!(!RootClaimable::<Test>::get(hk2).contains_key(&netuid));
        assert_eq!(
            *RootClaimable::<Test>::get(hk1).get(&netuid).unwrap(),
            hk1_claimable,
            "hk1 RootClaimable must be restored after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_with_revert_stake_the_same --exact --nocapture
#[test]
fn test_revert_hotkey_swap_with_revert_stake_the_same() {
    new_test_ext(1).execute_with(|| {
        let netuid_1 = NetUid::from(1);
        let netuid_2 = NetUid::from(2);
        let tempo: u16 = 13;
        let hk1 = U256::from(1);
        let new_hotkey = U256::from(2);
        let random_hotkey = U256::from(3);
        let coldkey = U256::from(3);
        let coldkey_2 = U256::from(4);
        let coldkey_3 = U256::from(5);
        let coldkey_4 = U256::from(6);
        let random_coldkey = U256::from(7);
        let initial_balance = 10_000_000_000u64 * 2;
        let stake1 = 500_000_000u64;
        let stake2 = 1_000_000_000u64;
        let stake_ck2 = 1_500_000_000u64;
        let stake_ck3 = 300_000_000u64;
        let stake_ck4 = 900_000_000u64;

        assert_ok!(SubtensorModule::try_associate_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(random_coldkey),
            random_hotkey
        ));

        // Setup
        super::super::mock::setup_reserves(
            netuid_1,
            (stake_ck4 * 100).into(),
            (stake_ck4 * 100).into(),
        );
        super::super::mock::setup_reserves(
            netuid_2,
            (stake_ck4 * 100).into(),
            (stake_ck4 * 100).into(),
        );

        add_network(netuid_1, tempo, 0);
        add_network(netuid_2, tempo, 0);

        SubnetMechanism::<Test>::insert(netuid_1, 1);
        SubnetMechanism::<Test>::insert(netuid_2, 1);

        register_ok_neuron(netuid_1, hk1, coldkey, 0);
        register_ok_neuron(netuid_2, hk1, coldkey, 0);

        add_balance_to_coldkey_account(&coldkey, initial_balance.into());
        add_balance_to_coldkey_account(&coldkey_4, initial_balance.into());
        add_balance_to_coldkey_account(&random_coldkey, initial_balance.into());
        step_block(20); // Waiting interval to be able to swap later

        // Checking stake for hk1 on both networks
        let hk1_stake_before_increase_sn_1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid_1);
        assert!(
            hk1_stake_before_increase_sn_1 == 0.into(),
            "hk1 should have empty stake"
        );

        let hk1_stake_before_increase_sn_2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid_2);
        assert!(
            hk1_stake_before_increase_sn_2 == 0.into(),
            "hk1 should have empty stake"
        );

        // Adding stake to hk1 on both networks
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey,
            netuid_1,
            stake1.into(),
        );
        // Adding another stake for different coldkey
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey_2,
            netuid_1,
            stake_ck2.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey_3,
            netuid_1,
            stake_ck3.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey,
            netuid_2,
            stake2.into(),
        );

        // The stake for validator
        let hk1_stake_before_swap_sn_1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid_1);
        assert!(
            hk1_stake_before_swap_sn_1 == stake1.into(),
            "hk1 should have stake before swap on sn_1"
        );

        // Let's check individual stake
        let hk1_stake_before_swap_sn_1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey_2, netuid_1);
        assert_eq!(
            hk1_stake_before_swap_sn_1,
            (stake_ck2).into(),
            "stake for ck2 should be only his stake"
        );

        let hk1_stake_before_swap_sn_2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid_2);
        assert!(
            hk1_stake_before_swap_sn_2 == stake2.into(),
            "hk1 should have stake before swap on sn_2"
        );

        assert_ok!(SubtensorModule::perform_hotkey_swap(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &hk1,
            &new_hotkey,
            Some(netuid_1),
            false
        ));

        assert_eq!(Owner::<Test>::get(hk1), coldkey);

        SubtensorModule::do_add_stake(
            RawOrigin::Signed(random_coldkey).into(),
            hk1,
            netuid_1,
            stake_ck4.into(),
        )
        .unwrap();

        // Check stake moved to new hotkey on subnet1
        let new_hotkey_stake_after_swap_ck =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey,
                netuid_1,
            );
        assert_eq!(new_hotkey_stake_after_swap_ck, stake1.into());

        // Check stake moved for ck2
        let new_hotkey_stake_after_swap_ck_1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey_2,
                netuid_1,
            );
        assert_eq!(new_hotkey_stake_after_swap_ck_1, stake_ck2.into());

        // Check stake moved for ck3
        let new_hotkey_stake_after_swap_ck_3 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey_3,
                netuid_1,
            );
        assert_eq!(new_hotkey_stake_after_swap_ck_3, stake_ck3.into());

        step_block(20);

        // Let's check individual stakes; they changed because of emissions
        let new_hotkey_stake_before_revert_ck =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey,
                netuid_1,
            );
        assert!(new_hotkey_stake_before_revert_ck > stake1.into());

        let new_hotkey_stake_before_revert_ck_2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey_2,
                netuid_1,
            );
        assert!(new_hotkey_stake_before_revert_ck_2 > stake_ck2.into());

        let new_hotkey_stake_before_revert_ck_3 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey_3,
                netuid_1,
            );
        assert!(new_hotkey_stake_before_revert_ck_3 > stake_ck3.into());

        // Reverting back: hk2 -> hk1
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &new_hotkey,
            &hk1,
            Some(netuid_1),
            false
        ));

        // Let's check individual stakes; they changed because of emissions
        let old_hotkey_stake_after_revert_ck =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid_1);
        assert_eq!(
            old_hotkey_stake_after_revert_ck,
            new_hotkey_stake_before_revert_ck
        );

        let old_hotkey_stake_after_revert_ck_2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey_2, netuid_1);
        assert_eq!(
            old_hotkey_stake_after_revert_ck_2,
            new_hotkey_stake_before_revert_ck_2
        );

        let old_hotkey_stake_after_revert_ck_3 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey_3, netuid_1);
        assert_eq!(
            old_hotkey_stake_after_revert_ck_3,
            new_hotkey_stake_before_revert_ck_3
        );
    });
}
