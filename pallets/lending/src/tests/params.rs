//! Root setters (SPEC 2.1, I26): origins, bounds at the edges, the pending-apply cycle and the
//! ring re-seeds on window changes.

use frame_support::{assert_noop, assert_ok, dispatch::DispatchResult};
use sp_runtime::DispatchError;
use subtensor_runtime_common::NetUid;

use super::mock::*;
use crate::{
    DeregParamsStore, Enabled, Error, Event, LendingParamsStore, LongParamsStore, LongsEnabled,
    PendingDeregParams, PendingLendingParams, PendingLongParams, PendingShortParams, PoolTaoEma,
    RankGeneration, RankPrice, RankRing, RankRingGeneration, ShortParamsStore, TwapRing,
    math::fixed::Price,
    params::{DeregParams, LendingParams, LongParams, RAO, ShortParams},
    prices::{RankState, TwapState},
};

fn root() -> RuntimeOrigin {
    RuntimeOrigin::root()
}

fn set_lending(p: LendingParams) -> DispatchResult {
    Lending::set_lending_params(root(), p)
}
fn set_short(p: ShortParams) -> DispatchResult {
    Lending::set_short_params(root(), p)
}
fn set_long(p: LongParams) -> DispatchResult {
    Lending::set_long_params(root(), p)
}
fn set_dereg(p: DeregParams) -> DispatchResult {
    Lending::set_dereg_params(root(), p)
}

/// Through the setter, `field` accepts exactly `[min, max]` around an otherwise valid `base`.
fn check_edges<P: Clone>(
    name: &str,
    base: &P,
    (min, max): (u64, u64),
    field: impl Fn(&mut P, u64),
    call: impl Fn(P) -> DispatchResult,
) {
    let with = |v: u64| {
        let mut p = base.clone();
        field(&mut p, v);
        call(p)
    };
    assert_eq!(with(min), Ok(()), "{name} min");
    assert_eq!(with(max), Ok(()), "{name} max");
    assert_noop!(with(max + 1), Error::<Test>::InvalidParams);
    if min > 0 {
        assert_noop!(with(min - 1), Error::<Test>::InvalidParams);
    }
}

#[test]
fn every_root_call_rejects_signed_and_unsigned_origins() {
    migrated_ext().execute_with(|| {
        for origin in [RuntimeOrigin::signed(1), RuntimeOrigin::none()] {
            assert_noop!(
                Lending::set_enabled(origin.clone(), true),
                DispatchError::BadOrigin
            );
            assert_noop!(
                Lending::set_longs_enabled(origin.clone(), true),
                DispatchError::BadOrigin
            );
            assert_noop!(
                Lending::set_lending_params(origin.clone(), LendingParams::default()),
                DispatchError::BadOrigin
            );
            assert_noop!(
                Lending::set_short_params(origin.clone(), ShortParams::default()),
                DispatchError::BadOrigin
            );
            assert_noop!(
                Lending::set_long_params(origin.clone(), LongParams::default()),
                DispatchError::BadOrigin
            );
            assert_noop!(
                Lending::set_dereg_params(origin, DeregParams::default()),
                DispatchError::BadOrigin
            );
        }
        assert!(!Enabled::<Test>::get());
        assert!(!LongsEnabled::<Test>::get());
        assert!(lending_events().is_empty());
    });
}

#[test]
fn set_lending_params_bounds_at_the_edges() {
    migrated_ext().execute_with(|| {
        let base = LendingParams::default();
        let c = set_lending;
        // The base APR's upper edge needs a cap that admits it.
        let wide = LendingParams {
            rate_cap_bps: 1_000_000,
            ..Default::default()
        };
        check_edges(
            "apr_bps",
            &wide,
            (1, 10_000),
            |p, v| p.apr_bps = v as u32,
            c,
        );
        check_edges("rate_k", &base, (1, 8), |p, v| p.rate_k = v as u8, c);
        // The cap's lower edge is the base APR of the same value (2,000 at launch).
        check_edges(
            "rate_cap_bps",
            &base,
            (2_000, 1_000_000),
            |p, v| p.rate_cap_bps = v as u32,
            c,
        );
        check_edges(
            "rate_delay_blocks",
            &base,
            (0, 216_000),
            |p, v| p.rate_delay_blocks = v as u32,
            c,
        );
        check_edges(
            "charge_interval_blocks",
            &base,
            (1, 7_200),
            |p, v| p.charge_interval_blocks = v as u32,
            c,
        );
        check_edges(
            "min_pool_tao",
            &base,
            (100 * RAO, 100_000 * RAO),
            |p, v| p.min_pool_tao = v,
            c,
        );
        check_edges(
            "min_notional",
            &base,
            (RAO / 10, 10_000 * RAO),
            |p, v| p.min_notional = v,
            c,
        );
        check_edges("deposit", &base, (0, 10 * RAO), |p, v| p.deposit = v, c);
        check_edges(
            "twap_samples",
            &base,
            (3, 72),
            |p, v| p.twap_samples = v as u32,
            c,
        );
        check_edges(
            "twap_interval_blocks",
            &base,
            (50, 1_200),
            |p, v| p.twap_interval_blocks = v as u32,
            c,
        );
    });
}

/// The launch group with the three rate terms set; every other field is at its launch value.
fn with_terms(apr_bps: u32, rate_k: u8, rate_cap_bps: u32) -> LendingParams {
    LendingParams {
        apr_bps,
        rate_k,
        rate_cap_bps,
        ..Default::default()
    }
}

#[test]
fn the_rate_curve_is_set_within_its_bounds() {
    migrated_ext().execute_with(|| {
        // Lower terms do not tighten: the group is queued for the next block.
        assert_ok!(set_lending(with_terms(1_000, 1, 2_000)));
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((with_terms(1_000, 1, 2_000), 2))
        );
        // The widest terms raise the rate: queued for the launch delay of 21,600 blocks.
        assert_ok!(set_lending(with_terms(10_000, 8, 1_000_000)));
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((with_terms(10_000, 8, 1_000_000), 1 + 21_600))
        );
        // Each rejected call writes nothing: the pending group is the last accepted one.
        for rejected in [
            with_terms(2_000, 0, 20_000),
            with_terms(2_000, 9, 20_000),
            with_terms(0, 1, 20_000),
            with_terms(10_001, 1, 20_000),
            with_terms(2_000, 2, 1_000_001),
            // The cap may not be below the base APR, whichever of the two moved.
            with_terms(2_000, 2, 1_999),
            with_terms(10_000, 2, 9_999),
            with_terms(2_001, 2, 2_000),
        ] {
            assert_noop!(set_lending(rejected), Error::<Test>::InvalidParams);
        }
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((with_terms(10_000, 8, 1_000_000), 1 + 21_600))
        );
    });
}

#[test]
fn the_cap_admits_the_base_apr_under_the_live_and_the_new_group() {
    migrated_ext().execute_with(|| {
        set_live_lending(|p| {
            p.apr_bps = 5_000;
            p.rate_cap_bps = 5_000;
        });
        // Lowering both in one call leaves a cap below the live base APR: refused.
        assert_noop!(
            set_lending(with_terms(1_000, 1, 1_000)),
            Error::<Test>::InvalidParams
        );
        // Lowering the APR first, with the cap still at 5,000, is accepted.
        assert_ok!(set_lending(with_terms(1_000, 1, 5_000)));
        next_block();
        assert_eq!(
            LendingParamsStore::<Test>::get(),
            with_terms(1_000, 1, 5_000)
        );
        assert_ok!(set_lending(with_terms(1_000, 1, 1_000)));
    });
}

#[test]
fn the_launch_curve_is_the_spec_table() {
    migrated_ext().execute_with(|| {
        let live = LendingParamsStore::<Test>::get();
        assert_eq!(
            (live.apr_bps, live.rate_k, live.rate_cap_bps),
            (2_000, 1, 20_000)
        );
        // A raised rate waits three days.
        assert_eq!(live.rate_delay_blocks, 21_600);
        assert_eq!(PendingLendingParams::<Test>::get(), None);
    });
}

#[test]
fn set_short_params_bounds_at_the_edges() {
    migrated_ext().execute_with(|| {
        let base = ShortParams::default();
        let c = set_short;
        check_edges(
            "stress_bps",
            &base,
            (10_000, 100_000),
            |p, v| p.stress_bps = v as u32,
            c,
        );
        check_edges(
            "open_limit_bps",
            &base,
            (1, 1_000),
            |p, v| p.open_limit_bps = v as u32,
            c,
        );
    });
}

#[test]
fn set_long_params_bounds_at_the_edges_and_the_stress_floor_cross_check() {
    migrated_ext().execute_with(|| {
        let c = set_long;
        // Floor at its minimum, so `stress_bps` meets its own fixed bounds.
        let low_floor = LongParams {
            stress_floor_bps: 10_000,
            ..Default::default()
        };
        check_edges(
            "stress_bps",
            &low_floor,
            (10_000, 100_000),
            |p, v| p.stress_bps = v as u32,
            c,
        );
        // Stress at its maximum, so `stress_floor_bps` meets its own fixed bounds.
        let high_stress = LongParams {
            stress_bps: 100_000,
            ..Default::default()
        };
        check_edges(
            "stress_floor_bps",
            &high_stress,
            (10_000, 100_000),
            |p, v| p.stress_floor_bps = v as u32,
            c,
        );
        let base = LongParams::default();
        check_edges(
            "lift_cap_bps",
            &base,
            (1, 5_000),
            |p, v| p.lift_cap_bps = v as u32,
            c,
        );
        check_edges(
            "open_limit_bps",
            &base,
            (1, 1_000),
            |p, v| p.open_limit_bps = v as u32,
            c,
        );

        let with = |stress_bps, stress_floor_bps| {
            set_long(LongParams {
                stress_bps,
                stress_floor_bps,
                ..Default::default()
            })
        };
        assert_ok!(with(15_000, 15_000));
        assert_noop!(with(14_999, 15_000), Error::<Test>::InvalidParams);
        assert_noop!(with(20_000, 20_001), Error::<Test>::InvalidParams);
    });
}

#[test]
fn set_dereg_params_bounds_at_the_edges() {
    migrated_ext().execute_with(|| {
        let base = DeregParams::default();
        let c = set_dereg;
        check_edges(
            "zone_bps",
            &base,
            (10_000, 30_000),
            |p, v| p.zone_bps = v as u32,
            c,
        );
        check_edges(
            "pool_tao_ema_blocks",
            &base,
            (300, 216_000),
            |p, v| p.pool_tao_ema_blocks = v as u32,
            c,
        );
        check_edges(
            "rank_window_days",
            &base,
            (7, 30),
            |p, v| p.rank_window_days = v as u32,
            c,
        );
        // The upper edge is the runtime's `MaxLongPositionsCeiling` (1,024).
        check_edges(
            "max_long_positions_per_subnet",
            &base,
            (0, 1_024),
            |p, v| p.max_long_positions_per_subnet = v as u32,
            c,
        );
    });
}

#[test]
fn a_rejected_setter_writes_nothing() {
    migrated_ext().execute_with(|| {
        assert_noop!(
            set_lending(LendingParams {
                apr_bps: 0,
                ..Default::default()
            }),
            Error::<Test>::InvalidParams
        );
        assert_noop!(
            set_short(ShortParams {
                open_limit_bps: 0,
                ..Default::default()
            }),
            Error::<Test>::InvalidParams
        );
        assert_noop!(
            set_long(LongParams {
                lift_cap_bps: 0,
                ..Default::default()
            }),
            Error::<Test>::InvalidParams
        );
        assert_noop!(
            set_dereg(DeregParams {
                rank_window_days: 6,
                ..Default::default()
            }),
            Error::<Test>::InvalidParams
        );
        assert_eq!(PendingLendingParams::<Test>::get(), None);
        assert_eq!(PendingShortParams::<Test>::get(), None);
        assert_eq!(PendingLongParams::<Test>::get(), None);
        assert_eq!(PendingDeregParams::<Test>::get(), None);
        next_block();
        assert!(lending_events().is_empty());
        assert_eq!(LendingParamsStore::<Test>::get(), LendingParams::default());
    });
}

#[test]
fn each_group_applies_in_the_next_block_with_exactly_one_event() {
    migrated_ext().execute_with(|| {
        // A lower base APR does not tighten: it is live in the next block.
        let lending = LendingParams {
            apr_bps: 1_500,
            ..Default::default()
        };
        let short = ShortParams {
            stress_bps: 40_000,
            ..Default::default()
        };
        let long = LongParams {
            lift_cap_bps: 500,
            ..Default::default()
        };
        let dereg = DeregParams {
            zone_bps: 15_000,
            ..Default::default()
        };
        assert_ok!(set_lending(lending.clone()));
        assert_ok!(set_short(short.clone()));
        assert_ok!(set_long(long.clone()));
        assert_ok!(set_dereg(dereg.clone()));

        // Nothing is live, and nothing is announced, in the block of the calls.
        assert_eq!(LendingParamsStore::<Test>::get(), LendingParams::default());
        assert_eq!(ShortParamsStore::<Test>::get(), ShortParams::default());
        assert_eq!(LongParamsStore::<Test>::get(), LongParams::default());
        assert_eq!(DeregParamsStore::<Test>::get(), DeregParams::default());
        assert!(lending_events().is_empty());

        next_block();
        assert_eq!(LendingParamsStore::<Test>::get(), lending);
        assert_eq!(ShortParamsStore::<Test>::get(), short);
        assert_eq!(LongParamsStore::<Test>::get(), long);
        assert_eq!(DeregParamsStore::<Test>::get(), dereg);
        assert_eq!(
            lending_events(),
            vec![
                Event::LendingParamsSet {
                    old: LendingParams::default(),
                    new: lending
                },
                Event::ShortParamsSet {
                    old: ShortParams::default(),
                    new: short
                },
                Event::LongParamsSet {
                    old: LongParams::default(),
                    new: long
                },
                Event::DeregParamsSet {
                    old: DeregParams::default(),
                    new: dereg
                },
            ]
        );
        assert_eq!(PendingLendingParams::<Test>::get(), None);
        assert_eq!(PendingShortParams::<Test>::get(), None);
        assert_eq!(PendingLongParams::<Test>::get(), None);
        assert_eq!(PendingDeregParams::<Test>::get(), None);
        assert_ok!(Lending::do_try_state());

        // Applied once: later blocks announce nothing.
        next_block();
        assert!(lending_events().is_empty());
    });
}

#[test]
fn two_sets_in_one_block_apply_the_last_with_one_event() {
    migrated_ext().execute_with(|| {
        let first = ShortParams {
            stress_bps: 40_000,
            ..Default::default()
        };
        let second = ShortParams {
            stress_bps: 50_000,
            ..Default::default()
        };
        assert_ok!(set_short(first));
        assert_ok!(set_short(second.clone()));
        next_block();
        assert_eq!(
            lending_events(),
            vec![Event::ShortParamsSet {
                old: ShortParams::default(),
                new: second
            }]
        );
    });
}

#[test]
fn enable_flags_are_set_immediately_with_an_event() {
    migrated_ext().execute_with(|| {
        assert_ok!(Lending::set_longs_enabled(root(), true));
        assert!(LongsEnabled::<Test>::get());
        assert_ok!(Lending::set_enabled(root(), true));
        assert!(Enabled::<Test>::get());
        assert_ok!(Lending::set_enabled(root(), false));
        assert!(!Enabled::<Test>::get());
        assert_eq!(
            lending_events(),
            vec![
                Event::LongsEnabledSet { enabled: true },
                Event::EnabledSet { enabled: true },
                Event::EnabledSet { enabled: false },
            ]
        );
    });
}

/// Gives SN1 a TWAP ring whose median (3) differs from every other statistic of the ring.
fn skew_sn1_twap() -> Price {
    let mut ring = TwapState::seeded(Price::from_bits(3), 72);
    for bits in [1u128, 1, 900, 900, 900] {
        ring.push(Price::from_bits(bits));
    }
    TwapRing::<Test>::insert(NetUid::from(1), ring);
    Price::from_bits(3)
}

/// Sets the delay of a tightening to zero, so that a tightening is live in the next block.
fn no_delay() {
    set_live_lending(|p| p.rate_delay_blocks = 0);
}

#[test]
fn a_twap_samples_change_reseeds_every_ring_with_its_median_not_spot() {
    migrated_ext().execute_with(|| {
        no_delay();
        let median = skew_sn1_twap();
        let sn2_median = px(5_000, 10_000);
        assert_ok!(set_lending(LendingParams {
            twap_samples: 12,
            ..Default::default()
        }));
        // A one-block spot spike at the switch must not become the window.
        set_pool(1, 145_000, 10_000);
        next_block();
        assert_eq!(
            TwapRing::<Test>::get(NetUid::from(1)),
            Some(TwapState::seeded(median, 12))
        );
        assert_eq!(
            TwapRing::<Test>::get(NetUid::from(2)),
            Some(TwapState::seeded(sn2_median, 12))
        );
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn a_twap_interval_change_reseeds_and_other_lending_changes_do_not() {
    migrated_ext().execute_with(|| {
        no_delay();
        let median = skew_sn1_twap();
        let skewed = TwapRing::<Test>::get(NetUid::from(1));

        // Each group carries no delay: a group's delay is live once it applies, so a default
        // (three days) would hold back the next tightening.
        assert_ok!(set_lending(LendingParams {
            apr_bps: 2_500,
            rate_delay_blocks: 0,
            ..Default::default()
        }));
        next_block();
        assert_eq!(TwapRing::<Test>::get(NetUid::from(1)), skewed);

        assert_ok!(set_lending(LendingParams {
            apr_bps: 2_500,
            twap_interval_blocks: 600,
            rate_delay_blocks: 0,
            ..Default::default()
        }));
        next_block();
        assert_eq!(
            TwapRing::<Test>::get(NetUid::from(1)),
            Some(TwapState::seeded(median, 72))
        );
        // The new cadence is in force: block 300 is skipped, block 600 samples.
        run_to(599);
        assert_eq!(
            TwapRing::<Test>::get(NetUid::from(1)),
            Some(TwapState::seeded(median, 72))
        );
        run_to(600);
        let ring = TwapRing::<Test>::get(NetUid::from(1)).expect("ring");
        assert_eq!(ring.cursor, 1);
        assert_eq!(ring.samples.first(), Some(&px(1_000, 10_000)));
    });
}

#[test]
fn a_rank_window_change_bumps_the_generation_and_reseeds_from_rank_price() {
    migrated_ext().execute_with(|| {
        // Published SMA and moving price differ, and the ring is mid-day.
        let published = Price::from_bits(777);
        let mut ring = RankState::seeded(published, 30);
        ring.sample(Price::from_bits(5));
        RankRing::<Test>::insert(NetUid::from(1), ring);
        RankPrice::<Test>::insert(NetUid::from(1), published);
        assert_ne!(published, px(800, 10_000));
        let generation = RankGeneration::<Test>::get();

        assert_ok!(set_dereg(DeregParams {
            rank_window_days: 14,
            ..Default::default()
        }));
        assert_eq!(RankGeneration::<Test>::get(), generation);
        next_block();

        assert_eq!(RankGeneration::<Test>::get(), generation + 1);
        for (netuid, price) in [(1, published), (2, px(4_000, 10_000))] {
            let netuid = NetUid::from(netuid);
            assert_eq!(
                RankRing::<Test>::get(netuid),
                Some(RankState::seeded(price, 14))
            );
            assert_eq!(RankPrice::<Test>::get(netuid), Some(price));
            assert_eq!(
                RankRingGeneration::<Test>::get(netuid),
                Some(generation + 1)
            );
        }
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn other_dereg_changes_leave_rings_and_the_pool_tao_ema_value_alone() {
    migrated_ext().execute_with(|| {
        let generation = RankGeneration::<Test>::get();
        let ring = RankRing::<Test>::get(NetUid::from(1));
        // Pool TAO equals the EMA, so a block's step leaves the EMA where it is.
        let ema = PoolTaoEma::<Test>::get(NetUid::from(1));
        assert_eq!(ema, Some(1_000));
        assert_ok!(set_dereg(DeregParams {
            zone_bps: 20_000,
            pool_tao_ema_blocks: 300,
            max_long_positions_per_subnet: 1_024,
            ..Default::default()
        }));
        next_block();
        assert_eq!(RankGeneration::<Test>::get(), generation);
        assert_eq!(RankRing::<Test>::get(NetUid::from(1)), ring);
        assert_eq!(PoolTaoEma::<Test>::get(NetUid::from(1)), ema);

        // Only the smoothing factor changed: the next step uses the 300-block window.
        set_pool(1, 4_000, 10_000);
        next_block();
        assert_eq!(PoolTaoEma::<Test>::get(NetUid::from(1)), Some(1_010));
    });
}
