#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
use super::super::mock;
use super::super::mock::*;
use approx::assert_abs_diff_eq;
use frame_support::{assert_noop, assert_ok};
use subtensor_runtime_common::{NetUidStorageIndex, TaoBalance};

use crate::{utils::rate_limiting::TransactionType, *};
use sp_core::U256;
use sp_runtime::PerU16;

// 24: Test childkey take functionality
// This test verifies the functionality of setting and getting childkey take:
// - Sets up a network and registers a hotkey
// - Checks default and maximum childkey take values
// - Sets a new childkey take value
// - Verifies the new take value is stored correctly
// - Attempts to set an invalid take value and checks for appropriate error
// - Tries to set take with a non-associated coldkey and verifies the error
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::childkey_take::test_childkey_take_functionality --exact --show-output --nocapture
#[test]
fn test_childkey_take_functionality() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Test default and max childkey take
        let default_take = SubtensorModule::get_default_childkey_take();
        let min_take = SubtensorModule::get_min_childkey_take();
        log::info!("Default take: {default_take}, Max take: {min_take}");

        // Check if default take and max take are the same
        assert_eq!(
            default_take, min_take,
            "Default take should be equal to max take"
        );

        // Log the actual value of MaxChildkeyTake
        log::info!(
            "MaxChildkeyTake value: {:?}",
            MaxChildkeyTake::<Test>::get()
        );

        // Test setting childkey take
        let new_take: u16 = SubtensorModule::get_max_childkey_take() / 2; // 50% of max_take
        assert_ok!(SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            PerU16::from_parts(new_take)
        ));

        // Verify childkey take was set correctly
        let stored_take = SubtensorModule::get_childkey_take(&hotkey, netuid);
        log::info!("Stored take: {stored_take}");
        assert_eq!(stored_take, new_take);

        // Test setting childkey take outside of allowed range
        let invalid_take: u16 = SubtensorModule::get_max_childkey_take() + 1;
        assert_noop!(
            SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                PerU16::from_parts(invalid_take)
            ),
            Error::<Test>::InvalidChildkeyTake
        );

        // Test setting childkey take with non-associated coldkey
        let non_associated_coldkey = U256::from(999);
        assert_noop!(
            SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(non_associated_coldkey),
                hotkey,
                netuid,
                PerU16::from_parts(new_take)
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

#[test]
fn test_childkey_take_respects_effective_subnet_minimum() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);
        let subnet_min = SubtensorModule::get_max_childkey_take() / 2;

        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        SubtensorModule::set_min_childkey_take_for_subnet(netuid, PerU16::from_parts(subnet_min));

        assert_eq!(
            SubtensorModule::get_effective_min_childkey_take(netuid),
            subnet_min
        );
        assert_eq!(
            SubtensorModule::get_childkey_take(&hotkey, netuid),
            subnet_min
        );

        assert_noop!(
            SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                PerU16::from_parts(subnet_min - 1)
            ),
            Error::<Test>::InvalidChildkeyTake
        );

        assert_ok!(SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            PerU16::from_parts(subnet_min)
        ));

        ChildkeyTake::<Test>::insert(hotkey, netuid, PerU16::from_parts(subnet_min - 1));
        assert_eq!(
            SubtensorModule::get_childkey_take(&hotkey, netuid),
            subnet_min
        );
    });
}

// 25: Test childkey take rate limiting
// This test verifies the rate limiting functionality for setting childkey take:
// - Sets up a network and registers a hotkey
// - Sets a rate limit for childkey take changes
// - Performs multiple attempts to set childkey take
// - Verifies that rate limiting prevents frequent changes
// - Advances blocks to bypass rate limit and confirms successful change
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::childkey_take::test_childkey_take_rate_limiting --exact --show-output --nocapture
#[test]
fn test_childkey_take_rate_limiting() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set a rate limit for childkey take changes
        let rate_limit: u64 = 100;
        SubtensorModule::set_tx_childkey_take_rate_limit(rate_limit);

        log::info!(
            "Set TxChildkeyTakeRateLimit: {:?}",
            TxChildkeyTakeRateLimit::<Test>::get()
        );

        // Helper function to log rate limit information
        let log_rate_limit_info = || {
            let current_block = SubtensorModule::get_current_block_as_u64();
            let last_block = TransactionType::SetChildkeyTake.last_block_on_subnet::<Test>(
                &hotkey,
                netuid,
            );
            let passes = TransactionType::SetChildkeyTake.passes_rate_limit_on_subnet::<Test>(
                &hotkey,
                netuid,
            );
            let limit = TransactionType::SetChildkeyTake.rate_limit_on_subnet::<Test>(netuid);
            log::info!(
                "Rate limit info: current_block: {}, last_block: {}, limit: {}, passes: {}, diff: {}",
                current_block,
                last_block,
                limit,
                passes,
                current_block - last_block
            );
        };

        // First transaction (should succeed)
        log_rate_limit_info();
        assert_ok!(SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            PerU16::from_parts(500)
        ));
        log_rate_limit_info();

        // Second transaction (should fail due to rate limit)
        log_rate_limit_info();
        assert_noop!(
            SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                PerU16::from_parts(600)
            ),
            Error::<Test>::TxChildkeyTakeRateLimitExceeded
        );
        log_rate_limit_info();

        // Advance the block number to just before the rate limit
        run_to_block(rate_limit - 1);

        // Third transaction (should still fail)
        log_rate_limit_info();
        assert_noop!(
            SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                PerU16::from_parts(650)
            ),
            Error::<Test>::TxChildkeyTakeRateLimitExceeded
        );
        log_rate_limit_info();

        // Advance the block number to just after the rate limit
        run_to_block(rate_limit + 1);

        // Fourth transaction (should succeed)
        log_rate_limit_info();
        assert_ok!(SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            PerU16::from_parts(700)
        ));
        log_rate_limit_info();

        // Verify the final take was set
        let stored_take = SubtensorModule::get_childkey_take(&hotkey, netuid);
        assert_eq!(stored_take, 700);
    });
}

// 26: Test childkey take functionality across multiple networks
// This test verifies the childkey take functionality across multiple networks:
// - Creates multiple networks and sets up neurons
// - Sets unique childkey take values for each network
// - Verifies that each network has a different childkey take value
// - Attempts to set childkey take again (should fail due to rate limit)
// - Advances blocks to bypass rate limit and successfully updates take value
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::childkey_take::test_multiple_networks_childkey_take --exact --show-output --nocapture
#[test]
fn test_multiple_networks_childkey_take() {
    new_test_ext(1).execute_with(|| {
        const NUM_NETWORKS: u16 = 10;
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        // Create 10 networks and set up neurons (skip network 0)
        for netuid in 1..NUM_NETWORKS {
            let netuid = NetUid::from(netuid);
            // Add network
            add_network(netuid, 13, 0);

            // Register neuron
            register_ok_neuron(netuid, hotkey, coldkey, 0);

            // Set a unique childkey take value for each network
            let take_value = u16::from(netuid.next()) * 100; // Values will be 200, 300, ..., 1000
            assert_ok!(SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                PerU16::from_parts(take_value)
            ));

            // Verify the childkey take was set correctly
            let stored_take = SubtensorModule::get_childkey_take(&hotkey, netuid);
            assert_eq!(
                stored_take, take_value,
                "Childkey take not set correctly for network {netuid}"
            );

            // Log the set value
            log::info!("Network {netuid}: Childkey take set to {take_value}");
        }

        // Verify all networks have different childkey take values
        for i in 1..NUM_NETWORKS {
            for j in (i + 1)..NUM_NETWORKS {
                let take_i = SubtensorModule::get_childkey_take(&hotkey, i.into());
                let take_j = SubtensorModule::get_childkey_take(&hotkey, j.into());
                assert_ne!(
                    take_i, take_j,
                    "Childkey take values should be different for networks {i} and {j}"
                );
            }
        }

        // Attempt to set childkey take again (should fail due to rate limit)
        let result = SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            1.into(),
            PerU16::from_parts(1100),
        );
        assert_noop!(result, Error::<Test>::TxChildkeyTakeRateLimitExceeded);

        // Advance blocks to bypass rate limit
        run_to_block(SubtensorModule::get_tx_childkey_take_rate_limit() + 1);

        // Now setting childkey take should succeed
        assert_ok!(SubtensorModule::set_childkey_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            1.into(),
            PerU16::from_parts(1100)
        ));

        // Verify the new take value
        let new_take = SubtensorModule::get_childkey_take(&hotkey, 1.into());
        assert_eq!(new_take, 1100, "Childkey take not updated after rate limit");
    });
}

/// Test that distribute_emission sends childkey take fully to the nominators if childkey
/// doesn't have its own stake, independently of parent hotkey take.
/// cargo test --package pallet-subtensor --lib -- tests::children::childkey_take::test_childkey_take_drain --exact --show-output
#[allow(clippy::assertions_on_constants)]
#[test]
fn test_childkey_take_drain() {
    // Test cases: parent_hotkey_take
    [0_u16, u16::MAX / 5].iter().for_each(|parent_hotkey_take| {
        new_test_ext(1).execute_with(|| {
            let parent_coldkey = U256::from(1);
            let parent_hotkey = U256::from(3);
            let child_coldkey = U256::from(2);
            let child_hotkey = U256::from(4);
            let miner_coldkey = U256::from(5);
            let miner_hotkey = U256::from(6);
            let nominator = U256::from(7);
            let netuid = NetUid::from(1);
            let subnet_tempo = 10;
            let stake = 100_000_000_000_u64;
            let proportion: u64 = u64::MAX / 2;

            // Add network, register hotkeys, and setup network parameters
            add_network(netuid, subnet_tempo, 0);
            SubtensorModule::set_ck_burn(0);
            mock::setup_reserves(netuid, (stake * 10_000).into(), (stake * 10_000).into());
            register_ok_neuron(netuid, child_hotkey, child_coldkey, 0);
            register_ok_neuron(netuid, parent_hotkey, parent_coldkey, 1);
            register_ok_neuron(netuid, miner_hotkey, miner_coldkey, 1);
            add_balance_to_coldkey_account(
                &parent_coldkey,
                TaoBalance::from(stake) + ExistentialDeposit::get(),
            );
            add_balance_to_coldkey_account(
                &nominator,
                TaoBalance::from(stake) + ExistentialDeposit::get(),
            );
            SubtensorModule::set_weights_set_rate_limit(netuid, 0);
            SubtensorModule::set_max_allowed_validators(netuid, 2);
            step_block(subnet_tempo);
            SubnetOwnerCut::<Test>::set(0);

            // Set children
            mock_set_children_no_epochs(netuid, &parent_hotkey, &[(proportion, child_hotkey)]);

            // Set 20% childkey take
            let max_take: u16 = 0xFFFF / 5;
            SubtensorModule::set_max_childkey_take(PerU16::from_parts(max_take));
            assert_ok!(SubtensorModule::set_childkey_take(
                RuntimeOrigin::signed(child_coldkey),
                child_hotkey,
                netuid,
                PerU16::from_parts(max_take)
            ));

            // Set hotkey take for parent
            SubtensorModule::set_max_delegate_take(PerU16::from_parts(*parent_hotkey_take));
            Delegates::<Test>::insert(parent_hotkey, PerU16::from_parts(*parent_hotkey_take));

            // Set 0% for childkey-as-a-delegate take
            Delegates::<Test>::insert(child_hotkey, PerU16::zero());

            // Setup stakes:
            //   Stake from parent
            //   Stake from nominator to childkey
            //   Parent gives 50% of stake to childkey
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(parent_coldkey),
                parent_hotkey,
                netuid,
                stake.into()
            ));
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(nominator),
                child_hotkey,
                netuid,
                stake.into()
            ));

            // Setup YUMA so that it creates emissions
            Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 0, vec![(2, 0xFFFF)]);
            Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 1, vec![(2, 0xFFFF)]);
            BlockAtRegistration::<Test>::set(netuid, 0, 1);
            BlockAtRegistration::<Test>::set(netuid, 1, 1);
            BlockAtRegistration::<Test>::set(netuid, 2, 1);
            LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2, 2, 2]);
            Kappa::<Test>::set(netuid, u16::MAX / 5);
            ActivityCutoff::<Test>::set(netuid, u16::MAX); // makes all stake active
            ValidatorPermit::<Test>::insert(netuid, vec![true, true, false]);

            // Run run_coinbase to hit subnet epoch
            let child_stake_before = SubtensorModule::get_total_stake_for_coldkey(&child_coldkey);
            let parent_stake_before = SubtensorModule::get_total_stake_for_coldkey(&parent_coldkey);
            let nominator_stake_before = SubtensorModule::get_total_stake_for_coldkey(&nominator);

            step_block(subnet_tempo);

            // Verify how emission is split between keys
            //   - Child stake remains 0
            //   - Childkey take is 20% of its total emission that rewards both inherited from
            //     parent stake and nominated stake, which all goes to nominators. Because child
            //     validator emission is 50% of total emission, 20% of it is 10% of total emission
            //     and it all goes to nominator. If childkey take was 0%, then only 5% would go to
            //     the nominator, so the final solit is:
            //   - Parent stake increases by 45% of total emission
            //   - Nominator stake increases by 55% of total emission
            let child_emission =
                SubtensorModule::get_total_stake_for_coldkey(&child_coldkey) - child_stake_before;
            let parent_emission =
                SubtensorModule::get_total_stake_for_coldkey(&parent_coldkey) - parent_stake_before;
            let nominator_emission =
                SubtensorModule::get_total_stake_for_coldkey(&nominator) - nominator_stake_before;
            let total_emission = child_emission + parent_emission + nominator_emission;

            assert_abs_diff_eq!(child_emission, TaoBalance::ZERO, epsilon = 10.into());
            assert_abs_diff_eq!(
                parent_emission,
                total_emission * 9.into() / 20.into(),
                epsilon = 10.into()
            );
            assert_abs_diff_eq!(
                nominator_emission,
                total_emission * 11.into() / 20.into(),
                epsilon = 10.into()
            );
        });
    });
}

#[test]
fn test_do_set_childkey_take_success() {
    new_test_ext(1).execute_with(|| {
        // Setup
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);
        let take = 5000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set childkey take
        assert_ok!(SubtensorModule::do_set_childkey_take(
            coldkey,
            hotkey,
            netuid,
            PerU16::from_parts(take)
        ));

        // Verify the take was set correctly
        assert_eq!(SubtensorModule::get_childkey_take(&hotkey, netuid), take);
        let tx_type: u16 = TransactionType::SetChildkeyTake.into();
        assert_eq!(
            TransactionKeyLastBlock::<Test>::get((hotkey, netuid, tx_type,)),
            System::block_number()
        );
    });
}

#[test]
fn test_do_set_childkey_take_non_associated_coldkey() {
    new_test_ext(1).execute_with(|| {
        // Setup
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let hotkey2 = U256::from(3);
        let netuid = NetUid::from(1);
        let take = 5000;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set childkey take
        assert_noop!(
            SubtensorModule::do_set_childkey_take(
                coldkey,
                hotkey2,
                netuid,
                PerU16::from_parts(take)
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

#[test]
fn test_do_set_childkey_take_invalid_take_value() {
    new_test_ext(1).execute_with(|| {
        // Setup
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);
        let take = SubtensorModule::get_max_childkey_take() + 1;

        // Add network and register hotkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set childkey take
        assert_noop!(
            SubtensorModule::do_set_childkey_take(
                coldkey,
                hotkey,
                netuid,
                PerU16::from_parts(take)
            ),
            Error::<Test>::InvalidChildkeyTake
        );
    });
}

#[test]
fn test_do_set_childkey_take_rate_limit_exceeded() {
    new_test_ext(1).execute_with(|| {
        // Setup
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);
        let initial_take = 3000;
        let higher_take = 5000;
        let lower_take = 1000;

        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set initial childkey take
        assert_ok!(SubtensorModule::do_set_childkey_take(
            coldkey,
            hotkey,
            netuid,
            PerU16::from_parts(initial_take)
        ));

        // Try to increase the take value, should hit rate limit
        assert_noop!(
            SubtensorModule::do_set_childkey_take(
                coldkey,
                hotkey,
                netuid,
                PerU16::from_parts(higher_take)
            ),
            Error::<Test>::TxChildkeyTakeRateLimitExceeded
        );

        // lower take value should be ok
        assert_ok!(SubtensorModule::do_set_childkey_take(
            coldkey,
            hotkey,
            netuid,
            PerU16::from_parts(lower_take)
        ));
    });
}
