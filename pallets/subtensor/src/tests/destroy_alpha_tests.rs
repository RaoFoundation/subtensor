#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]

use super::mock::*;
use crate::*;
use frame_support::{assert_ok, weights::Weight};
use sp_core::U256;
use subtensor_runtime_common::TaoBalance;
use subtensor_swap_interface::SwapHandler;

fn setup_staked_subnet() -> (U256, U256, NetUid) {
    let owner_cold = U256::from(1001);
    let owner_hot = U256::from(1002);
    let netuid = add_dynamic_network(&owner_hot, &owner_cold);

    let stake_tao: u64 = 1000;
    setup_reserves(
        netuid,
        (stake_tao * 1_000_000).into(),
        (stake_tao * 10_000_000).into(),
    );
    let amount: TaoBalance = stake_tao.into();
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

    (owner_cold, owner_hot, netuid)
}

#[test]
fn test_destroy_alpha_in_out_stakes_get_total_alpha_value() {
    new_test_ext(0).execute_with(|| {
        let (_, _, netuid) = setup_staked_subnet();
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = WeightMeter::with_limit(w);
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
            "destroy_alpha_in_out_stakes_get_total_alpha_value should complete"
        );
        let mut status = dissolve_cleanup_status(netuid);
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
        );
        assert!(status.subnet_total_alpha_value.is_some());
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_settle_stakes() {
    new_test_ext(0).execute_with(|| {
        let (_, _, netuid) = setup_staked_subnet();
        run_destroy_alpha_get_total_and_settle(netuid);
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_clean_alpha() {
    new_test_ext(0).execute_with(|| {
        let (_, owner_hot, netuid) = setup_staked_subnet();
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = WeightMeter::with_limit(w);
        let mut status = dissolve_cleanup_status(netuid);
        assert!(run_resumable_netuid_cleanup_with_status(
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
        ));
        status.subnet_distributed_tao = Some(0);
        let mut weight_meter2 = WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup_with_status(
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
        ));
        let mut weight_meter3 = WeightMeter::with_limit(w);
        assert!(
            run_resumable_netuid_cleanup(
                netuid,
                &mut weight_meter3,
                SubtensorModule::destroy_alpha_in_out_stakes_clean_alpha,
            ),
            "destroy_alpha_in_out_stakes_clean_alpha should complete"
        );
        assert_eq!(
            Alpha::<Test>::iter()
                .filter(|((_, _, nu), _)| *nu == netuid)
                .count(),
            0
        );
        assert!(TotalHotkeyAlpha::<Test>::contains_key(owner_hot, netuid));
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_clear_hotkey_totals() {
    new_test_ext(0).execute_with(|| {
        let (_, owner_hot, netuid) = setup_staked_subnet();
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = WeightMeter::with_limit(w);
        let mut status = dissolve_cleanup_status(netuid);
        assert!(run_resumable_netuid_cleanup_with_status(
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
        ));
        status.subnet_distributed_tao = Some(0);
        let mut weight_meter2 = WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup_with_status(
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
        ));
        let mut weight_meter3 = WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup(
            netuid,
            &mut weight_meter3,
            SubtensorModule::destroy_alpha_in_out_stakes_clean_alpha,
        ));
        let mut weight_meter4 = WeightMeter::with_limit(w);
        assert!(
            run_resumable_netuid_cleanup(
                netuid,
                &mut weight_meter4,
                SubtensorModule::destroy_alpha_in_out_stakes_clear_hotkey_totals,
            ),
            "destroy_alpha_in_out_stakes_clear_hotkey_totals should complete"
        );
        assert!(!TotalHotkeyAlpha::<Test>::contains_key(owner_hot, netuid));
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_clear_locks() {
    new_test_ext(0).execute_with(|| {
        let (owner_cold, owner_hot, netuid) = setup_staked_subnet();
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = WeightMeter::with_limit(w);
        let mut status = dissolve_cleanup_status(netuid);
        assert!(run_resumable_netuid_cleanup_with_status(
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
        ));
        status.subnet_distributed_tao = Some(0);
        let mut weight_meter2 = WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup_with_status(
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
        ));
        let mut weight_meter3 = WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup(
            netuid,
            &mut weight_meter3,
            SubtensorModule::destroy_alpha_in_out_stakes_clean_alpha,
        ));
        let mut weight_meter4 = WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup(
            netuid,
            &mut weight_meter4,
            SubtensorModule::destroy_alpha_in_out_stakes_clear_hotkey_totals,
        ));

        Lock::<Test>::insert(
            (owner_cold, netuid, owner_hot),
            crate::staking::lock::LockState {
                locked_mass: 10u64.into(),
                conviction: substrate_fixed::types::U64F64::from_num(1.5),
                last_update: 1,
            },
        );

        let mut weight_meter5 = WeightMeter::with_limit(w);
        assert!(
            run_resumable_netuid_cleanup(
                netuid,
                &mut weight_meter5,
                SubtensorModule::destroy_alpha_in_out_stakes_clear_locks,
            ),
            "destroy_alpha_in_out_stakes_clear_locks should complete"
        );
        assert!(!Lock::<Test>::contains_key((owner_cold, netuid, owner_hot)));
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes() {
    new_test_ext(0).execute_with(|| {
        let (_, _, netuid) = setup_staked_subnet();
        let mut status = run_destroy_alpha_get_total_and_settle(netuid);
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = WeightMeter::with_limit(w);
        assert!(
            SubtensorModule::destroy_alpha_in_out_stakes(netuid, &mut weight_meter, &mut status),
            "destroy_alpha_in_out_stakes should complete"
        );
    });
}

#[test]
fn test_destroy_alpha_clean_alpha_resumes_with_limited_weight() {
    new_test_ext(0).execute_with(|| {
        let (_, _, netuid) = setup_staked_subnet();
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = WeightMeter::with_limit(w);
        let mut status = dissolve_cleanup_status(netuid);
        assert!(run_resumable_netuid_cleanup_with_status(
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
        ));
        status.subnet_distributed_tao = Some(0);
        let mut weight_meter2 = WeightMeter::with_limit(w);
        assert!(run_resumable_netuid_cleanup_with_status(
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
        ));

        let read_weight = <Test as frame_system::Config>::DbWeight::get().reads(1);
        let mut weight_meter3 = WeightMeter::with_limit(read_weight);
        let (done, mut last_key) = SubtensorModule::destroy_alpha_in_out_stakes_clean_alpha(
            netuid,
            &mut weight_meter3,
            None,
        );
        assert!(!done);

        let mut iterations = 0;
        while Alpha::<Test>::iter().any(|((_, _, nu), _)| nu == netuid) {
            let mut weight_meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
            let (done, new_key) = SubtensorModule::destroy_alpha_in_out_stakes_clean_alpha(
                netuid,
                &mut weight_meter,
                last_key,
            );
            last_key = new_key;
            assert!(
                done,
                "clean_alpha should finish once all alpha entries are removed"
            );
            iterations += 1;
            assert!(
                iterations < 10,
                "clean_alpha should complete within a few passes"
            );
        }
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_settle_stakes_multi_block_total_issuance() {
    new_test_ext(0).execute_with(|| {
        // Create 3 independent stakers to force multi-block settle.
        let cold_base = U256::from(2000);
        let hot_base = U256::from(3000);
        let netuid = add_dynamic_network(&hot_base, &cold_base);

        let stake_tao: u64 = 1000;
        setup_reserves(
            netuid,
            (stake_tao * 1_000_000).into(),
            (stake_tao * 10_000_000).into(),
        );
        let amount: TaoBalance = stake_tao.into();

        for index in 1..=10 {
            let cold = U256::from(cold_base + index);
            let hot = U256::from(hot_base + index);
            assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &hot));
            add_balance_to_coldkey_account(&cold, amount);
            assert_ok!(SubtensorModule::stake_into_subnet(
                &hot,
                &cold,
                netuid,
                amount,
                <Test as Config>::SwapInterface::max_price(),
                false,
            ));
        }
        let w = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = WeightMeter::with_limit(w);
        let mut status = dissolve_cleanup_status(netuid);

        // Phase 1: Get total alpha (prerequisite for settle_stakes)
        assert!(run_resumable_netuid_cleanup_with_status(
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
        ));

        status.subnet_distributed_tao = Some(0);
        status.last_key = None;

        let total_issuance_before = TotalIssuance::<Test>::get();

        // Phase 2: settle_stakes with per-call weight enough for 2 out of 10 stakers.
        //
        // Each hotkey+coldkey consumes:
        //   reads(1) outer  +  reads(2) inner  +  writes(1) value  +
        //   reads_writes(11, 3) transfer  =  reads(14) + writes(4)
        // Weight for two hotkeys = reads(28) + writes(8)
        // Plus reads(1) to attempt the third outer iteration  →  reads(29) + writes(8)
        let per_call = <Test as frame_system::Config>::DbWeight::get()
            .reads(29)
            .saturating_add(<Test as frame_system::Config>::DbWeight::get().writes(8));

        let mut last_key = status.last_key.clone();
        let mut completed = false;
        let mut iterations = 0u32;

        for block in 1..=10 {
            let mut meter = WeightMeter::with_limit(per_call);
            let (done, new_key) = SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
                netuid,
                &mut meter,
                last_key.clone(),
                &mut status,
            );
            last_key = new_key;

            next_block();

            assert_eq!(
                TotalIssuance::<Test>::get(),
                total_issuance_before,
                "TotalIssuance unchanged after block {block}"
            );

            if done {
                completed = true;
                iterations = block;
                break;
            }
        }

        assert!(completed, "settle_stakes should finish");
        assert!(
            iterations >= 3,
            "should need multiple blocks, completed in {iterations}"
        );
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_settle_stakes_finishes_hotkey_past_weight() {
    new_test_ext(0).execute_with(|| {
        let cold_base = U256::from(7000);
        let hot_base = U256::from(8000);
        let netuid = add_dynamic_network(&hot_base, &cold_base);

        setup_reserves(netuid, 1_000_000u64.into(), 10_000_000u64.into());

        for index in 1..=3 {
            let cold = U256::from(cold_base + index);
            let hot = U256::from(hot_base + index);
            assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &hot));
            add_balance_to_coldkey_account(&cold, 1_000u64.into());
            assert_ok!(SubtensorModule::stake_into_subnet(
                &hot,
                &cold,
                netuid,
                1_000u64.into(),
                <Test as Config>::SwapInterface::max_price(),
                false,
            ));
        }

        let mut status = dissolve_cleanup_status(netuid);
        let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(run_resumable_netuid_cleanup_with_status(
            netuid,
            &mut meter,
            &mut status,
            |netuid, weight_meter, last_key, status| {
                SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                    netuid,
                    weight_meter,
                    last_key,
                    status,
                )
            },
        ));
        status.subnet_distributed_tao = Some(0);

        let first_hot = TotalHotkeyAlpha::<Test>::iter()
            .find_map(|(hot, this_netuid, _)| (this_netuid == netuid).then_some(hot))
            .expect("staked hotkey should exist");
        let first_cold = SubtensorModule::alpha_iter_single_prefix(&first_hot)
            .find_map(|(cold, this_netuid, _)| (this_netuid == netuid).then_some(cold))
            .expect("staked coldkey should exist");
        let first_balance_before = SubtensorModule::get_coldkey_balance(&first_cold);

        // Enough to start the first hotkey, not enough to reserve its payout weight.
        // Cleanup must still finish and pay that hotkey so dissolution cannot livelock.
        let tight = <Test as frame_system::Config>::DbWeight::get()
            .reads(3)
            .saturating_add(<Test as frame_system::Config>::DbWeight::get().writes(1));
        let mut tight_meter = WeightMeter::with_limit(tight);
        let (done, new_key) = SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
            netuid,
            &mut tight_meter,
            None,
            &mut status,
        );

        assert!(!done);
        assert_eq!(
            new_key,
            Some(TotalHotkeyAlpha::<Test>::hashed_key_for(&first_hot, netuid)),
            "cursor must advance past the hotkey that was finished over budget"
        );
        assert!(
            SubtensorModule::get_coldkey_balance(&first_cold) > first_balance_before,
            "started hotkey must be paid even when weight is exhausted mid-prefix"
        );

        let mut full_meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        let (done, _) = SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
            netuid,
            &mut full_meter,
            new_key,
            &mut status,
        );

        assert!(done);
    });
}

#[test]
fn test_destroy_alpha_in_out_stakes_settle_stakes_keeps_previous_cursor() {
    new_test_ext(0).execute_with(|| {
        let cold_base = U256::from(9000);
        let hot_base = U256::from(10000);
        let netuid = add_dynamic_network(&hot_base, &cold_base);

        setup_reserves(netuid, 1_000_000u64.into(), 10_000_000u64.into());

        for index in 1..=3 {
            let cold = U256::from(cold_base + index);
            let hot = U256::from(hot_base + index);
            assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &hot));
            add_balance_to_coldkey_account(&cold, 1_000u64.into());
            assert_ok!(SubtensorModule::stake_into_subnet(
                &hot,
                &cold,
                netuid,
                1_000u64.into(),
                <Test as Config>::SwapInterface::max_price(),
                false,
            ));
        }

        let mut status = dissolve_cleanup_status(netuid);
        let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(run_resumable_netuid_cleanup_with_status(
            netuid,
            &mut meter,
            &mut status,
            |netuid, weight_meter, last_key, status| {
                SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                    netuid,
                    weight_meter,
                    last_key,
                    status,
                )
            },
        ));
        status.subnet_distributed_tao = Some(0);

        let first_hot = TotalHotkeyAlpha::<Test>::iter()
            .find_map(|(hot, this_netuid, _)| (this_netuid == netuid).then_some(hot))
            .expect("staked hotkey should exist");
        let first_cold = SubtensorModule::alpha_iter_single_prefix(&first_hot)
            .find_map(|(cold, this_netuid, _)| (this_netuid == netuid).then_some(cold))
            .expect("staked coldkey should exist");
        let first_balance_before = SubtensorModule::get_coldkey_balance(&first_cold);

        let one_hotkey = <Test as frame_system::Config>::DbWeight::get()
            .reads(14)
            .saturating_add(<Test as frame_system::Config>::DbWeight::get().writes(4));
        let mut first_meter = WeightMeter::with_limit(one_hotkey);
        let (done, previous_key) = SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
            netuid,
            &mut first_meter,
            None,
            &mut status,
        );
        let previous_key = previous_key.expect("first pass should complete one hotkey");

        assert!(!done);
        assert!(
            SubtensorModule::get_coldkey_balance(&first_cold) > first_balance_before,
            "first pass should pay the completed hotkey"
        );
        let first_balance_after = SubtensorModule::get_coldkey_balance(&first_cold);

        // Not enough weight to start another hotkey outer read — cursor must stay put.
        let mut tight_meter = WeightMeter::with_limit(Weight::zero());
        let (done, retry_key) = SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
            netuid,
            &mut tight_meter,
            Some(previous_key.clone()),
            &mut status,
        );

        assert!(!done);
        assert_eq!(
            retry_key,
            Some(previous_key),
            "cursor should stay at the previous completed hotkey when no new hotkey can start"
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&first_cold),
            first_balance_after,
            "empty-budget pass should not rewind and pay the completed hotkey again"
        );
    });
}
