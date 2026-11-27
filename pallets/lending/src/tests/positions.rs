//! Position operations on both sides (SPEC 1.1-1.4, P2-P23, L2-L12) against the in-memory pools,
//! the try-state faults on the books, the interest settlement, and the golden coverage
//! differential through the pallet's own need dispatch.

use frame_support::{assert_noop, assert_ok, dispatch::DispatchResult};
use sp_runtime::TryRuntimeError;
use subtensor_runtime_common::NetUid;

use super::mock::*;
use crate::{
    DeregParamsStore, Error, Event, LendingPoolInterface, LongLiftedTao, LongLockedAlpha,
    LongPositionCount, Positions, SyntheticOutstanding,
    math::{
        PriceRefs,
        curve::{ConstantProduct, PoolState},
        fixed::{Price, price},
        rate::RateCurve,
    },
    params::{DeregParams, LendingParams, LongParams, RAO, ShortParams},
    positions::{ALPHA_SUPPLY_CAP, Params, coverage_need},
    types::{Leg, Position, Side},
};

const ALICE: AccountId = 1;
const BOB: AccountId = 2;
const SN1: u16 = 1;

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
/// Alice and Bob funded with 100 TAO each.
fn live() -> sp_io::TestExternalities {
    let mut ext = migrated_ext();
    ext.execute_with(|| {
        set_pool(SN1, tao(5_000), alpha(50_000));
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_ok!(Lending::set_longs_enabled(RuntimeOrigin::root(), true));
        set_balance(ALICE, tao(100));
        set_balance(BOB, tao(100));
    });
    ext
}

fn position(who: AccountId, side: Side) -> Option<Position<u64>> {
    Positions::<Test>::get((who, sn1(), side))
}

/// The custody account of an open position.
fn custody(who: AccountId, side: Side) -> AccountId {
    let pos = position(who, side).expect("position is open");
    Lending::custody_of(side, &who, sn1(), pos.opened_at)
}

fn open_short(who: AccountId, q: u64, collateral: u64) -> DispatchResult {
    Lending::open_short(RuntimeOrigin::signed(who), sn1(), q, collateral, 0)
}

fn open_long(who: AccountId, lift: u64, collateral: u64) -> DispatchResult {
    Lending::open_long(RuntimeOrigin::signed(who), sn1(), lift, collateral, 0)
}

fn healthy() {
    assert_ok!(Lending::do_try_state());
}

fn event_amount(f: impl Fn(&Event<Test>) -> Option<u64>) -> Option<u64> {
    lending_events().iter().find_map(f)
}

/// Runs `fault` on an open short and expects try-state to fail with `message`.
fn assert_fault(message: &'static str, fault: impl FnOnce()) {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE, alpha(200), tao(45)));
        healthy();
        fault();
        assert_eq!(
            Lending::do_try_state(),
            Err(TryRuntimeError::Other(message))
        );
    });
}

// Shorts.

#[test]
fn open_short_locks_collateral_deposit_and_proceeds() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE, alpha(200), tao(45)));
        let pos = position(ALICE, Side::Short).expect("opened");
        assert_eq!(pos.collateral, tao(45));
        assert_eq!(pos.deposit, RAO / 10);
        let Leg::Short { minted, proceeds } = pos.leg else {
            panic!("short leg expected");
        };
        assert_eq!(minted, alpha(200));
        assert!(proceeds > 0);
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), alpha(200));
        assert_eq!(FakePool::tao_balance(&ALICE), tao(100) - tao(45) - RAO / 10);
        assert_eq!(
            FakePool::tao_balance(&custody(ALICE, Side::Short)),
            tao(45) + RAO / 10 + proceeds
        );
        assert!(lending_events().iter().any(|e| matches!(
            e,
            Event::PositionOpened { side: Side::Short, size, .. } if *size == alpha(200)
        )));
        healthy();
    });
}

#[test]
fn shorts_are_refused_while_lending_is_off() {
    live().execute_with(|| {
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), false));
        assert_noop!(
            open_short(ALICE, alpha(200), tao(45)),
            Error::<Test>::Disabled
        );
    });
}

#[test]
fn one_side_per_holder_and_subnet() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE, alpha(200), tao(45)));
        assert_noop!(
            open_short(ALICE, alpha(200), tao(45)),
            Error::<Test>::PositionExists
        );
        assert_noop!(
            open_long(ALICE, tao(20), tao(12)),
            Error::<Test>::OppositeSideOpen
        );
    });
}

#[test]
fn increase_needs_an_open_position_and_grows_the_book() {
    live().execute_with(|| {
        assert_noop!(
            Lending::increase_short(RuntimeOrigin::signed(ALICE), sn1(), alpha(200), tao(45), 0),
            Error::<Test>::NoPosition
        );
        assert_ok!(open_short(ALICE, alpha(200), tao(45)));
        assert_ok!(Lending::increase_short(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            alpha(100),
            tao(40),
            0
        ));
        assert_eq!(
            position(ALICE, Side::Short).expect("open").leg.alpha(),
            alpha(300)
        );
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), alpha(300));
        healthy();
    });
}

#[test]
fn partial_short_close_keeps_the_position_and_retires_its_alpha() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE, alpha(200), tao(45)));
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            alpha(100),
            u64::MAX
        ));
        let pos = position(ALICE, Side::Short).expect("still open");
        assert_eq!(pos.leg.alpha(), alpha(100));
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), alpha(100));
        assert!(lending_events().iter().any(|e| matches!(
            e,
            Event::PositionClosed { alpha: closed, full: false, .. } if *closed == alpha(100)
        )));
        healthy();
    });
}

#[test]
fn full_short_close_empties_custody_and_removes_the_position() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE, alpha(200), tao(45)));
        let custody = custody(ALICE, Side::Short);
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            alpha(200),
            u64::MAX
        ));
        assert!(position(ALICE, Side::Short).is_none());
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), 0);
        assert_eq!(FakePool::tao_balance(&custody), 0);
        assert_eq!(FakePool::held_alpha(&custody, sn1()), 0);
        healthy();
    });
}

#[test]
fn every_position_has_its_own_custody_account() {
    let base = Lending::custody_of(Side::Short, &ALICE, sn1(), 1);
    for other in [
        Lending::custody_of(Side::Long, &ALICE, sn1(), 1),
        Lending::custody_of(Side::Short, &BOB, sn1(), 1),
        Lending::custody_of(Side::Short, &ALICE, NetUid::from(2), 1),
        Lending::custody_of(Side::Short, &ALICE, sn1(), 2),
    ] {
        assert_ne!(other, base);
    }
    // Two holders open on one subnet in one block: each custody holds its own position (I3).
    live().execute_with(|| {
        assert_ok!(open_short(ALICE, alpha(200), tao(45)));
        assert_ok!(open_long(BOB, tao(20), tao(12)));
        assert_ne!(custody(ALICE, Side::Short), custody(BOB, Side::Long));
        healthy();
    });
}

#[test]
fn short_close_refuses_a_buy_back_above_the_bound() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE, alpha(200), tao(45)));
        assert_noop!(
            Lending::close_short(RuntimeOrigin::signed(ALICE), sn1(), alpha(200), 0),
            Error::<Test>::SlippageExceeded
        );
    });
}

/// Collateral leaves a position only on close (approved change beyond SPEC-v2.1): the pallet
/// has no withdraw call, and neither a top-up, an increase nor a scheduled charge pays the
/// owner anything or lowers the collateral by more than the interest that goes to the pool.
#[test]
fn only_a_close_returns_collateral_to_the_owner() {
    use frame_support::traits::GetCallName;
    let calls = crate::Call::<Test>::get_call_names();
    assert!(calls.iter().all(|name| !name.contains("withdraw")));
    assert_eq!(
        calls
            .iter()
            .filter(|name| name.contains("collateral"))
            .collect::<Vec<_>>(),
        [&"add_collateral", &"add_long_collateral"]
    );

    type Op = fn() -> DispatchResult;
    let origin = || RuntimeOrigin::signed(ALICE);
    let short: [(u64, Op); 3] = [
        (tao(5), || {
            Lending::add_collateral(RuntimeOrigin::signed(ALICE), sn1(), tao(5))
        }),
        (tao(3), || {
            Lending::increase_short(RuntimeOrigin::signed(ALICE), sn1(), alpha(10), tao(3), 0)
        }),
        (0, || {
            Lending::increase_short(RuntimeOrigin::signed(ALICE), sn1(), alpha(10), 0, 0)
        }),
    ];
    let long: [(u64, Op); 3] = [
        (tao(5), || {
            Lending::add_long_collateral(RuntimeOrigin::signed(ALICE), sn1(), tao(5))
        }),
        (tao(3), || {
            Lending::increase_long(RuntimeOrigin::signed(ALICE), sn1(), tao(2), tao(3), 0)
        }),
        (0, || {
            Lending::increase_long(RuntimeOrigin::signed(ALICE), sn1(), tao(1), 0, 0)
        }),
    ];
    for (side, ops) in [(Side::Short, short), (Side::Long, long)] {
        live().execute_with(|| {
            match side {
                Side::Short => {
                    assert_ok!(open_short(ALICE, alpha(200), tao(60)));
                }
                Side::Long => {
                    assert_ok!(open_long(ALICE, tao(20), tao(30)));
                }
            }
            for (added, op) in ops {
                // Interest accrues before each call, so the call also settles a charge.
                run_to(System::block_number() + 500);
                let free = FakePool::tao_balance(&ALICE);
                let held = position(ALICE, side).expect("open").collateral;
                assert_ok!(op());
                let after = position(ALICE, side).expect("open").collateral;
                // The owner pays exactly what the call adds and receives nothing.
                assert_eq!(FakePool::tao_balance(&ALICE), free - added, "{side:?}");
                // The collateral moves by the addition, less the interest settled.
                assert!(
                    after <= held + added && after > held + added - RAO,
                    "{side:?}"
                );
            }
            // The scheduled charges pay the pool, not the owner.
            let free = FakePool::tao_balance(&ALICE);
            let held = position(ALICE, side).expect("open").collateral;
            run_to(System::block_number() + 7_200);
            assert_eq!(FakePool::tao_balance(&ALICE), free);
            let left = position(ALICE, side).expect("open").collateral;
            assert!(left < held);
            healthy();

            // The full close is what returns the collateral.
            match side {
                Side::Short => {
                    assert_ok!(Lending::close_short(origin(), sn1(), alpha(220), u64::MAX));
                }
                Side::Long => {
                    let Leg::Long { locked_alpha, .. } = position(ALICE, side).expect("open").leg
                    else {
                        panic!("long leg expected");
                    };
                    assert_ok!(Lending::close_long(origin(), sn1(), locked_alpha, 0));
                }
            }
            assert!(position(ALICE, side).is_none());
            assert!(FakePool::tao_balance(&ALICE) > free);
            healthy();
        });
    }
}

#[test]
fn interest_is_charged_from_collateral_to_the_pool_on_touch() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE, alpha(200), tao(45)));
        let pool_before = FakePool::pool_tao(sn1());
        // Inside the first charge interval, so the touch is the only charge.
        run_to(300);
        assert_ok!(Lending::add_collateral(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            tao(1)
        ));
        let charged = event_amount(|e| match e {
            Event::InterestCharged { to_pool, .. } => Some(*to_pool),
            _ => None,
        })
        .expect("interest was charged");
        assert!(charged > 0);
        assert_eq!(FakePool::pool_tao(sn1()), pool_before + charged);
        assert_eq!(
            position(ALICE, Side::Short).expect("open").collateral,
            tao(46) - charged
        );
        healthy();
    });
}

#[test]
fn short_open_refuses_without_collateral_coverage() {
    live().execute_with(|| {
        assert_noop!(
            open_short(ALICE, alpha(200), 0),
            Error::<Test>::CoverageNotMet
        );
    });
}

#[test]
fn short_open_refuses_below_min_notional() {
    live().execute_with(|| {
        assert_noop!(
            open_short(ALICE, alpha(50), tao(20)),
            Error::<Test>::BelowMinNotional
        );
    });
}

#[test]
fn short_open_refuses_an_impact_past_the_block_limit() {
    live().execute_with(|| {
        // 1% of 50,000 alpha is 500 alpha per block.
        assert_noop!(
            open_short(ALICE, alpha(600), tao(20)),
            Error::<Test>::OpenImpactTooHigh
        );
    });
}

#[test]
fn short_open_refuses_past_the_supply_cap() {
    live().execute_with(|| {
        set_issuance(SN1, ALPHA_SUPPLY_CAP - alpha(100));
        assert_noop!(
            open_short(ALICE, alpha(200), tao(45)),
            Error::<Test>::AlphaSupplyCap
        );
    });
}

#[test]
fn open_refuses_a_shallow_pool() {
    live().execute_with(|| {
        set_pool(SN1, tao(500), alpha(5_000));
        assert_noop!(
            open_short(ALICE, alpha(200), tao(45)),
            Error::<Test>::PoolTooShallow
        );
    });
}

#[test]
fn open_refuses_a_price_worse_than_the_bound() {
    live().execute_with(|| {
        assert_noop!(
            Lending::open_short(
                RuntimeOrigin::signed(ALICE),
                sn1(),
                alpha(200),
                tao(45),
                u64::MAX
            ),
            Error::<Test>::SlippageExceeded
        );
    });
}

#[test]
fn open_refuses_without_the_funds() {
    live().execute_with(|| {
        set_balance(BOB, 0);
        assert_noop!(
            open_short(BOB, alpha(200), tao(45)),
            Error::<Test>::InsufficientBalance
        );
    });
}

#[test]
fn increase_needs_a_position_to_exist() {
    live().execute_with(|| {
        assert_noop!(
            Lending::increase_long(RuntimeOrigin::signed(ALICE), sn1(), tao(20), tao(10), 0),
            Error::<Test>::NoPosition
        );
    });
}

// Longs.

#[test]
fn open_long_lifts_tao_and_locks_the_bought_alpha() {
    live().execute_with(|| {
        let pool_before = FakePool::pool_tao(sn1());
        assert_ok!(open_long(ALICE, tao(20), tao(12)));
        let pos = position(ALICE, Side::Long).expect("opened");
        let Leg::Long { locked_alpha, debt } = pos.leg else {
            panic!("long leg expected");
        };
        assert_eq!(debt, tao(20));
        assert!(locked_alpha > 0);
        // The lift leaves the pool less the swap fee on it.
        let pool_after = FakePool::pool_tao(sn1());
        assert!(pool_after < pool_before && pool_before - pool_after < tao(1));
        assert_eq!(LongLiftedTao::<Test>::get(sn1()), tao(20));
        assert_eq!(LongLockedAlpha::<Test>::get(sn1()), locked_alpha);
        assert_eq!(LongPositionCount::<Test>::get(sn1()), 1);
        assert_eq!(
            FakePool::held_alpha(&custody(ALICE, Side::Long), sn1()),
            locked_alpha
        );
        healthy();
    });
}

#[test]
fn longs_are_refused_while_longs_are_off() {
    live().execute_with(|| {
        assert_ok!(Lending::set_longs_enabled(RuntimeOrigin::root(), false));
        assert_noop!(
            open_long(ALICE, tao(20), tao(12)),
            Error::<Test>::LongsDisabled
        );
    });
}

#[test]
fn long_open_refuses_without_collateral_coverage() {
    live().execute_with(|| {
        assert_noop!(
            open_long(ALICE, tao(20), 0),
            Error::<Test>::LongCoverageNotMet
        );
    });
}

#[test]
fn long_open_refuses_past_the_block_lift_limit() {
    live().execute_with(|| {
        // 1% of the 5,000 TAO pool is 50 TAO of net lift per block.
        assert_noop!(
            open_long(ALICE, tao(60), tao(12)),
            Error::<Test>::LongOpenLimit
        );
    });
}

#[test]
fn long_open_refuses_past_the_lift_cap() {
    live().execute_with(|| {
        // 10% of the pool is 500 TAO of outstanding lift.
        assert_noop!(
            open_long(ALICE, tao(600), tao(12)),
            Error::<Test>::LiftCapExceeded
        );
    });
}

#[test]
fn long_open_refuses_past_the_longs_cap() {
    live().execute_with(|| {
        DeregParamsStore::<Test>::put(DeregParams {
            max_long_positions_per_subnet: 0,
            ..Default::default()
        });
        assert_noop!(
            open_long(ALICE, tao(20), tao(12)),
            Error::<Test>::TooManyLongPositions
        );
    });
}

#[test]
fn full_long_close_repays_the_debt_and_removes_the_position() {
    live().execute_with(|| {
        assert_ok!(open_long(ALICE, tao(20), tao(12)));
        let custody = custody(ALICE, Side::Long);
        let locked = position(ALICE, Side::Long).expect("open").leg.alpha();
        let before = FakePool::tao_balance(&ALICE);
        assert_ok!(Lending::close_long(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            locked,
            0
        ));
        assert!(position(ALICE, Side::Long).is_none());
        assert_eq!(LongLiftedTao::<Test>::get(sn1()), 0);
        assert_eq!(LongLockedAlpha::<Test>::get(sn1()), 0);
        assert_eq!(LongPositionCount::<Test>::get(sn1()), 0);
        assert_eq!(FakePool::held_alpha(&custody, sn1()), 0);
        assert_eq!(FakePool::tao_balance(&custody), 0);
        assert!(FakePool::tao_balance(&ALICE) > before);
        healthy();
    });
}

#[test]
fn partial_long_close_keeps_the_position() {
    live().execute_with(|| {
        assert_ok!(open_long(ALICE, tao(20), tao(12)));
        let locked = position(ALICE, Side::Long).expect("open").leg.alpha();
        assert_ok!(Lending::close_long(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            locked / 2,
            0
        ));
        let pos = position(ALICE, Side::Long).expect("still open");
        assert_eq!(pos.leg.alpha(), locked - locked / 2);
        assert_eq!(LongLockedAlpha::<Test>::get(sn1()), locked - locked / 2);
        assert_eq!(LongPositionCount::<Test>::get(sn1()), 1);
        healthy();
    });
}

#[test]
fn long_collateral_is_added() {
    live().execute_with(|| {
        assert_ok!(open_long(ALICE, tao(20), tao(12)));
        assert_ok!(Lending::add_long_collateral(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            tao(5)
        ));
        assert_eq!(
            position(ALICE, Side::Long).expect("open").collateral,
            tao(17)
        );
        healthy();
    });
}

// Try-state faults on the books (SPEC I1, I3, I12, I21).

#[test]
fn i1_fails_when_the_short_book_is_not_the_sum_of_the_shorts() {
    assert_fault(
        "I1: SyntheticOutstanding is not the sum of the shorts' minted alpha",
        || SyntheticOutstanding::<Test>::insert(sn1(), alpha(201)),
    );
}

#[test]
fn i3_fails_when_custody_holds_other_tao() {
    assert_fault(
        "I3: custody TAO differs from collateral + proceeds + deposit",
        || {
            let custody = custody(ALICE, Side::Short);
            set_balance(custody, FakePool::tao_balance(&custody) + 1);
        },
    );
}

#[test]
fn i12_fails_when_the_long_debt_book_drifts() {
    live().execute_with(|| {
        assert_ok!(open_long(ALICE, tao(20), tao(12)));
        healthy();
        LongLiftedTao::<Test>::insert(sn1(), tao(21));
        assert_eq!(
            Lending::do_try_state(),
            Err(TryRuntimeError::Other(
                "I12: LongLiftedTao is not the sum of the longs' debt"
            ))
        );
    });
}

#[test]
fn i21_fails_when_issuance_passes_the_cap() {
    assert_fault("I21: alpha issuance exceeds the 21,000,000 cap", || {
        set_issuance(SN1, ALPHA_SUPPLY_CAP + 1)
    });
}

// Interest settlement (SPEC P15, charge on touch).

const YEAR_BLOCKS: u64 = 7_200 * 365;

fn position_at(collateral: u64, due: u64, leg: Leg, opened: u64) -> Position<u64> {
    Position {
        collateral,
        deposit: 0,
        opened_at: opened,
        charge_at: opened + 360,
        accrual: crate::types::Accrual {
            last_accrued: opened,
            remainder: 0,
            due,
        },
        leg,
    }
}

const LAUNCH_CURVE: RateCurve = RateCurve {
    apr_bps: 2_000,
    k: 2,
    cap_bps: 10_000,
    // The least floor, so the curve below the base rate is what the test sees.
    floor_bps: 1,
};

#[test]
fn settle_charges_interest_up_to_the_collateral() {
    // A short of 10 alpha at TWAP 1.0 has a notional of 10 TAO. Holding 1 TAO against it is
    // u = 10: the capped 100% a year owes 10 TAO, and the collateral pays what it has.
    let leg = Leg::Short {
        minted: alpha(10),
        proceeds: 0,
    };
    let twap = price(1, 1);
    let (drained, charge) =
        position_at(tao(1), 0, leg, 1).settle(twap, &LAUNCH_CURVE, 1 + YEAR_BLOCKS);
    assert_eq!((charge.amount, charge.rate_bps), (tao(1), 10_000));
    assert_eq!(drained.collateral, 0);
    assert_eq!(drained.accrual.due, tao(9));

    // Holding 10 TAO is u = 1: the base 20% a year owes 2 TAO.
    let (covered, charge) =
        position_at(tao(10), 0, leg, 1).settle(twap, &LAUNCH_CURVE, 1 + YEAR_BLOCKS);
    assert_eq!((charge.amount, charge.rate_bps), (tao(2), 2_000));
    assert_eq!(covered.collateral, tao(8));
    assert_eq!(covered.accrual.due, 0);
    assert_eq!(covered.accrual.last_accrued, 1 + YEAR_BLOCKS);
}

#[test]
fn settle_reads_the_rate_before_it_takes_the_charge() {
    // u = 10 / 20 = 0.5: 5% a year on 10 TAO owes 0.5 TAO. The rate is the one of the 20 TAO
    // held before the charge, not of the 19.5 TAO left after it.
    let leg = Leg::Short {
        minted: alpha(10),
        proceeds: tao(5),
    };
    let pos = position_at(tao(15), 0, leg, 1);
    let (settled, charge) = pos.settle(price(1, 1), &LAUNCH_CURVE, 1 + YEAR_BLOCKS);
    assert_eq!((charge.amount, charge.rate_bps), (tao(1) / 2, 500));
    assert_eq!(pos.rate_bps(price(1, 1), &LAUNCH_CURVE), 500);
    assert_eq!(settled.rate_bps(price(1, 1), &LAUNCH_CURVE), 525);
}

#[test]
fn settle_at_the_same_block_is_a_no_op() {
    let leg = Leg::Long {
        locked_alpha: alpha(10),
        debt: tao(10),
    };
    let pos = position_at(tao(5), tao(1), leg, 7);
    let (settled, charge) = pos.settle(price(1, 1), &LAUNCH_CURVE, 7);
    // Debt 10 over 10 of alpha value plus 5 of collateral: 2,000 * (2/3)^2, floored per step.
    assert_eq!((charge.amount, charge.rate_bps), (tao(1), 888));
    assert_eq!(settled.collateral, tao(4));
    assert_eq!(settled.accrual.due, 0);
}

// Golden differential: the pallet's coverage dispatch on the model's coverage rows.

fn golden_amount(v: &serde_json::Value) -> u64 {
    v.as_str()
        .expect("decimal string")
        .parse()
        .expect("fits u64")
}

fn golden_price(v: &serde_json::Value) -> Price {
    Price::from_bits(
        v.as_str()
            .expect("decimal string")
            .parse()
            .expect("fits u128"),
    )
}

fn golden_params() -> Params {
    Params {
        lending: LendingParams::default(),
        short: ShortParams::default(),
        long: LongParams::default(),
        dereg: DeregParams::default(),
    }
}

fn golden_refs(row: &serde_json::Value) -> PriceRefs {
    PriceRefs {
        spot: golden_price(&row["spot"]),
        twap: golden_price(&row["twap"]),
        sma: golden_price(&row["sma"]),
    }
}

#[test]
fn short_coverage_need_matches_the_model_on_golden_rows() {
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("golden/v21.json")).expect("golden JSON parses");
    let rows = doc["coverage"].as_array().expect("array");
    assert_eq!(rows.len(), 540);
    let params = golden_params();
    for (i, row) in rows.iter().enumerate() {
        let pool = PoolState::model(golden_amount(&row["tao"]), golden_amount(&row["alpha"]));
        let leg = Leg::Short {
            minted: golden_amount(&row["q"]),
            proceeds: 0,
        };
        let need = (!row["need"].is_null()).then(|| golden_amount(&row["need"]));
        assert_eq!(
            coverage_need::<ConstantProduct>(
                &leg,
                golden_amount(&row["book"]),
                &pool,
                &golden_refs(row),
                &params
            ),
            need,
            "short need, row {i}"
        );
    }
}

#[test]
fn long_coverage_need_matches_the_model_on_golden_rows() {
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("golden/math.json")).expect("golden JSON parses");
    let rows = doc["long_coverage"].as_array().expect("array");
    assert_eq!(rows.len(), 180);
    let params = golden_params();
    for (i, row) in rows.iter().enumerate() {
        let pool = PoolState::model(golden_amount(&row["tao"]), golden_amount(&row["alpha"]));
        let long = &row["long"];
        let leg = Leg::Long {
            locked_alpha: golden_amount(&long["a"]),
            debt: golden_amount(&long["debt"]),
        };
        assert_eq!(
            coverage_need::<ConstantProduct>(
                &leg,
                golden_amount(&long["book"]),
                &pool,
                &golden_refs(row),
                &params
            ),
            Some(golden_amount(&long["need"])),
            "long need, row {i}"
        );
    }
}
