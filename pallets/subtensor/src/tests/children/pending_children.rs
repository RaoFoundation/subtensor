#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
use super::super::mock;
use super::super::mock::*;
use frame_support::{assert_err, assert_noop, assert_ok};

use crate::{utils::rate_limiting::TransactionType, *};
use sp_core::U256;

use super::helpers::close;

// Test that min stake is enforced for setting children
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::pending_children::test_do_set_child_below_min_stake --exact --show-output --nocapture
#[test]
fn test_do_set_child_below_min_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        StakeThreshold::<Test>::set(1_000_000_000_000);

        // Attempt to set child
        assert_err!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(proportion, child)]
            ),
            Error::<Test>::NotEnoughStakeToSetChildkeys
        );
    });
}

/// --- test_do_remove_stake_clears_pending_childkeys ---
///
/// Test Description: Ensures that removing stake clears any pending childkeys.
///
/// Expected Behavior:
/// - Pending childkeys should be cleared when stake is removed
/// - Cooldown block should be reset to 0
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::pending_children::test_do_remove_stake_clears_pending_childkeys --exact --show-output --nocapture
#[test]
fn test_do_remove_stake_clears_pending_childkeys() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, 10_000_000_000_000_u64.into());
        SubtokenEnabled::<Test>::insert(netuid, true);

        let reserve = 1_000_000_000_000_000_u64;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Set non-default value for childkey stake threshold
        StakeThreshold::<Test>::set(1_000_000_000_000);

        assert_ok!(SubtensorModule::do_add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            (StakeThreshold::<Test>::get() * 2).into()
        ));

        let alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        println!(
            "StakeThreshold::<Test>::get() = {:?}",
            StakeThreshold::<Test>::get()
        );
        println!("alpha                         = {alpha:?}");

        // Attempt to set child
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            vec![(proportion, child)]
        ));

        // Check that pending child exists
        let pending_before = PendingChildKeys::<Test>::get(netuid, hotkey);
        assert!(!pending_before.0.is_empty());
        assert!(pending_before.1 > 0);

        // Remove stake
        assert_ok!(SubtensorModule::do_remove_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            alpha,
        ));

        // Assert that pending child is removed
        let pending_after = PendingChildKeys::<Test>::get(netuid, hotkey);
        close(
            pending_after.0.len() as u64,
            0,
            0,
            "Pending children vector should be empty",
        );
        close(pending_after.1, 0, 0, "Cooldown block should be zero");
    });
}

// Test that pending childkeys do not apply immediately and apply after cooldown period
//
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::pending_children::test_do_set_child_cooldown_period --exact --show-output --nocapture
#[cfg(test)]
#[test]
fn test_do_set_child_cooldown_period() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let parent = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, parent, coldkey, 0);

        // Set minimum stake for setting children
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            StakeThreshold::<Test>::get().into(),
        );

        // Schedule parent-child relationship
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            parent,
            netuid,
            vec![(proportion, child)],
        ));

        // Ensure the childkeys are not yet applied
        let children_before = SubtensorModule::get_children(&parent, netuid);
        close(
            children_before.len() as u64,
            0,
            0,
            "Children vector should be empty before cooldown",
        );

        wait_and_set_pending_children(netuid);
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            StakeThreshold::<Test>::get().into(),
        );

        // Verify child assignment
        let children_after = SubtensorModule::get_children(&parent, netuid);
        close(
            children_after.len() as u64,
            1,
            0,
            "Children vector should have one entry after cooldown",
        );
        close(
            children_after[0].0,
            proportion,
            0,
            "Child proportion should match",
        );
        close(
            children_after[0].1.try_into().unwrap(),
            child.try_into().unwrap(),
            0,
            "Child key should match",
        );
    });
}

// Test that pending childkeys get set during the epoch after the cooldown period.
//
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::pending_children::test_do_set_pending_children_runs_in_epoch --exact --show-output --nocapture
#[cfg(test)]
#[test]
fn test_do_set_pending_children_runs_in_epoch() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let parent = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, parent, coldkey, 0);

        // Set minimum stake for setting children
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            StakeThreshold::<Test>::get().into(),
        );

        // Schedule parent-child relationship
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            parent,
            netuid,
            vec![(proportion, child)],
        ));

        // Ensure the childkeys are not yet applied
        let children_before = SubtensorModule::get_children(&parent, netuid);
        close(
            children_before.len() as u64,
            0,
            0,
            "Children vector should be empty before cooldown",
        );

        wait_set_pending_children_cooldown(netuid);

        // Verify child assignment
        let children_after = SubtensorModule::get_children(&parent, netuid);
        close(
            children_after.len() as u64,
            1,
            0,
            "Children vector should have one entry after cooldown",
        );
        close(
            children_after[0].0,
            proportion,
            0,
            "Child proportion should match",
        );
        close(
            children_after[0].1.try_into().unwrap(),
            child.try_into().unwrap(),
            0,
            "Child key should match",
        );
    });
}

// Test that revoking childkeys does not require minimum stake
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::pending_children::test_revoke_child_no_min_stake_check --exact --show-output --nocapture
#[test]
fn test_revoke_child_no_min_stake_check() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let parent = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(NetUid::ROOT, 13, 0);
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, parent, coldkey, 0);

        let reserve = 1_000_000_000_000_000_u64;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());
        mock::setup_reserves(NetUid::ROOT, reserve.into(), reserve.into());

        // Set minimum stake for setting children
        StakeThreshold::<Test>::put(1_000_000_000_000);

        let (_, fee) = mock::swap_tao_to_alpha(NetUid::ROOT, StakeThreshold::<Test>::get().into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            NetUid::ROOT,
            (StakeThreshold::<Test>::get() + fee).into(),
        );

        // Schedule parent-child relationship
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            parent,
            netuid,
            vec![(proportion, child)],
        ));

        // Ensure the childkeys are not yet applied
        let children_before = SubtensorModule::get_children(&parent, netuid);
        assert_eq!(children_before, vec![]);

        wait_and_set_pending_children(netuid);
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            NetUid::ROOT,
            (StakeThreshold::<Test>::get() + fee).into(),
        );

        // Ensure the childkeys are applied
        let children_after = SubtensorModule::get_children(&parent, netuid);
        assert_eq!(children_after, vec![(proportion, child)]);

        // Bypass tx rate limit
        TransactionType::SetChildren.set_last_block_on_subnet::<Test>(&parent, netuid, 0);

        // Schedule parent-child relationship revokation
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            parent,
            netuid,
            vec![],
        ));

        wait_and_set_pending_children(netuid);

        // Ensure the childkeys are revoked
        let children_after = SubtensorModule::get_children(&parent, netuid);
        assert_eq!(children_after, vec![]);
    });
}

// Test that setting childkeys works even if subnet registration is disabled
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::pending_children::test_do_set_child_registration_disabled --exact --show-output --nocapture
#[test]
fn test_do_set_child_registration_disabled() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let parent = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1);
        let proportion: u64 = 1000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, parent, coldkey, 0);

        let reserve = 1_000_000_000_000_000_u64;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Set minimum stake for setting children
        StakeThreshold::<Test>::put(1_000_000_000_000);
        let (_, fee) = mock::swap_tao_to_alpha(netuid, StakeThreshold::<Test>::get().into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            (StakeThreshold::<Test>::get() + fee).into(),
        );

        // Disable subnet registrations
        NetworkRegistrationAllowed::<Test>::insert(netuid, false);

        // Schedule parent-child relationship
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            parent,
            netuid,
            vec![(proportion, child)],
        ));

        wait_and_set_pending_children(netuid);
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            (StakeThreshold::<Test>::get() + fee).into(),
        );

        // Ensure the childkeys are applied
        let children_after = SubtensorModule::get_children(&parent, netuid);
        assert_eq!(children_after, vec![(proportion, child)]);
    });
}

// 60: Test set_children rate limiting - Fail then succeed
// This test ensures that an immediate second `set_children` transaction fails due to rate limiting:
// - Sets up a network and registers a hotkey
// - Performs a `set_children` transaction
// - Attempts a second `set_children` transaction immediately
// - Verifies that the second transaction fails with `TxRateLimitExceeded`
// Then the rate limit period passes and the second transaction succeeds
// - Steps blocks for the rate limit period
// - Attempts the second transaction again and verifies it succeeds
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::pending_children::test_set_children_rate_limit_fail_then_succeed --exact --show-output --nocapture
#[test]
fn test_set_children_rate_limit_fail_then_succeed() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(1);
        let tempo = 13;

        // Add network and register hotkey
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // First set_children transaction
        mock_set_children(&coldkey, &hotkey, netuid, &[(100, child)]);

        // Immediate second transaction should fail due to rate limit
        assert_noop!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                vec![(100, child2)]
            ),
            Error::<Test>::TxRateLimitExceeded
        );

        // Verify first children assignment remains
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(100, child)]);

        // Try again after rate limit period has passed
        // Check rate limit
        let limit = TransactionType::SetChildren.rate_limit_on_subnet::<Test>(netuid);

        // Step that many blocks
        step_block(limit as u16);

        // Verify rate limit passes
        assert!(TransactionType::SetChildren.passes_rate_limit_on_subnet::<Test>(&hotkey, netuid));

        // Try again
        mock_set_children(&coldkey, &hotkey, netuid, &[(100, child2)]);

        // Verify children assignment has changed
        let children = SubtensorModule::get_children(&hotkey, netuid);
        assert_eq!(children, vec![(100, child2)]);
    });
}

#[test]
fn test_do_set_child_as_sn_owner_not_enough_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let sn_owner_hotkey = U256::from(4);

        let child_coldkey = U256::from(2);
        let child_hotkey = U256::from(5);

        let threshold = 10_000;
        SubtensorModule::set_stake_threshold(threshold);

        let proportion: u64 = 1000;

        let netuid = add_dynamic_network(&sn_owner_hotkey, &coldkey);
        remove_owner_registration_stake(netuid);
        register_ok_neuron(netuid, child_hotkey, child_coldkey, 0);

        // Verify stake of sn_owner_hotkey is NOT enough
        assert!(
            SubtensorModule::get_total_stake_for_hotkey(&sn_owner_hotkey)
                < StakeThreshold::<Test>::get().into()
        );

        // Verify that we can set child as sn owner, even though sn_owner_hotkey has insufficient stake
        assert_ok!(SubtensorModule::do_schedule_children(
            RuntimeOrigin::signed(coldkey),
            sn_owner_hotkey,
            netuid,
            vec![(proportion, child_hotkey)]
        ));

        // Make new hotkey from owner coldkey
        let other_sn_owner_hotkey = U256::from(6);
        register_ok_neuron(netuid, other_sn_owner_hotkey, coldkey, 1234);

        // Verify stake of other_sn_owner_hotkey is NOT enough
        assert!(
            SubtensorModule::get_total_stake_for_hotkey(&other_sn_owner_hotkey)
                < StakeThreshold::<Test>::get().into()
        );

        // Can't set child as sn owner, because it is not in SubnetOwnerHotkey map
        assert_noop!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                other_sn_owner_hotkey,
                netuid,
                vec![(proportion, child_hotkey)]
            ),
            Error::<Test>::NotEnoughStakeToSetChildkeys
        );
    });
}

#[test]
fn test_pending_cooldown_as_expected() {
    let curr_block = 1;
    // TODO: Fix when CHK splitting patched
    // let expected_cooldown = prod_or_fast!(7200, 15);

    new_test_ext(curr_block).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child1 = U256::from(3);
        let child2 = U256::from(4);
        let netuid = NetUid::from(1);
        let proportion1: u64 = 1000;
        let proportion2: u64 = 2000;
        let expected_cooldown = PendingChildKeyCooldown::<Test>::get();

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set multiple children
        mock_schedule_children(
            &coldkey,
            &hotkey,
            netuid,
            &[(proportion1, child1), (proportion2, child2)],
        );

        // Verify pending map
        let pending_children = PendingChildKeys::<Test>::get(netuid, hotkey);
        assert_eq!(
            pending_children.0,
            vec![(proportion1, child1), (proportion2, child2)]
        );
        assert_eq!(pending_children.1, curr_block + expected_cooldown);
    });
}
