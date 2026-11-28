//! Try-state checks I25 and I26 (SPEC section 5), the per-subnet position index and the
//! dissolution settlement (I8): passing on healthy state and failing under fault injection.

use frame_support::{assert_ok, weights::WeightMeter};
use sp_runtime::TryRuntimeError;
use subtensor_runtime_common::{NetUid, TaoBalance};

use super::mock::*;
use crate::{
    DeregParamsStore, LendingParamsStore, LongParamsStore, PendingDeregParams,
    PendingLendingParams, PendingLongParams, PendingShortParams, RankGeneration, RankPrice,
    RankRing, RankRingGeneration, ShortParamsStore, TwapRing,
    math::fixed::Price,
    params::{DeregParams, LendingParams, LongParams, ShortParams},
    prices::{RankState, TwapState},
};
use crate::{
    DissolutionSettlements, LendingInterface, PositionsByNetuid, ResidualAlpha, SettlementRetry,
    dissolution::SettlementStage, params::RAO, types::Side,
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
        LendingParamsStore::<Test>::mutate(|p| p.rate_k = 9);
    });
    assert_fault("I26: LendingParams out of bounds", || {
        LendingParamsStore::<Test>::mutate(|p| p.rate_cap_bps = p.apr_bps - 1);
    });
    assert_fault("I26: LendingParams out of bounds", || {
        LendingParamsStore::<Test>::mutate(|p| p.rate_cap_bps = 1_000_001);
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
    // Pending groups are due at block 2 or later, the block after the test's block 1.
    assert_fault("I26: pending LendingParams out of bounds", || {
        PendingLendingParams::<Test>::put((
            LendingParams {
                twap_samples: 73,
                ..Default::default()
            },
            2,
        ));
    });
    assert_fault("I26: pending LendingParams out of bounds", || {
        // A cap below the live base APR fails the cross-check against the live group.
        PendingLendingParams::<Test>::put((
            LendingParams {
                apr_bps: 2_001,
                rate_cap_bps: 2_000,
                ..Default::default()
            },
            2,
        ));
    });
    assert_fault("I26: pending LendingParams is overdue", || {
        PendingLendingParams::<Test>::put((LendingParams::default(), 1));
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

const ALICE: AccountId = 1;
const BOB: AccountId = 2;

/// Runs `fault` on subnet 1 with Alice's and Bob's shorts open and the subnet flagged for
/// dissolution, and expects try-state to fail with `message`.
fn assert_short_fault(message: &'static str, fault: impl FnOnce()) {
    migrated_ext().execute_with(|| {
        set_pool(1, 5_000 * RAO, 50_000 * RAO);
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        for who in [ALICE, BOB] {
            set_balance(who, 100 * RAO);
            assert_ok!(Lending::open_short(
                RuntimeOrigin::signed(who),
                sn1(),
                200 * RAO,
                45 * RAO,
                0
            ));
        }
        <Lending as LendingInterface<AccountId>>::on_dissolve_flag(
            sn1(),
            TaoBalance::from(5_000 * RAO),
        );
        assert_ok!(Lending::do_try_state());
        fault();
        assert_eq!(
            Lending::do_try_state(),
            Err(TryRuntimeError::Other(message))
        );
    });
}

/// One settlement step that affords `visits` visits.
fn settle_visits(visits: u32) {
    let mut meter = WeightMeter::with_limit(Lending::dissolution_weight(visits));
    <Lending as LendingInterface<AccountId>>::on_dissolve(sn1(), &mut meter);
}

#[test]
fn the_position_index_fails_for_a_missing_or_a_stale_key() {
    assert_short_fault("PositionsByNetuid misses an open position", || {
        PositionsByNetuid::<Test>::remove((sn1(), Side::Short, ALICE));
    });
    assert_short_fault("PositionsByNetuid has a key with no open position", || {
        PositionsByNetuid::<Test>::insert((sn1(), Side::Short, 77), ());
    });
    // A key on the wrong side is a key with no position too.
    assert_short_fault("PositionsByNetuid has a key with no open position", || {
        PositionsByNetuid::<Test>::insert((sn1(), Side::Long, ALICE), ());
    });
}

#[test]
fn i8_fails_for_a_short_the_settlement_would_not_reach() {
    // The walk's cursor is past a short that is still open and not queued.
    assert_short_fault(
        "I8: a short of a dissolving subnet is neither settled nor pending",
        || {
            // Two accruals and no settlement: the settling walk is at its start.
            settle_visits(2);
            assert_ok!(Lending::do_try_state());
            let last = PositionsByNetuid::<Test>::iter_key_prefix((sn1(), Side::Short))
                .last()
                .expect("two shorts are open");
            DissolutionSettlements::<Test>::mutate(sn1(), |progress| {
                let progress = progress.as_mut().expect("the step left its progress");
                assert_eq!(progress.stage, SettlementStage::Settling);
                progress.cursor = Some(last);
            });
        },
    );
    // The retry pass with a short that is not in the queue.
    assert_short_fault(
        "I8: a short of a dissolving subnet is neither settled nor pending",
        || {
            settle_visits(2);
            DissolutionSettlements::<Test>::mutate(sn1(), |progress| {
                progress.as_mut().expect("progress").stage = SettlementStage::Retrying;
            });
        },
    );
    // A settlement recorded as done with a short left open.
    assert_short_fault(
        "I8: a short of a dissolving subnet is neither settled nor pending",
        || {
            settle_visits(2);
            DissolutionSettlements::<Test>::mutate(sn1(), |progress| {
                progress.as_mut().expect("progress").stage = SettlementStage::Done;
            });
        },
    );
}

#[test]
fn i8_fails_for_records_the_settlement_does_not_own() {
    assert_short_fault("I8: settlement retry without an open short", || {
        settle_visits(2);
        SettlementRetry::<Test>::insert(sn1(), 77, 1);
    });
    assert_short_fault(
        "I8: settlement retry on a subnet whose settlement is not running",
        || {
            SettlementRetry::<Test>::insert(sn1(), ALICE, 1);
        },
    );
    assert_short_fault("I8: ResidualAlpha left after the settlement", || {
        <Lending as LendingInterface<AccountId>>::on_dissolve(sn1(), &mut WeightMeter::new());
        assert_ok!(Lending::do_try_state());
        ResidualAlpha::<Test>::insert(sn1(), 1);
    });
    assert_short_fault(
        "I8: settlement progress without a dissolution record",
        || {
            settle_visits(2);
            crate::DissolvingSubnets::<Test>::remove(sn1());
        },
    );
}
