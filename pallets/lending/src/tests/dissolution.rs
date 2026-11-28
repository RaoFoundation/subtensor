//! Shorts at subnet dissolution (`dissolution`, SPEC 7.4 phase E, P12, P16): the record the flag
//! leaves, the settlement at the dereg rate, the custody paid out, and the books it leaves.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used
)]

use frame_support::{assert_ok, weights::WeightMeter};
use subtensor_runtime_common::{NetUid, TaoBalance};

use super::mock::*;
use crate::{
    DissolvingSubnets, Due, Event, HolderPositions, InterestRetry, InterestTotals,
    LendingInterface, LendingPoolInterface, PositionCount, Positions, ResidualAlpha,
    SyntheticOutstanding, TotalPositions,
    dissolution::{P16_ZONE_SETTLEMENT, settle_short_amount, settle_short_amount_with},
    math::{
        dereg::{rate, short_owed},
        fixed::{Price, mul_price_floor, price},
    },
    params::RAO,
    types::{PositionId, Side},
};

const ALICE: AccountId = 1;
const BOB: AccountId = 2;
const CAROL: AccountId = 3;
const SN1: u16 = 1;
const SN2: u16 = 2;

fn sn1() -> NetUid {
    NetUid::from(SN1)
}

fn sn2() -> NetUid {
    NetUid::from(SN2)
}

fn short_of(who: AccountId) -> PositionId<AccountId> {
    (who, sn1(), Side::Short)
}

/// Subnet 1 with a 5,000 TAO / 50,000 alpha pool and lending on, everybody funded.
fn live() -> sp_io::TestExternalities {
    let mut ext = migrated_ext();
    ext.execute_with(|| {
        set_pool(SN1, 5_000 * RAO, 50_000 * RAO);
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_ok!(Lending::set_longs_enabled(RuntimeOrigin::root(), true));
        for who in [ALICE, BOB, CAROL] {
            set_balance(who, 100 * RAO);
        }
    });
    ext
}

/// Alice opens a short of 200 RAO of alpha on subnet 1 with 45 RAO of collateral.
fn open_short(who: AccountId) {
    assert_ok!(Lending::open_short(
        RuntimeOrigin::signed(who),
        sn1(),
        200 * RAO,
        45 * RAO,
        0
    ));
}

/// The flag of `pallet_subtensor`'s `do_dissolve_network`: the pool's TAO when it runs.
fn flag(netuid: NetUid) -> u64 {
    let pool = <FakePool as LendingPoolInterface<AccountId>>::pool_tao(netuid);
    <Lending as LendingInterface<AccountId>>::on_dissolve_flag(netuid, TaoBalance::from(pool));
    pool
}

/// Sets the emission EMA above any pool of these tests, so that it does not bind the dereg rate
/// (`min(T_flag, T_ema, T_A)`); the rate then reads `T_flag` and `T_A`.
fn ema_does_not_bind(netuid: NetUid) {
    crate::PoolTaoEma::<Test>::insert(netuid, u64::MAX);
}

/// The settlement of the cleanup's hook, metered on its own.
fn settle(netuid: NetUid) -> WeightMeter {
    let mut meter = WeightMeter::new();
    <Lending as LendingInterface<AccountId>>::on_dissolve(netuid, &mut meter);
    meter
}

/// The `ShortSettledOnDissolution` events of the current block, as `(who, price, paid)`.
fn settled_events() -> Vec<(AccountId, Price, u64)> {
    lending_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::ShortSettledOnDissolution {
                who, price, paid, ..
            } => Some((who, price, paid)),
            _ => None,
        })
        .collect()
}

/// The interest credited so far on subnet 1 (`InterestTotals`).
fn interest_charged() -> u128 {
    InterestTotals::<Test>::get(sn1()).charged
}

/// The free TAO balance of `who`.
fn tao_balance(who: AccountId) -> u64 {
    <FakePool as LendingPoolInterface<AccountId>>::tao_balance(&who)
}

/// Asserts the books are as they are with no position on the subnet: no position, custody
/// record, `Due` entry, retry count, holder entry, count or synthetic alpha is left for it.
/// `others` is the number of positions that stay open on every subnet (longs of the test).
fn assert_no_short_left(who: AccountId, others: u32) {
    let id = short_of(who);
    assert!(!Positions::<Test>::contains_key(id));
    assert!(!InterestRetry::<Test>::contains_key(id));
    assert!(!Due::<Test>::iter_keys().any(|(_, key)| key == id));
    assert!(!HolderPositions::<Test>::get(who).contains(&(sn1(), Side::Short)));
    assert_eq!(PositionCount::<Test>::get(sn1()), others);
    assert_eq!(TotalPositions::<Test>::get(), others);
    assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), 0);
    assert_eq!(ResidualAlpha::<Test>::get(sn1()), 0);
    assert_ok!(Lending::do_try_state());
}

#[test]
fn the_flag_records_the_pool_and_the_lending_alpha_of_a_subnet_with_shorts() {
    live().execute_with(|| {
        open_short(ALICE);
        let pool = <FakePool as LendingPoolInterface<AccountId>>::pool_tao(sn1());
        assert_eq!(flag(sn1()), pool);
        let record = DissolvingSubnets::<Test>::get(sn1()).expect("the flag records the subnet");
        assert_eq!(record.t_flag, pool);
        assert_eq!(record.lending_alpha, 200 * RAO);
        assert_eq!(record.settled_tao, 0);
    });
}

#[test]
fn a_subnet_with_no_lending_records_nothing_and_settles_nothing() {
    live().execute_with(|| {
        // Subnet 2 has no position, count, book or residual.
        flag(sn2());
        assert!(DissolvingSubnets::<Test>::get(sn2()).is_none());
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        settle(sn2());
        assert_eq!(before, sp_io::storage::root(sp_runtime::StateVersion::V1));
        assert!(settled_events().is_empty());
    });
}

#[test]
fn a_profitable_short_settles_at_the_dereg_rate_and_its_holdings_go_back() {
    live().execute_with(|| {
        open_short(ALICE);
        // A large issuance makes r small, so the short's holdings cover what it owes.
        set_issuance(SN1, 1_000_000 * RAO);
        let pos = Positions::<Test>::get(short_of(ALICE)).expect("the short is open");
        let alice_before = tao_balance(ALICE);
        let pool_at_flag = flag(sn1());
        let charged_at_flag = interest_charged();
        run_to(System::block_number() + 50);
        ema_does_not_bind(sn1());
        // The custody holds the collateral, the proceeds and the deposit as the settlement starts;
        // the settlement's own interest comes out of it first.
        let custody = Lending::position_custody(&short_of(ALICE), pos.opened_at);
        let custody_before = tao_balance(custody);
        let charged_before_settle = interest_charged();
        settle(sn1());
        // The settlement accrues the short to the block first, so that interest leaves custody too.
        let settle_interest = u64::try_from(interest_charged() - charged_before_settle).unwrap();

        let events = settled_events();
        assert_eq!(events.len(), 1);
        let (who, r, paid) = events[0];
        assert_eq!(who, ALICE);
        // Every interest credit since the flag reached the pool, so the pool the rate reads is the
        // flag's pool plus that interest: `T_A`, with `T_flag` the flag's pool.
        let charged = interest_charged() - charged_at_flag;
        assert!(
            charged > 0,
            "the test needs interest accrued before the settlement"
        );
        let t_a = pool_at_flag + u64::try_from(charged).unwrap();
        assert_eq!(
            r,
            rate(pool_at_flag, u64::MAX, t_a, 1_000_000 * u128::from(RAO))
        );
        // Profitable: the short pays what it owes, not its whole holdings.
        assert_eq!(paid, mul_price_floor(200 * RAO, r));
        assert!(
            paid < custody_before - pos.deposit,
            "holdings are collateral plus proceeds"
        );
        // The custody's rest (its holdings net of the payment, and the deposit) goes back to the
        // holder, and nothing is left in custody.
        assert_eq!(
            tao_balance(ALICE),
            alice_before + custody_before - settle_interest - paid
        );
        assert_eq!(tao_balance(custody), 0);
        // The pool keeps the interest and the payment.
        assert_eq!(
            <FakePool as LendingPoolInterface<AccountId>>::pool_tao(sn1()),
            t_a + paid
        );
        assert_no_short_left(ALICE, 0);
        assert_eq!(
            DissolvingSubnets::<Test>::get(sn1()).map(|rec| rec.settled_tao),
            Some(paid)
        );
    });
}

#[test]
fn a_losing_short_is_capped_at_its_holdings_and_the_event_reports_the_shortfall() {
    live().execute_with(|| {
        open_short(ALICE);
        // A one-rao issuance makes r large, so what the short owes exceeds its holdings.
        set_issuance(SN1, RAO);
        let pos = Positions::<Test>::get(short_of(ALICE)).expect("the short is open");
        let holdings = pos.collateral + pos.leg.proceeds();
        let alice_before = tao_balance(ALICE);
        flag(sn1());
        ema_does_not_bind(sn1());
        settle(sn1());

        let events = settled_events();
        assert_eq!(events.len(), 1);
        let (_, r, paid) = events[0];
        let owed = mul_price_floor(200 * RAO, r);
        assert!(
            owed > holdings,
            "the test needs a short that owes more than it holds"
        );
        assert_eq!(paid, holdings);
        assert!(paid < owed, "the shortfall is visible: paid < q * r");
        // Only the deposit comes back: the holdings all went into the pot.
        assert_eq!(tao_balance(ALICE), alice_before + pos.deposit);
        assert_no_short_left(ALICE, 0);
    });
}

#[test]
fn the_rate_reads_the_pool_after_the_interest_is_credited() {
    live().execute_with(|| {
        open_short(ALICE);
        set_issuance(SN1, 1_000_000 * RAO);
        let pool_at_flag = flag(sn1());
        let charged_at_flag = interest_charged();
        // The pool falls after the flag and before the settlement, so `T_A` binds the rate.
        set_pool(SN1, pool_at_flag - 500 * RAO, 50_000 * RAO);
        run_to(System::block_number() + 50);
        ema_does_not_bind(sn1());
        settle(sn1());

        let interest = u64::try_from(interest_charged() - charged_at_flag).unwrap();
        assert!(
            interest > 0,
            "the test needs interest accrued before the settlement"
        );
        let (_, r, paid) = settled_events()[0];
        // The rate is over the pool as it stood after the interest and before the payments.
        let pool_now = <FakePool as LendingPoolInterface<AccountId>>::pool_tao(sn1());
        let t_a = pool_now - paid;
        assert_eq!(t_a, pool_at_flag - 500 * RAO + interest);
        assert_eq!(
            r,
            rate(pool_at_flag, u64::MAX, t_a, 1_000_000 * u128::from(RAO))
        );
        assert!(t_a < pool_at_flag, "the pool after the fall binds the rate");
    });
}

#[test]
fn the_zone_switch_settles_both_ways_and_the_zone_part_is_isolated() {
    let r = price(1, 2);
    let q = 1_000u64;
    // Off: every short settles at r.
    assert_eq!(
        settle_short_amount_with(false, q, 600, 300, r),
        mul_price_floor(q, r)
    );
    // On: the zone-minted part settles at its proceeds, the rest at r.
    assert_eq!(
        settle_short_amount_with(true, q, 600, 300, r),
        short_owed(q, 600, 300, r)
    );
    assert_eq!(settle_short_amount_with(true, q, 600, 300, r), 300 + 200);
    // With no zone part (the positions carry none yet) the two settings agree.
    assert_eq!(
        settle_short_amount_with(true, q, 0, 0, r),
        settle_short_amount_with(false, q, 0, 0, r)
    );
    // The production amount is the one the build's switch names.
    assert_eq!(
        settle_short_amount(q, 600, 300, r),
        settle_short_amount_with(P16_ZONE_SETTLEMENT, q, 600, 300, r)
    );
}

#[test]
fn a_settled_short_is_settled_once() {
    live().execute_with(|| {
        open_short(ALICE);
        set_issuance(SN1, 1_000_000 * RAO);
        flag(sn1());
        settle(sn1());
        let first = DissolvingSubnets::<Test>::get(sn1()).expect("recorded");
        assert_eq!(settled_events().len(), 1);

        // A second settlement finds no short and pays nothing.
        System::reset_events();
        settle(sn1());
        assert!(settled_events().is_empty());
        assert_eq!(DissolvingSubnets::<Test>::get(sn1()), Some(first));
    });
}

#[test]
fn the_residual_alpha_is_cleared_and_counts_in_the_record() {
    live().execute_with(|| {
        open_short(ALICE);
        set_issuance(SN1, 1_000_000 * RAO);
        // A defaulted short's alpha that is still in circulation (`ResidualAlpha`).
        ResidualAlpha::<Test>::insert(sn1(), 7 * RAO);
        flag(sn1());
        let record = DissolvingSubnets::<Test>::get(sn1()).expect("recorded");
        assert_eq!(record.lending_alpha, 207 * RAO);
        settle(sn1());
        assert_eq!(ResidualAlpha::<Test>::get(sn1()), 0);
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), 0);
        assert_no_short_left(ALICE, 0);
    });
}

#[test]
fn the_meter_is_charged_for_every_short_settled() {
    live().execute_with(|| {
        open_short(ALICE);
        open_short(BOB);
        set_issuance(SN1, 1_000_000 * RAO);
        flag(sn1());
        let meter = settle(sn1());
        // The measured settlement of two shorts, with no other position on the pass, and more
        // for two shorts than for one.
        assert_eq!(meter.consumed(), Lending::dissolution_weight(2, 0));
        assert!(
            Lending::dissolution_weight(2, 0).ref_time()
                > Lending::dissolution_weight(1, 0).ref_time()
        );
    });
}

#[test]
fn longs_are_left_as_they_are() {
    live().execute_with(|| {
        open_short(ALICE);
        assert_ok!(Lending::open_long(
            RuntimeOrigin::signed(CAROL),
            sn1(),
            20 * RAO,
            12 * RAO,
            0
        ));
        set_issuance(SN1, 1_000_000 * RAO);
        flag(sn1());
        ema_does_not_bind(sn1());
        settle(sn1());
        // The short is settled; the long keeps its position and custody.
        assert_no_short_left(ALICE, 1);
        assert!(Positions::<Test>::contains_key((CAROL, sn1(), Side::Long)));
    });
}
