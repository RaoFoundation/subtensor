#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! ConvictionModel roll-forward math.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 5: ConvictionModel roll-forward math
// =========================================================================

#[test]
fn test_exp_decay_zero_dt() {
    new_test_ext(1).execute_with(|| {
        let result = ConvictionModel::exp_decay(0, 216000);
        assert_eq!(result, U64F64::from_num(1));
    });
}

#[test]
fn test_exp_decay_zero_tau() {
    new_test_ext(1).execute_with(|| {
        let result = ConvictionModel::exp_decay(1000, 0);
        assert_eq!(result, U64F64::from_num(0));
    });
}

#[test]
fn test_exp_decay_one_tau() {
    new_test_ext(1).execute_with(|| {
        let tau = 216000u64;
        let result = ConvictionModel::exp_decay(tau, tau);
        // exp(-1) ~= 0.36787944
        let expected = U64F64::from_num(0.36787944f64);
        let diff = if result > expected {
            result - expected
        } else {
            expected - result
        };
        assert!(diff < U64F64::from_num(0.001));
    });
}

#[test]
fn test_exp_decay_clamps_large_dt_to_min_ratio() {
    new_test_ext(1).execute_with(|| {
        let tau = 216000u64;
        let clamped_result = ConvictionModel::exp_decay(40 * tau, tau);
        let oversized_result = ConvictionModel::exp_decay(100 * tau, tau);

        let diff = if oversized_result > clamped_result {
            oversized_result - clamped_result
        } else {
            clamped_result - oversized_result
        };

        assert!(diff < U64F64::from_num(0.000000001));
        assert!(oversized_result > U64F64::from_num(0));
    });
}

#[test]
fn test_roll_forward_individual_lock_uses_lock_owner_and_decay_mode() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let owner_hotkey = SubnetOwnerHotkey::<Test>::get(netuid);
        DecayingLock::<Test>::remove(coldkey, netuid);

        let lock = LockState {
            locked_mass: 10_000u64.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let now = 1_000u64;

        let rolled =
            roll_forward_individual_lock(&coldkey, netuid, &owner_hotkey, lock.clone(), now);
        let expected = ConvictionModel::roll_forward_lock(
            lock,
            now,
            UnlockRate::<Test>::get(),
            MaturityRate::<Test>::get(),
            true,
            false,
        )
        .0;

        assert_eq!(rolled, expected);
    });
}

#[test]
fn test_roll_forward_hotkey_lock_uses_perpetual_general_mode() {
    new_test_ext(1).execute_with(|| {
        let lock = LockState {
            locked_mass: 10_000u64.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let now = 1_000u64;

        let rolled = roll_forward_hotkey_lock(lock.clone(), now);
        let expected = ConvictionModel::roll_forward_lock(
            lock,
            now,
            UnlockRate::<Test>::get(),
            MaturityRate::<Test>::get(),
            false,
            true,
        )
        .0;

        assert_eq!(rolled, expected);
    });
}

#[test]
fn test_roll_forward_decaying_hotkey_lock_uses_decaying_general_mode() {
    new_test_ext(1).execute_with(|| {
        let lock = LockState {
            locked_mass: 10_000u64.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let now = 1_000u64;

        let rolled = roll_forward_decaying_hotkey_lock(lock.clone(), now);
        let expected = ConvictionModel::roll_forward_lock(
            lock,
            now,
            UnlockRate::<Test>::get(),
            MaturityRate::<Test>::get(),
            false,
            false,
        )
        .0;

        assert_eq!(rolled, expected);
    });
}

#[test]
fn test_roll_forward_locked_mass_decays() {
    new_test_ext(1).execute_with(|| {
        let lock_amount = 10000u64;
        let lock = LockState {
            locked_mass: lock_amount.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let rolled = roll_forward_lock(lock, UnlockRate::<Test>::get(), false, false);

        assert!(rolled.locked_mass < lock_amount.into());
        assert!(rolled.locked_mass > AlphaBalance::ZERO);
    });
}

#[test]
fn test_roll_forward_conviction_uses_unequal_rate_closed_form() {
    new_test_ext(1).execute_with(|| {
        let locked_mass = 10_000u64;
        let dt = 10_000u64;
        let unlock_rate = 200_000u64;
        let maturity_rate = 240_000u64;
        UnlockRate::<Test>::set(unlock_rate);
        MaturityRate::<Test>::set(maturity_rate);
        assert_ne!(unlock_rate, maturity_rate);

        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let rolled = roll_forward_lock(lock, dt, false, false);

        let unlock_decay = ConvictionModel::exp_decay(dt, unlock_rate);
        let maturity_decay = ConvictionModel::exp_decay(dt, maturity_rate);
        let gamma = U64F64::from_num(unlock_rate)
            .saturating_mul(maturity_decay.saturating_sub(unlock_decay))
            .safe_div(U64F64::from_num(maturity_rate.saturating_sub(unlock_rate)));
        let expected = U64F64::from_num(locked_mass).saturating_mul(gamma);

        assert_abs_diff_eq!(
            rolled.conviction.to_num::<f64>(),
            expected.to_num::<f64>(),
            epsilon = 0.0000001
        );
    });
}

#[test]
fn test_roll_forward_adjacent_large_rates_and_large_mass_match_f64_closed_form() {
    new_test_ext(1).execute_with(|| {
        let unlock_rate = 1_142_108u64;
        let maturity_rate = unlock_rate + 1;
        let locked_mass = 21_000_000_000_000_000u64;
        let dt = unlock_rate;
        UnlockRate::<Test>::put(unlock_rate);
        MaturityRate::<Test>::put(maturity_rate);

        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let rolled = roll_forward_lock(lock, dt, false, false);

        let decay_x = (-(dt as f64) / unlock_rate as f64).exp();
        let decay_z = (-(dt as f64) / maturity_rate as f64).exp();
        let gamma =
            unlock_rate as f64 * (decay_x - decay_z) / (unlock_rate as f64 - maturity_rate as f64);
        let expected_conviction = locked_mass as f64 * gamma;
        let expected_locked_mass = locked_mass as f64 * decay_x;

        assert_abs_diff_eq!(
            rolled.conviction.to_num::<f64>(),
            expected_conviction,
            epsilon = 50_000.0
        );
        assert_abs_diff_eq!(
            u64::from(rolled.locked_mass) as f64,
            expected_locked_mass,
            epsilon = 2_000.0
        );
    });
}

#[test]
fn test_roll_forward_scales_linearly_with_locked_mass() {
    new_test_ext(1).execute_with(|| {
        let dt = 25_000u64;
        let base_mass = 10_000u64;
        let base = LockState {
            locked_mass: base_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let double = LockState {
            locked_mass: (base_mass * 2).into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };

        let rolled_base = roll_forward_lock(base, dt, false, false);
        let rolled_double = roll_forward_lock(double, dt, false, false);

        assert_abs_diff_eq!(
            u64::from(rolled_double.locked_mass) as f64,
            (u64::from(rolled_base.locked_mass) * 2) as f64,
            epsilon = 1.0
        );
        assert_abs_diff_eq!(
            rolled_double.conviction.to_num::<f64>(),
            rolled_base.conviction.to_num::<f64>() * 2.0,
            epsilon = 0.0000001
        );
    });
}

#[test]
fn test_roll_forward_chunked_update_matches_single_update() {
    new_test_ext(1).execute_with(|| {
        let lock = LockState {
            locked_mass: 1_000_000_000u64.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let mid = 10_000u64;
        let end = 20_000u64;

        let rolled_once = roll_forward_lock(lock.clone(), end, false, false);
        let rolled_twice = roll_forward_lock(
            roll_forward_lock(lock, mid, false, false),
            end,
            false,
            false,
        );

        assert_abs_diff_eq!(
            u64::from(rolled_twice.locked_mass) as f64,
            u64::from(rolled_once.locked_mass) as f64,
            epsilon = 1.0
        );
        assert_abs_diff_eq!(
            rolled_twice.conviction.to_num::<f64>(),
            rolled_once.conviction.to_num::<f64>(),
            epsilon = 0.1
        );
    });
}

#[test]
fn test_roll_forward_conviction_stays_below_original_mass_for_one_shot_lock() {
    new_test_ext(1).execute_with(|| {
        let locked_mass = 10_000u64;
        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };
        let cap = U64F64::from_num(locked_mass);

        for dt in [
            1_000u64,
            10_000u64,
            UnlockRate::<Test>::get(),
            MaturityRate::<Test>::get(),
            MaturityRate::<Test>::get().saturating_mul(5),
        ] {
            let rolled = roll_forward_lock(lock.clone(), dt, false, false);
            assert!(rolled.conviction <= cap);
        }
    });
}

#[test]
fn test_roll_forward_decaying_conviction_peak_is_below_original_lock() {
    new_test_ext(1).execute_with(|| {
        UnlockRate::<Test>::set(200_000u64);
        MaturityRate::<Test>::set(240_000u64);

        let locked_mass = 10_000u64;
        let unlock_rate = UnlockRate::<Test>::get() as f64;
        let maturity_rate = MaturityRate::<Test>::get() as f64;
        assert_ne!(unlock_rate, maturity_rate);

        let peak_block = ((unlock_rate * maturity_rate) / (unlock_rate - maturity_rate)
            * (unlock_rate / maturity_rate).ln())
        .round() as u64;
        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };

        let rolled = roll_forward_lock(lock, peak_block, false, false);

        assert!(rolled.conviction < U64F64::from_num(locked_mass));
    });
}

#[test]
fn test_roll_forward_perpetual_mass_does_not_decay_and_conviction_matures() {
    new_test_ext(1).execute_with(|| {
        let locked_mass = 10_000u64;
        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };

        let rolled = roll_forward_lock(lock, MaturityRate::<Test>::get(), false, true);

        assert_eq!(rolled.locked_mass, locked_mass.into());
        assert!(rolled.conviction > U64F64::from_num(0));
        assert!(rolled.conviction < U64F64::from_num(locked_mass));
    });
}

#[test]
fn test_roll_forward_perpetual_conviction_never_exceeds_lock() {
    new_test_ext(1).execute_with(|| {
        let locked_mass = 10_000u64;
        let lock = LockState {
            locked_mass: locked_mass.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };

        for dt in [
            1u64,
            1_000u64,
            MaturityRate::<Test>::get(),
            MaturityRate::<Test>::get().saturating_mul(10),
            MaturityRate::<Test>::get().saturating_mul(1_000),
        ] {
            let rolled = roll_forward_lock(lock.clone(), dt, false, true);
            assert_eq!(rolled.locked_mass, locked_mass.into());
            assert!(rolled.conviction <= U64F64::from_num(locked_mass));
        }
    });
}

#[test]
fn test_roll_forward_conviction_converges_to_zero() {
    new_test_ext(1).execute_with(|| {
        let lock_amount = 10000u64;
        let lock = LockState {
            locked_mass: lock_amount.into(),
            conviction: U64F64::from_num(0),
            last_update: 0,
        };

        let c0 = lock.conviction;
        assert_eq!(c0, U64F64::from_num(0));

        let rolled = roll_forward_lock(lock.clone(), 100, false, false);
        let c1 = rolled.conviction;
        assert!(c1 > U64F64::from_num(0));

        let rolled = roll_forward_lock(lock.clone(), 1_100, false, false);
        let c2 = rolled.conviction;
        assert!(c2 > c1);

        let tau = MaturityRate::<Test>::get();
        let c_late = roll_forward_lock(lock, tau * 1000, false, false).conviction;
        assert_abs_diff_eq!(c_late.to_num::<f64>(), 0., epsilon = 0.0000001);
    });
}

#[test]
fn test_roll_forward_normalizes_dust_to_zero() {
    new_test_ext(1).execute_with(|| {
        let lock = LockState {
            locked_mass: 99u64.into(),
            conviction: U64F64::from_num(99),
            last_update: 100,
        };

        let rolled = roll_forward_lock(lock, 100, false, false);

        assert_eq!(rolled.locked_mass, AlphaBalance::ZERO);
        assert_eq!(rolled.conviction, U64F64::from_num(0));
        assert_eq!(rolled.last_update, 100);
    });
}

#[test]
fn test_roll_forward_no_change_when_now_equals_last_update() {
    new_test_ext(1).execute_with(|| {
        let lock = LockState {
            locked_mass: 5000.into(),
            conviction: U64F64::from_num(1234),
            last_update: 100,
        };
        let rolled = roll_forward_lock(lock.clone(), 100, false, false);
        assert_eq!(rolled.locked_mass, lock.locked_mass);
        assert_eq!(rolled.conviction, lock.conviction);
        assert_eq!(rolled.last_update, 100);
    });
}
