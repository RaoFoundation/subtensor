#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]

use super::mock::*;
use crate::subnets::dissolution::DissolveCleanupStatus;
use crate::*;
use frame_support::{assert_ok, weights::Weight};
use sp_core::U256;
use subtensor_runtime_common::{AlphaBalance, TaoBalance};
use subtensor_swap_interface::SwapHandler;

// Import required types from the correct locations
use pallet_commitments::pallet::Pallet as CommitmentsPallet;
use pallet_commitments::{CommitmentInfo, Data};
use sp_runtime::BoundedVec;

fn call_remove_single_value(weight_meter: &mut WeightMeter, weight: Weight) -> bool {
    if !weight_meter.can_consume(weight) {
        return false;
    }
    weight_meter.consume(weight);
    CurrentDissolveCleanupStatus::<Test>::kill();
    true
}

#[test]
fn test_remove_single_value() {
    new_test_ext(0).execute_with(|| {
        CurrentDissolveCleanupStatus::<Test>::set(Some(DissolveCleanupStatus::new(NetUid::from(
            1,
        ))));
        let w = Weight::from_parts(100_u64, 100_u64);

        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        assert!(call_remove_single_value(&mut weight_meter, w));
        assert!(CurrentDissolveCleanupStatus::<Test>::get().is_none());
    });
}

#[test]
fn test_remove_single_value_failed() {
    new_test_ext(0).execute_with(|| {
        CurrentDissolveCleanupStatus::<Test>::set(Some(DissolveCleanupStatus::new(NetUid::from(
            1,
        ))));
        let w = Weight::from_parts(100_u64, 100_u64);

        let mut weight_meter =
            frame_support::weights::WeightMeter::with_limit(Weight::from_parts(50_u64, 50_u64));
        assert!(!call_remove_single_value(&mut weight_meter, w));
        assert!(CurrentDissolveCleanupStatus::<Test>::get().is_some());
    });
}

/// Test the remove_data_for_dissolved_networks macro function by testing each phase individually
#[test]
fn test_remove_data_for_dissolved_networks_all_phases() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        let stake_tao: u64 = 1000000;
        let lock_tao: u64 = 500;
        let amount: TaoBalance = (stake_tao).into();
        setup_reserves(
            netuid,
            (stake_tao * 1_000_000).into(),
            (stake_tao * 10_000_000).into(),
        );

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_cold,
            &owner_hot
        ));
        add_balance_to_coldkey_account(&owner_cold, amount);
        // Stake into subnet to create some alpha
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // Now add additional balance for locking
        let lock_amount: TaoBalance = (lock_tao).into();
        add_balance_to_coldkey_account(&owner_cold, lock_amount);

        // Add some commitment data
        assert_ok!(CommitmentsPallet::<Test>::set_commitment(
            <Test as frame_system::Config>::RuntimeOrigin::signed(owner_hot),
            netuid,
            Box::new(
                CommitmentInfo::<<Test as pallet_commitments::Config>::MaxFields> {
                    fields: BoundedVec::try_from(vec![Data::Raw(
                        BoundedVec::try_from(vec![1, 2, 3]).unwrap()
                    )])
                    .unwrap(),
                }
            )
        ));

        // Add some lock data - balance already added above
        assert_ok!(SubtensorModule::lock_stake(
            <Test as frame_system::Config>::RuntimeOrigin::signed(owner_cold),
            owner_hot,
            netuid,
            AlphaBalance::from(lock_tao),
        ));

        // Now test the full dissolution cleanup process by running on_idle multiple times
        // until all phases complete
        let total_weight = Weight::from_parts(u64::MAX, u64::MAX);
        let mut remaining_weight = total_weight;

        // First dissolve the network
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));

        // Verify it's in the dissolved networks queue
        assert!(DissolveCleanupQueue::<Test>::get().contains(&netuid));

        // Run cleanup phases until completion
        let mut iterations = 0;
        let max_iterations = 30; // Should be enough to go through all phases

        while !DissolveCleanupQueue::<Test>::get().is_empty() && iterations < max_iterations {
            let used_weight = SubtensorModule::on_idle(0, remaining_weight);
            remaining_weight = remaining_weight.saturating_sub(used_weight);
            iterations += 1;

            // If we've used all weight, reset for next iteration
            if remaining_weight.is_zero() {
                remaining_weight = total_weight;
            }
        }

        // Verify the network has been fully removed
        assert!(!DissolveCleanupQueue::<Test>::get().contains(&netuid));
        assert_eq!(
            CurrentDissolveCleanupStatus::<Test>::get(),
            None,
            "Cleanup phase should be None after completion"
        );

        // Verify the subnet no longer exists
        assert!(!SubtensorModule::if_subnet_exist(netuid));
    });
}

#[test]
fn test_clean_up_root_claimable_for_subnet() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some stake
        let stake_tao: u64 = 1000;
        setup_reserves(
            netuid,
            (stake_tao * 1_000_000).into(),
            (stake_tao * 10_000_000).into(),
        );
        let amount: TaoBalance = (stake_tao).into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_cold,
            &owner_hot
        ));
        add_balance_to_coldkey_account(&owner_cold, amount);
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // Verify root dividend exists before cleanup - we'll check this by running the function

        // Test the cleanup function
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        let (result, _) =
            SubtensorModule::clean_up_root_claimable_for_subnet(netuid, &mut weight_meter, None);
        // This function should return true when it completes its work (or false if weight limited)
        // In our test case with generous weight limit, it should complete
        assert!(
            result,
            "clean_up_root_claimable_for_subnet should complete successfully"
        );
    });
}

#[test]
fn test_clean_up_root_claimed_for_subnet() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some stake to have alpha value
        let stake_tao: u64 = 1000;
        setup_reserves(
            netuid,
            (stake_tao * 1_000_000).into(),
            (stake_tao * 10_000_000).into(),
        );
        let amount: TaoBalance = (stake_tao).into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_cold,
            &owner_hot
        ));
        add_balance_to_coldkey_account(&owner_cold, amount);
        // Stake into subnet to create some alpha
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // Note: We don't need to actually create root dividend data for this test
        // The cleanup function should handle the case where there's nothing to clean up

        // Test the cleanup function
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        let result = SubtensorModule::clean_up_root_claimed_for_subnet(netuid, &mut weight_meter);
        // This function should return true when it completes its work
        assert!(
            result,
            "clean_up_root_claimed_for_subnet should complete successfully"
        );
    });
}

#[test]
fn test_purge_netuid() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some commitment data
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_cold,
            &owner_hot
        ));
        assert_ok!(CommitmentsPallet::<Test>::set_commitment(
            <Test as frame_system::Config>::RuntimeOrigin::signed(owner_hot),
            netuid,
            Box::new(
                CommitmentInfo::<<Test as pallet_commitments::Config>::MaxFields> {
                    fields: BoundedVec::try_from(vec![Data::Raw(
                        BoundedVec::try_from(vec![1, 2, 3]).unwrap()
                    )])
                    .unwrap(),
                }
            )
        ));

        // Verify commitment exists
        assert!(CommitmentsPallet::<Test>::commitment_of(netuid, owner_hot).is_some());

        // Test the purge function
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        let result =
            <Test as crate::Config>::CommitmentsInterface::purge_netuid(netuid, &mut weight_meter);
        assert!(
            result,
            "purge_netuid should return true when it successfully purges data"
        );

        // Verify commitment was purged
        assert!(CommitmentsPallet::<Test>::commitment_of(netuid, owner_hot).is_none());
    });
}

#[test]
fn test_clear_protocol_liquidity() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some stake to have alpha value
        let stake_tao: u64 = 1000;
        setup_reserves(
            netuid,
            (stake_tao * 1_000_000).into(),
            (stake_tao * 10_000_000).into(),
        );
        let amount: TaoBalance = (stake_tao).into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_cold,
            &owner_hot
        ));
        add_balance_to_coldkey_account(&owner_cold, amount);
        // Stake into subnet to create some alpha and potentially protocol liquidity
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // Test the clear protocol liquidity function (through swap interface)
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        let result =
            <Test as Config>::SwapInterface::clear_protocol_liquidity(netuid, &mut weight_meter);
        // This function should return true when it completes its work
        assert!(
            result,
            "clear_protocol_liquidity should complete successfully"
        );
    });
}

#[test]
fn test_remove_data_for_dissolved_networks_via_on_idle() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some stake to have alpha value and create various data
        let stake_tao: u64 = 1000;
        setup_reserves(
            netuid,
            (stake_tao * 1_000_000).into(),
            (stake_tao * 10_000_000).into(),
        );
        let amount: TaoBalance = (stake_tao).into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_cold,
            &owner_hot
        ));
        add_balance_to_coldkey_account(&owner_cold, amount);
        // Stake into subnet to create some alpha, locks, etc.
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // Add some commitment data
        assert_ok!(CommitmentsPallet::<Test>::set_commitment(
            <Test as frame_system::Config>::RuntimeOrigin::signed(owner_hot),
            netuid,
            Box::new(
                CommitmentInfo::<<Test as pallet_commitments::Config>::MaxFields> {
                    fields: BoundedVec::try_from(vec![Data::Raw(
                        BoundedVec::try_from(vec![1, 2, 3]).unwrap()
                    )])
                    .unwrap(),
                }
            )
        ));

        // Now test the full dissolution cleanup process by running on_idle multiple times
        // until all phases complete (this indirectly tests remove_data_for_dissolved_networks)
        let total_weight = Weight::from_parts(u64::MAX, u64::MAX);
        let mut remaining_weight = total_weight;

        // First dissolve the network
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));

        // Verify it's in the dissolved networks queue
        assert!(DissolveCleanupQueue::<Test>::get().contains(&netuid));

        // Run cleanup phases until completion
        let mut iterations = 0;
        let max_iterations = 30; // Should be enough to go through all phases

        while !DissolveCleanupQueue::<Test>::get().is_empty() && iterations < max_iterations {
            let used_weight = SubtensorModule::on_idle(0, remaining_weight);
            remaining_weight = remaining_weight.saturating_sub(used_weight);
            iterations += 1;

            // If we've used all weight, reset for next iteration
            if remaining_weight.is_zero() {
                remaining_weight = total_weight;
            }
        }

        // Verify the network has been fully removed
        assert!(!DissolveCleanupQueue::<Test>::get().contains(&netuid));
        assert_eq!(
            CurrentDissolveCleanupStatus::<Test>::get(),
            None,
            "Cleanup phase should be None after completion"
        );

        // Verify the subnet no longer exists
        assert!(!SubtensorModule::if_subnet_exist(netuid));

        // Verify data has been cleaned up
        assert_eq!(SubtensorModule::get_subnet_owner(netuid), U256::from(0));
        assert!(CommitmentsPallet::<Test>::commitment_of(netuid, owner_hot).is_none());
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_get_total_alpha_value() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some stake to have alpha value
        let stake_tao: u64 = 1000;
        setup_reserves(netuid, (stake_tao * 1_000_000).into(), (stake_tao * 10_000_000).into());
        let amount: TaoBalance = (stake_tao).into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(&owner_cold, &owner_hot));
        add_balance_to_coldkey_account(&owner_cold, amount);
        // Stake into subnet to create some alpha
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // Now test the function
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        assert!(
            run_resumable_netuid_cleanup_with_status(
                netuid,
                &mut weight_meter,
                &mut dissolve_cleanup_status(netuid),
                |netuid, weight_meter, last_key, status| {
                    SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                        netuid,
                        weight_meter,
                        last_key,
                        status,
                    )
                },
            ),
            "destroy_alpha_in_out_stakes_get_total_alpha_value should return true when there is alpha to process"
        );
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_settle_stakes() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some stake to have alpha value
        let stake_tao: u64 = 1000;
        setup_reserves(
            netuid,
            (stake_tao * 1_000_000).into(),
            (stake_tao * 10_000_000).into(),
        );
        let amount: TaoBalance = (stake_tao).into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner_cold,
            &owner_hot
        ));
        add_balance_to_coldkey_account(&owner_cold, amount);
        // Stake into subnet to create some alpha
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // First, we need to get the total alpha value (simulate the previous step)
        run_destroy_alpha_get_total_and_settle(netuid);
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_clean_alpha() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some stake to have alpha value
        let stake_tao: u64 = 1000;
        setup_reserves(netuid, (stake_tao * 1_000_000).into(), (stake_tao * 10_000_000).into());
        let amount: TaoBalance = (stake_tao).into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(&owner_cold, &owner_hot));
        add_balance_to_coldkey_account(&owner_cold, amount);
        // Stake into subnet to create some alpha
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // Simulate the previous two steps: get total alpha and settle stakes
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        let mut status = dissolve_cleanup_status(netuid);
        assert!(
            run_resumable_netuid_cleanup_with_status(
                netuid,
                &mut weight_meter,
                &mut status,
                |netuid, weight_meter, last_key, status| {
                    SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                        netuid,
                        weight_meter,
                        last_key,
                        status,
                    )
                },
            )
        );
        status.subnet_distributed_tao = Some(0);
        let mut weight_meter2 = frame_support::weights::WeightMeter::with_limit(w);
        assert!(
            run_resumable_netuid_cleanup_with_status(
                netuid,
                &mut weight_meter2,
                &mut status,
                |netuid, weight_meter, last_key, status| {
                    SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
                        netuid,
                        weight_meter,
                        last_key,
                        status,
                    )
                },
            )
        );
        // Now test the clean_alpha function
        let mut weight_meter3 = frame_support::weights::WeightMeter::with_limit(w);
        assert!(
            run_resumable_netuid_cleanup(
                netuid,
                &mut weight_meter3,
                SubtensorModule::destroy_alpha_in_out_stakes_clean_alpha,
            ),
            "destroy_alpha_in_out_stakes_clean_alpha should return true when there is alpha to clean"
        );
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_clear_hotkey_totals() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some stake to have alpha value and hotkey totals
        let stake_tao: u64 = 1000;
        setup_reserves(netuid, (stake_tao * 1_000_000).into(), (stake_tao * 10_000_000).into());
        let amount: TaoBalance = (stake_tao).into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(&owner_cold, &owner_hot));
        add_balance_to_coldkey_account(&owner_cold, amount);
        // Stake into subnet to create some alpha and hotkey totals
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // Simulate the previous three steps
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        let mut status = dissolve_cleanup_status(netuid);
        assert!(
            run_resumable_netuid_cleanup_with_status(
                netuid,
                &mut weight_meter,
                &mut status,
                |netuid, weight_meter, last_key, status| {
                    SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                        netuid,
                        weight_meter,
                        last_key,
                        status,
                    )
                },
            )
        );
        status.subnet_distributed_tao = Some(0);
        let mut weight_meter2 = frame_support::weights::WeightMeter::with_limit(w);
        assert!(
            run_resumable_netuid_cleanup_with_status(
                netuid,
                &mut weight_meter2,
                &mut status,
                |netuid, weight_meter, last_key, status| {
                    SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
                        netuid,
                        weight_meter,
                        last_key,
                        status,
                    )
                },
            )
        );
        let mut weight_meter3 = frame_support::weights::WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup(
            netuid,
            &mut weight_meter3,
            SubtensorModule::destroy_alpha_in_out_stakes_clean_alpha,
        ));
        // Now test the clear_hotkey_totals function
        let mut weight_meter4 = frame_support::weights::WeightMeter::with_limit(w);
        assert!(
            run_resumable_netuid_cleanup(
                netuid,
                &mut weight_meter4,
                SubtensorModule::destroy_alpha_in_out_stakes_clear_hotkey_totals,
            ),
            "destroy_alpha_in_out_stakes_clear_hotkey_totals should return true when there are hotkey totals to clear"
        );
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_clear_locks() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some stake to have alpha value and create locks
        let stake_tao: u64 = 1000;
        setup_reserves(netuid, (stake_tao * 1_000_000).into(), (stake_tao * 10_000_000).into());
        let amount: TaoBalance = (stake_tao).into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(&owner_cold, &owner_hot));
        add_balance_to_coldkey_account(&owner_cold, amount);
        // Stake into subnet to create some alpha and locks
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // Simulate the previous four steps
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        let mut status = dissolve_cleanup_status(netuid);
        assert!(
            run_resumable_netuid_cleanup_with_status(
                netuid,
                &mut weight_meter,
                &mut status,
                |netuid, weight_meter, last_key, status| {
                    SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                        netuid,
                        weight_meter,
                        last_key,
                        status,
                    )
                },
            )
        );
        status.subnet_distributed_tao = Some(0);
        let mut weight_meter2 = frame_support::weights::WeightMeter::with_limit(w);
        assert!(
            run_resumable_netuid_cleanup_with_status(
                netuid,
                &mut weight_meter2,
                &mut status,
                |netuid, weight_meter, last_key, status| {
                    SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
                        netuid,
                        weight_meter,
                        last_key,
                        status,
                    )
                },
            )
        );
        let mut weight_meter3 = frame_support::weights::WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup(
            netuid,
            &mut weight_meter3,
            SubtensorModule::destroy_alpha_in_out_stakes_clean_alpha,
        ));
        let mut weight_meter4 = frame_support::weights::WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup(
            netuid,
            &mut weight_meter4,
            SubtensorModule::destroy_alpha_in_out_stakes_clear_hotkey_totals,
        ));
        // Now test the clear_locks function
        let mut weight_meter5 = frame_support::weights::WeightMeter::with_limit(w);
        assert!(
            run_resumable_netuid_cleanup(
                netuid,
                &mut weight_meter5,
                SubtensorModule::destroy_alpha_in_out_stakes_clear_locks,
            ),
            "destroy_alpha_in_out_stakes_clear_locks should return true when there are locks to clear"
        );
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);

        // Add some stake to have alpha value and create locks, etc.
        let stake_tao: u64 = 1000;
        setup_reserves(netuid, (stake_tao * 1_000_000).into(), (stake_tao * 10_000_000).into());
        let amount: TaoBalance = (stake_tao).into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(&owner_cold, &owner_hot));
        add_balance_to_coldkey_account(&owner_cold, amount);
        // Stake into subnet to create some alpha and locks
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hot,
            &owner_cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));

        // Now test the main destroy function (which should call all the steps internally)
        let mut status = run_destroy_alpha_get_total_and_settle(netuid);
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        let result =
            SubtensorModule::destroy_alpha_in_out_stakes(netuid, &mut weight_meter, &mut status);
        assert!(result, "destroy_alpha_in_out_stakes should return true when it successfully processes the netuid");
    });
}

#[test]
fn test_clean_up_hotkey_swap_records() {
    new_test_ext(0).execute_with(|| {
        // Create two subnets: netuid 1 and netuid 2
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid_1 = add_dynamic_network(&owner_hot, &owner_cold);
        assert_eq!(netuid_1, 1.into());

        let owner_cold_2 = U256::from(2001);
        let owner_hot_2 = U256::from(2002);
        let netuid_2 = add_dynamic_network(&owner_hot_2, &owner_cold_2);
        assert_eq!(netuid_2, 2.into());

        // We will choose a block number such that block_number % interval == 1
        // With the default interval of 15, we use block_number = 16 (16 % 15 = 1)
        // So only netuid_1 (which is 1) will be processed because 1 % 15 == 1
        let block_number: u64 = 16; // 16 % 15 = 1

        // Insert some hotkey swap records for netuid_1
        // We'll insert two records: one old (should be removed) and one new (should remain)
        let coldkey_old = U256::from(3001);
        let coldkey_new = U256::from(3002);
        // Set an old swap block number: old enough to be removed
        let swap_block_old: u64 = 0; // This is definitely < block_number - interval (101 - 100 = 1)
        // Set a new swap block number: recent enough to remain
        let swap_block_new: u64 = 101; // This is >= block_number - interval (1) so should remain

        // Insert the records
        LastHotkeySwapOnNetuid::<Test>::insert(netuid_1, coldkey_old, swap_block_old);
        LastHotkeySwapOnNetuid::<Test>::insert(netuid_1, coldkey_new, swap_block_new);

        // Insert some hotkey swap records for netuid_2 (should not be processed because 2 % 100 != 1)
        let coldkey_other = U256::from(4001);
        let swap_block_other: u64 = 0; // old, but netuid_2 won't be processed
        LastHotkeySwapOnNetuid::<Test>::insert(netuid_2, coldkey_other, swap_block_other);

        // Also insert a record for netuid_2 with a new swap block number to show it remains untouched
        let coldkey_other_new = U256::from(4002);
        let swap_block_other_new: u64 = 101;
        LastHotkeySwapOnNetuid::<Test>::insert(netuid_2, coldkey_other_new, swap_block_other_new);

        // Before calling the function, verify the records exist
        assert!(LastHotkeySwapOnNetuid::<Test>::contains_key(
            netuid_1,
            coldkey_old
        ));
        assert!(LastHotkeySwapOnNetuid::<Test>::contains_key(
            netuid_1,
            coldkey_new
        ));
        assert!(LastHotkeySwapOnNetuid::<Test>::contains_key(
            netuid_2,
            coldkey_other
        ));
        assert!(LastHotkeySwapOnNetuid::<Test>::contains_key(
            netuid_2,
            coldkey_other_new
        ));

        // Call the function and get the returned weight
        let returned_weight = SubtensorModule::clean_up_hotkey_swap_records(block_number.into());

        // After the function call, for netuid_1:
        //   - The old record (coldkey_old, swap_block_old) should be removed because swap_block_old + interval < block_number
        //     (0 + 100 < 101 -> 100 < 101 -> true)
        //   - The new record (coldkey_new, swap_block_new) should remain because swap_block_new + interval >= block_number
        //     (101 + 100 >= 101 -> 201 >= 101 -> true)
        assert!(
            !LastHotkeySwapOnNetuid::<Test>::contains_key(netuid_1, coldkey_old),
            "Old hotkey swap record for netuid_1 should have been removed"
        );
        assert!(
            LastHotkeySwapOnNetuid::<Test>::contains_key(netuid_1, coldkey_new),
            "New hotkey swap record for netuid_1 should still exist"
        );
        // For netuid_2, since it was not processed (netuid_2 % interval != block_number % interval), both records should remain
        assert!(
            LastHotkeySwapOnNetuid::<Test>::contains_key(netuid_2, coldkey_other),
            "Hotkey swap record for netuid_2 should remain untouched"
        );
        assert!(
            LastHotkeySwapOnNetuid::<Test>::contains_key(netuid_2, coldkey_other_new),
            "Hotkey swap record for netuid_2 should remain untouched"
        );

        // We can also check that the weight returned is reasonable (non-zero and not max)
        // Note: Weight comparison is tricky, but we can at least check it's not zero
        assert!(
            returned_weight.ref_time() > 0,
            "Returned weight should have positive ref_time"
        );
    });
}

#[test]
fn test_remove_network_lock() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let other_netuid = NetUid::from(2);
        let cold_1 = U256::from(1001);
        let cold_2 = U256::from(1002);
        let hot_1 = U256::from(2001);
        let hot_2 = U256::from(2002);
        let lock_state = crate::staking::lock::LockState {
            locked_mass: 10u64.into(),
            conviction: substrate_fixed::types::U64F64::from_num(1.5),
            last_update: 1,
        };

        Lock::<Test>::insert((cold_1, netuid, hot_1), lock_state.clone());
        Lock::<Test>::insert((cold_2, netuid, hot_2), lock_state.clone());
        Lock::<Test>::insert((cold_1, other_netuid, hot_1), lock_state);

        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup(
            netuid,
            &mut weight_meter,
            SubtensorModule::remove_network_lock,
        ));

        assert!(!Lock::<Test>::contains_key((cold_1, netuid, hot_1)));
        assert!(!Lock::<Test>::contains_key((cold_2, netuid, hot_2)));
        assert!(Lock::<Test>::contains_key((cold_1, other_netuid, hot_1)));
    });
}

#[test]
fn test_remove_network_decaying_lock() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let other_netuid = NetUid::from(2);
        let cold_1 = U256::from(1001);
        let cold_2 = U256::from(1002);

        DecayingLock::<Test>::insert(cold_1, netuid, true);
        DecayingLock::<Test>::insert(cold_2, netuid, true);
        DecayingLock::<Test>::insert(cold_1, other_netuid, true);

        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup(
            netuid,
            &mut weight_meter,
            SubtensorModule::remove_network_decaying_lock,
        ));

        assert!(!DecayingLock::<Test>::contains_key(cold_1, netuid));
        assert!(!DecayingLock::<Test>::contains_key(cold_2, netuid));
        assert!(DecayingLock::<Test>::contains_key(cold_1, other_netuid));
    });
}

#[test]
fn test_remove_network_hotkey_and_owner_lock_maps() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let other_netuid = NetUid::from(2);
        let hot_1 = U256::from(2001);
        let hot_2 = U256::from(2002);
        let lock_state = crate::staking::lock::LockState {
            locked_mass: 10u64.into(),
            conviction: substrate_fixed::types::U64F64::from_num(1.5),
            last_update: 1,
        };

        DissolveCleanupQueue::<Test>::set(vec![netuid]);

        HotkeyLock::<Test>::insert(netuid, hot_1, lock_state.clone());
        HotkeyLock::<Test>::insert(netuid, hot_2, lock_state.clone());
        HotkeyLock::<Test>::insert(other_netuid, hot_1, lock_state.clone());

        DecayingHotkeyLock::<Test>::insert(netuid, hot_1, lock_state.clone());
        DecayingHotkeyLock::<Test>::insert(netuid, hot_2, lock_state.clone());
        DecayingHotkeyLock::<Test>::insert(other_netuid, hot_1, lock_state.clone());

        OwnerLock::<Test>::insert(netuid, lock_state.clone());
        OwnerLock::<Test>::insert(other_netuid, lock_state);

        run_block_idle();

        assert!(!HotkeyLock::<Test>::contains_key(netuid, hot_1));
        assert!(!HotkeyLock::<Test>::contains_key(netuid, hot_2));
        assert!(HotkeyLock::<Test>::iter_prefix(netuid).next().is_none());

        assert!(!DecayingHotkeyLock::<Test>::contains_key(netuid, hot_1));
        assert!(!DecayingHotkeyLock::<Test>::contains_key(netuid, hot_2));
        assert!(
            DecayingHotkeyLock::<Test>::iter_prefix(netuid)
                .next()
                .is_none()
        );

        assert!(!OwnerLock::<Test>::contains_key(netuid));

        assert!(HotkeyLock::<Test>::contains_key(other_netuid, hot_1));
        assert!(DecayingHotkeyLock::<Test>::contains_key(
            other_netuid,
            hot_1
        ));
        assert!(OwnerLock::<Test>::contains_key(other_netuid));
    });
}

#[test]
fn test_remove_network_decaying_lock_resumes_with_limited_weight() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        for i in 0..5 {
            DecayingLock::<Test>::insert(U256::from(10_000 + i), netuid, true);
        }

        let read_weight = <Test as frame_system::Config>::DbWeight::get().reads(1);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(read_weight);
        let (done, mut last_key) =
            SubtensorModule::remove_network_decaying_lock(netuid, &mut weight_meter, None);
        assert!(!done);

        let mut iterations = 0;
        while DecayingLock::<Test>::iter().any(|(_, n, _)| n == netuid) {
            let mut weight_meter =
                frame_support::weights::WeightMeter::with_limit(Weight::from_parts(u64::MAX, 0));
            let (done, new_key) =
                SubtensorModule::remove_network_decaying_lock(netuid, &mut weight_meter, last_key);
            last_key = new_key;
            assert!(
                done,
                "remove_network_decaying_lock should finish once all entries are removed"
            );
            iterations += 1;
            assert!(
                iterations < 10,
                "cleanup should complete within a few passes"
            );
        }
        assert_eq!(
            DecayingLock::<Test>::iter()
                .filter(|(_, n, _)| *n == netuid)
                .count(),
            0
        );
    });
}

#[test]
fn test_remove_network_childkeys_resumes_with_limited_weight() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        for i in 0..5 {
            ChildKeys::<Test>::insert(U256::from(20_000 + i), netuid, vec![(1, U256::from(1))]);
        }

        let read_weight = <Test as frame_system::Config>::DbWeight::get().reads(1);
        let mut weight_meter = WeightMeter::with_limit(read_weight);
        let (done, mut last_key) =
            SubtensorModule::remove_network_childkeys(netuid, &mut weight_meter, None);
        assert!(!done);

        let mut iterations = 0;
        while ChildKeys::<Test>::iter().any(|(_, n, _)| n == netuid) {
            let mut weight_meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
            let (done, new_key) =
                SubtensorModule::remove_network_childkeys(netuid, &mut weight_meter, last_key);
            last_key = new_key;
            assert!(
                done,
                "remove_network_childkeys should finish once all entries are removed"
            );
            iterations += 1;
            assert!(
                iterations < 10,
                "cleanup should complete within a few passes"
            );
        }
        assert_eq!(
            ChildKeys::<Test>::iter()
                .filter(|(_, n, _)| *n == netuid)
                .count(),
            0
        );
    });
}
