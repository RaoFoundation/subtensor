//! Try-state checks I25 and I26 (SPEC section 5): passing on healthy state and failing under
//! fault injection.

use frame_support::assert_ok;
use sp_runtime::TryRuntimeError;
use subtensor_runtime_common::NetUid;

use super::mock::*;
use crate::{
    DeregParamsStore, LendingParamsStore, LongParamsStore, PendingDeregParams,
    PendingLendingParams, PendingLongParams, PendingShortParams, RankGeneration, RankPrice,
    RankRing, RankRingGeneration, ShortParamsStore, TwapRing,
    math::fixed::Price,
    params::{DeregParams, LendingParams, LongParams, ShortParams},
    prices::{RankState, TwapState},
};

/// Runs `fault` on healthy migrated state and expects try-state to fail with `message`.
fn assert_fault(message: &'static str, fault: impl FnOnce()) {
    migrated_ext().execute_with(|| {
        assert_ok!(Lending::do_try_state());
        fault();
        assert_eq!(
            Lending::do_try_state(),
            Err(TryRuntimeError::Other(message))
        );
    });
}

fn sn1() -> NetUid {
    NetUid::from(1)
}

#[test]
fn healthy_state_passes_across_blocks_setters_and_reseeds() {
    migrated_ext().execute_with(|| {
        assert_ok!(Lending::do_try_state());
        run_to(7_200);
        assert_ok!(Lending::do_try_state());
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_ok!(Lending::do_try_state());
        assert_ok!(Lending::set_lending_params(
            RuntimeOrigin::root(),
            LendingParams {
                twap_samples: 3,
                ..Default::default()
            }
        ));
        assert_ok!(Lending::set_dereg_params(
            RuntimeOrigin::root(),
            DeregParams {
                rank_window_days: 7,
                ..Default::default()
            }
        ));
        // Pending values are in bounds while they wait.
        assert_ok!(Lending::do_try_state());
        next_block();
        assert_ok!(Lending::do_try_state());
        run_to(2 * 7_200);
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn i25_fails_for_a_live_subnet_without_a_rank_ring() {
    assert_fault("I25: live subnet has no rank ring", || {
        RankRing::<Test>::remove(sn1());
    });
    // A subnet that goes live without the registration hook is caught the same way.
    assert_fault("I25: live subnet has no rank ring", || {
        set_pool(3, 1_000, 10_000);
    });
}

#[test]
fn i25_fails_for_a_partial_rank_ring() {
    assert_fault("I25: rank ring is not a full window", || {
        RankRing::<Test>::insert(sn1(), RankState::seeded(px(800, 10_000), 29));
    });
}

#[test]
fn i25_fails_for_a_stale_or_missing_generation() {
    assert_fault("I25: rank ring generation is stale", || {
        RankGeneration::<Test>::mutate(|g| *g += 1);
    });
    assert_fault("I25: rank ring generation is stale", || {
        RankRingGeneration::<Test>::remove(sn1());
    });
}

#[test]
fn i25_fails_for_a_missing_or_wrong_rank_price() {
    assert_fault("I25: live subnet has no published RankPrice", || {
        RankPrice::<Test>::remove(sn1());
    });
    // Zero, the emission EMA or any other substitute is not the ring's mean.
    assert_fault("I25: RankPrice is not the mean of the ring", || {
        RankPrice::<Test>::insert(sn1(), Price::from_bits(0));
    });
    assert_fault("I25: RankPrice is not the mean of the ring", || {
        let mut ring = RankState::seeded(px(800, 10_000), 30);
        ring.sample(px(100, 10_000));
        assert!(ring.close_day().is_some());
        RankRing::<Test>::insert(sn1(), ring);
    });
}

#[test]
fn i25_fails_for_a_missing_or_partial_twap_ring() {
    assert_fault("I25: live subnet has no TWAP ring", || {
        TwapRing::<Test>::remove(sn1());
    });
    assert_fault("I25: TWAP ring is not a full window", || {
        TwapRing::<Test>::insert(sn1(), TwapState::seeded(px(1_000, 10_000), 71));
    });
}

#[test]
fn i26_fails_for_out_of_bounds_live_params() {
    assert_fault("I26: LendingParams out of bounds", || {
        LendingParamsStore::<Test>::mutate(|p| p.apr_bps = 0);
    });
    assert_fault("I26: LendingParams out of bounds", || {
        LendingParamsStore::<Test>::mutate(|p| p.exit_hysteresis_bps = 5_001);
    });
    assert_fault("I26: LendingParams out of bounds", || {
        LendingParamsStore::<Test>::mutate(|p| p.withdraw_buffer_blocks = 7_199);
    });
    assert_fault("I26: ShortParams out of bounds", || {
        ShortParamsStore::<Test>::mutate(|p| p.stress_bps = 9_999);
    });
    assert_fault("I26: LongParams out of bounds", || {
        LongParamsStore::<Test>::mutate(|p| p.stress_bps = 14_999);
    });
    assert_fault("I26: DeregParams out of bounds", || {
        DeregParamsStore::<Test>::mutate(|p| p.max_long_positions_per_subnet = 1_025);
    });
}

#[test]
fn i26_fails_for_out_of_bounds_pending_params() {
    assert_fault("I26: LendingParams out of bounds", || {
        PendingLendingParams::<Test>::put(LendingParams {
            twap_samples: 73,
            ..Default::default()
        });
    });
    assert_fault("I26: ShortParams out of bounds", || {
        PendingShortParams::<Test>::put(ShortParams {
            open_limit_bps: 0,
            ..Default::default()
        });
    });
    assert_fault("I26: LongParams out of bounds", || {
        PendingLongParams::<Test>::put(LongParams {
            lift_cap_bps: 5_001,
            ..Default::default()
        });
    });
    assert_fault("I26: DeregParams out of bounds", || {
        PendingDeregParams::<Test>::put(DeregParams {
            rank_window_days: 31,
            ..Default::default()
        });
    });
}
