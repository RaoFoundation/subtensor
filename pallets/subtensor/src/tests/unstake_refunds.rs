#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use super::mock::*;
use crate::weights::WeightInfo;
use crate::*;
use frame_support::{
    assert_ok,
    dispatch::{GetDispatchInfo, Pays},
    traits::Currency,
};
use share_pool::SafeFloat;
use sp_core::U256;
use sp_runtime::traits::Dispatchable;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

#[test]
fn test_pr3190_both_cleanup_helpers_preserve_a_live_seed_cursor() {
    use crate::migrations::migrate_seed_beta_basket::{
        SeedBetaBasketV2Migration, SeedBetaBasketV2Progress,
    };
    use codec::Encode;
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(8000);
        let hotkey = U256::from(9000);
        let sibling = U256::from(9001);
        StakingHotkeys::<Test>::insert(coldkey, vec![hotkey, sibling]);
        SeedBetaBasketV2Migration::<Test>::put(SeedBetaBasketV2Progress::ReconcileClaimants {
            after: None,
            coldkey: Some(coldkey.encode()),
            hotkey_index: 1,
        });
        SubtensorModule::maybe_remove_staking_hotkey(&hotkey, &coldkey);
        SubtensorModule::maybe_remove_staking_hotkey_bounded(&hotkey, &coldkey);
        assert_eq!(StakingHotkeys::<Test>::get(coldkey), vec![hotkey, sibling]);
        SeedBetaBasketV2Migration::<Test>::kill();
        SubtensorModule::maybe_remove_staking_hotkey_bounded(&hotkey, &coldkey);
        assert_eq!(StakingHotkeys::<Test>::get(coldkey), vec![sibling]);
    });
}

#[test]
fn test_pr3190_bounded_cleanup_vector_boundary() {
    for count in [crate::MAX_STAKING_HOTKEYS, crate::MAX_STAKING_HOTKEYS + 1] {
        new_test_ext(1).execute_with(|| {
            let coldkey = U256::from(8000);
            let hotkeys: Vec<_> = (0..count).map(|i| U256::from(10000 + i)).collect();
            let hotkey = hotkeys[0];
            StakingHotkeys::<Test>::insert(coldkey, &hotkeys);
            SubtensorModule::maybe_remove_staking_hotkey_bounded(&hotkey, &coldkey);
            if count == crate::MAX_STAKING_HOTKEYS {
                assert_eq!(StakingHotkeys::<Test>::get(coldkey), hotkeys[1..]);
            } else {
                assert_eq!(StakingHotkeys::<Test>::get(coldkey), hotkeys);
            }
        });
    }
}

#[test]
fn test_pr3190_bulk_flush_allowance_fits_normal_extrinsic_admission() {
    use frame_support::dispatch::DispatchClass;
    new_test_ext(1).execute_with(|| {
        // Cover both the default topology and the larger historical envelope.
        for networks in [128u16, 568] {
            TotalNetworks::<Test>::put(networks);
            let maximum = <Test as frame_system::Config>::BlockWeights::get()
                .get(DispatchClass::Normal)
                .max_extrinsic
                .unwrap();
            for declared in [
                SubtensorModule::unstake_all_declared_weight(),
                SubtensorModule::unstake_all_alpha_declared_weight(),
            ] {
                assert!(
                    maximum.all_gte(declared),
                    "{declared:?} must fit {maximum:?}"
                );
            }
        }
    });
}

#[test]
fn test_pr3190_successful_full_exit_charges_pre_execution_walk() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = add_dynamic_network(&U256::from(1002), &U256::from(1001));
        Balances::make_free_balance_be(&coldkey, 1_000_000_000u64.into());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey, &hotkey
        ));
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, 200_000u64.into(), netuid);
        let amount =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let walk = SubtensorModule::staking_hotkeys_walk_actual(&coldkey);
        let result =
            SubtensorModule::remove_stake(RuntimeOrigin::signed(coldkey), hotkey, netuid, amount)
                .unwrap();
        assert!(!StakingHotkeys::<Test>::get(coldkey).contains(&hotkey));
        assert_eq!(
            result.actual_weight,
            Some(<Test as crate::Config>::WeightInfo::remove_stake().saturating_add(walk))
        );
    });
}

#[test]
fn test_pr3190_all_nine_single_leg_error_refunds() {
    for (net, other) in [(65000u16, 65001u16), (0, 65001), (65000, 0), (0, 0)] {
        for count in [0u64, 1, 256] {
            new_test_ext(1).execute_with(|| {
                let coldkey = U256::from(8000);
                let hotkey = U256::from(9000);
                StakingHotkeys::<Test>::insert(
                    coldkey,
                    (0..count)
                        .map(|n| U256::from(n + 10000))
                        .collect::<Vec<_>>(),
                );
                let netuid = NetUid::from(net);
                let other_netuid = NetUid::from(other);
                let destination_coldkey = U256::from(8001);
                let destination_hotkey = U256::from(9001);
                let amount = AlphaBalance::from(1);
                let price = TaoBalance::from(1);
                let cases = vec![
                    (
                        crate::Call::<Test>::remove_stake {
                            hotkey,
                            netuid,
                            amount_unstaked: amount,
                        },
                        <Test as crate::Config>::WeightInfo::remove_stake(),
                    ),
                    (
                        crate::Call::<Test>::remove_stake_limit {
                            hotkey,
                            netuid,
                            amount_unstaked: amount,
                            limit_price: price,
                            allow_partial: false,
                        },
                        <Test as crate::Config>::WeightInfo::remove_stake_limit(),
                    ),
                    (
                        crate::Call::<Test>::remove_stake_full_limit {
                            hotkey,
                            netuid,
                            limit_price: None,
                        },
                        <Test as crate::Config>::WeightInfo::remove_stake_full_limit(),
                    ),
                    (
                        crate::Call::<Test>::move_stake {
                            origin_hotkey: hotkey,
                            destination_hotkey,
                            origin_netuid: netuid,
                            destination_netuid: other_netuid,
                            alpha_amount: amount,
                        },
                        <Test as crate::Config>::WeightInfo::move_stake(),
                    ),
                    (
                        crate::Call::<Test>::move_stake_limit {
                            origin_hotkey: hotkey,
                            destination_hotkey,
                            origin_netuid: netuid,
                            destination_netuid: other_netuid,
                            alpha_amount: amount,
                            limit_price: price,
                            allow_partial: false,
                        },
                        <Test as crate::Config>::WeightInfo::move_stake_limit(),
                    ),
                    (
                        crate::Call::<Test>::transfer_stake {
                            destination_coldkey,
                            hotkey,
                            origin_netuid: netuid,
                            destination_netuid: other_netuid,
                            alpha_amount: amount,
                        },
                        <Test as crate::Config>::WeightInfo::transfer_stake(),
                    ),
                    (
                        crate::Call::<Test>::transfer_stake_and_hotkey {
                            destination_coldkey,
                            origin_hotkey: hotkey,
                            destination_hotkey,
                            origin_netuid: netuid,
                            destination_netuid: other_netuid,
                            alpha_amount: amount,
                        },
                        <Test as crate::Config>::WeightInfo::transfer_stake_and_hotkey(),
                    ),
                    (
                        crate::Call::<Test>::swap_stake {
                            hotkey,
                            origin_netuid: netuid,
                            destination_netuid: other_netuid,
                            alpha_amount: amount,
                        },
                        <Test as crate::Config>::WeightInfo::swap_stake(),
                    ),
                    (
                        crate::Call::<Test>::swap_stake_limit {
                            hotkey,
                            origin_netuid: netuid,
                            destination_netuid: other_netuid,
                            alpha_amount: amount,
                            limit_price: price,
                            allow_partial: false,
                        },
                        <Test as crate::Config>::WeightInfo::swap_stake_limit(),
                    ),
                ];
                let walk = SubtensorModule::staking_hotkeys_walk_actual(&coldkey);
                for (call, base) in cases {
                    let flush = match &call {
                        crate::Call::remove_stake { netuid, .. }
                        | crate::Call::remove_stake_limit { netuid, .. }
                        | crate::Call::remove_stake_full_limit { netuid, .. } => {
                            SubtensorModule::remove_stake_basket_flush_weight(*netuid)
                        }
                        crate::Call::move_stake {
                            origin_netuid,
                            destination_netuid,
                            ..
                        }
                        | crate::Call::move_stake_limit {
                            origin_netuid,
                            destination_netuid,
                            ..
                        }
                        | crate::Call::transfer_stake_and_hotkey {
                            origin_netuid,
                            destination_netuid,
                            ..
                        } => SubtensorModule::transition_stake_basket_flush_weight(
                            *origin_netuid,
                            *destination_netuid,
                            false,
                        ),
                        _ => SubtensorModule::transition_stake_basket_flush_weight(
                            netuid,
                            other_netuid,
                            true,
                        ),
                    };
                    let base = base.saturating_add(flush);
                    let call = RuntimeCall::SubtensorModule(call);
                    let declared = call.get_dispatch_info().call_weight;
                    let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
                    let error = call.dispatch(RuntimeOrigin::signed(coldkey)).unwrap_err();
                    assert_eq!(
                        error.post_info.actual_weight,
                        Some(base.saturating_add(walk))
                    );
                    assert_eq!(error.post_info.pays_fee, Pays::Yes);
                    assert!(declared.all_gte(error.post_info.actual_weight.unwrap()));
                    assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
                }
            });
        }
    }
}

#[test]
fn test_pr3190_unstake_all_early_failures_charge_base_and_preserve_state() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(8000);
        let hotkey = U256::from(9000);
        StakingHotkeys::<Test>::insert(
            coldkey,
            (0..256).map(|n| U256::from(n + 10000)).collect::<Vec<_>>(),
        );
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        for (result, base) in [
            (
                SubtensorModule::unstake_all(RuntimeOrigin::signed(coldkey), hotkey),
                <Test as crate::Config>::WeightInfo::unstake_all(),
            ),
            (
                SubtensorModule::unstake_all_alpha(RuntimeOrigin::signed(coldkey), hotkey),
                <Test as crate::Config>::WeightInfo::unstake_all_alpha(),
            ),
        ] {
            let error = result.unwrap_err();
            assert_eq!(error.error, Error::<Test>::HotKeyAccountNotExists.into());
            assert_eq!(
                error.post_info.actual_weight,
                Some(base.saturating_add(SubtensorModule::staking_basket_flush_weight_bound()))
            );
            assert_eq!(error.post_info.pays_fee, Pays::Yes);
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        }
    });
}

#[test]
fn test_pr3190_bounded_prune_retains_shares_and_signed_watermarks() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(8000);
        let hotkey = U256::from(9000);
        let sibling = U256::from(9001);
        let netuid = NetUid::from(42);
        StakingHotkeys::<Test>::insert(coldkey, vec![hotkey, sibling]);
        for mark in [-100i128, 100] {
            BasketClaimed::<Test>::insert(hotkey, coldkey, mark);
            SubtensorModule::maybe_remove_staking_hotkey_bounded(&hotkey, &coldkey);
            assert_eq!(StakingHotkeys::<Test>::get(coldkey), vec![hotkey, sibling]);
        }
        BasketClaimed::<Test>::insert(hotkey, coldkey, 0i128);
        AlphaV2::<Test>::insert((hotkey, coldkey, netuid), SafeFloat::from(0u64));
        SubtensorModule::maybe_remove_staking_hotkey_bounded(&hotkey, &coldkey);
        assert_eq!(StakingHotkeys::<Test>::get(coldkey), vec![hotkey, sibling]);
        AlphaV2::<Test>::remove((hotkey, coldkey, netuid));
        SubtensorModule::maybe_remove_staking_hotkey_bounded(&hotkey, &coldkey);
        assert_eq!(StakingHotkeys::<Test>::get(coldkey), vec![sibling]);
        SubtensorModule::maybe_remove_staking_hotkey_bounded(&sibling, &coldkey);
        assert!(!StakingHotkeys::<Test>::contains_key(coldkey));
    });
}

#[test]
fn test_pr3190_full_burn_and_recycle_prune_last_position() {
    for burn in [false, true] {
        new_test_ext(1).execute_with(|| {
            let coldkey = U256::from(1);
            let hotkey = U256::from(2);
            let netuid = add_dynamic_network(&U256::from(1002), &U256::from(1001));
            Balances::make_free_balance_be(&coldkey, 1_000_000_000u64.into());
            let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
            register_ok_neuron(netuid, hotkey, coldkey, 0);
            increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, 200_000u64.into(), netuid);
            let amount = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
            assert!(!amount.is_zero());
            assert!(StakingHotkeys::<Test>::get(coldkey).contains(&hotkey));
            if burn {
                assert_ok!(SubtensorModule::burn_alpha(
                    RuntimeOrigin::signed(coldkey),
                    hotkey,
                    amount,
                    netuid
                ));
            } else {
                assert_ok!(SubtensorModule::recycle_alpha(
                    RuntimeOrigin::signed(coldkey),
                    hotkey,
                    amount,
                    netuid
                ));
            }
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey, &coldkey, netuid
                ),
                AlphaBalance::ZERO
            );
            assert!(!StakingHotkeys::<Test>::get(coldkey).contains(&hotkey));
        });
    }
}

#[test]
fn test_pr3190_seed_cursor_preserves_unvisited_claimants_during_zero_burn() {
    use crate::migrations::migrate_seed_beta_basket::{
        SeedBetaBasketV2Migration, SeedBetaBasketV2Progress, migrate_seed_beta_basket_v2_limited,
        seed_beta_basket_v2_in_progress,
    };
    use codec::Encode;
    use substrate_fixed::types::I96F32;
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(8000);
        let empty_hotkey = U256::from(9000);
        let root_hotkey = U256::from(9001);
        let netuid = add_dynamic_network(&U256::from(1002), &U256::from(1001));
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(coldkey),
            empty_hotkey
        ));
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_hotkey,
            &coldkey,
            NetUid::ROOT,
            100_000u64.into(),
        );
        BasketRate::<Test>::insert(root_hotkey, I96F32::saturating_from_num(1));
        BasketShares::<Test>::remove(root_hotkey);
        assert_eq!(
            StakingHotkeys::<Test>::get(coldkey),
            vec![empty_hotkey, root_hotkey]
        );
        let owed = SubtensorModule::get_basket_owed_shares(&root_hotkey, &coldkey);
        assert_eq!(owed, 100_000);
        // Valid resume state after ClearShares and processing the empty entry.
        SeedBetaBasketV2Migration::<Test>::put(SeedBetaBasketV2Progress::ReconcileClaimants {
            after: None,
            coldkey: Some(coldkey.encode()),
            hotkey_index: 1,
        });
        assert_ok!(SubtensorModule::burn_alpha(
            RuntimeOrigin::signed(coldkey),
            empty_hotkey,
            AlphaBalance::ZERO,
            netuid
        ));
        for _ in 0..10 {
            if !seed_beta_basket_v2_in_progress::<Test>() {
                break;
            }
            migrate_seed_beta_basket_v2_limited::<Test>(10, 10, 10);
        }
        assert!(!seed_beta_basket_v2_in_progress::<Test>());
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&root_hotkey, &coldkey),
            owed
        );
        assert_eq!(
            BasketShares::<Test>::get(root_hotkey),
            owed,
            "migration must include the live claimant after an allowed zero-burn"
        );
    });
}

#[test]
fn test_pr3190_bounded_cleanup_defers_oversized_association_vector() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(8000);
        for n in 0..300u64 {
            assert_ok!(SubtensorModule::try_associate_hotkey(
                RuntimeOrigin::signed(coldkey),
                U256::from(10000 + n)
            ));
        }
        assert_eq!(StakingHotkeys::<Test>::get(coldkey).len(), 300);
        SubtensorModule::maybe_remove_staking_hotkey_bounded(&U256::from(10000), &coldkey);
        assert_eq!(StakingHotkeys::<Test>::get(coldkey).len(), 300);
    });
}
