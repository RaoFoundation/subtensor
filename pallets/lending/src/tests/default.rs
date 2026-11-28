//! Short default at collateral exhaustion (SPEC section 1 item 4, I2, I5, I28), the charge
//! policy behind it (`math::default`), the partial-close rule (SPEC 1.4), the emission-set gate
//! of a short's growth and the `lending_alpha` / `long_credit` hooks.

use frame_support::{assert_noop, assert_ok, dispatch::DispatchResult, traits::Get};
use subtensor_runtime_common::{NetUid, TaoBalance};

use super::{golden, interest::model_accrue, mock::*};
use crate::{
    Custody, Due, Error, Event, HolderPositions, InterestRetry, InterestTotals, LendingInterface,
    LendingPoolInterface, LongLiftedTao, NextDue, PositionCount, Positions, ResidualAlpha,
    SyntheticOutstanding, TotalPositions,
    math::{
        default::{ChargePolicy, CollateralOnly, CollateralThenProceeds},
        fixed::Price,
        rate::short_close_derisks,
    },
    params::{MAX_RATE_CAP_BPS, RAO},
    positions::partial_close_allowed,
    types::{Leg, Position, PositionId, Side},
};

const ALICE: AccountId = 1;
const BOB: AccountId = 2;
const SN1: u16 = 1;
/// The launch cadence (SPEC P15).
const INTERVAL: u64 = 360;
/// The base APR of the tests' curve.
const APR: u32 = 2_000;

fn sn1() -> NetUid {
    NetUid::from(SN1)
}

fn tao(n: u64) -> u64 {
    n * RAO
}

fn alpha(n: u64) -> u64 {
    n * RAO
}

/// Subnet 1 with a deep pool (5,000 TAO / 50,000 alpha, spot 0.1), lending and longs on, the
/// test curve of `tests/interest.rs` (exponent 2, cap 100%, no delay) and Alice and Bob funded.
fn live() -> sp_io::TestExternalities {
    let mut ext = migrated_ext();
    ext.execute_with(|| {
        bare_curve();
        set_pool(SN1, tao(5_000), alpha(50_000));
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_ok!(Lending::set_longs_enabled(RuntimeOrigin::root(), true));
        set_balance(ALICE, tao(100));
        set_balance(BOB, tao(100));
    });
    ext
}

fn id(who: AccountId) -> PositionId<AccountId> {
    (who, sn1(), Side::Short)
}

fn short(who: AccountId) -> Option<Position<u64>> {
    Positions::<Test>::get(id(who))
}

fn open(who: AccountId) -> Position<u64> {
    short(who).expect("short is open")
}

fn custody(who: AccountId) -> AccountId {
    Lending::custody_of(Side::Short, &who, sn1(), open(who).opened_at)
}

/// A short of 200 alpha with 45 TAO of collateral: about 20 TAO of proceeds at spot 0.1.
fn open_short(who: AccountId) -> DispatchResult {
    Lending::open_short(RuntimeOrigin::signed(who), sn1(), alpha(200), tao(45), 0)
}

fn healthy() {
    assert_ok!(Lending::do_try_state());
}

/// Leaves `collateral` rao in a short and its custody, as after a long bleed.
fn leave_collateral(who: AccountId, collateral: u64) {
    let pos = open(who);
    set_balance(custody(who), collateral + pos.leg.proceeds() + pos.deposit);
    Positions::<Test>::insert(id(who), Position { collateral, ..pos });
}

/// Moves the pool and both slow prices to 0.05, half the price the shorts opened at. Every
/// short then owes less than it holds whatever its collateral (`u < 1`: 10 TAO against about
/// 20 TAO of proceeds), so it accrues at the base APR, and the TWAP stays where it is while
/// blocks pass.
fn settle_price_at_half() {
    set_pool(SN1, tao(2_500), alpha(50_000));
    seed_slow_prices(SN1, px(5, 100));
}

/// `floor(alpha * TWAP_median)` at the TWAP the pallet reads now.
fn at_twap(alpha: u64) -> u64 {
    let twap = Lending::price_refs(sn1()).expect("rings are seeded").twap;
    u64::try_from((u128::from(alpha) * twap.to_bits()) >> 64).expect("fits")
}

/// What a short of 200 alpha owes for one interval at the base APR, by the model's `_accrue`.
fn interval_interest() -> u64 {
    let (owed, _) = model_accrue(at_twap(alpha(200)), APR, INTERVAL, 0);
    assert!(owed > 1_000, "the charge is large enough to test around");
    owed
}

/// The interest charged in the current block: `(holder, amount)`, ordered by holder.
fn charges() -> Vec<(AccountId, u64)> {
    let mut charged: Vec<_> = lending_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::InterestCharged { who, to_pool, .. } => Some((who, to_pool)),
            _ => None,
        })
        .collect();
    charged.sort_unstable();
    charged
}

/// The defaults of the current block: `(holder, residual_alpha, proceeds_to_pool)`.
fn defaults() -> Vec<(AccountId, u64, u64)> {
    lending_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::ShortDefaulted {
                who,
                residual_alpha,
                proceeds_to_pool,
                ..
            } => Some((who, residual_alpha, proceeds_to_pool)),
            _ => None,
        })
        .collect()
}

/// Everything TAO can be in the mock: free balances (holders and custody), the pool reserves
/// and what was burned.
fn all_tao() -> u128 {
    total_balances()
        + u128::from(FakePool::pool_tao(sn1()))
        + u128::from(FakePool::pool_tao(NetUid::from(2)))
        + u128::from(burned_tao())
}

// The trigger.

#[test]
fn collateral_that_exactly_covers_the_charge_does_not_default() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        settle_price_at_half();
        let due = interval_interest();
        leave_collateral(ALICE, due);
        let first = open(ALICE).charge_at;
        run_to(first);
        assert_eq!(interval_interest(), due, "the TWAP did not move");

        // The charge took the whole collateral and nothing is left due: no default.
        assert_eq!(charges(), vec![(ALICE, due)]);
        assert_eq!(defaults(), vec![]);
        let pos = open(ALICE);
        assert_eq!((pos.collateral, pos.accrual.due), (0, 0));
        assert_eq!(pos.charge_at, first + INTERVAL);
        assert_eq!(ResidualAlpha::<Test>::get(sn1()), 0);
        healthy();

        // With no collateral left, the next scheduled charge finds interest due and defaults.
        run_to(first + INTERVAL);
        assert_eq!(charges(), vec![]);
        assert_eq!(defaults(), vec![(ALICE, alpha(200), pos.leg.proceeds())]);
        assert!(short(ALICE).is_none());
        healthy();
    });
}

#[test]
fn collateral_one_rao_short_of_the_charge_defaults() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        assert_ok!(open_short(BOB));
        settle_price_at_half();
        let due = interval_interest();
        leave_collateral(ALICE, due - 1);
        let before = open(ALICE);
        let (proceeds, deposit) = (before.leg.proceeds(), before.deposit);
        assert!(proceeds > 0 && deposit > 0);
        let alice_custody = custody(ALICE);
        let first = before.charge_at;
        run_to(first - 1);
        healthy();

        let pool = FakePool::pool_tao(sn1());
        let alpha_in = FakePool::subnet_alpha_in(sn1());
        let issuance = FakePool::alpha_issuance(sn1());
        let everything = all_tao();
        let alice = FakePool::tao_balance(&ALICE);
        let burned = burned_tao();
        let bob = open(BOB);
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), alpha(400));
        run_to(first);

        // The charge takes the collateral that is there, and the rao it could not take is a
        // default in the same step.
        let bob_charge = bob.collateral - open(BOB).collateral;
        assert_eq!(charges(), vec![(ALICE, due - 1), (BOB, bob_charge)]);
        assert_eq!(defaults(), vec![(ALICE, alpha(200), proceeds)]);

        // I2: q moves from the short book to the residual, and no alpha is minted or burned.
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), alpha(200));
        assert_eq!(ResidualAlpha::<Test>::get(sn1()), alpha(200));
        assert_eq!(
            Lending::lending_alpha_books(sn1()),
            (alpha(200), alpha(200))
        );
        assert_eq!(FakePool::alpha_issuance(sn1()), issuance);
        assert_eq!(FakePool::subnet_alpha_in(sn1()), alpha_in);

        // The proceeds go to the pool's TAO reserve with the interest, the deposit is burned,
        // the holder gets nothing back, and no TAO appears or vanishes.
        assert_eq!(
            FakePool::pool_tao(sn1()),
            pool + (due - 1) + bob_charge + proceeds
        );
        assert_eq!(burned_tao(), burned + deposit);
        assert_eq!(FakePool::tao_balance(&alice_custody), 0);
        assert_eq!(FakePool::tao_balance(&ALICE), alice);
        assert_eq!(all_tao(), everything);
        // The proceeds are not interest (I19 counts the charges alone).
        let total = InterestTotals::<Test>::get(sn1());
        assert_eq!(total.charged, u128::from(due - 1 + bob_charge));
        assert_eq!(total.to_pool, total.charged);

        // Nothing of the position is left.
        assert!(short(ALICE).is_none());
        assert!(!Custody::<Test>::contains_key(id(ALICE)));
        assert!(!HolderPositions::<Test>::contains_key(ALICE));
        assert!(!InterestRetry::<Test>::contains_key(id(ALICE)));
        assert_eq!(Due::<Test>::iter().count(), 1);
        assert_eq!(PositionCount::<Test>::get(sn1()), 1);
        assert_eq!(TotalPositions::<Test>::get(), 1);
        // Bob's short is as its own charge left it.
        assert_eq!(open(BOB).leg, bob.leg);
        healthy();

        // The holder may open again; the old custody account is not reused.
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            alpha(400),
            tao(45),
            0
        ));
        assert_ne!(custody(ALICE), alice_custody);
        assert_eq!(ResidualAlpha::<Test>::get(sn1()), alpha(200));
        healthy();
    });
}

#[test]
fn a_default_removes_the_short_from_the_per_subnet_index() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        assert_ok!(open_short(BOB));
        settle_price_at_half();
        leave_collateral(ALICE, interval_interest() - 1);
        let indexed = || {
            let mut holders: Vec<_> =
                crate::PositionsByNetuid::<Test>::iter_key_prefix((sn1(), Side::Short)).collect();
            holders.sort_unstable();
            holders
        };
        assert_eq!(indexed(), vec![ALICE, BOB]);
        run_to(open(ALICE).charge_at);
        assert_eq!(defaults().len(), 1);
        assert!(short(ALICE).is_none());
        // The defaulted short's key is gone and the other short's stays.
        assert_eq!(indexed(), vec![BOB]);
        healthy();
    });
}

#[test]
fn the_last_default_clears_the_queue() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        settle_price_at_half();
        leave_collateral(ALICE, 0);
        run_to(open(ALICE).charge_at);
        assert_eq!(defaults().len(), 1);
        assert_eq!(TotalPositions::<Test>::get(), 0);
        assert_eq!(PositionCount::<Test>::get(sn1()), 0);
        assert_eq!(NextDue::<Test>::get(), None);
        assert_eq!(Due::<Test>::iter().count(), 0);
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), 0);
        healthy();
    });
}

#[test]
fn no_touch_defaults_a_short() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        settle_price_at_half();
        leave_collateral(ALICE, 0);
        let first = open(ALICE).charge_at;
        // Interest accrues for 100 blocks with no collateral to pay it and no scheduled charge.
        run_to(first - 260);
        let origin = || RuntimeOrigin::signed(ALICE);

        // A partial close charges min(due, C) = 0 and leaves the rest due (model: `_charge`).
        assert_ok!(Lending::close_short(origin(), sn1(), alpha(50), u64::MAX));
        let pos = open(ALICE);
        assert_eq!(pos.collateral, 0);
        assert!(pos.accrual.due > 0);
        assert_eq!((charges(), defaults()), (vec![], vec![]));
        // A short stored with C == 0 and interest due is a legal state between charges.
        healthy();

        // A top-up settles before it adds, so the interest stays due against the new collateral.
        next_block();
        assert_ok!(Lending::add_collateral(origin(), sn1(), 1));
        let pos = open(ALICE);
        assert_eq!(pos.collateral, 1);
        assert!(pos.accrual.due > 0);
        assert_eq!(defaults(), vec![]);
        healthy();

        // An increase charges what the collateral can pay and never defaults.
        next_block();
        assert_ok!(Lending::increase_short(
            origin(),
            sn1(),
            alpha(250),
            tao(40),
            0
        ));
        assert_eq!(defaults(), vec![]);
        assert!(short(ALICE).is_some());
        healthy();

        // Back at C == 0 with interest due, by a touch: still open until the scheduled charge.
        leave_collateral(ALICE, 0);
        next_block();
        assert_ok!(Lending::close_short(origin(), sn1(), alpha(1), u64::MAX));
        let pos = open(ALICE);
        assert_eq!(pos.collateral, 0);
        assert!(pos.accrual.due > 0);
        assert_eq!(defaults(), vec![]);
        run_to(first - 1);
        assert!(short(ALICE).is_some());
        healthy();

        // The scheduled charge is what defaults it.
        run_to(first);
        assert_eq!(
            defaults(),
            vec![(ALICE, pos.leg.alpha(), pos.leg.proceeds())]
        );
        assert!(short(ALICE).is_none());
        healthy();
    });
}

#[test]
fn a_full_close_of_a_short_out_of_collateral_pays_its_interest_and_does_not_default() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        settle_price_at_half();
        leave_collateral(ALICE, 0);
        run_to(open(ALICE).charge_at - 1);
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            alpha(200),
            u64::MAX
        ));
        assert_eq!(defaults(), vec![]);
        assert!(short(ALICE).is_none());
        assert_eq!(ResidualAlpha::<Test>::get(sn1()), 0);
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), 0);
        assert_eq!(burned_tao(), 0);
        healthy();
    });
}

#[test]
fn defaults_are_bounded_by_the_charges_of_a_block() {
    live().execute_with(|| {
        // Two more shorts than one block charges, all opened in one block and out of collateral.
        let budget: u32 = <Test as crate::Config>::MaxChargesPerBlock::get();
        let holders: Vec<AccountId> = (100..100 + u128::from(budget) + 2).collect();
        set_pool(SN1, tao(5_000_000), alpha(50_000_000));
        seed_slow_prices(SN1, px(1, 10));
        for who in &holders {
            set_balance(*who, tao(100));
            assert_ok!(open_short(*who));
        }
        set_pool(SN1, tao(2_500_000), alpha(50_000_000));
        seed_slow_prices(SN1, px(5, 100));
        for who in &holders {
            leave_collateral(*who, 0);
        }
        let first = open(holders[0]).charge_at;
        run_to(first);
        assert_eq!(defaults().len(), budget as usize);
        assert_eq!(TotalPositions::<Test>::get(), 2);
        // The two the budget left are stored with C == 0 and are still scheduled.
        healthy();
        run_to(first + 1);
        assert_eq!(defaults().len(), 2);
        assert_eq!(TotalPositions::<Test>::get(), 0);
        assert_eq!(
            ResidualAlpha::<Test>::get(sn1()),
            alpha(200) * (u64::from(budget) + 2)
        );
        healthy();
    });
}

// Both charge policies through the scheduled charge.

/// Runs Alice's scheduled charge under the policy `P`, as `collect_due` does, in the block it
/// is due. Returns whether it defaulted.
fn scheduled_charge<P: ChargePolicy>() -> bool {
    let at = open(ALICE).charge_at;
    System::reset_events();
    System::set_block_number(at);
    Due::<Test>::remove(at, id(ALICE));
    Lending::charge_due::<P>(&id(ALICE), at, &Lending::params().lending)
}

/// A short one rao of collateral short of its first charge.
fn one_rao_short() -> (u64, Position<u64>) {
    assert_ok!(open_short(ALICE));
    settle_price_at_half();
    let due = interval_interest();
    leave_collateral(ALICE, due - 1);
    (due, open(ALICE))
}

#[test]
fn the_launch_policy_defaults_through_the_generic_charge() {
    live().execute_with(|| {
        let (due, before) = one_rao_short();
        assert!(scheduled_charge::<CollateralOnly>());
        assert_eq!(charges(), vec![(ALICE, due - 1)]);
        assert_eq!(defaults(), vec![(ALICE, alpha(200), before.leg.proceeds())]);
        healthy();
    });
}

#[test]
fn the_alternative_policy_pays_from_the_proceeds_and_defaults_when_both_are_spent() {
    live().execute_with(|| {
        let (due, before) = one_rao_short();
        let proceeds = before.leg.proceeds();
        let pool = FakePool::pool_tao(sn1());
        let everything = all_tao();

        // The same charge code under the other policy: the missing rao comes from P.
        assert!(!scheduled_charge::<CollateralThenProceeds>());
        assert_eq!(charges(), vec![(ALICE, due)]);
        assert_eq!(defaults(), vec![]);
        let pos = open(ALICE);
        assert_eq!((pos.collateral, pos.accrual.due), (0, 0));
        assert_eq!(pos.leg.proceeds(), proceeds - 1);
        assert_eq!(pos.leg.alpha(), alpha(200));
        assert_eq!(FakePool::pool_tao(sn1()), pool + due);
        assert_eq!(
            FakePool::tao_balance(&custody(ALICE)),
            proceeds - 1 + pos.deposit
        );
        assert_eq!(pos.charge_at, before.charge_at + INTERVAL);
        healthy();

        // No collateral, proceeds left: the next charge pays from the proceeds, no default.
        let (second, _) = model_accrue(at_twap(alpha(200)), APR, INTERVAL, pos.accrual.remainder);
        assert!(!scheduled_charge::<CollateralThenProceeds>());
        assert_eq!(charges(), vec![(ALICE, second)]);
        assert_eq!(open(ALICE).leg.proceeds(), proceeds - 1 - second);
        healthy();

        // Three rao of proceeds left: the charge takes them and the rest due is a default
        // with nothing left for the pool.
        let pos = open(ALICE);
        set_balance(custody(ALICE), 3 + pos.deposit);
        Positions::<Test>::insert(
            id(ALICE),
            Position {
                leg: Leg::Short {
                    minted: alpha(200),
                    proceeds: 3,
                },
                ..pos
            },
        );
        // The TAO taken out of custody by hand, for the conservation check.
        let removed = u128::from(proceeds - 1 - second - 3);
        assert!(scheduled_charge::<CollateralThenProceeds>());
        assert_eq!(charges(), vec![(ALICE, 3)]);
        assert_eq!(defaults(), vec![(ALICE, alpha(200), 0)]);
        assert_eq!(ResidualAlpha::<Test>::get(sn1()), alpha(200));
        assert_eq!(burned_tao(), pos.deposit);
        assert_eq!(all_tao() + removed, everything);
        assert!(short(ALICE).is_none());
        healthy();
    });
}

#[test]
fn both_policies_agree_while_the_collateral_pays() {
    fn charge_with<P: ChargePolicy>() -> (Position<u64>, u64) {
        let mut out = None;
        live().execute_with(|| {
            assert_ok!(open_short(ALICE));
            settle_price_at_half();
            assert!(!scheduled_charge::<P>());
            healthy();
            out = Some((open(ALICE), FakePool::pool_tao(sn1())));
        });
        out.expect("ran")
    }
    assert_eq!(
        charge_with::<CollateralOnly>(),
        charge_with::<CollateralThenProceeds>()
    );
}

// Partial close (SPEC 1.4).

fn rate_of(who: AccountId) -> u32 {
    let refs = Lending::price_refs(sn1()).expect("rings are seeded");
    open(who).rate_bps(&refs, &Lending::params().lending.rate_curve())
}

fn close(who: AccountId, amount: u64) -> DispatchResult {
    Lending::close_short(RuntimeOrigin::signed(who), sn1(), amount, u64::MAX)
}

#[test]
fn a_partial_close_that_leaves_the_short_covered_is_allowed() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        assert_ok!(close(ALICE, alpha(100)));
        assert_eq!(open(ALICE).leg.alpha(), alpha(100));
        assert_eq!(rate_of(ALICE), APR);
        healthy();
    });
}

#[test]
fn an_underwater_short_can_close_half_without_a_top_up() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        // The price is pumped: the slow price reads 0.5 and the pool trades at 0.2. The short
        // owes 100 TAO against the 65 it holds (u about 1.5), far from covered.
        set_pool(SN1, tao(10_000), alpha(50_000));
        seed_slow_prices(SN1, px(5, 10));
        let before = open(ALICE);
        let rate_before = rate_of(ALICE);
        assert!(rate_before > APR);
        // It cannot grow: growth needs coverage.
        assert_noop!(
            Lending::increase_short(RuntimeOrigin::signed(ALICE), sn1(), alpha(100), 0, 0),
            Error::<Test>::CoverageNotMet
        );

        // Closing half costs less per alpha than the short holds per alpha (the proceeds and a
        // little collateral pay for it), so its debt over its holdings falls: allowed with no
        // top-up, though what is left is still uncovered.
        assert_ok!(close(ALICE, alpha(100)));
        let after = open(ALICE);
        assert_eq!(after.leg.alpha(), alpha(100));
        let held = |pos: &Position<u64>| u128::from(pos.collateral + pos.leg.proceeds());
        assert!(u128::from(alpha(100)) * held(&before) <= u128::from(alpha(200)) * held(&after));
        let rate_after = rate_of(ALICE);
        assert!(rate_after < rate_before, "{rate_after} < {rate_before}");
        assert!(rate_after > APR, "still above u = 1: not covered");
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), alpha(100));
        healthy();
    });
}

#[test]
fn a_partial_close_that_raises_the_debt_over_the_holdings_is_refused() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        // Pumped to 0.5 on the pool as well: buying back half costs about 50 of the 65 TAO
        // held, so what is left would owe 50 against 15.
        set_pool(SN1, tao(25_000), alpha(50_000));
        seed_slow_prices(SN1, px(5, 10));
        assert!(rate_of(ALICE) > APR);
        assert_noop!(close(ALICE, alpha(100)), Error::<Test>::CoverageNotMet);
        // A top-up makes the same close one that does not raise u: 65 of 115 TAO are left
        // against half the debt.
        assert_ok!(Lending::add_collateral(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            tao(50)
        ));
        assert_ok!(close(ALICE, alpha(100)));
        healthy();
        // The full close has no such rule.
        assert_ok!(close(ALICE, alpha(100)));
        assert!(short(ALICE).is_none());
        healthy();
    });
}

#[test]
fn the_partial_close_rule_is_coverage_or_no_rise_of_u() {
    let pos = |collateral: u64, minted: u64, proceeds: u64| Position::<u64> {
        collateral,
        leg: Leg::Short { minted, proceeds },
        ..Position::fresh(Side::Short, 1, 361)
    };
    let before = pos(40, 200, 60);
    let half = Leg::Short {
        minted: 100,
        proceeds: 10,
    };
    // Covered: allowed whatever u does.
    assert!(partial_close_allowed(Some(50), &before, 40, &half));
    // Not covered, u unchanged (100 * 100 == 200 * 50): allowed.
    assert!(partial_close_allowed(Some(51), &before, 40, &half));
    assert!(partial_close_allowed(None, &before, 40, &half));
    // Not covered and u rises by the smallest step: refused.
    assert!(!partial_close_allowed(Some(50), &before, 39, &half));
    assert!(!partial_close_allowed(None, &before, 39, &half));
    // The cross-multiplication is exact at the edges and fails closed on overflow.
    assert!(short_close_derisks(u64::MAX, 1, u64::MAX - 1, 1));
    assert!(!short_close_derisks(1, u128::MAX, 2, u128::MAX));
    assert!(short_close_derisks(2, 0, 1, 0));
    // A long has the coverage rule only.
    let long = |locked_alpha: u64, debt: u64| Leg::Long { locked_alpha, debt };
    let held = Position::<u64> {
        collateral: 40,
        leg: long(200, 60),
        ..Position::fresh(Side::Long, 1, 361)
    };
    assert!(partial_close_allowed(Some(40), &held, 40, &long(100, 30)));
    assert!(!partial_close_allowed(Some(41), &held, 40, &long(100, 30)));
}

// The emission-set gate.

#[test]
fn a_short_opens_and_grows_only_on_a_subnet_in_the_emission_set() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        set_in_emission(SN1, false);
        assert_noop!(open_short(BOB), Error::<Test>::SubnetNotInEmission);
        assert_noop!(
            Lending::increase_short(RuntimeOrigin::signed(ALICE), sn1(), alpha(100), tao(30), 0),
            Error::<Test>::SubnetNotInEmission
        );
        assert_eq!(
            Lending::quote_open_short(sn1(), alpha(200)).err(),
            Some(Error::<Test>::SubnetNotInEmission.into())
        );

        // What reduces risk stays open on a subnet that dropped out: top-up and close.
        assert_ok!(Lending::add_collateral(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            tao(1)
        ));
        assert_ok!(close(ALICE, alpha(50)));
        assert_ok!(close(ALICE, alpha(150)));
        assert!(short(ALICE).is_none());
        healthy();

        // Back in the set, shorts open and grow again.
        set_in_emission(SN1, true);
        assert_ok!(open_short(BOB));
        assert_ok!(Lending::increase_short(
            RuntimeOrigin::signed(BOB),
            sn1(),
            alpha(100),
            tao(30),
            0
        ));
        healthy();
    });
}

#[test]
fn scheduled_charges_and_defaults_run_on_a_subnet_out_of_the_emission_set() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        assert_ok!(open_short(BOB));
        settle_price_at_half();
        leave_collateral(ALICE, 0);
        set_in_emission(SN1, false);
        run_to(open(ALICE).charge_at);
        assert_eq!(charges().len(), 1);
        assert_eq!(defaults().len(), 1);
        healthy();
    });
}

#[test]
fn longs_are_not_gated_by_the_emission_set() {
    live().execute_with(|| {
        set_in_emission(SN1, false);
        assert_ok!(Lending::open_long(
            RuntimeOrigin::signed(BOB),
            sn1(),
            tao(20),
            tao(12),
            0
        ));
        healthy();
    });
}

// The hooks.

#[test]
fn lending_alpha_counts_the_residual_of_defaulted_shorts() {
    live().execute_with(|| {
        let lending_alpha = || <Lending as LendingInterface<AccountId>>::lending_alpha(sn1());
        assert_ok!(open_short(ALICE));
        assert_ok!(open_short(BOB));
        assert_eq!(lending_alpha(), alpha(400));
        settle_price_at_half();
        leave_collateral(ALICE, 0);
        run_to(open(ALICE).charge_at);
        assert_eq!(defaults().len(), 1);
        // The defaulted q left the short book and is still lending's alpha in circulation.
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), alpha(200));
        assert_eq!(lending_alpha(), alpha(400));
        // A close retires its own alpha and leaves the residual.
        assert_ok!(close(BOB, alpha(200)));
        assert_eq!(lending_alpha(), alpha(200));
        assert_eq!(ResidualAlpha::<Test>::get(sn1()), alpha(200));
        assert_eq!(<() as LendingInterface<AccountId>>::lending_alpha(sn1()), 0);
        healthy();
    });
}

#[test]
fn long_credit_is_the_tao_lifted_by_the_longs() {
    live().execute_with(|| {
        let long_credit = || <Lending as LendingInterface<AccountId>>::long_credit(sn1());
        assert_eq!(long_credit(), TaoBalance::from(0u64));
        LongLiftedTao::<Test>::insert(sn1(), tao(20));
        assert_eq!(long_credit(), TaoBalance::from(tao(20)));
        assert_eq!(
            <() as LendingInterface<AccountId>>::long_credit(sn1()),
            TaoBalance::from(0u64)
        );
    });
}

// I28: no forced trade.

/// SplitMix64: a small deterministic generator, so a failing seed replays.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `low..=high`.
    fn range(&mut self, low: u64, high: u64) -> u64 {
        low + self.next() % (high - low + 1)
    }
}

/// What lending is allowed to change, read before and after a step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Snapshot {
    pool_tao: u64,
    alpha_in: u64,
    issuance: u64,
    lending_alpha: u64,
    residual: u64,
    burned: u64,
    all_tao: u128,
    open: u32,
}

fn snapshot() -> Snapshot {
    let (book, residual) = Lending::lending_alpha_books(sn1());
    Snapshot {
        pool_tao: FakePool::pool_tao(sn1()),
        alpha_in: FakePool::subnet_alpha_in(sn1()),
        issuance: FakePool::alpha_issuance(sn1()),
        lending_alpha: book + residual,
        residual,
        burned: burned_tao(),
        all_tao: all_tao(),
        open: TotalPositions::<Test>::get(),
    }
}

/// Checks one step in which only charges, defaults and top-ups ran (SPEC I28): the pool's TAO
/// reserve rose by exactly the interest credited plus the proceeds of the shorts that
/// defaulted, and nothing else of the pool or of the alpha supply moved. Returns the number of
/// charges and of defaults in the step.
fn assert_no_forced_trade(before: Snapshot, deposit: u64) -> (u64, u64) {
    let (charged, defaulted) = (charges(), defaults());
    let interest: u64 = charged.iter().map(|(_, amount)| amount).sum();
    let proceeds: u64 = defaulted.iter().map(|(_, _, proceeds)| proceeds).sum();
    let residual: u64 = defaulted.iter().map(|(_, q, _)| q).sum();
    let after = snapshot();
    assert_eq!(after.pool_tao, before.pool_tao + interest + proceeds);
    assert_eq!(
        after.alpha_in, before.alpha_in,
        "no alpha is bought or sold"
    );
    assert_eq!(
        after.issuance, before.issuance,
        "no alpha is minted or burned"
    );
    assert_eq!(after.lending_alpha, before.lending_alpha);
    assert_eq!(after.residual, before.residual + residual);
    assert_eq!(
        after.burned,
        before.burned + deposit * defaulted.len() as u64
    );
    assert_eq!(after.all_tao, before.all_tao, "TAO is conserved");
    assert_eq!(after.open + defaulted.len() as u32, before.open);
    (charged.len() as u64, defaulted.len() as u64)
}

/// One random run: a random curve, cadence and deposit within the SPEC 2.1 bounds and five
/// holders whose shorts have random sizes and little collateral, then 1,200 blocks in which
/// other accounts move the pool and the slow prices at random, holders sometimes top up and a
/// holder whose short defaulted sometimes opens another.
fn random_run(seed: u64) -> (u64, u64) {
    let mut totals = (0u64, 0u64);
    live().execute_with(|| {
        let mut rng = Rng(seed);
        set_pool(SN1, tao(50_000), alpha(500_000));
        seed_slow_prices(SN1, px(1, 10));
        let apr = rng.range(1, 10_000);
        set_live_lending(|p| {
            p.apr_bps = apr as u32;
            p.rate_k = rng.range(1, 8) as u8;
            p.rate_cap_bps = rng.range(apr, u64::from(MAX_RATE_CAP_BPS)) as u32;
            p.charge_interval_blocks = rng.range(1, 90) as u32;
            p.deposit = rng.range(0, 10 * RAO);
        });
        let deposit = Lending::params().lending.deposit;
        assert_ok!(Lending::params().lending.validate());
        // A short with between nothing and a few TAO of collateral left.
        let open_thin = |who: AccountId, rng: &mut Rng| {
            set_balance(who, tao(10_000));
            let q = alpha(rng.range(120, 400));
            let opened =
                Lending::open_short(RuntimeOrigin::signed(who), sn1(), q, tao(200), 0).is_ok();
            if opened {
                leave_collateral(who, rng.range(0, 3) * rng.range(0, RAO));
            }
            opened
        };
        let holders: Vec<AccountId> = (10..15).collect();
        for who in &holders {
            assert!(open_thin(*who, &mut rng));
        }
        healthy();

        for _ in 0..1_200 {
            // Other accounts trade: the pool and the slow prices move, by anything.
            if rng.range(0, 9) == 0 {
                let pool_tao = tao(rng.range(2_000, 400_000));
                set_pool(SN1, pool_tao, alpha(rng.range(100_000, 2_000_000)));
            }
            if rng.range(0, 19) == 0 {
                seed_slow_prices(SN1, px(rng.range(1, 400), 400));
            }
            let before = snapshot();
            next_block();
            let (charged, defaulted) = assert_no_forced_trade(before, deposit);
            totals = (totals.0 + charged, totals.1 + defaulted);

            let who = holders[rng.range(0, 4) as usize];
            match (short(who).is_some(), rng.range(0, 19)) {
                // A holder tops up: the only reserve change is the interest it settles.
                (true, 0) => {
                    System::reset_events();
                    let before = snapshot();
                    assert_ok!(Lending::add_collateral(
                        RuntimeOrigin::signed(who),
                        sn1(),
                        rng.range(1, RAO / 10)
                    ));
                    assert_eq!(assert_no_forced_trade(before, deposit).1, 0);
                }
                // A holder whose short defaulted opens another, if the market admits it.
                (false, 0..=4) => {
                    open_thin(who, &mut rng);
                }
                _ => {}
            }
            if rng.range(0, 99) == 0 {
                healthy();
            }
        }
        healthy();
    });
    totals
}

#[test]
fn i28_charges_and_defaults_are_the_only_reserve_changes_lending_makes() {
    let mut totals = (0u64, 0u64);
    for seed in 1..=24 {
        let (charged, defaulted) = random_run(seed);
        totals = (totals.0 + charged, totals.1 + defaulted);
    }
    // The runs did exercise both: many charges and many defaults.
    assert!(totals.0 > 1_000, "{} charges", totals.0);
    assert!(totals.1 > 24, "{} defaults", totals.1);
}

/// The slow price as a `Price`, to keep the import used where the tests read it.
#[test]
fn settle_price_at_half_matches_the_pool() {
    live().execute_with(|| {
        settle_price_at_half();
        let refs = Lending::price_refs(sn1()).expect("rings are seeded");
        let spot: Price = FakePool::spot_price(sn1());
        assert_eq!((refs.twap, refs.sma), (spot, spot));
    });
}

/// Rows of the `default` section written by `model/gen_golden_v21.py`; a silently shorter file
/// must not pass.
const DEFAULT_ROWS: usize = 48;

/// The scheduled charges of the current block as the golden rows list them: `(amount, rate)`.
fn charges_with_rate() -> Vec<(u64, u32)> {
    lending_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::InterestCharged {
                to_pool, rate_bps, ..
            } => Some((to_pool, rate_bps)),
            _ => None,
        })
        .collect()
}

/// Differential test against `model/v21`: a short whose collateral runs out at a known charge
/// defaults in the same block as in the model, after the same charges at the same rates, with
/// the same residual alpha, the same proceeds to the pool, the same deposit burned and the same
/// pool TAO after it.
#[test]
fn golden_default_matches_the_model() {
    let doc = golden::golden_v21();
    let all = golden::rows(&doc, "default");
    assert_eq!(all.len(), DEFAULT_ROWS);
    let (mut first_charge, mut later, mut convex) = (0usize, 0usize, 0usize);
    for (i, row) in all.iter().enumerate() {
        migrated_ext().execute_with(|| {
            let amount = |key: &str| golden::amount(&row[key]);
            let (apr, cap) = (golden::small(&row["apr"]), golden::small(&row["cap"]));
            set_live_lending(|p| {
                p.apr_bps = apr;
                p.rate_k = u8::try_from(golden::small(&row["k"])).expect("exponent fits");
                p.rate_cap_bps = cap;
                p.rate_delay_blocks = 0;
            });
            // The model's subnet starts with both slow prices at its spot.
            set_pool(SN1, amount("tao"), amount("alpha"));
            assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
            seed_slow_prices(SN1, FakePool::spot_price(sn1()));
            let (q, deposit) = (amount("q"), amount("deposit"));
            set_balance(ALICE, amount("collateral_open") + deposit);
            assert_ok!(Lending::open_short(
                RuntimeOrigin::signed(ALICE),
                sn1(),
                q,
                amount("collateral_open"),
                0
            ));
            let opened = open(ALICE);
            assert_eq!(
                (opened.leg.proceeds(), opened.deposit),
                (amount("proceeds"), deposit),
                "open, row {i}"
            );
            assert_eq!(FakePool::tao_balance(&ALICE), 0);

            // The slow prices move and the collateral bleeds down, as the generator writes them.
            let samples = crate::LendingParamsStore::<Test>::get().twap_samples;
            crate::TwapRing::<Test>::insert(
                sn1(),
                crate::prices::TwapState::seeded(golden::price(&row["twap"]), samples),
            );
            let sma = golden::price(&row["sma"]);
            crate::RankRing::<Test>::insert(sn1(), crate::prices::RankState::seeded(sma, 30));
            crate::RankPrice::<Test>::insert(sn1(), sma);
            leave_collateral(ALICE, amount("collateral_left"));

            let expected: Vec<(u64, u64, u32)> = row["charges"]
                .as_array()
                .expect("charges are a list")
                .iter()
                .map(|c| {
                    (
                        c[0].as_u64().expect("block offset"),
                        golden::amount(&c[1]),
                        golden::small(&c[2]),
                    )
                })
                .collect();
            let default_at = row["default_at"].as_u64().expect("block offset");
            let open_block = System::block_number();
            let mut charged = Vec::new();
            for offset in 1..=default_at {
                next_block();
                assert_eq!(System::block_number(), open_block + offset);
                charged.extend(
                    charges_with_rate()
                        .into_iter()
                        .map(|(paid, rate)| (offset, paid, rate)),
                );
                if offset < default_at {
                    assert!(defaults().is_empty(), "early default, row {i}");
                    assert!(short(ALICE).is_some());
                }
            }
            assert_eq!(charged, expected, "charges, row {i}");
            assert_eq!(
                defaults(),
                vec![(ALICE, amount("residual"), amount("proceeds_to_pool"))],
                "default, row {i}"
            );
            assert_eq!(ResidualAlpha::<Test>::get(sn1()), amount("residual"));
            assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), 0);
            assert_eq!(burned_tao(), amount("burned"), "burned, row {i}");
            assert_eq!(
                FakePool::pool_tao(sn1()),
                amount("tao_after"),
                "pool TAO, row {i}"
            );
            assert!(short(ALICE).is_none());
            assert_eq!(total_balances(), 0, "custody is empty, row {i}");
            healthy();

            first_charge += usize::from(default_at == INTERVAL);
            later += usize::from(default_at > INTERVAL);
            convex += usize::from(expected.iter().any(|(_, _, rate)| *rate > apr));
        });
    }
    // Both outcomes of the first charge, and rates on the convex part of the curve.
    assert!(first_charge > 8 && later > 8 && convex > 4);
}
