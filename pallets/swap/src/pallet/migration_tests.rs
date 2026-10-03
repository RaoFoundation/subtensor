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
fn migration_preserves_price_balances_and_local_calibration_for_extreme_weights() {
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
            let q = 100_000_000u64;
            let output = ellipse.buy_output(x, y, q).unwrap();
            let next_price = ellipse
                .calculate_price(x - output, y + q)
                .unwrap()
                .to_num::<f64>();
            let measured_sensitivity = (next_price / p_after).ln() / q as f64;
            let base_weight = 1.0 - parts as f64 / 1000.0;
            let old_sensitivity = 1.0 / (base_weight * y as f64);
            assert!((measured_sensitivity / old_sensitivity - 1.0).abs() < 1e-3);
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
        // Once stamped, later reserve changes must not recalibrate migrated pools.
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
