#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
use super::mock::*;
use crate::{subnets::leasing::SubnetLeaseOf, *};
use frame_support::{StorageDoubleMap, assert_err, assert_ok};
use pallet_subtensor_utility as pallet_utility;
use sp_core::U256;
use sp_runtime::Percent;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::AlphaBalance;

#[test]
fn test_coldkey_swap_migrates_lease_shares_beneficiary_and_proxy() {
    new_test_ext(1).execute_with(|| {
        let beneficiary = U256::from(1);
        let contributor = U256::from(2);
        let destination = U256::from(3);
        let new_beneficiary = U256::from(4);
        setup_crowdloan(
            0,
            10_000_000_000,
            1_000_000_000_000,
            beneficiary,
            &[(contributor, 600_000_000_000), (destination, 390_000_000_000)],
        );
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            Percent::from_percent(30),
            Some(500),
            Some(100_000_000_000),
        );
        let combined_share = SubnetLeaseShares::<Test>::get(lease_id, contributor)
            + SubnetLeaseShares::<Test>::get(lease_id, destination);

        // The destination already owns a share, but has no stake yet.
        assert_ok!(SubtensorModule::do_swap_coldkey(&contributor, &destination));
        assert!(!SubnetLeaseShares::<Test>::contains_key(lease_id, contributor));
        assert_eq!(SubnetLeaseShares::<Test>::get(lease_id, destination), combined_share);
        assert_ok!(SubtensorModule::do_swap_coldkey(&beneficiary, &new_beneficiary));
        let migrated = SubnetLeases::<Test>::get(lease_id).unwrap();
        assert_eq!(migrated.beneficiary, new_beneficiary);
        assert_eq!(migrated.coldkey, lease.coldkey);
        assert_eq!(migrated.hotkey, lease.hotkey);
        assert_eq!(SubnetOwner::<Test>::get(lease.netuid), lease.coldkey);
        assert!(PROXIES.with_borrow(|proxies| {
            proxies.0.contains(&(lease.coldkey, new_beneficiary))
                && !proxies.0.contains(&(lease.coldkey, beneficiary))
        }));

        System::set_block_number(<Test as Config>::LeaseDividendsDistributionInterval::get() as u64);
        SubtensorModule::distribute_leased_network_dividends(
            lease_id,
            AlphaBalance::from(5_000_000_000_u64),
        );
        for retired in [contributor, beneficiary] {
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &lease.hotkey, &retired, lease.netuid,
                ),
                AlphaBalance::ZERO,
            );
        }
        for current in [destination, new_beneficiary] {
            assert!(SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey, &current, lease.netuid,
            ) > AlphaBalance::ZERO);
        }

        System::set_block_number(500);
        let hotkey = U256::from(5);
        assert_ok!(SubtensorModule::create_account_if_non_existent(&new_beneficiary, &hotkey));
        assert_err!(
            SubtensorModule::do_terminate_lease(RuntimeOrigin::signed(beneficiary), lease_id, hotkey),
            Error::<Test>::ExpectedBeneficiaryOrigin,
        );
        assert_ok!(SubtensorModule::do_terminate_lease(
            RuntimeOrigin::signed(new_beneficiary), lease_id, hotkey,
        ));
        assert_eq!(SubnetOwner::<Test>::get(lease.netuid), new_beneficiary);
        assert!(!PROXIES.with_borrow(|proxies| proxies.0.contains(&(lease.coldkey, new_beneficiary))));
    });
}

#[test]
fn lease_payouts_require_funded_debits_and_roll_back() {
    let dividends = 1_500_000_000u64;
    // Empty payer, partial funding, insufficient final payout, and late minimum failure.
    for (available, tao_reserves) in [
        (0, 100_000_000_000u64),
        (dividends / 2, 100_000_000_000),
        (dividends - 1, 100_000_000_000),
        (dividends, 100_000_000),
    ] {
        new_test_ext(1).execute_with(|| {
            let beneficiary = U256::from(1);
            let contributor = U256::from(2);
            setup_crowdloan(
                0,
                500_000_000_000,
                1_000_000_000_000,
                beneficiary,
                &[(contributor, 500_000_000_000)],
            );
            let (lease_id, lease) =
                setup_leased_network(beneficiary, Percent::from_percent(30), Some(500), None);
            setup_reserves(lease.netuid, tao_reserves.into(), 100_000_000_000u64.into());
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &lease.coldkey,
                lease.netuid,
                available.into(),
            );
            System::set_block_number(
                <Test as Config>::LeaseDividendsDistributionInterval::get() as u64
            );
            let stake = |coldkey| {
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &lease.hotkey,
                    &coldkey,
                    lease.netuid,
                )
            };
            let before = (stake(lease.coldkey), stake(contributor), stake(beneficiary));
            let total = TotalHotkeyAlpha::<Test>::get(lease.hotkey, lease.netuid);
            let events = System::events();

            SubtensorModule::distribute_leased_network_dividends(lease_id, 5_000_000_000u64.into());

            assert_eq!(
                (stake(lease.coldkey), stake(contributor), stake(beneficiary)),
                before
            );
            assert_eq!(
                TotalHotkeyAlpha::<Test>::get(lease.hotkey, lease.netuid),
                total
            );
            assert_eq!(System::events(), events);
            assert_eq!(
                AccumulatedLeaseDividends::<Test>::get(lease_id),
                dividends.into()
            );

            // Retry the same accrued dividends once funding and the minimum permit payment.
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &lease.coldkey,
                lease.netuid,
                (dividends - available).into(),
            );
            setup_reserves(
                lease.netuid,
                100_000_000_000u64.into(),
                100_000_000_000u64.into(),
            );
            SubtensorModule::distribute_leased_network_dividends(lease_id, AlphaBalance::ZERO);
            assert_eq!(stake(lease.coldkey), AlphaBalance::ZERO);
            assert_eq!(
                stake(contributor).saturating_add(stake(beneficiary)),
                dividends.into()
            );
            assert_eq!(
                TotalHotkeyAlpha::<Test>::get(lease.hotkey, lease.netuid),
                dividends.into()
            );
            assert_eq!(
                AccumulatedLeaseDividends::<Test>::get(lease_id),
                AlphaBalance::ZERO
            );
        });
    }
}

#[test]
fn underfunded_lease_dividends_remain_pending_after_owner_changes() {
    new_test_ext(1).execute_with(|| {
        let beneficiary = U256::from(1);
        setup_crowdloan(0, 10_000_000_000, 1_000_000_000_000, beneficiary,
            &[(U256::from(2), 990_000_000_000)]);
        let (_, lease) = setup_leased_network(beneficiary, Percent::from_percent(30), Some(500), None);
        // The subnet owner can change between emission epochs; the lease payer stays fixed.
        let new_owner = U256::from(201);
        let new_hotkey = U256::from(202);
        SubnetOwner::<Test>::insert(lease.netuid, new_owner);
        SubnetOwnerHotkey::<Test>::insert(lease.netuid, new_hotkey);
        setup_reserves(lease.netuid, 100_000_000_000u64.into(), 100_000_000_000u64.into());
        System::set_block_number(<Test as Config>::LeaseDividendsDistributionInterval::get() as u64);
        let cut = AlphaBalance::from(5_000_000_000u64);
        SubtensorModule::distribute_dividends_and_incentives(
            lease.netuid, cut, Default::default(), Default::default(), Default::default(),
        );
        // Preserve owner-cut allocation while refusing to mint unfunded lease payouts.
        assert_eq!(TotalHotkeyAlpha::<Test>::get(new_hotkey, lease.netuid), cut);
        assert_eq!(TotalHotkeyAlpha::<Test>::get(lease.hotkey, lease.netuid), AlphaBalance::ZERO);
        assert_eq!(SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey, &U256::from(2), lease.netuid,
        ), AlphaBalance::ZERO);
        assert_eq!(AccumulatedLeaseDividends::<Test>::get(0),
            AlphaBalance::from(lease.emissions_share.mul_ceil(cut.to_u64())));
    });
}

#[test]
fn test_coldkey_swap_migrates_lease_awaiting_dissolution_cleanup() {
    new_test_ext(1).execute_with(|| {
        let beneficiary = U256::from(1);
        let contributor = U256::from(2);
        let destination = U256::from(3);
        let new_beneficiary = U256::from(4);
        setup_crowdloan(
            0,
            10_000_000_000,
            1_000_000_000_000,
            beneficiary,
            &[(contributor, 990_000_000_000)],
        );
        let (lease_id, lease) =
            setup_leased_network(beneficiary, Percent::from_percent(30), Some(500), None);
        let share = SubnetLeaseShares::<Test>::get(lease_id, contributor);

        assert_ok!(SubtensorModule::do_dissolve_network(lease.netuid));
        assert!(!SubtensorModule::if_subnet_exist(lease.netuid));
        assert_eq!(
            SubnetUidToLeaseId::<Test>::get(lease.netuid),
            Some(lease_id)
        );

        assert_ok!(SubtensorModule::do_swap_coldkey(&contributor, &destination));
        assert!(!SubnetLeaseShares::<Test>::contains_key(
            lease_id,
            contributor
        ));
        assert_eq!(SubnetLeaseShares::<Test>::get(lease_id, destination), share);

        assert_ok!(SubtensorModule::do_swap_coldkey(
            &beneficiary,
            &new_beneficiary
        ));
        assert_eq!(
            SubnetLeases::<Test>::get(lease_id).unwrap().beneficiary,
            new_beneficiary
        );
        assert!(PROXIES.with_borrow(|proxies| {
            proxies.0.contains(&(lease.coldkey, new_beneficiary))
                && !proxies.0.contains(&(lease.coldkey, beneficiary))
        }));
    });
}

#[test]
fn test_coldkey_swap_rolls_back_if_lease_shares_overflow() {
    new_test_ext(1).execute_with(|| {
        let beneficiary = U256::from(1);
        let contributor = U256::from(2);
        let destination = U256::from(3);
        setup_crowdloan(
            0,
            10_000_000_000,
            1_000_000_000_000,
            beneficiary,
            &[(contributor, 990_000_000_000)],
        );
        let (lease_id, _) =
            setup_leased_network(beneficiary, Percent::from_percent(30), Some(500), None);
        // Corrupt destination state must fail atomically, never silently lose shares.
        SubnetLeaseShares::<Test>::insert(lease_id, destination, U64F64::from_bits(u128::MAX));
        frame_support::assert_noop!(
            SubtensorModule::do_swap_coldkey(&contributor, &destination),
            sp_runtime::ArithmeticError::Overflow,
        );
    });
}

#[test]
fn test_crowdloan_batch_filter_failure_rolls_back_unsettled_finalization() {
    use frame_support::traits::OriginTrait;

    new_test_ext(1).execute_with(|| {
        let creator = U256::from(1);
        let contributor = U256::from(2);
        let cap = 1_000_000_000_000u64;
        let deposit = 10_000_000_000u64;
        add_balance_to_coldkey_account(&creator, deposit.into());
        add_balance_to_coldkey_account(&contributor, (cap - deposit).into());
        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![
                RuntimeCall::System(frame_system::Call::remark_with_event { remark: vec![1] }),
                RuntimeCall::SubtensorModule(crate::Call::burned_register {
                    netuid: 1.into(),
                    hotkey: U256::from(3),
                }),
                RuntimeCall::SubtensorModule(crate::Call::register_leased_network {
                    emissions_share: Percent::from_percent(30),
                    end_block: Some(500),
                }),
            ],
        });
        assert_ok!(Crowdloan::create(
            RuntimeOrigin::signed(creator),
            deposit.into(),
            10.into(),
            cap.into(),
            50,
            Some(Box::new(call)),
            None,
        ));
        assert_ok!(Crowdloan::contribute(
            RuntimeOrigin::signed(contributor),
            0,
            (cap - deposit).into(),
        ));
        let before = pallet_crowdloan::Crowdloans::<Test>::get(0).unwrap();
        let events = System::events();
        let mut origin = RuntimeOrigin::signed(creator);
        // Model the NonCritical proxy's restriction on burned registration.
        origin.add_filter(|call| {
            !matches!(
                call,
                RuntimeCall::SubtensorModule(crate::Call::burned_register { .. })
            )
        });
        frame_support::assert_noop!(
            Crowdloan::finalize(origin, 0),
            pallet_crowdloan::Error::<Test>::FundsNotSettled,
        );
        assert_eq!(System::events(), events);
        assert_eq!(Balances::free_balance(before.funds_account), cap.into());
        assert_eq!(pallet_crowdloan::Crowdloans::<Test>::get(0), Some(before));
        assert!(pallet_crowdloan::CurrentCrowdloanId::<Test>::get().is_none());
        assert!(SubnetLeases::<Test>::iter().next().is_none());
        System::set_block_number(60);
        assert_ok!(Crowdloan::withdraw(RuntimeOrigin::signed(contributor), 0));
    });
}

#[test]
fn test_crowdloan_lease_finalization_settles_raised_funds() {
    new_test_ext(1).execute_with(|| {
        let creator = U256::from(1);
        let contributor = U256::from(2);
        let cap = 1_000_000_000_000u64;
        let deposit = 10_000_000_000u64;
        add_balance_to_coldkey_account(&creator, deposit.into());
        add_balance_to_coldkey_account(&contributor, (cap - deposit).into());
        assert_ok!(Crowdloan::create(
            RuntimeOrigin::signed(creator),
            deposit.into(),
            10.into(),
            cap.into(),
            50,
            Some(Box::new(RuntimeCall::SubtensorModule(
                crate::Call::register_leased_network {
                    emissions_share: Percent::from_percent(30),
                    end_block: Some(500),
                },
            ))),
            None,
        ));
        assert_ok!(Crowdloan::contribute(
            RuntimeOrigin::signed(contributor),
            0,
            (cap - deposit).into(),
        ));
        assert_ok!(Crowdloan::finalize(RuntimeOrigin::signed(creator), 0));
        let crowdloan = pallet_crowdloan::Crowdloans::<Test>::get(0).unwrap();
        assert!(crowdloan.finalized);
        assert_eq!(
            Balances::free_balance(crowdloan.funds_account),
            TaoBalance::ZERO
        );
        assert_eq!(SubnetLeases::<Test>::get(0).unwrap().beneficiary, creator);
        assert!(pallet_crowdloan::CurrentCrowdloanId::<Test>::get().is_none());
    });
}

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
        let contributors_count = 1 + contributions.len() as u32;
        assert_ok!(
            SubtensorModule::register_leased_network(
                RuntimeOrigin::signed(beneficiary),
                emissions_share,
                Some(end_block),
            )
            .map(|post_info| post_info.actual_weight),
            Some(
                <<Test as crate::Config>::WeightInfo as crate::weights::WeightInfo>::register_leased_network(
                    contributors_count,
                ),
            )
        );

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
    let mut ext = new_test_ext(1);
    let beneficiary = U256::from(1);
    let contributions = vec![(U256::from(2), 990_000_000_000)]; // 990 TAO
    let end_block = 500;

    let (lease_id, lease) = ext.execute_with(|| {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        // Setup a leased network
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let emissions_share = Percent::from_percent(30);
        setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            Some(tao_to_stake),
        )
    });

    // Commit the lease setup so clear_prefix reports the same backend removals as on-chain.
    assert_ok!(ext.commit_all());

    ext.execute_with(|| {

        // Run to the end of the lease
        run_to_block(end_block);

        // Create a hotkey for the beneficiary
        let hotkey = U256::from(3);
        let _ = SubtensorModule::create_account_if_non_existent(&beneficiary, &hotkey);

        assert_eq!(
            SubnetLeaseShares::<Test>::iter_prefix(lease_id).count(),
            contributions.len()
        );

        // Terminate the lease
        let contributors_count = 1 + contributions.len() as u32;
        assert_ok!(
            SubtensorModule::terminate_lease(
                RuntimeOrigin::signed(beneficiary),
                lease_id,
                hotkey,
            )
            .map(|post_info| post_info.actual_weight),
            Some(
                <<Test as crate::Config>::WeightInfo as crate::weights::WeightInfo>::terminate_lease(
                    contributors_count,
                ),
            )
        );

        // Ensure the beneficiary is now the owner of the subnet
        assert_eq!(SubnetOwner::<Test>::get(lease.netuid), beneficiary);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(lease.netuid), hotkey);

        // Ensure everything has been cleaned up
        assert_eq!(SubnetLeases::<Test>::get(lease_id), None);
        assert!(!SubnetLeaseShares::<Test>::contains_prefix(lease_id));
        assert!(!AccumulatedLeaseDividends::<Test>::contains_key(lease_id));
        assert!(!SubnetUidToLeaseId::<Test>::contains_key(lease.netuid));

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
                .floor()
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
                .floor()
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

// One contributor whose slice cannot be transferred (too small for the minimum transfer)
// must not block the other contributors or the beneficiary. Its slice stays in the pot.
#[test]
fn test_distribute_lease_network_dividends_isolates_unpayable_contributor() {
    new_test_ext(1).execute_with(|| {
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let dust_contributor = U256::from(4);
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 389_999_990_000), // ~390 TAO
            (dust_contributor, 10_000),       // a dust share
        ];
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        let end_block = 500;
        let emissions_share = Percent::from_percent(30);
        let tao_to_stake = 100_000_000_000; // 100 TAO
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(end_block),
            Some(tao_to_stake),
        );
        run_to_block(<Test as Config>::LeaseDividendsDistributionInterval::get() as u64);

        let accumulated_dividends = AlphaBalance::from(10_000_000_000_u64);
        AccumulatedLeaseDividends::<Test>::insert(lease_id, accumulated_dividends);
        let owner_cut_alpha = AlphaBalance::from(5_000_000_000_u64);
        let pot: AlphaBalance =
            accumulated_dividends + emissions_share.mul_ceil(owner_cut_alpha.to_u64()).into();
        let dust_slice: u64 = SubnetLeaseShares::<Test>::get(lease_id, dust_contributor)
            .saturating_mul(U64F64::from(pot.to_u64()))
            .floor()
            .to_num::<u64>();
        assert!(
            dust_slice > 0,
            "the dust share still rounds to a non-zero slice"
        );

        SubtensorModule::distribute_leased_network_dividends(lease_id, owner_cut_alpha);

        let stake = |who: &U256| {
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                who,
                lease.netuid,
            )
        };
        assert!(stake(&contributions[0].0) > AlphaBalance::ZERO);
        assert!(stake(&contributions[1].0) > AlphaBalance::ZERO);
        assert!(stake(&beneficiary) > AlphaBalance::ZERO);
        assert_eq!(stake(&dust_contributor), AlphaBalance::ZERO);

        // Everything but the unpayable slice was paid out; the slice waits in the pot.
        assert_eq!(
            stake(&contributions[0].0) + stake(&contributions[1].0) + stake(&beneficiary),
            pot - dust_slice.into()
        );
        assert_eq!(
            AccumulatedLeaseDividends::<Test>::get(lease_id),
            AlphaBalance::from(dust_slice)
        );
        assert!(System::events().iter().any(|record| {
            record.event
                == RuntimeEvent::SubtensorModule(Event::SubnetLeaseDividendSkipped {
                    lease_id,
                    contributor: dust_contributor,
                    alpha: dust_slice.into(),
                })
        }));
    });
}

// With the owner-cut auto-lock on, only the part of the cut the lease keeps is locked. The
// contributors' share stays transferable, so the next distribution still pays everyone.
#[test]
fn test_leased_owner_cut_auto_lock_keeps_contributor_share_unlocked() {
    new_test_ext(1).execute_with(|| {
        let crowdloan_id = 0;
        let beneficiary = U256::from(1);
        let deposit = 10_000_000_000; // 10 TAO
        let cap = 1_000_000_000_000; // 1000 TAO
        let contributions = vec![
            (U256::from(2), 600_000_000_000), // 600 TAO
            (U256::from(3), 390_000_000_000), // 390 TAO
        ];
        setup_crowdloan(crowdloan_id, deposit, cap, beneficiary, &contributions);

        let emissions_share = Percent::from_percent(30);
        let (lease_id, lease) = setup_leased_network(
            beneficiary,
            emissions_share,
            Some(500),
            Some(100_000_000_000),
        );
        OwnerCutAutoLockEnabled::<Test>::insert(lease.netuid, true);
        run_to_block(<Test as Config>::LeaseDividendsDistributionInterval::get() as u64);

        // Mirror the coinbase owner-cut step: credit the cut, distribute, lock the retained part.
        let owner_cut = AlphaBalance::from(5_000_000_000_u64);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &lease.coldkey,
            lease.netuid,
            owner_cut,
        );
        SubtensorModule::distribute_leased_network_dividends(lease_id, owner_cut);
        let retained = SubtensorModule::leased_owner_cut_retained(lease_id, owner_cut);
        assert_eq!(
            retained,
            owner_cut - emissions_share.mul_ceil(owner_cut.to_u64()).into()
        );
        SubtensorModule::auto_lock_owner_cut(lease.netuid, retained);

        let stake = |who: &U256| {
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                who,
                lease.netuid,
            )
        };
        assert!(stake(&contributions[0].0) > AlphaBalance::ZERO);
        assert!(stake(&contributions[1].0) > AlphaBalance::ZERO);
        assert!(stake(&beneficiary) > AlphaBalance::ZERO);
        assert_eq!(
            AccumulatedLeaseDividends::<Test>::get(lease_id),
            AlphaBalance::ZERO
        );

        // The lease coldkey's lock grew by the retained part only.
        let lock = Lock::<Test>::get((lease.coldkey, lease.netuid, lease.hotkey)).unwrap();
        assert_eq!(lock.locked_mass, retained);

        // The next interval pays out again: the contributors' share was never locked.
        let before = stake(&contributions[0].0);
        run_to_block(2 * <Test as Config>::LeaseDividendsDistributionInterval::get() as u64);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &lease.hotkey,
            &lease.coldkey,
            lease.netuid,
            owner_cut,
        );
        SubtensorModule::distribute_leased_network_dividends(lease_id, owner_cut);
        SubtensorModule::auto_lock_owner_cut(
            lease.netuid,
            SubtensorModule::leased_owner_cut_retained(lease_id, owner_cut),
        );
        assert!(stake(&contributions[0].0) > before);
        assert_eq!(
            AccumulatedLeaseDividends::<Test>::get(lease_id),
            AlphaBalance::ZERO
        );

        // An ended lease keeps the whole cut.
        SubnetLeases::<Test>::mutate(lease_id, |maybe| {
            if let Some(l) = maybe {
                l.end_block = Some(1);
            }
        });
        assert_eq!(
            SubtensorModule::leased_owner_cut_retained(lease_id, owner_cut),
            owner_cut
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
