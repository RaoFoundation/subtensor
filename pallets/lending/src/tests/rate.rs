//! The convex rate in the position paths (SPEC section 1 items 2 and 11, P2, P3, P5, I23): the
//! rate every touch accrues at, a top-up, split invariance, and the short coverage reference
//! without the TWAP median.
//!
//! Expected rates and amounts are computed with the plain-integer closed forms of
//! `tests::interest` (the model's `rate_from` and `_accrue`), not with the pallet's own math.

use frame_support::{assert_noop, assert_ok, dispatch::DispatchResult};
use subtensor_runtime_common::NetUid;

use super::interest::{Curve, model_accrue, model_rate};
use super::mock::*;
use crate::{
    Error, Event, LendingParamsStore, Positions, RankPrice, RankRing, TwapRing,
    math::{
        fixed::{Price, price},
        rate::RateCurve,
    },
    params::RAO,
    prices::{RankState, TwapState},
    types::{Accrual, Leg, Position, Side},
};

const ALICE: AccountId = 1;
const SN1: u16 = 1;
/// The launch curve (SPEC 2.1) under the least floor, which `live` sets so that these tests
/// see the curve below the base rate.
const LAUNCH: Curve = (2_000, 2, 10_000, 1);
const INTERVAL: u64 = 360;
/// The model's `BPS * BPY`.
const DEN: u128 = 10_000 * 7_200 * 365;

fn sn1() -> NetUid {
    NetUid::from(SN1)
}

fn tao(n: u64) -> u64 {
    n * RAO
}

fn alpha(n: u64) -> u64 {
    n * RAO
}

/// Subnet 1 with a deep pool (5,000 TAO / 50,000 alpha, spot 0.1), lending and longs on, and
/// Alice funded with 200 TAO, on the launch parameters.
fn launch() -> sp_io::TestExternalities {
    let mut ext = migrated_ext();
    ext.execute_with(|| {
        set_pool(SN1, tao(5_000), alpha(50_000));
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_ok!(Lending::set_longs_enabled(RuntimeOrigin::root(), true));
        set_balance(ALICE, tao(200));
    });
    ext
}

/// [`launch`] with the least rate floor and no delay of a rate increase (see [`bare_curve`]).
fn live() -> sp_io::TestExternalities {
    let mut ext = launch();
    ext.execute_with(bare_curve);
    ext
}

fn position(who: AccountId, side: Side) -> Position<u64> {
    Positions::<Test>::get((who, sn1(), side)).expect("position is open")
}

fn healthy() {
    assert_ok!(Lending::do_try_state());
}

fn origin() -> RuntimeOrigin {
    RuntimeOrigin::signed(ALICE)
}

/// A short of 200 alpha (about 20 TAO of notional) holding 60 TAO of collateral.
fn open_short() -> DispatchResult {
    Lending::open_short(origin(), sn1(), alpha(200), tao(60), 0)
}

/// A long with 20 TAO lifted, holding 20 TAO of collateral.
fn open_long() -> DispatchResult {
    Lending::open_long(origin(), sn1(), tao(20), tao(20), 0)
}

fn open(side: Side) -> DispatchResult {
    match side {
        Side::Short => open_short(),
        Side::Long => open_long(),
    }
}

/// The interest charges of the current block: side, amount and the rate charged.
fn charges() -> Vec<(Side, u64, u32)> {
    lending_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::InterestCharged {
                side,
                to_pool,
                rate_bps,
                ..
            } => Some((side, to_pool, rate_bps)),
            _ => None,
        })
        .collect()
}

fn twap() -> Price {
    Lending::price_refs(sn1()).expect("rings are seeded").twap
}

/// `floor(alpha * TWAP_median)` at the TWAP the pallet reads now.
fn at_twap(alpha: u64) -> u64 {
    u64::try_from((u128::from(alpha) * twap().to_bits()) >> 64).expect("fits")
}

/// The notional interest accrues on: `N` of a short, `D` of a long.
fn notional(pos: &Position<u64>) -> u64 {
    match pos.leg {
        Leg::Short { minted, .. } => at_twap(minted),
        Leg::Long { debt, .. } => debt,
    }
}

/// A position's debt over its holdings (the model's `short_u`, `long_u`): a short's
/// `N / (C + P)`, a long's `D / (floor(A * TWAP_median) + C)`.
fn debt_over_holdings(pos: &Position<u64>) -> (u128, u128) {
    match pos.leg {
        Leg::Short { minted, proceeds } => (
            u128::from(at_twap(minted)),
            u128::from(pos.collateral) + u128::from(proceeds),
        ),
        Leg::Long { locked_alpha, debt } => (
            u128::from(debt),
            u128::from(at_twap(locked_alpha)) + u128::from(pos.collateral),
        ),
    }
}

/// The rate of `pos` as stored, on `curve`.
fn rate_on(curve: Curve, pos: &Position<u64>) -> u32 {
    let (num, den) = debt_over_holdings(pos);
    model_rate(curve, num, den)
}

/// The rate of `pos` as stored, on the curve `live` starts with.
fn rate_of(pos: &Position<u64>) -> u32 {
    rate_on(LAUNCH, pos)
}

// The rate is read before the touch (SPEC section 1 item 2).

/// One touch of an open position: its name, its side, whether it closes the position, and
/// the call.
type Touch = (&'static str, Side, bool, fn() -> DispatchResult);

fn touches() -> Vec<Touch> {
    vec![
        ("increase_short", Side::Short, false, || {
            Lending::increase_short(origin(), sn1(), alpha(20), 0, 0)
        }),
        ("add_collateral", Side::Short, false, || {
            Lending::add_collateral(origin(), sn1(), tao(5))
        }),
        ("close_short (partial)", Side::Short, false, || {
            Lending::close_short(origin(), sn1(), alpha(100), u64::MAX)
        }),
        ("close_short (full)", Side::Short, true, || {
            Lending::close_short(origin(), sn1(), u64::MAX, u64::MAX)
        }),
        ("increase_long", Side::Long, false, || {
            Lending::increase_long(origin(), sn1(), tao(5), 0, 0)
        }),
        ("add_long_collateral", Side::Long, false, || {
            Lending::add_long_collateral(origin(), sn1(), tao(5))
        }),
        ("close_long (partial)", Side::Long, false, || {
            Lending::close_long(origin(), sn1(), alpha(50), 0)
        }),
        ("close_long (full)", Side::Long, true, || {
            Lending::close_long(origin(), sn1(), u64::MAX, 0)
        }),
    ]
}

#[test]
fn every_touch_accrues_at_the_rate_read_before_it_mutates() {
    for (name, side, closes, touch) in touches() {
        live().execute_with(|| {
            assert_ok!(open(side));
            run_to(100);
            let before = position(ALICE, side);
            let rate = rate_of(&before);
            let (owed, remainder) = model_accrue(notional(&before), rate, 99, 0);
            assert!(rate > 0 && owed > 0, "{name}");

            assert_eq!(touch(), Ok(()), "{name}");
            // The 99 blocks since the open accrue at the rate of the position as it stood.
            assert_eq!(
                charges().first(),
                Some(&(side, owed, rate)),
                "{name}: the touch's accrual"
            );
            if closes {
                assert!(!Positions::<Test>::contains_key((ALICE, sn1(), side)));
            } else {
                // The touch moved the position's rate, and the accrual did not use it.
                let after = position(ALICE, side);
                assert_ne!(rate_of(&after), rate, "{name} moves the rate");
                assert_eq!(after.accrual.remainder, remainder, "{name}");
                assert_eq!(after.accrual.last_accrued, 100, "{name}");

                // The next scheduled charge accrues the 261 blocks since at the new rate.
                run_to(1 + INTERVAL);
                let (next, _) = model_accrue(notional(&after), rate_of(&after), 261, remainder);
                assert_eq!(charges(), vec![(side, next, rate_of(&after))], "{name}");
            }
            healthy();
        });
    }
}

#[test]
fn a_top_up_lowers_the_rate_only_from_its_block() {
    let adds: [(Side, fn(RuntimeOrigin, NetUid, u64) -> DispatchResult); 2] = [
        (Side::Short, Lending::add_collateral),
        (Side::Long, Lending::add_long_collateral),
    ];
    for (side, add) in adds {
        live().execute_with(|| {
            assert_ok!(open(side));
            run_to(200);
            let before = position(ALICE, side);
            let high = rate_of(&before);
            assert_ok!(add(origin(), sn1(), tao(40)));
            let after = position(ALICE, side);
            let low = rate_of(&after);
            assert!(low < high / 2, "{side:?}: {low} against {high}");

            // The 199 blocks before the top-up are charged at the rate before it.
            let (first, remainder) = model_accrue(notional(&before), high, 199, 0);
            assert_eq!(charges(), vec![(side, first, high)]);
            // The 161 blocks after it are charged at the lower rate.
            run_to(1 + INTERVAL);
            let (second, _) = model_accrue(notional(&after), low, 161, remainder);
            assert_eq!(charges(), vec![(side, second, low)]);
            // One charge of the whole interval at either rate would have been a different sum.
            let whole = |rate| model_accrue(notional(&before), rate, 360, 0).0;
            assert!(whole(low) < first + second && first + second < whole(high));
            healthy();
        });
    }
}

// Split invariance (SPEC I23).

/// The pool of the split tests: 2,000,000 TAO / 16,000,000 alpha, with every price reference
/// at exactly 0.125, so that a pro-rata piece's notional is exact.
fn split_ext() -> sp_io::TestExternalities {
    let mut ext = migrated_ext();
    ext.execute_with(|| {
        set_pool(SN1, tao(2_000_000), alpha(16_000_000));
        let eighth = price(1, 8);
        let samples = LendingParamsStore::<Test>::get().twap_samples;
        TwapRing::<Test>::insert(sn1(), TwapState::seeded(eighth, samples));
        RankRing::<Test>::insert(sn1(), RankState::seeded(eighth, 30));
        RankPrice::<Test>::insert(sn1(), eighth);
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        bare_curve();
    });
    ext
}

/// Opens a short of 8,000 alpha (1,000 TAO of notional) holding 1,600 TAO of collateral and
/// 900 TAO of proceeds as `pieces` equal positions of different holders, runs three scheduled
/// charges, and returns each charge's rate and the total charged over the pieces.
fn charges_of_a_split_short(pieces: u64) -> Vec<(u32, u64)> {
    let mut rounds = Vec::new();
    split_ext().execute_with(|| {
        for who in 100..100 + AccountId::from(pieces) {
            set_balance(who, tao(4_000) / pieces + tao(1));
            assert_ok!(Lending::open_short(
                RuntimeOrigin::signed(who),
                sn1(),
                alpha(8_000) / pieces,
                tao(4_000) / pieces,
                0
            ));
            // The piece's exact pro-rata share of the collateral and the proceeds: the sale's
            // own proceeds depend on the order of the opens.
            let key = (who, sn1(), Side::Short);
            let pos = Positions::<Test>::get(key).expect("open");
            let (collateral, proceeds) = (tao(1_600) / pieces, tao(900) / pieces);
            let custody = Lending::custody_of(Side::Short, &who, sn1(), pos.opened_at);
            set_balance(custody, collateral + proceeds + pos.deposit);
            let leg = Leg::Short {
                minted: alpha(8_000) / pieces,
                proceeds,
            };
            Positions::<Test>::insert(
                key,
                Position {
                    collateral,
                    leg,
                    ..pos
                },
            );
        }
        healthy();
        for round in 1..=3 {
            run_to(1 + round * INTERVAL);
            assert_eq!(twap(), price(1, 8));
            let charged = charges();
            assert_eq!(charged.len() as u64, pieces);
            // Every piece pays the same rate and, to the rao, the same amount.
            let (_, amount, rate) = charged[0];
            assert!(charged.iter().all(|charge| charge.2 == rate));
            assert!(charged.iter().all(|charge| charge.1.abs_diff(amount) <= 1));
            rounds.push((rate, charged.iter().map(|charge| charge.1).sum()));
        }
        healthy();
    });
    rounds
}

#[test]
fn a_short_and_its_pro_rata_pieces_pay_the_same_rate_and_interest() {
    let whole = charges_of_a_split_short(1);
    // u = 1,000 / 2,500 = 0.4: 2,000 * 0.4^2 = 320 bps at the first charge.
    assert_eq!(whole[0].0, model_rate(LAUNCH, 1_000, 2_500));
    assert_eq!(whole[0].0, 320);
    assert_eq!(whole[0].1, model_accrue(tao(1_000), 320, INTERVAL, 0).0);
    for pieces in [10, 100] {
        let split = charges_of_a_split_short(pieces);
        for (round, (one, many)) in whole.iter().zip(&split).enumerate() {
            assert_eq!(one.0, many.0, "rate of {pieces} pieces, charge {round}");
            // Each piece floors its own charge: at most one rao apart per piece.
            assert!(
                one.1.abs_diff(many.1) <= pieces,
                "interest of {pieces} pieces, charge {round}: {} against {}",
                many.1,
                one.1
            );
        }
    }
}

#[test]
fn a_long_and_its_pro_rata_pieces_pay_the_same_rate_and_interest() {
    let curve = RateCurve {
        apr_bps: 2_000,
        k: 2,
        cap_bps: 10_000,
        // The least floor, so the curve below the base rate is what the test sees.
        floor_bps: 1,
    };
    let twap = price(1, 8);
    // A long of 1,000 TAO of debt holding 8,000 alpha (1,000 TAO at the TWAP) and 600 TAO.
    let piece = |pieces: u64| Position {
        collateral: tao(600) / pieces,
        deposit: 0,
        opened_at: 1u64,
        charge_at: 361,
        accrual: Accrual {
            last_accrued: 1,
            remainder: 0,
            due: 0,
        },
        leg: Leg::Long {
            locked_alpha: alpha(8_000) / pieces,
            debt: tao(1_000) / pieces,
        },
    };
    // u = 1,000 / 1,600: 2,000 * 0.625 = 1,250, then 781.25 floored.
    let expected = model_rate(LAUNCH, 1_000, 1_600);
    assert_eq!(expected, 781);
    let mut whole = piece(1);
    let mut split = [piece(10), piece(100)];
    for round in 1..=3 {
        let now = 1 + round * INTERVAL;
        let (next, one) = whole.settle(twap, &curve, now);
        whole = next;
        for part in &mut split {
            let pieces = tao(1_000) / part.leg.notional(twap);
            let (next, many) = part.settle(twap, &curve, now);
            *part = next;
            assert_eq!(
                many.rate_bps, one.rate_bps,
                "{pieces} pieces, charge {round}"
            );
            assert!(
                one.amount.abs_diff(many.amount * pieces) <= pieces,
                "{pieces} pieces, charge {round}"
            );
        }
        if round == 1 {
            assert_eq!(one.rate_bps, expected);
            assert_eq!(
                one.amount,
                model_accrue(tao(1_000), expected, INTERVAL, 0).0
            );
        }
    }
}

// The short coverage reference (SPEC P2, section 1 item 1).

#[test]
fn the_twap_median_does_not_move_the_short_open_check() {
    let q = alpha(400);
    let quote = |twap: Option<Price>, sma: Option<Price>| {
        let mut ext = live();
        ext.execute_with(|| {
            let samples = LendingParamsStore::<Test>::get().twap_samples;
            if let Some(twap) = twap {
                TwapRing::<Test>::insert(sn1(), TwapState::seeded(twap, samples));
            }
            if let Some(sma) = sma {
                RankRing::<Test>::insert(sn1(), RankState::seeded(sma, 30));
                RankPrice::<Test>::insert(sn1(), sma);
            }
            let quote = Lending::quote_open_short(sn1(), q).expect("quote");
            // The quoted collateral is the least the open accepts.
            assert_noop!(
                Lending::open_short(origin(), sn1(), q, quote.min_collateral - 1, 0),
                Error::<Test>::CoverageNotMet
            );
            assert_ok!(Lending::open_short(
                origin(),
                sn1(),
                q,
                quote.min_collateral,
                0
            ));
            healthy();
            (quote.coverage_need, quote.min_collateral)
        })
    };
    let base = quote(None, None);
    // A TWAP median at twice and at half the spot of 0.1: the same need and collateral.
    assert_eq!(quote(Some(price(2, 10)), None), base);
    assert_eq!(quote(Some(price(1, 20)), None), base);
    // The SMA below spot leaves the reference at spot; above spot it is the reference.
    assert_eq!(quote(None, Some(price(1, 20))), base);
    let raised = quote(None, Some(price(15, 100)));
    assert!(raised.0 > base.0 && raised.1 > base.1);
    // With the SMA as the reference, the TWAP median still moves nothing.
    assert_eq!(quote(Some(price(1, 2)), Some(price(15, 100))), raised);
}

// Try-state (SPEC I23).

#[test]
fn i23_holds_on_every_curve_and_fails_for_a_remainder_of_a_rao_or_more() {
    live().execute_with(|| {
        assert_ok!(open_short());
        healthy();
        // No holdings left: the rate is the cap, on the widest and the narrowest curve.
        let pos = position(ALICE, Side::Short);
        let custody = Lending::custody_of(Side::Short, &ALICE, sn1(), pos.opened_at);
        set_balance(custody, pos.deposit);
        let leg = Leg::Short {
            minted: alpha(200),
            proceeds: 0,
        };
        let drained = Position {
            collateral: 0,
            leg,
            ..pos
        };
        Positions::<Test>::insert((ALICE, sn1(), Side::Short), drained);
        for (apr_bps, rate_k, rate_cap_bps) in
            [(2_000, 2, 10_000), (10_000, 8, 1_000_000), (1, 1, 1)]
        {
            LendingParamsStore::<Test>::mutate(|p| {
                (p.apr_bps, p.rate_k, p.rate_cap_bps) = (apr_bps, rate_k, rate_cap_bps);
            });
            let curve = LendingParamsStore::<Test>::get().rate_curve();
            assert_eq!(drained.rate_bps(twap(), &curve), rate_cap_bps);
            healthy();
        }

        let mut broken = drained;
        broken.accrual.remainder = DEN;
        Positions::<Test>::insert((ALICE, sn1(), Side::Short), broken);
        assert_eq!(
            Lending::do_try_state(),
            Err(sp_runtime::TryRuntimeError::Other(
                "I23: accrual remainder is a rao or more"
            ))
        );
        broken.accrual.remainder = DEN - 1;
        Positions::<Test>::insert((ALICE, sn1(), Side::Short), broken);
        healthy();
    });
}
