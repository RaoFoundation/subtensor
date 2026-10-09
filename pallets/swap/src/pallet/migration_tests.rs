#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]

use super::*;
use crate::mock::{AlphaReserve, TaoReserve, Test, new_test_ext};
use crate::pallet::migrations::migrate_balancer_to_superellipse::{
    MIGRATION_NAME, migrate_balancer_to_superellipse,
};
use sp_arithmetic::Perquintill;
use subtensor_runtime_common::{Token, TokenReserve};

fn stamped() -> bool {
    HasMigrationRun::<Test>::get(BoundedVec::truncate_from(MIGRATION_NAME.to_vec()))
}

#[test]
fn migration_preserves_price_balances_and_baseline_curve_for_extreme_weights() {
    new_test_ext().execute_with(|| {
        let x = 10_000_000_000_000_000u64;
        let y = 500_000_000_000_000u64;
        for (id, parts) in [(51u16, 10u64), (52, 500), (53, 990)] {
            let netuid = NetUid::from(id);
            let quote = Perquintill::from_rational(parts, 1000u64);
            let balancer = Balancer::new(quote).unwrap();
            AlphaReserve::set_mock_reserve(netuid, x.into());
            TaoReserve::set_mock_reserve(netuid, y.into());
            SwapBalancer::<Test>::insert(netuid, balancer);
            PalSwapInitialized::<Test>::insert(netuid, true);
            BalancerAlphaReservoir::<Test>::insert(netuid, AlphaBalance::from(17u64));
            BalancerTaoReservoir::<Test>::insert(netuid, TaoBalance::from(29u64));
        }
        migrate_balancer_to_superellipse::<Test>();
        assert!(stamped());
        for (id, parts) in [(51u16, 10u64), (52, 500), (53, 990)] {
            let netuid = NetUid::from(id);
            let quote = Perquintill::from_rational(parts, 1000u64);
            let old = SwapBalancer::<Test>::get(netuid);
            let ellipse = SwapSuperellipse::<Test>::get(netuid).unwrap();
            let expected = super::superellipse::Superellipse::from_balancer(x, y, quote).unwrap();
            assert_eq!(ellipse, expected);
            let p_before = old.calculate_price(x, y).to_num::<f64>();
            let p_after = ellipse.calculate_price(x, y).unwrap().to_num::<f64>();
            assert!((p_after / p_before - 1.0).abs() < 1e-10);
            assert_eq!(AlphaReserve::reserve(netuid), AlphaBalance::from(x));
            assert_eq!(TaoReserve::reserve(netuid), TaoBalance::from(y));
            assert_eq!(
                BalancerAlphaReservoir::<Test>::get(netuid),
                AlphaBalance::from(17u64)
            );
            assert_eq!(
                BalancerTaoReservoir::<Test>::get(netuid),
                TaoBalance::from(29u64)
            );
            assert_eq!(old.get_quote_weight(), quote);
        }
    });
}

#[test]
fn migration_and_lazy_initialization_preserve_deep_pool_sensitivity_and_capacity() {
    new_test_ext().execute_with(|| {
        // A Chutes-sized pool previously had its scales tightened to meet the
        // 500-TAO target. Both eager and lazy conversion now retain the baseline.
        let alpha = 2_880_603_110_475_064_u64;
        let tao = 203_305_249_479_705_u64;
        let expected =
            Superellipse::from_balancer(alpha, tao, Perquintill::from_percent(50)).unwrap();
        let eager = NetUid::from(54);
        let lazy = NetUid::from(55);
        for netuid in [eager, lazy] {
            AlphaReserve::set_mock_reserve(netuid, alpha.into());
            TaoReserve::set_mock_reserve(netuid, tao.into());
        }
        PalSwapInitialized::<Test>::insert(eager, true);
        migrate_balancer_to_superellipse::<Test>();
        Pallet::<Test>::maybe_initialize_palswap(lazy, None).unwrap();
        for netuid in [eager, lazy] {
            let curve = SwapSuperellipse::<Test>::get(netuid).unwrap();
            assert_eq!(curve, expected);
            assert_eq!(curve.max_buy_input(alpha, tao).unwrap(), tao - 1);
            let input = 500_000_000_000_u64;
            let bought = curve.buy_output(alpha, tao, input).unwrap();
            let before = curve.calculate_price(alpha, tao).unwrap().to_num::<f64>();
            let after = curve
                .calculate_price(alpha - bought, tao + input)
                .unwrap()
                .to_num::<f64>();
            assert!(after > before && after < before * 1.01);
            let quotes = (
                curve.buy_output(alpha, tao, input).unwrap(),
                curve.sell_output(alpha, tao, input).unwrap(),
            );
            Pallet::<Test>::extract_unreachable_reserves(netuid).unwrap();
            let active_alpha = u64::from(AlphaReserve::reserve(netuid));
            let active_tao = u64::from(TaoReserve::reserve(netuid));
            let funded = SwapSuperellipse::<Test>::get(netuid).unwrap();
            assert_eq!(
                funded.max_buy_input(active_alpha, active_tao).unwrap(),
                tao - 1
            );
            assert_eq!(
                (
                    funded.buy_output(active_alpha, active_tao, input).unwrap(),
                    funded.sell_output(active_alpha, active_tao, input).unwrap(),
                ),
                quotes
            );
        }
    });
}

#[test]
fn migration_handles_initialized_only_archived_only_empty_root_and_removed_pools() {
    new_test_ext().execute_with(|| {
        let implicit = NetUid::from(61);
        let archived = NetUid::from(62);
        let empty = NetUid::from(63);
        let root = NetUid::from(0);
        let removed = NetUid::from(crate::mock::NON_EXISTENT_NETUID);
        PalSwapInitialized::<Test>::insert(implicit, true);
        SwapBalancer::<Test>::insert(archived, Balancer::default());
        for netuid in [empty, root, removed] {
            PalSwapInitialized::<Test>::insert(netuid, true);
            SwapBalancer::<Test>::insert(netuid, Balancer::default());
        }
        AlphaReserve::set_mock_reserve(empty, AlphaBalance::ZERO);
        migrate_balancer_to_superellipse::<Test>();
        assert!(stamped());
        assert!(SwapSuperellipse::<Test>::contains_key(implicit));
        assert!(SwapSuperellipse::<Test>::contains_key(archived));
        assert!(PalSwapInitialized::<Test>::get(archived));
        for netuid in [empty, root, removed] {
            assert!(!SwapSuperellipse::<Test>::contains_key(netuid));
        }
        let curve = SwapSuperellipse::<Test>::get(implicit).unwrap();
        // Once stamped, later reserve changes must not rebuild migrated pools.
        TaoReserve::set_mock_reserve(implicit, TaoBalance::from(123u64));
        migrate_balancer_to_superellipse::<Test>();
        assert_eq!(SwapSuperellipse::<Test>::get(implicit), Some(curve));
    });
}

#[test]
fn migration_retry_preserves_already_converted_curves() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(71);
        PalSwapInitialized::<Test>::insert(netuid, true);
        migrate_balancer_to_superellipse::<Test>();
        let curve = SwapSuperellipse::<Test>::get(netuid).unwrap();
        HasMigrationRun::<Test>::remove(BoundedVec::truncate_from(MIGRATION_NAME.to_vec()));
        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(111u64));
        migrate_balancer_to_superellipse::<Test>();
        assert!(stamped());
        assert_eq!(SwapSuperellipse::<Test>::get(netuid), Some(curve));
    });
}

#[test]
fn migration_invalid_funded_pool_remains_unstamped_and_can_retry() {
    use codec::{Decode, Encode};
    new_test_ext().execute_with(|| {
        let valid = NetUid::from(72);
        let invalid = NetUid::from(73);
        PalSwapInitialized::<Test>::insert(valid, true);
        PalSwapInitialized::<Test>::insert(invalid, true);
        // Decode models legacy corrupt storage without bypassing the constructor
        // in production code. A zero quote weight cannot preserve a valid price.
        let encoded = 0u64.encode();
        let corrupted = Balancer::decode(&mut &encoded[..]).unwrap();
        SwapBalancer::<Test>::insert(invalid, corrupted);
        migrate_balancer_to_superellipse::<Test>();
        assert!(!stamped());
        assert!(SwapSuperellipse::<Test>::contains_key(valid));
        assert!(!SwapSuperellipse::<Test>::contains_key(invalid));
        let converted = SwapSuperellipse::<Test>::get(valid).unwrap();
        SwapBalancer::<Test>::insert(invalid, Balancer::default());
        migrate_balancer_to_superellipse::<Test>();
        assert!(stamped());
        assert!(SwapSuperellipse::<Test>::contains_key(invalid));
        assert_eq!(SwapSuperellipse::<Test>::get(valid), Some(converted));
    });
}

#[cfg(feature = "try-runtime")]
#[test]
fn migration_try_runtime_checks_initial_and_repeated_upgrades() {
    use crate::pallet::migrations::migrate_balancer_to_superellipse::{post_upgrade, pre_upgrade};
    new_test_ext().execute_with(|| {
        PalSwapInitialized::<Test>::insert(NetUid::from(81), true);
        for _ in 0..2 {
            let before = pre_upgrade::<Test>().unwrap();
            migrate_balancer_to_superellipse::<Test>();
            post_upgrade::<Test>(before).unwrap();
        }
    });
}

#[cfg(feature = "try-runtime")]
#[test]
fn migration_try_runtime_rejects_reserve_drift() {
    use crate::pallet::migrations::migrate_balancer_to_superellipse::{post_upgrade, pre_upgrade};
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(82);
        PalSwapInitialized::<Test>::insert(netuid, true);
        let before = pre_upgrade::<Test>().unwrap();
        migrate_balancer_to_superellipse::<Test>();
        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(999u64));
        assert!(post_upgrade::<Test>(before).is_err());
    });
}

#[cfg(feature = "try-runtime")]
#[test]
fn migration_try_runtime_rejects_price_preserving_depth_changes() {
    use crate::pallet::migrations::migrate_balancer_to_superellipse::{post_upgrade, pre_upgrade};
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(83);
        PalSwapInitialized::<Test>::insert(netuid, true);
        let before = pre_upgrade::<Test>().unwrap();
        migrate_balancer_to_superellipse::<Test>();
        let alpha = u64::from(AlphaReserve::reserve(netuid));
        let tao = u64::from(TaoReserve::reserve(netuid));
        let original_price = Pallet::<Test>::current_price(netuid);
        let mut tightened =
            Superellipse::from_balancer(alpha / 2, tao / 2, Perquintill::from_percent(50)).unwrap();
        tightened.translate_liquidity(alpha / 2, tao / 2).unwrap();
        assert_eq!(
            tightened.calculate_price(alpha, tao).unwrap(),
            original_price
        );
        SwapSuperellipse::<Test>::insert(netuid, tightened);
        assert!(post_upgrade::<Test>(before).is_err());
    });
}

#[test]
fn reserve_extraction_preserves_price_quotes_and_is_idempotent() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(91);
        let alpha = 2_880_603_110_475_064_u64;
        let tao = 203_305_249_479_705_u64;
        AlphaReserve::set_mock_reserve(netuid, alpha.into());
        TaoReserve::set_mock_reserve(netuid, tao.into());
        PalSwapInitialized::<Test>::insert(netuid, true);
        migrate_balancer_to_superellipse::<Test>();
        let curve = SwapSuperellipse::<Test>::get(netuid).unwrap();
        let price = Pallet::<Test>::current_price(netuid);
        let quote = curve.sell_output(alpha, tao, 1_000_000_000).unwrap();
        let (take_alpha, take_tao) = Pallet::<Test>::extract_unreachable_reserves(netuid).unwrap();
        assert!(u64::from(take_alpha) > 0 && u64::from(take_tao) > 0);
        assert_eq!(
            ExtractedReserves::<Test>::get(netuid),
            (take_alpha, take_tao)
        );
        assert_eq!(
            u64::from(AlphaReserve::reserve(netuid)) + u64::from(take_alpha),
            alpha
        );
        assert_eq!(
            u64::from(TaoReserve::reserve(netuid)) + u64::from(take_tao),
            tao
        );
        assert_eq!(Pallet::<Test>::current_price(netuid), price);
        let after = SwapSuperellipse::<Test>::get(netuid).unwrap();
        assert_eq!(
            after
                .sell_output(
                    AlphaReserve::reserve(netuid).into(),
                    TaoReserve::reserve(netuid).into(),
                    1_000_000_000
                )
                .unwrap(),
            quote
        );
        assert_eq!(
            Pallet::<Test>::extract_unreachable_reserves(netuid).unwrap(),
            (AlphaBalance::ZERO, TaoBalance::ZERO)
        );
    });
}

#[test]
fn pool_cleanup_clears_curve_and_extraction_counters_for_netuid_reuse() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(93);
        PalSwapInitialized::<Test>::insert(netuid, true);
        migrate_balancer_to_superellipse::<Test>();
        ExtractedReserves::<Test>::insert(
            netuid,
            (AlphaBalance::from(11_u64), TaoBalance::from(17_u64)),
        );
        let mut meter = frame_support::weights::WeightMeter::new();
        assert!(Pallet::<Test>::do_clear_protocol_liquidity(
            netuid, &mut meter
        ));
        assert!(!SwapSuperellipse::<Test>::contains_key(netuid));
        assert!(!ExtractedReserves::<Test>::contains_key(netuid));
    });
}

#[cfg(feature = "try-runtime")]
#[test]
fn migration_try_runtime_accepts_certified_combined_funding_extraction() {
    use crate::pallet::migrations::migrate_balancer_to_superellipse::{post_upgrade, pre_upgrade};
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(94);
        PalSwapInitialized::<Test>::insert(netuid, true);
        let before = pre_upgrade::<Test>().unwrap();
        migrate_balancer_to_superellipse::<Test>();
        Pallet::<Test>::extract_unreachable_reserves(netuid).unwrap();
        post_upgrade::<Test>(before).unwrap();
    });
}

#[test]
fn lending_buy_capacity_inverts_actual_fees_and_never_partially_fills() {
    use crate::mock::GetAlphaForTao;
    use subtensor_swap_interface::{Order, SwapEngine};
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(95);
        let alpha = 2_880_603_110_475_064_u64;
        let tao = 203_305_249_479_705_u64;
        AlphaReserve::set_mock_reserve(netuid, alpha.into());
        TaoReserve::set_mock_reserve(netuid, tao.into());
        PalSwapInitialized::<Test>::insert(netuid, true);
        migrate_balancer_to_superellipse::<Test>();
        Pallet::<Test>::extract_unreachable_reserves(netuid).unwrap();
        for fee in [0, 33, 10_000] {
            FeeRate::<Test>::insert(netuid, fee);
            let capacity = u64::from(Pallet::<Test>::maximum_buy_input(netuid));
            assert!(capacity > 0);
            let filled = <Pallet<Test> as SwapEngine<GetAlphaForTao>>::swap(
                netuid,
                GetAlphaForTao::with_amount(capacity),
                u64::MAX.into(),
                false,
                true,
            )
            .unwrap();
            assert_eq!(
                u64::from(filled.amount_paid_in) + u64::from(filled.fee_paid),
                capacity
            );
            assert!(filled.amount_paid_out > AlphaBalance::ZERO);
            let next = <Pallet<Test> as SwapEngine<GetAlphaForTao>>::swap(
                netuid,
                GetAlphaForTao::with_amount(capacity + 1),
                u64::MAX.into(),
                false,
                true,
            );
            assert!(
                next.is_err()
                    || next.is_ok_and(|fill| u64::from(fill.amount_paid_in)
                        + u64::from(fill.fee_paid)
                        < capacity + 1)
            );
        }
        assert_eq!(
            Pallet::<Test>::maximum_buy_input(NetUid::from(crate::mock::NON_EXISTENT_NETUID)),
            TaoBalance::ZERO
        );
    });
}

#[test]
fn lending_buy_capacity_handles_dust_quotes_and_near_sell_endpoint_input_guard() {
    use crate::mock::GetAlphaForTao;
    use subtensor_swap_interface::{Order, SwapEngine};
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(96);
        AlphaReserve::set_mock_reserve(netuid, 1_000_000_000_u64.into());
        TaoReserve::set_mock_reserve(netuid, 1_000_000_000_000_000_u64.into());
        Pallet::<Test>::maybe_initialize_palswap(netuid, None).unwrap();
        assert!(
            <Pallet<Test> as SwapEngine<GetAlphaForTao>>::swap(
                netuid,
                GetAlphaForTao::with_amount(1_u64),
                u64::MAX.into(),
                false,
                true,
            )
            .is_err()
        );
        let capacity = u64::from(Pallet::<Test>::maximum_buy_input(netuid));
        assert!(capacity > 0);
        let fill = <Pallet<Test> as SwapEngine<GetAlphaForTao>>::swap(
            netuid,
            GetAlphaForTao::with_amount(capacity),
            u64::MAX.into(),
            false,
            true,
        )
        .unwrap();
        assert_eq!(
            u64::from(fill.amount_paid_in) + u64::from(fill.fee_paid),
            capacity
        );
        Pallet::<Test>::extract_unreachable_reserves(netuid).unwrap();
        let curve = SwapSuperellipse::<Test>::get(netuid).unwrap();
        let alpha = u64::from(AlphaReserve::reserve(netuid));
        let tao = u64::from(TaoReserve::reserve(netuid));
        let sell = curve
            .max_sell_input_with_reserve_floor(alpha, tao, 1)
            .unwrap();
        let output = curve.sell_output(alpha, tao, sell).unwrap();
        AlphaReserve::set_mock_reserve(netuid, (alpha + sell).into());
        TaoReserve::set_mock_reserve(netuid, (tao - output).into());
        let capacity = u64::from(Pallet::<Test>::maximum_buy_input(netuid));
        let fee = u64::from(Pallet::<Test>::calculate_fee_amount(
            netuid,
            TaoBalance::from(capacity),
            false,
        ));
        assert!(capacity - fee <= (tao - output).saturating_mul(1_000));
    });
}
