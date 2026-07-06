#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
use super::mock::*;
use crate::{subnets::leasing::SubnetLeaseOf, *};
use frame_support::{StorageDoubleMap, assert_err, assert_ok};
use sp_core::U256;
use sp_runtime::Percent;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::AlphaBalance;

#[test]
fn test_register_leased_network_works() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 390_000_000_000), // 390 TAO
        ];
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Register the leased network
        let end_block = 500;
        let emissions_share = Percent::from_percent(30);
        assert_ok!(SubtensorModule::register_leased_network(
            RuntimeOrigin::signed(beneficiary),
            emissions_share,
            Some(end_block),
        ));

        // Ensure the lease was created
        let lease_id = 0;
        let lease = SubnetLeases::<Test>::get(lease_id).unwrap();
        assert_eq!(lease.beneficiary, beneficiary);
        assert_eq!(lease.emissions_share, emissions_share);
        assert_eq!(lease.end_block, Some(end_block));

        // Ensure the subnet exists
        assert!(SubnetMechanism::<Test>::contains_key(lease.netuid));

        // Ensure the subnet uid to lease id mapping exists
        assert_eq!(
            SubnetUidToLeaseId::<Test>::get(lease.netuid),
            Some(lease_id)
        );

        // Ensure the beneficiary has been added as a proxy
        assert!(PROXIES.with_borrow(|proxies| proxies.0 == vec![(lease.coldkey, beneficiary)]));

        // Ensure the lease shares have been created for each contributor
        let contributor1_share = U64F64::from(contributions[0].1).saturating_div(U64F64::from(cap));
        assert_eq!(
            SubnetLeaseShares::<Test>::get(lease_id, contributions[0].0),
            contributor1_share
        );
        let contributor2_share = U64F64::from(contributions[1].1).saturating_div(U64F64::from(cap));
        assert_eq!(
            SubnetLeaseShares::<Test>::get(lease_id, contributions[1].0),
            contributor2_share
        );

        // Ensure the lease hotkey has 0 take from staking
        assert_eq!(SubtensorModule::get_hotkey_take(&lease.hotkey), 0);

        // Ensure each contributor and beneficiary has been refunded their share of the leftover cap
        let leftover_cap = cap.saturating_sub(lease.cost.into());

        let expected_contributor1_refund = U64F64::from(leftover_cap)
            .saturating_mul(contributor1_share)
            .floor()
            .to_num::<u64>();
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&contributions[0].0),
            expected_contributor1_refund.into()
        );

        let expected_contributor2_refund = U64F64::from(leftover_cap)
            .saturating_mul(contributor2_share)
            .floor()
            .to_num::<u64>();
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&contributions[1].0),
            expected_contributor2_refund.into()
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&beneficiary),
            (leftover_cap - (expected_contributor1_refund + expected_contributor2_refund)).into()
        );

        // Ensure the event is emitted
        assert_eq!(
            last_event(),
            crate::Event::<Test>::SubnetLeaseCreated {
                beneficiary,
                lease_id,
                netuid: lease.netuid,
                end_block: Some(end_block),
            }
            .into()
        );
    });
}

#[test]
fn test_register_leased_network_fails_if_bad_origin() {
    new_test_ext(1).execute_with(|| {
        let end_block = 500;
        let emissions_share = Percent::from_percent(30);

        assert_err!(
            SubtensorModule::register_leased_network(
                RuntimeOrigin::none(),
                emissions_share,
                Some(end_block),
            ),
            DispatchError::BadOrigin,
        );

        assert_err!(
            SubtensorModule::register_leased_network(
                RuntimeOrigin::root(),
                emissions_share,
                Some(end_block),
            ),
            DispatchError::BadOrigin,
        );
    });
}

#[test]
fn test_register_leased_network_fails_if_crowdloan_does_not_exists() {
    new_test_ext(1).execute_with(|| {
        let beneficiary = U256::from(1);
        let end_block = 500;
        let emissions_share = Percent::from_percent(30);

        assert_err!(
            SubtensorModule::register_leased_network(
                RuntimeOrigin::signed(beneficiary),
                emissions_share,
                Some(end_block),
            ),
            pallet_crowdloan::Error::<Test>::InvalidCrowdloanId,
        );
    });
}

#[test]
fn test_register_lease_network_fails_if_current_crowdloan_id_is_not_set() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 390_000_000_000), // 390 TAO
        ];
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Mark as if the current crowdloan id is not set
        pallet_crowdloan::CurrentCrowdloanId::<Test>::set(None);

        let end_block = 500;
        let emissions_share = Percent::from_percent(30);

        assert_err!(
            SubtensorModule::register_leased_network(
                RuntimeOrigin::signed(beneficiary),
                emissions_share,
                Some(end_block),
            ),
            pallet_crowdloan::Error::<Test>::InvalidCrowdloanId,
        );
    });
}

#[test]
fn test_register_leased_network_fails_if_origin_is_not_crowdloan_creator() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 390_000_000_000), // 390 TAO
        ];
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        let end_block = 500;
        let emissions_share = Percent::from_percent(30);

        assert_err!(
            SubtensorModule::register_leased_network(
                RuntimeOrigin::signed(U256::from(2)),
                emissions_share,
                Some(end_block),
            ),
            Error::<Test>::InvalidLeaseBeneficiary,
        );
    });
}

#[test]
fn test_register_lease_network_fails_if_end_block_is_in_the_past() {
    new_test_ext(501).execute_with(|| {
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 390_000_000_000), // 390 TAO
        ];
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        let end_block = 500;
        let emissions_share = Percent::from_percent(30);

        assert_err!(
            SubtensorModule::register_leased_network(
                RuntimeOrigin::signed(beneficiary),
                emissions_share,
                Some(end_block),
            ),
            Error::<Test>::LeaseCannotEndInThePast,
        );
    });
}

#[test]
fn test_terminate_lease_works() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![(U256::from(2), 990_000_000_000)]; // 990 TAO
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let end_block = 500;
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let emissions_share = Percent::from_percent(30);
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            Some(tao_to_stake),
        );

        // Run to the end of the lease
        run_to_block(end_block);

        // Create a hotkey for the beneficiary
        let hotkey = U256::from(3);
        let _ = SubtensorModule::create_account_if_non_existent(&beneficiary, &hotkey);

        // Terminate the lease
        assert_ok!(SubtensorModule::terminate_lease(
            RuntimeOrigin::signed(beneficiary),
            lease_id,
            hotkey,
        ));

        // Ensure the beneficiary is now the owner of the subnet
        assert_eq!(SubnetOwner::<Test>::get(lease.netuid), beneficiary);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(lease.netuid), hotkey);

        // Ensure everything has been cleaned up
        assert_eq!(SubnetLeases::<Test>::get(lease_id), None);
        assert!(!SubnetLeaseShares::<Test>::contains_prefix(lease_id));
        assert!(!AccumulatedLeaseDividends::<Test>::contains_key(lease_id));

        // Ensure the beneficiary has been removed as a proxy
        assert!(PROXIES.with_borrow(|proxies| proxies.0.is_empty()));

        // Ensure the event is emitted
        assert_eq!(
            last_event(),
            crate::Event::<Test>::SubnetLeaseTerminated {
                beneficiary: lease.beneficiary,
                netuid: lease.netuid,
            }
            .into()
        );
    });
}

#[test]
fn test_terminate_lease_fails_if_bad_origin() {
    new_test_ext(1).execute_with(|| {
        let lease_id = 0;
        let hotkey = U256::from(1);

        assert_err!(
            SubtensorModule::terminate_lease(RuntimeOrigin::none(), lease_id, hotkey),
            DispatchError::BadOrigin,
        );

        assert_err!(
            SubtensorModule::terminate_lease(RuntimeOrigin::root(), lease_id, hotkey),
            DispatchError::BadOrigin,
        );
    });
}

#[test]
fn test_terminate_lease_fails_if_lease_does_not_exist() {
    new_test_ext(1).execute_with(|| {
        let lease_id = 0;
        let beneficiary = U256::from(1);
        let hotkey = U256::from(2);

        assert_err!(
            SubtensorModule::terminate_lease(RuntimeOrigin::signed(beneficiary), lease_id, hotkey),
            Error::<Test>::LeaseDoesNotExist,
        );
    });
}

#[test]
fn test_terminate_lease_fails_if_origin_is_not_beneficiary() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![(U256::from(2), 990_000_000_000)]; // 990 TAO
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let end_block = 500;
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let emissions_share = Percent::from_percent(30);
        let (lease_id, _lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            Some(tao_to_stake),
        );

        // Run to the end of the lease
        run_to_block(end_block);

        // Create a hotkey for the beneficiary
        let hotkey = U256::from(3);
        let _ = SubtensorModule::create_account_if_non_existent(&beneficiary, &hotkey);

        // Terminate the lease
        assert_err!(
            SubtensorModule::terminate_lease(
                RuntimeOrigin::signed(U256::from(42)),
                lease_id,
                hotkey,
            ),
            Error::<Test>::ExpectedBeneficiaryOrigin,
        );
    });
}

#[test]
fn test_terminate_lease_fails_if_lease_has_no_end_block() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![(U256::from(2), 990_000_000_000)]; // 990 TAO
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let emissions_share = Percent::from_percent(30);
        let (lease_id, lease) =
            setup_leased_network(beneficiary, emissions_share, None, Some(tao_to_stake));

        // Create a hotkey for the beneficiary
        let hotkey = U256::from(3);
        let _ = SubtensorModule::create_account_if_non_existent(&beneficiary, &hotkey);

        // Terminate the lease
        assert_err!(
            SubtensorModule::terminate_lease(
                RuntimeOrigin::signed(lease.beneficiary),
                lease_id,
                hotkey,
            ),
            Error::<Test>::LeaseHasNoEndBlock,
        );
    });
}

#[test]
fn test_terminate_lease_fails_if_lease_has_not_ended() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![(U256::from(2), 990_000_000_000)]; // 990 TAO
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let end_block = 500;
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let emissions_share = Percent::from_percent(30);
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            Some(tao_to_stake),
        );

        // Create a hotkey for the beneficiary
        let hotkey = U256::from(3);
        let _ = SubtensorModule::create_account_if_non_existent(&beneficiary, &hotkey);

        // Terminate the lease
        assert_err!(
            SubtensorModule::terminate_lease(
                RuntimeOrigin::signed(lease.beneficiary),
                lease_id,
                hotkey,
            ),
            Error::<Test>::LeaseHasNotEnded,
        );
    });
}

#[test]
fn test_terminate_lease_fails_if_beneficiary_does_not_own_hotkey() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![(U256::from(2), 990_000_000_000)]; // 990 TAO
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let end_block = 500;
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let emissions_share = Percent::from_percent(30);
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            Some(tao_to_stake),
        );

        // Run to the end of the lease
        run_to_block(end_block);

        // Terminate the lease
        assert_err!(
            SubtensorModule::terminate_lease(
                RuntimeOrigin::signed(lease.beneficiary),
                lease_id,
                U256::from(42),
            ),
            Error::<Test>::BeneficiaryDoesNotOwnHotkey,
        );
    });
}
#[test]
fn test_distribute_lease_network_dividends_multiple_contributors_works() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 390_000_000_000), // 390 TAO
        ];
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let end_block = 500;
        let emissions_share = Percent::from_percent(30);
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            Some(tao_to_stake),
        );

        // Setup the correct block to distribute dividends
        run_to_block(<Test as Config>::LeaseDividendsDistributionInterval::get() as u64);

        // Get the initial alpha for the contributors and beneficiary and ensure they are zero
        let contributor1_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[0].0,
            lease.netuid,
        );
        assert_eq!(contributor1_alpha_before, AlphaBalance::ZERO);
        let contributor2_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[1].0,
            lease.netuid,
        );
        assert_eq!(contributor2_alpha_before, AlphaBalance::ZERO);
        let beneficiary_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &beneficiary,
            lease.netuid,
        );
        assert_eq!(beneficiary_alpha_before, AlphaBalance::ZERO);

        // Setup some previously accumulated dividends
        let accumulated_dividends = AlphaBalance::from(10_000_000_000_u64);
        AccumulatedLeaseDividends::<Test>::insert(lease_id, accumulated_dividends);

        // Distribute the dividends
        let owner_cut_alpha = AlphaBalance::from(5_000_000_000_u64);
        SubtensorModule::distribute_leased_network_dividends(lease_id, owner_cut_alpha);

        // Ensure the dividends were distributed correctly relative to their shares
        let distributed_alpha =
            accumulated_dividends + emissions_share.mul_ceil(owner_cut_alpha.to_u64()).into();
        assert_ne!(distributed_alpha, AlphaBalance::ZERO);

        let contributor1_alpha_delta = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[0].0,
            lease.netuid,
        )
        .saturating_sub(contributor1_alpha_before);
        assert_ne!(contributor1_alpha_delta, AlphaBalance::ZERO);

        let contributor2_alpha_delta = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[1].0,
            lease.netuid,
        )
        .saturating_sub(contributor2_alpha_before);
        assert_ne!(contributor2_alpha_delta, AlphaBalance::ZERO);

        let beneficiary_alpha_delta = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &beneficiary,
            lease.netuid,
        )
        .saturating_sub(beneficiary_alpha_before);
        assert_ne!(beneficiary_alpha_delta, AlphaBalance::ZERO);

        // What has been distributed should be equal to the sum of all contributors received alpha
        assert_eq!(
            distributed_alpha,
            (beneficiary_alpha_delta + contributor1_alpha_delta + contributor2_alpha_delta).into()
        );

        let expected_contributor1_alpha =
            SubnetLeaseShares::<Test>::get(lease_id, contributions[0].0)
                .saturating_mul(U64F64::from(distributed_alpha.to_u64()))
                .ceil()
                .to_num::<u64>();
        assert_eq!(contributor1_alpha_delta, expected_contributor1_alpha.into());
        assert_eq!(
            System::events()[3].event,
            RuntimeEvent::SubtensorModule(Event::SubnetLeaseDividendsDistributed {
                lease_id,
                contributor: contributions[0].0.into(),
                alpha: expected_contributor1_alpha.into(),
            },)
        );

        let expected_contributor2_alpha =
            SubnetLeaseShares::<Test>::get(lease_id, contributions[1].0)
                .saturating_mul(U64F64::from(distributed_alpha.to_u64()))
                .ceil()
                .to_num::<u64>();
        assert_eq!(contributor2_alpha_delta, expected_contributor2_alpha.into());
        assert_eq!(
            System::events()[6].event,
            RuntimeEvent::SubtensorModule(Event::SubnetLeaseDividendsDistributed {
                lease_id,
                contributor: contributions[1].0.into(),
                alpha: expected_contributor2_alpha.into(),
            },)
        );

        // The beneficiary should have received the remaining dividends
        let expected_beneficiary_alpha = distributed_alpha.to_u64()
            - (expected_contributor1_alpha + expected_contributor2_alpha);
        assert_eq!(beneficiary_alpha_delta, expected_beneficiary_alpha.into());
        assert_eq!(
            System::events()[9].event,
            RuntimeEvent::SubtensorModule(Event::SubnetLeaseDividendsDistributed {
                lease_id,
                contributor: beneficiary.into(),
                alpha: expected_beneficiary_alpha.into(),
            },)
        );

        // Ensure nothing was accumulated for later distribution
        assert_eq!(
            AccumulatedLeaseDividends::<Test>::get(lease_id),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_distribute_lease_network_dividends_only_beneficiary_works() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![(U256::from(1), 990_000_000_000)]; // 990 TAO
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let end_block = 500;
        let emissions_share = Percent::from_percent(30);
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            Some(tao_to_stake),
        );

        // Setup the correct block to distribute dividends
        run_to_block(<Test as Config>::LeaseDividendsDistributionInterval::get() as u64);

        // Get the initial alpha for the beneficiary and ensure it is zero
        let beneficiary_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &beneficiary,
            lease.netuid,
        );
        assert_eq!(beneficiary_alpha_before, AlphaBalance::ZERO);

        // Setup some previously accumulated dividends
        let accumulated_dividends = AlphaBalance::from(10_000_000_000_u64);
        AccumulatedLeaseDividends::<Test>::insert(lease_id, accumulated_dividends);

        // Distribute the dividends
        let owner_cut_alpha = AlphaBalance::from(5_000_000_000_u64);
        SubtensorModule::distribute_leased_network_dividends(lease_id, owner_cut_alpha);

        // Ensure the dividends were distributed correctly relative to their shares
        let distributed_alpha =
            accumulated_dividends + emissions_share.mul_ceil(owner_cut_alpha.to_u64()).into();
        assert_ne!(distributed_alpha, AlphaBalance::ZERO);
        let beneficiary_alpha_delta = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &beneficiary,
            lease.netuid,
        )
        .saturating_sub(beneficiary_alpha_before);
        assert_eq!(beneficiary_alpha_delta, distributed_alpha.into());
        assert_last_event::<Test>(RuntimeEvent::SubtensorModule(
            Event::SubnetLeaseDividendsDistributed {
                lease_id,
                contributor: beneficiary.into(),
                alpha: distributed_alpha,
            },
        ));

        // Ensure nothing was accumulated for later distribution
        assert_eq!(
            AccumulatedLeaseDividends::<Test>::get(lease_id),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_distribute_lease_network_dividends_accumulates_if_not_the_correct_block() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 390_000_000_000), // 390 TAO
        ];
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let end_block = 500;
        let emissions_share = Percent::from_percent(30);
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            Some(tao_to_stake),
        );

        // Setup incorrect block to distribute dividends
        run_to_block(<Test as Config>::LeaseDividendsDistributionInterval::get() as u64 + 1);

        // Get the initial alpha for the contributors and beneficiary and ensure they are zero
        let contributor1_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[0].0,
            lease.netuid,
        );
        assert_eq!(contributor1_alpha_before, AlphaBalance::ZERO);
        let contributor2_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[1].0,
            lease.netuid,
        );
        assert_eq!(contributor2_alpha_before, AlphaBalance::ZERO);
        let beneficiary_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &beneficiary,
            lease.netuid,
        );
        assert_eq!(beneficiary_alpha_before, AlphaBalance::ZERO);

        // Setup some previously accumulated dividends
        let accumulated_dividends = AlphaBalance::from(10_000_000_000_u64);
        AccumulatedLeaseDividends::<Test>::insert(lease_id, accumulated_dividends);

        // Distribute the dividends
        let owner_cut_alpha = AlphaBalance::from(5_000_000_000_u64);
        SubtensorModule::distribute_leased_network_dividends(lease_id, owner_cut_alpha);

        // Ensure the dividends were not distributed
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &contributions[0].0,
                lease.netuid
            ),
            contributor1_alpha_before
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &contributions[1].0,
                lease.netuid
            ),
            contributor2_alpha_before
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &beneficiary,
                lease.netuid
            ),
            beneficiary_alpha_before
        );

        // Ensure we correctly accumulated the dividends
        assert_eq!(
            AccumulatedLeaseDividends::<Test>::get(lease_id),
            (accumulated_dividends + emissions_share.mul_ceil(owner_cut_alpha.to_u64()).into())
                .into()
        );
    });
}

#[test]
fn test_distribute_lease_network_dividends_does_nothing_if_lease_does_not_exist() {
    new_test_ext(1).execute_with(|| {
        let lease_id = 0;
        let owner_cut_alpha = AlphaBalance::from(5_000_000);
        SubtensorModule::distribute_leased_network_dividends(lease_id, owner_cut_alpha);
    });
}

#[test]
fn test_distribute_lease_network_dividends_does_nothing_if_lease_has_ended() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 390_000_000_000), // 390 TAO
        ];
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let end_block = 500;
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let emissions_share = Percent::from_percent(30);
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            Some(tao_to_stake),
        );

        // Run to the end of the lease
        run_to_block(end_block);

        // Get the initial alpha for the contributors and beneficiary and ensure they are zero
        let contributor1_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[0].0,
            lease.netuid,
        );
        assert_eq!(contributor1_alpha_before, AlphaBalance::ZERO);
        let contributor2_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[1].0,
            lease.netuid,
        );
        assert_eq!(contributor2_alpha_before, AlphaBalance::ZERO);
        let beneficiary_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &beneficiary,
            lease.netuid,
        );
        assert_eq!(beneficiary_alpha_before, AlphaBalance::ZERO);

        // No dividends are present, lease is new
        let accumulated_dividends_before = AccumulatedLeaseDividends::<Test>::get(lease_id);
        assert_eq!(accumulated_dividends_before, AlphaBalance::ZERO);

        // Try to distribute the dividends
        let owner_cut_alpha = AlphaBalance::from(5_000_000_000_u64);
        SubtensorModule::distribute_leased_network_dividends(lease_id, owner_cut_alpha);

        // Ensure the dividends were not distributed
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &contributions[0].0,
                lease.netuid
            ),
            contributor1_alpha_before
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &contributions[1].0,
                lease.netuid
            ),
            contributor2_alpha_before
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &beneficiary,
                lease.netuid
            ),
            beneficiary_alpha_before
        );
        // Ensure nothing was accumulated for later distribution
        assert_eq!(
            AccumulatedLeaseDividends::<Test>::get(lease_id),
            accumulated_dividends_before
        );
    });
}

#[test]
fn test_distribute_lease_network_dividends_accumulates_if_amount_is_too_low() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 390_000_000_000), // 390 TAO
        ];

        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let end_block = 500;
        let emissions_share = Percent::from_percent(30);
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            None, // We don't add any liquidity
        );

        // Get the initial alpha for the contributors and beneficiary and ensure they are zero
        let contributor1_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[0].0,
            lease.netuid,
        );
        assert_eq!(contributor1_alpha_before, AlphaBalance::ZERO);
        let contributor2_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[1].0,
            lease.netuid,
        );
        assert_eq!(contributor2_alpha_before, AlphaBalance::ZERO);
        let beneficiary_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &beneficiary,
            lease.netuid,
        );
        assert_eq!(beneficiary_alpha_before, AlphaBalance::ZERO);

        // Try to distribute the dividends
        let owner_cut_alpha = AlphaBalance::from(5_000);
        SubtensorModule::distribute_leased_network_dividends(lease_id, owner_cut_alpha);

        // Ensure the dividends were not distributed
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &contributions[0].0,
                lease.netuid
            ),
            contributor1_alpha_before
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &contributions[1].0,
                lease.netuid
            ),
            contributor2_alpha_before
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &beneficiary,
                lease.netuid
            ),
            beneficiary_alpha_before
        );
        // Ensure the correct amount of alpha was accumulated for later dividends distribution
        assert_eq!(
            AccumulatedLeaseDividends::<Test>::get(lease_id),
            emissions_share.mul_ceil(owner_cut_alpha.to_u64()).into()
        );
    });
}

#[test]
fn test_distribute_lease_network_dividends_accumulates_if_insufficient_liquidity() {
    new_test_ext(1).execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 390_000_000_000), // 390 TAO
        ];

        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let end_block = 500;
        let emissions_share = Percent::from_percent(30);
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            None, // We don't add any liquidity
        );

        let contributor1_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[0].0,
            lease.netuid,
        );
        assert_eq!(contributor1_alpha_before, AlphaBalance::ZERO);
        let contributor2_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &contributions[1].0,
            lease.netuid,
        );
        assert_eq!(contributor2_alpha_before, AlphaBalance::ZERO);
        let beneficiary_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &beneficiary,
            lease.netuid,
        );
        assert_eq!(beneficiary_alpha_before, AlphaBalance::ZERO);

        // Try to distribute the dividends
        let owner_cut_alpha = AlphaBalance::from(5_000_000);
        SubtensorModule::distribute_leased_network_dividends(lease_id, owner_cut_alpha);

        // Ensure the dividends were not distributed
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &contributions[0].0,
                lease.netuid
            ),
            contributor1_alpha_before
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &contributions[1].0,
                lease.netuid
            ),
            contributor2_alpha_before
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &beneficiary,
                lease.netuid
            ),
            beneficiary_alpha_before
        );
        // Ensure the correct amount of alpha was accumulated for later dividends distribution
        assert_eq!(
            AccumulatedLeaseDividends::<Test>::get(lease_id),
            emissions_share.mul_ceil(owner_cut_alpha.to_u64()).into()
        );
    });
}

fn setup_crowdloan(
    id: u32,
    deposit: u64,
    cap: u64,
    beneficiary: U256,
    contributions: &[(U256, u64)],
) {
    let funds_account = U256::from(42424242 + id);
    let deposit = TaoBalance::from(deposit);
    let cap = TaoBalance::from(cap);

    pallet_crowdloan::Crowdloans::<Test>::insert(
        id,
        pallet_crowdloan::CrowdloanInfo {
            creator: beneficiary,
            deposit,
            min_contribution: TaoBalance::ZERO,
            end: 0,
            cap,
            raised: cap,
            finalized: false,
            funds_account,
            call: None,
            target_address: None,
            contributors_count: 1 + contributions.len() as u32,
        },
    );

    // Simulate contributions
    pallet_crowdloan::Contributions::<Test>::insert(id, beneficiary, deposit);
    for (contributor, amount) in contributions {
        let amount = TaoBalance::from(*amount);
        pallet_crowdloan::Contributions::<Test>::insert(id, contributor, amount);
    }

    add_balance_to_coldkey_account(&funds_account, cap);

    // Mark the crowdloan as finalizing
    pallet_crowdloan::CurrentCrowdloanId::<Test>::set(Some(0));
}

fn setup_leased_network(
    beneficiary: U256,
    emissions_share: Percent,
    end_block: Option<u64>,
    tao_to_stake: Option<u64>,
) -> (u32, SubnetLeaseOf<Test>) {
    let lease_id = 0;
    assert_ok!(SubtensorModule::do_register_leased_network(
        RuntimeOrigin::signed(beneficiary),
        emissions_share,
        end_block,
    ));

    // Configure subnet and add some stake
    let lease = SubnetLeases::<Test>::get(lease_id).unwrap();
    let netuid = lease.netuid;
    SubtokenEnabled::<Test>::insert(netuid, true);

    if let Some(tao_to_stake) = tao_to_stake {
        add_balance_to_coldkey_account(&lease.coldkey, tao_to_stake.into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(lease.coldkey),
            lease.hotkey,
            netuid,
            tao_to_stake.into()
        ));
    }

    (lease_id, lease)
}
