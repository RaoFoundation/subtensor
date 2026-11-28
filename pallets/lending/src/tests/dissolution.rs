//! Shorts at subnet dissolution (`dissolution`, SPEC 7.4 phase E, P12, P16): the record the flag
//! leaves, the settlement at the dereg rate, the custody paid out, and the books it leaves.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used
)]

use frame_support::{
    assert_noop, assert_ok,
    weights::{Weight, WeightMeter},
};
use subtensor_runtime_common::{NetUid, TaoBalance};

use super::mock::*;
use crate::{
    DissolutionSettlements, DissolvingSubnets, Due, Error, Event, HolderPositions, InterestRetry,
    InterestTotals, LendingInterface, LendingPoolInterface, PositionCount, Positions,
    PositionsByNetuid, ResidualAlpha, SettlementRetry, SyntheticOutstanding, TotalPositions,
    dissolution::{
        MAX_SETTLEMENTS_PER_BLOCK, P16_ZONE_SETTLEMENT, SettlementStage, settle_short_amount,
        settle_short_amount_with,
    },
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

/// One step of the cleanup's hook on `meter`: whether the settlement is done.
fn settle_step(netuid: NetUid, meter: &mut WeightMeter) -> bool {
    <Lending as LendingInterface<AccountId>>::on_dissolve(netuid, meter)
}

/// The settlement of the cleanup's hook in one step on a meter with no limit, which must
/// finish it. Returns the meter.
fn settle(netuid: NetUid) -> WeightMeter {
    let mut meter = WeightMeter::new();
    assert!(
        settle_step(netuid, &mut meter),
        "one unmetered step settles"
    );
    meter
}

/// A meter that affords exactly `visits` visits of a settlement step.
fn meter_for(visits: u32) -> WeightMeter {
    WeightMeter::with_limit(Lending::dissolution_weight(visits))
}

/// The holders in the per-subnet index of `netuid`'s shorts, sorted.
fn indexed_shorts(netuid: NetUid) -> Vec<AccountId> {
    let mut holders: Vec<_> =
        PositionsByNetuid::<Test>::iter_key_prefix((netuid, Side::Short)).collect();
    holders.sort_unstable();
    holders
}

/// The holders of `netuid`'s shorts by `Positions`, sorted.
fn open_shorts(netuid: NetUid) -> Vec<AccountId> {
    let mut holders: Vec<_> = Positions::<Test>::iter_keys()
        .filter(|(_, on, side)| *on == netuid && *side == Side::Short)
        .map(|(who, _, _)| who)
        .collect();
    holders.sort_unstable();
    holders
}

/// Makes subnet 2 a pool like subnet 1's (5,000 TAO / 50,000 alpha) with slow prices at its
/// spot, so shorts open on it as they do on subnet 1.
fn second_pool() {
    set_pool(SN2, 5_000 * RAO, 50_000 * RAO);
    seed_slow_prices(SN2, px(1, 10));
}

/// The custody account of the open short of `who` on subnet 1.
fn custody_of(who: AccountId) -> AccountId {
    let pos = Positions::<Test>::get(short_of(who)).expect("the short is open");
    Lending::position_custody(&short_of(who), pos.opened_at)
}

/// The `ShortSettlementFailed` events of the current block, as `(who, error)`.
fn failed_events() -> Vec<(AccountId, sp_runtime::DispatchError)> {
    lending_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::ShortSettlementFailed { who, error, .. } => Some((who, error)),
            _ => None,
        })
        .collect()
}

/// Everything TAO can be in the mock: free balances (holders and custody) and the pools.
fn all_tao() -> u128 {
    total_balances()
        + u128::from(<FakePool as LendingPoolInterface<AccountId>>::pool_tao(
            sn1(),
        ))
        + u128::from(<FakePool as LendingPoolInterface<AccountId>>::pool_tao(
            sn2(),
        ))
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
fn a_subnet_with_no_lending_is_flagged_and_settles_nothing() {
    live().execute_with(|| {
        // Subnet 2 has no position, count, book or residual. The flag is recorded all the same:
        // it is what closes the subnet to the short calls until the cleanup is done.
        let pool = flag(sn2());
        let record = DissolvingSubnets::<Test>::get(sn2()).expect("the flag records the subnet");
        assert_eq!(
            (record.t_flag, record.lending_alpha, record.settled_tao),
            (pool, 0, 0)
        );
        let meter = settle(sn2());
        assert_eq!(meter.consumed(), Lending::dissolution_weight(1));
        assert!(settled_events().is_empty());
        assert!(failed_events().is_empty());
        assert_eq!(DissolvingSubnets::<Test>::get(sn2()), Some(record));
        assert_ok!(Lending::do_try_state());
        // The end of the cleanup drops the flag and the progress.
        <Lending as LendingInterface<AccountId>>::on_dissolve_done(sn2());
        assert!(DissolvingSubnets::<Test>::get(sn2()).is_none());
        assert!(DissolutionSettlements::<Test>::get(sn2()).is_none());
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
fn the_meter_is_charged_for_every_visit() {
    live().execute_with(|| {
        open_short(ALICE);
        open_short(BOB);
        set_issuance(SN1, 1_000_000 * RAO);
        flag(sn1());
        let meter = settle(sn1());
        // Two shorts are four visits: each is accrued and then settled. More visits cost more.
        assert_eq!(meter.consumed(), Lending::dissolution_weight(4));
        assert!(
            Lending::dissolution_weight(4).ref_time() > Lending::dissolution_weight(2).ref_time()
        );
        // A step is charged for one visit at least.
        assert_eq!(
            Lending::dissolution_weight(0),
            Lending::dissolution_weight(1)
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

// The per-subnet index.

#[test]
fn the_index_holds_exactly_the_open_positions() {
    live().execute_with(|| {
        second_pool();
        assert!(indexed_shorts(sn1()).is_empty());
        open_short(ALICE);
        open_short(BOB);
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(CAROL),
            sn2(),
            200 * RAO,
            45 * RAO,
            0
        ));
        assert_ok!(Lending::open_long(
            RuntimeOrigin::signed(CAROL),
            sn1(),
            20 * RAO,
            12 * RAO,
            0
        ));
        assert_eq!(indexed_shorts(sn1()), vec![ALICE, BOB]);
        assert_eq!(indexed_shorts(sn1()), open_shorts(sn1()));
        assert_eq!(indexed_shorts(sn2()), vec![CAROL]);
        // The long is indexed on its own side.
        assert!(PositionsByNetuid::<Test>::contains_key((
            sn1(),
            Side::Long,
            CAROL
        )));
        assert_eq!(PositionsByNetuid::<Test>::iter_keys().count(), 4);
        assert_ok!(Lending::do_try_state());

        // An increase keeps the one key; a partial close keeps it; a full close removes it.
        assert_ok!(Lending::increase_short(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            10 * RAO,
            5 * RAO,
            0
        ));
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            10 * RAO,
            u64::MAX
        ));
        assert_eq!(indexed_shorts(sn1()), vec![ALICE, BOB]);
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            u64::MAX,
            u64::MAX
        ));
        assert_eq!(indexed_shorts(sn1()), vec![BOB]);
        assert_eq!(indexed_shorts(sn1()), open_shorts(sn1()));
        assert_eq!(PositionsByNetuid::<Test>::iter_keys().count(), 3);
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn a_coldkey_swap_moves_the_index_key() {
    live().execute_with(|| {
        open_short(ALICE);
        open_short(BOB);
        let dave: AccountId = 4;
        assert!(
            <Lending as LendingInterface<AccountId>>::coldkey_swap_destination_ok(&ALICE, &dave)
        );
        <Lending as LendingInterface<AccountId>>::on_coldkey_swap(&ALICE, &dave);
        assert_eq!(indexed_shorts(sn1()), vec![BOB, dave]);
        assert_eq!(indexed_shorts(sn1()), open_shorts(sn1()));
        assert!(SettlementRetry::<Test>::iter().next().is_none());
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn a_subnet_is_settled_without_walking_another_subnets_shorts() {
    live().execute_with(|| {
        second_pool();
        open_short(ALICE);
        open_short(BOB);
        // Three shorts on subnet 2, which is not dissolved.
        for who in [ALICE, BOB, CAROL] {
            assert_ok!(Lending::open_short(
                RuntimeOrigin::signed(who),
                sn2(),
                200 * RAO,
                45 * RAO,
                0
            ));
            // The per-block open limit: one block per pair of opens.
            next_block();
        }
        let others: Vec<_> = Positions::<Test>::iter()
            .filter(|((_, netuid, _), _)| *netuid == sn2())
            .collect();
        set_issuance(SN1, 1_000_000 * RAO);
        flag(sn1());
        // The meter affords the four visits of subnet 1's two shorts and nothing more: the
        // settlement finishes in it, so no visit went to subnet 2.
        let mut meter = meter_for(4);
        assert!(settle_step(sn1(), &mut meter));
        assert_eq!(meter.consumed(), Lending::dissolution_weight(4));
        assert_eq!(settled_events().len(), 2);
        assert!(open_shorts(sn1()).is_empty());
        assert!(indexed_shorts(sn1()).is_empty());
        // Subnet 2's shorts are as they were, in the index too.
        let after: Vec<_> = Positions::<Test>::iter()
            .filter(|((_, netuid, _), _)| *netuid == sn2())
            .collect();
        assert_eq!(after, others);
        assert_eq!(indexed_shorts(sn2()), vec![ALICE, BOB, CAROL]);
        assert_eq!(PositionCount::<Test>::get(sn2()), 3);
        assert_eq!(SyntheticOutstanding::<Test>::get(sn2()), 600 * RAO);
        assert_ok!(Lending::do_try_state());
    });
}

// The bounded, resumable walk.

/// Funds `count` more holders and opens a short for each, one a block (the per-block open
/// limit); returns all of them.
fn open_shorts_for(count: u128) -> Vec<AccountId> {
    let holders: Vec<AccountId> = (10..10 + count).collect();
    for who in &holders {
        set_balance(*who, 100 * RAO);
        open_short(*who);
        next_block();
    }
    holders
}

#[test]
fn a_large_book_is_settled_over_several_steps() {
    live().execute_with(|| {
        let holders = open_shorts_for(5);
        set_issuance(SN1, 1_000_000 * RAO);
        flag(sn1());
        run_to(System::block_number() + 20);
        ema_does_not_bind(sn1());
        let before = all_tao();
        let at = System::block_number();

        // Three visits a step: ten visits (five accruals, five settlements) take four steps.
        let mut steps = 0u32;
        let mut settled = Vec::new();
        let mut rates = Vec::new();
        loop {
            steps += 1;
            assert!(steps <= 10, "the settlement makes progress every step");
            let mut meter = meter_for(3);
            let done = settle_step(sn1(), &mut meter);
            assert!(meter.consumed().all_lte(Lending::dissolution_weight(3)));
            for (who, r, paid) in settled_events() {
                settled.push((who, paid));
                rates.push(r);
            }
            assert!(failed_events().is_empty());
            assert_ok!(Lending::do_try_state());
            let progress = DissolutionSettlements::<Test>::get(sn1()).expect("progress is kept");
            // Every step accrues to the block of the first, whatever block it runs in.
            assert_eq!(progress.at, at);
            if done {
                assert_eq!(progress.stage, SettlementStage::Done);
                break;
            }
            assert_ne!(progress.stage, SettlementStage::Done);
            // The shorts still open are exactly those not settled yet.
            assert_eq!(open_shorts(sn1()).len(), holders.len() - settled.len());
            assert_eq!(indexed_shorts(sn1()), open_shorts(sn1()));
            next_block();
        }
        assert_eq!(steps, 4);

        // Each short settled exactly once, all at the one rate.
        let mut who: Vec<_> = settled.iter().map(|(who, _)| *who).collect();
        who.sort_unstable();
        assert_eq!(who, holders);
        assert!(rates.iter().all(|r| *r == rates[0]));
        let paid: u64 = settled.iter().map(|(_, paid)| *paid).sum();
        assert!(paid > 0);
        assert_eq!(
            DissolvingSubnets::<Test>::get(sn1()).map(|rec| rec.settled_tao),
            Some(paid)
        );
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), 0);
        assert_eq!(TotalPositions::<Test>::get(), 0);
        for who in &holders {
            assert!(!Positions::<Test>::contains_key(short_of(*who)));
        }
        assert_eq!(all_tao(), before, "the settlement moves TAO and makes none");

        // A step after the end settles nothing.
        System::reset_events();
        assert!(settle_step(sn1(), &mut meter_for(1)));
        assert!(settled_events().is_empty());
    });
}

#[test]
fn a_step_never_visits_more_than_the_limit() {
    live().execute_with(|| {
        open_short(ALICE);
        set_issuance(SN1, 1_000_000 * RAO);
        flag(sn1());
        // A meter with no limit is still charged for at most the limit's visits.
        let meter = settle(sn1());
        assert!(
            meter
                .consumed()
                .all_lte(Lending::dissolution_weight(MAX_SETTLEMENTS_PER_BLOCK))
        );
    });
}

#[test]
fn a_meter_without_room_for_one_visit_changes_nothing() {
    live().execute_with(|| {
        open_short(ALICE);
        flag(sn1());
        let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
        let mut meter = WeightMeter::with_limit(Weight::from_parts(1, 1));
        assert!(!settle_step(sn1(), &mut meter));
        assert_eq!(meter.consumed(), Weight::zero());
        assert_eq!(root, sp_io::storage::root(sp_runtime::StateVersion::V1));
        assert_eq!(open_shorts(sn1()), vec![ALICE]);
    });
}

// The calls on a flagged subnet.

#[test]
fn no_short_call_runs_on_a_flagged_subnet() {
    live().execute_with(|| {
        open_short(ALICE);
        // Before the flag every call and quote works.
        assert_ok!(Lending::quote_open_short(sn1(), 200 * RAO));
        assert_ok!(Lending::quote_increase_short(&ALICE, sn1(), 10 * RAO));
        assert_ok!(Lending::quote_close_short(&ALICE, sn1(), 10 * RAO));
        flag(sn1());

        let closed = || Error::<Test>::DissolutionInProgress;
        assert_noop!(
            Lending::open_short(RuntimeOrigin::signed(BOB), sn1(), 200 * RAO, 45 * RAO, 0),
            closed()
        );
        assert_noop!(
            Lending::increase_short(RuntimeOrigin::signed(ALICE), sn1(), 10 * RAO, 5 * RAO, 0),
            closed()
        );
        assert_noop!(
            Lending::add_collateral(RuntimeOrigin::signed(ALICE), sn1(), RAO),
            closed()
        );
        assert_noop!(
            Lending::close_short(RuntimeOrigin::signed(ALICE), sn1(), 10 * RAO, u64::MAX),
            closed()
        );
        assert_noop!(
            Lending::close_short(RuntimeOrigin::signed(ALICE), sn1(), u64::MAX, u64::MAX),
            closed()
        );
        // The quotes report what the calls do.
        assert_eq!(
            Lending::quote_open_short(sn1(), 200 * RAO),
            Err(closed().into())
        );
        assert_eq!(
            Lending::quote_increase_short(&ALICE, sn1(), 10 * RAO),
            Err(closed().into())
        );
        assert_eq!(
            Lending::quote_close_short(&ALICE, sn1(), 10 * RAO),
            Err(closed().into())
        );
        // Another subnet is not closed.
        second_pool();
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(BOB),
            sn2(),
            200 * RAO,
            45 * RAO,
            0
        ));
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn the_calls_stay_closed_through_the_settlement_and_a_short_is_paid_once() {
    live().execute_with(|| {
        open_short(ALICE);
        open_short(BOB);
        set_issuance(SN1, 1_000_000 * RAO);
        let closed = || Error::<Test>::DissolutionInProgress;
        let close = |who: AccountId| {
            Lending::close_short(RuntimeOrigin::signed(who), sn1(), u64::MAX, u64::MAX)
        };
        let alice_before = tao_balance(ALICE);
        let custody = custody_of(ALICE);
        let custody_before = tao_balance(custody);
        let before = all_tao();
        flag(sn1());
        ema_does_not_bind(sn1());
        assert_noop!(close(ALICE), closed());

        // Mid-settlement: the two shorts are accrued and one of them is settled.
        assert!(!settle_step(sn1(), &mut meter_for(3)));
        assert_eq!(settled_events().len(), 1);
        for who in [ALICE, BOB] {
            assert_noop!(close(who), closed());
        }
        assert!(settle_step(sn1(), &mut meter_for(3)));
        // After the settlement the short is gone, and the subnet is closed until the cleanup
        // ends: no close can pay the holder a second time.
        for who in [ALICE, BOB] {
            assert_noop!(close(who), closed());
        }

        // Alice was paid by exactly one path: the settlement's refund of her custody.
        let paid_by_alice: Vec<u64> = lending_events()
            .into_iter()
            .filter_map(|event| match event {
                Event::ShortSettledOnDissolution { who, paid, .. } if who == ALICE => Some(paid),
                _ => None,
            })
            .collect();
        assert_eq!(paid_by_alice.len(), 1);
        assert_eq!(
            tao_balance(ALICE),
            alice_before + custody_before - paid_by_alice[0]
        );
        assert_eq!(tao_balance(custody), 0);
        assert_eq!(all_tao(), before);
        assert_no_short_left(ALICE, 0);
        assert!(indexed_shorts(sn1()).is_empty());
    });
}

#[test]
fn a_scheduled_charge_takes_nothing_on_a_flagged_subnet() {
    live().execute_with(|| {
        open_short(ALICE);
        set_issuance(SN1, 1_000_000 * RAO);
        let pos = Positions::<Test>::get(short_of(ALICE)).expect("open");
        // A short with no collateral left would default at its next charge.
        let custody = custody_of(ALICE);
        set_balance(custody, pos.leg.proceeds() + pos.deposit);
        Positions::<Test>::insert(
            short_of(ALICE),
            crate::types::Position {
                collateral: 0,
                ..pos
            },
        );
        flag(sn1());
        let charged = interest_charged();
        run_to(pos.charge_at + 1);

        // No charge and no default: the short waits for the settlement, with one charge queued.
        let waiting = Positions::<Test>::get(short_of(ALICE)).expect("still open");
        assert_eq!(interest_charged(), charged);
        assert_eq!(waiting.leg, pos.leg);
        assert!(waiting.charge_at > pos.charge_at);
        assert_eq!(
            Due::<Test>::iter_keys()
                .filter(|(_, key)| *key == short_of(ALICE))
                .count(),
            1
        );
        assert_eq!(ResidualAlpha::<Test>::get(sn1()), 0);
        assert_ok!(Lending::do_try_state());
        // The settlement then settles it.
        ema_does_not_bind(sn1());
        settle(sn1());
        assert_eq!(settled_events().len(), 1);
        assert_no_short_left(ALICE, 0);
    });
}

// Failures.

#[test]
fn a_failed_settlement_is_reported_queued_and_tried_again() {
    live().execute_with(|| {
        open_short(ALICE);
        open_short(BOB);
        set_issuance(SN1, 1_000_000 * RAO);
        let alice_before = tao_balance(ALICE);
        let custody = custody_of(ALICE);
        let custody_before = tao_balance(custody);
        let before = all_tao();
        flag(sn1());
        ema_does_not_bind(sn1());
        // Alice's custody cannot pay: her settlement fails, Bob's does not.
        set_tao_frozen(custody, true);

        let mut meter = WeightMeter::new();
        assert!(!settle_step(sn1(), &mut meter));
        let failed = failed_events();
        assert!(!failed.is_empty());
        assert!(failed.iter().all(|(who, _)| *who == ALICE));
        assert_eq!(
            settled_events().iter().map(|e| e.0).collect::<Vec<_>>(),
            vec![BOB]
        );
        // The failed short is as it was, and queued.
        assert_eq!(open_shorts(sn1()), vec![ALICE]);
        assert_eq!(indexed_shorts(sn1()), vec![ALICE]);
        assert_eq!(tao_balance(custody), custody_before);
        assert_eq!(SyntheticOutstanding::<Test>::get(sn1()), 200 * RAO);
        let attempts = SettlementRetry::<Test>::get(sn1(), ALICE).expect("queued");
        assert!(attempts >= 1);
        assert_eq!(
            DissolutionSettlements::<Test>::get(sn1()).map(|p| p.stage),
            Some(SettlementStage::Retrying)
        );
        let bob_paid = settled_events()[0].2;
        assert_eq!(
            DissolvingSubnets::<Test>::get(sn1()).map(|rec| rec.settled_tao),
            Some(bob_paid)
        );
        assert_ok!(Lending::do_try_state());

        // A later step tries it again and, while the cause stands, reports it again.
        next_block();
        assert!(!settle_step(sn1(), &mut WeightMeter::new()));
        assert_eq!(failed_events().len(), 1);
        assert!(settled_events().is_empty());
        assert_eq!(
            SettlementRetry::<Test>::get(sn1(), ALICE),
            Some(attempts + 1)
        );
        assert_eq!(open_shorts(sn1()), vec![ALICE]);
        assert_ok!(Lending::do_try_state());

        // With the cause removed the next step settles it, once, at the rate read before.
        set_tao_frozen(custody, false);
        next_block();
        assert!(settle_step(sn1(), &mut WeightMeter::new()));
        assert!(failed_events().is_empty());
        let events = settled_events();
        assert_eq!(events.len(), 1);
        let (who, r, alice_paid) = events[0];
        assert_eq!(who, ALICE);
        assert_eq!(
            Some(r),
            DissolutionSettlements::<Test>::get(sn1()).map(|p| p.rate)
        );
        assert!(SettlementRetry::<Test>::iter().next().is_none());
        assert_eq!(tao_balance(custody), 0);
        assert_eq!(
            tao_balance(ALICE),
            alice_before + custody_before - alice_paid
        );
        // I8: the pot got each short's payment once.
        assert_eq!(
            DissolvingSubnets::<Test>::get(sn1()).map(|rec| rec.settled_tao),
            Some(bob_paid + alice_paid)
        );
        assert_eq!(all_tao(), before);
        assert_no_short_left(ALICE, 0);

        // Done stays done.
        System::reset_events();
        assert!(settle_step(sn1(), &mut WeightMeter::new()));
        assert!(settled_events().is_empty());
    });
}

#[test]
fn a_short_that_cannot_settle_holds_the_settlement_open() {
    live().execute_with(|| {
        open_short(ALICE);
        set_issuance(SN1, 1_000_000 * RAO);
        flag(sn1());
        // The terminal case: the custody can never pay.
        set_tao_frozen(custody_of(ALICE), true);
        for round in 1..=3u32 {
            assert!(!settle_step(sn1(), &mut WeightMeter::new()));
            assert_eq!(failed_events().len(), 1, "reported on every step");
            assert_eq!(SettlementRetry::<Test>::get(sn1(), ALICE), Some(round));
            assert_eq!(open_shorts(sn1()), vec![ALICE]);
            assert_eq!(
                DissolvingSubnets::<Test>::get(sn1()).map(|rec| rec.settled_tao),
                Some(0)
            );
            assert_ok!(Lending::do_try_state());
            next_block();
        }
    });
}

#[test]
fn a_short_rekeyed_during_the_settlement_is_still_settled_once() {
    live().execute_with(|| {
        let holders = open_shorts_for(4);
        set_issuance(SN1, 1_000_000 * RAO);
        flag(sn1());
        ema_does_not_bind(sn1());
        // Part of the walk has run when one holder's coldkey is swapped.
        assert!(!settle_step(sn1(), &mut meter_for(6)));
        let left = open_shorts(sn1());
        assert_eq!(left.len(), 2);
        let (old, new): (AccountId, AccountId) = (left[0], 99);
        assert!(<Lending as LendingInterface<AccountId>>::coldkey_swap_destination_ok(&old, &new));
        <Lending as LendingInterface<AccountId>>::on_coldkey_swap(&old, &new);
        // The moved short is queued, wherever its new key falls against the cursor.
        assert_eq!(SettlementRetry::<Test>::get(sn1(), new), Some(0));
        assert!(!SettlementRetry::<Test>::contains_key(sn1(), old));
        assert_eq!(indexed_shorts(sn1()), open_shorts(sn1()));
        assert_ok!(Lending::do_try_state());

        let mut settled: Vec<AccountId> = Vec::new();
        for _ in 0..4 {
            System::reset_events();
            let done = settle_step(sn1(), &mut WeightMeter::new());
            settled.extend(settled_events().iter().map(|e| e.0));
            assert!(failed_events().is_empty());
            assert_ok!(Lending::do_try_state());
            if done {
                break;
            }
        }
        settled.sort_unstable();
        let mut expected = vec![left[1], new];
        expected.sort_unstable();
        assert_eq!(settled, expected);
        assert!(open_shorts(sn1()).is_empty());
        assert!(SettlementRetry::<Test>::iter().next().is_none());
        assert_eq!(
            DissolutionSettlements::<Test>::get(sn1()).map(|p| p.stage),
            Some(SettlementStage::Done)
        );
        assert_eq!(holders.len(), 4);
        assert_eq!(TotalPositions::<Test>::get(), 0);
    });
}

/// The `ShortSettlementForced` events of the current block, as `(who, price, paid)`.
fn forced_events() -> Vec<(AccountId, Price, u64)> {
    lending_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::ShortSettlementForced {
                who, price, paid, ..
            } => Some((who, price, paid)),
            _ => None,
        })
        .collect()
}

/// The `ShortSettlementWrittenOff` events of the current block, as
/// `(who, paid, refunded, left_in_custody, custody)`.
fn written_off_events() -> Vec<(AccountId, u64, u64, u64, AccountId)> {
    lending_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::ShortSettlementWrittenOff {
                who,
                paid,
                refunded,
                left_in_custody,
                custody,
                ..
            } => Some((who, paid, refunded, left_in_custody, custody)),
            _ => None,
        })
        .collect()
}

/// Alice and Bob short subnet 1, the subnet is flagged, and Alice's custody is frozen: Bob is
/// settled and Alice holds the settlement open over `steps` steps. Returns Alice's custody
/// account and what Bob paid.
fn stuck_on_alice(steps: u32) -> (AccountId, u64) {
    open_short(ALICE);
    open_short(BOB);
    set_issuance(SN1, 1_000_000 * RAO);
    let custody = custody_of(ALICE);
    flag(sn1());
    ema_does_not_bind(sn1());
    set_tao_frozen(custody, true);
    assert!(!settle_step(sn1(), &mut WeightMeter::new()));
    let bob_paid = settled_events()[0].2;
    for _ in 1..steps {
        next_block();
        assert!(!settle_step(sn1(), &mut WeightMeter::new()));
        assert_eq!(failed_events().len(), 1, "reported on every step");
    }
    assert_eq!(open_shorts(sn1()), vec![ALICE]);
    assert_eq!(SettlementRetry::<Test>::get(sn1(), ALICE), Some(steps));
    (custody, bob_paid)
}

#[test]
fn root_settles_a_stuck_short_and_the_settlement_finishes() {
    live().execute_with(|| {
        let before = all_tao();
        let (custody, bob_paid) = stuck_on_alice(3);
        let alice_before = tao_balance(ALICE);
        let custody_before = tao_balance(custody);
        let r = DissolutionSettlements::<Test>::get(sn1())
            .expect("in progress")
            .rate;

        // While the cause stands, the call without the write-off fails with the settlement's
        // error and changes nothing.
        next_block();
        assert_noop!(
            Lending::force_settle_short(RuntimeOrigin::root(), sn1(), ALICE, false),
            sp_runtime::DispatchError::Other("mock: account frozen")
        );
        assert_eq!(SettlementRetry::<Test>::get(sn1(), ALICE), Some(3));
        assert_eq!(tao_balance(custody), custody_before);

        // With the cause removed, root settles it in the call, at the recorded rate.
        set_tao_frozen(custody, false);
        assert_ok!(Lending::force_settle_short(
            RuntimeOrigin::root(),
            sn1(),
            ALICE,
            false
        ));
        let forced = forced_events();
        assert_eq!(forced.len(), 1);
        let (who, price, alice_paid) = forced[0];
        assert_eq!((who, price), (ALICE, r));
        // It is the normal settlement: its own event is there too, with the same payment.
        assert_eq!(settled_events(), vec![(ALICE, r, alice_paid)]);
        assert!(written_off_events().is_empty());
        assert_eq!(
            alice_paid,
            settle_short_amount(200 * RAO, 0, 0, r).min(custody_before)
        );
        assert_eq!(tao_balance(custody), 0);
        assert_eq!(
            tao_balance(ALICE),
            alice_before + custody_before - alice_paid
        );
        // The queue entry and the index key went with the position.
        assert!(SettlementRetry::<Test>::iter().next().is_none());
        assert!(indexed_shorts(sn1()).is_empty());
        // I8: the pot holds each short's payment once.
        assert_eq!(
            DissolvingSubnets::<Test>::get(sn1()).map(|rec| rec.settled_tao),
            Some(bob_paid + alice_paid)
        );
        assert_eq!(all_tao(), before);
        assert_no_short_left(ALICE, 0);

        // No second payment: the call needs the queue entry.
        assert_noop!(
            Lending::force_settle_short(RuntimeOrigin::root(), sn1(), ALICE, true),
            Error::<Test>::ShortNotStuck
        );

        // The next cleanup step finds nothing left and is done, and pays nothing more.
        next_block();
        assert!(settle_step(sn1(), &mut WeightMeter::new()));
        assert!(settled_events().is_empty());
        assert!(failed_events().is_empty());
        assert_eq!(
            DissolvingSubnets::<Test>::get(sn1()).map(|rec| rec.settled_tao),
            Some(bob_paid + alice_paid)
        );
        assert_eq!(all_tao(), before);
    });
}

#[test]
fn root_writes_off_a_short_that_can_never_settle() {
    live().execute_with(|| {
        let before = all_tao();
        let (custody, bob_paid) = stuck_on_alice(2);
        let alice_before = tao_balance(ALICE);
        let custody_before = tao_balance(custody);
        let pool_before = <FakePool as LendingPoolInterface<AccountId>>::pool_tao(sn1());

        // The custody can never pay: the settlement fails, and with `write_off` the short is
        // removed without it. Nothing can leave the custody, so everything stays there.
        next_block();
        assert_ok!(Lending::force_settle_short(
            RuntimeOrigin::root(),
            sn1(),
            ALICE,
            true
        ));
        assert_eq!(
            written_off_events(),
            vec![(ALICE, 0, 0, custody_before, custody)]
        );
        assert!(forced_events().is_empty());
        assert!(settled_events().is_empty());
        assert_eq!(tao_balance(custody), custody_before);
        assert_eq!(tao_balance(ALICE), alice_before);
        assert_eq!(
            <FakePool as LendingPoolInterface<AccountId>>::pool_tao(sn1()),
            pool_before
        );
        assert_eq!(
            DissolvingSubnets::<Test>::get(sn1()).map(|rec| rec.settled_tao),
            Some(bob_paid)
        );
        assert!(SettlementRetry::<Test>::iter().next().is_none());
        assert!(indexed_shorts(sn1()).is_empty());
        assert_eq!(all_tao(), before, "TAO is only moved");
        assert_no_short_left(ALICE, 0);

        assert_noop!(
            Lending::force_settle_short(RuntimeOrigin::root(), sn1(), ALICE, true),
            Error::<Test>::ShortNotStuck
        );
        next_block();
        assert!(settle_step(sn1(), &mut WeightMeter::new()));
        assert!(settled_events().is_empty());
        assert_eq!(all_tao(), before);
    });
}

#[test]
fn a_write_off_pays_the_pot_first_from_what_the_custody_holds() {
    live().execute_with(|| {
        open_short(ALICE);
        set_issuance(SN1, 1_000_000 * RAO);
        let custody = custody_of(ALICE);
        flag(sn1());
        ema_does_not_bind(sn1());
        // The rate of this settlement, from the same reads the step makes.
        let pool = <FakePool as LendingPoolInterface<AccountId>>::pool_tao(sn1());
        let owed = settle_short_amount(
            200 * RAO,
            0,
            0,
            rate(pool, u64::MAX, pool, u128::from(1_000_000 * RAO)),
        );
        // The custody lost most of the TAO the position records: it holds less than the short
        // owes the pot, so the settlement's first payment fails. (The mock's balances are not
        // rolled back with storage, so the failure has to be at the first debit.)
        let held = owed / 2;
        assert!(held > 0);
        set_balance(custody, held);
        let alice_before = tao_balance(ALICE);
        let before = all_tao();
        assert!(!settle_step(sn1(), &mut WeightMeter::new()));
        assert_eq!(failed_events().len(), 1);
        assert_eq!(
            DissolutionSettlements::<Test>::get(sn1()).map(|p| p.rate),
            Some(rate(pool, u64::MAX, pool, u128::from(1_000_000 * RAO)))
        );

        assert_noop!(
            Lending::force_settle_short(RuntimeOrigin::root(), sn1(), ALICE, false),
            sp_runtime::DispatchError::Other("mock: insufficient TAO")
        );
        assert_ok!(Lending::force_settle_short(
            RuntimeOrigin::root(),
            sn1(),
            ALICE,
            true
        ));
        // Everything the custody holds goes to the pot, which is owed more; the holder, who
        // comes second, gets nothing; nothing stays behind.
        assert_eq!(written_off_events(), vec![(ALICE, held, 0, 0, custody)]);
        assert_eq!(tao_balance(custody), 0);
        assert_eq!(tao_balance(ALICE), alice_before);
        assert_eq!(
            <FakePool as LendingPoolInterface<AccountId>>::pool_tao(sn1()),
            pool + held
        );
        assert_eq!(
            DissolvingSubnets::<Test>::get(sn1()).map(|rec| rec.settled_tao),
            Some(held)
        );
        assert_eq!(all_tao(), before, "TAO is only moved");
        assert_no_short_left(ALICE, 0);
        next_block();
        assert!(settle_step(sn1(), &mut WeightMeter::new()));
    });
}

#[test]
fn force_settle_is_root_only_and_only_for_a_stuck_short() {
    live().execute_with(|| {
        // Not root.
        assert_noop!(
            Lending::force_settle_short(RuntimeOrigin::signed(ALICE), sn1(), ALICE, true),
            sp_runtime::DispatchError::BadOrigin
        );
        // An open short of a live subnet is not stuck.
        open_short(ALICE);
        open_short(BOB);
        assert_noop!(
            Lending::force_settle_short(RuntimeOrigin::root(), sn1(), ALICE, true),
            Error::<Test>::ShortNotStuck
        );
        // Queued, but the settlement has not read its rate: there is nothing to settle at.
        set_issuance(SN1, 1_000_000 * RAO);
        flag(sn1());
        ema_does_not_bind(sn1());
        SettlementRetry::<Test>::insert(sn1(), ALICE, 1);
        assert_noop!(
            Lending::force_settle_short(RuntimeOrigin::root(), sn1(), ALICE, true),
            Error::<Test>::SettlementRateNotRead
        );
        SettlementRetry::<Test>::remove(sn1(), ALICE);

        // One stuck short, one that settles: the call reaches the stuck one only.
        let custody = custody_of(ALICE);
        set_tao_frozen(custody, true);
        assert!(!settle_step(sn1(), &mut WeightMeter::new()));
        assert_eq!(open_shorts(sn1()), vec![ALICE]);
        assert_noop!(
            Lending::force_settle_short(RuntimeOrigin::root(), sn1(), BOB, true),
            Error::<Test>::ShortNotStuck
        );
        assert_noop!(
            Lending::force_settle_short(RuntimeOrigin::root(), sn2(), ALICE, true),
            Error::<Test>::ShortNotStuck
        );
        assert_eq!(open_shorts(sn1()), vec![ALICE]);
        assert_ok!(Lending::do_try_state());
    });
}
