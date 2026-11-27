//! Interest charging through the `Due` queue (SPEC section 1 item 2, P3, P4, P15, I5, I19):
//! the cadence, the carried remainder, parameter changes, the collateral cap, the bounded
//! per-block budget, retries, and a differential against the reference model's arithmetic.
//!
//! Expected amounts are computed here with the closed form of `model/v201/core.py`
//! (`_accrue`, `_charge`) in plain integers, not with the pallet's own math.

use frame_support::{assert_ok, dispatch::DispatchResult};
use sp_runtime::TryRuntimeError;
use subtensor_runtime_common::NetUid;

use super::mock::*;
use crate::{
    Due, Event, InterestRetry, InterestTotals, LendingPoolInterface, NextDue, PositionCount,
    Positions, TotalPositions,
    interest::RETRY_DELAY_BLOCKS,
    params::{LendingParams, RAO},
    types::{Leg, Position, PositionId, Side},
};

const ALICE: AccountId = 1;
const BOB: AccountId = 2;
const SN1: u16 = 1;
/// The launch APR and cadence (SPEC P3, P15).
const APR: u128 = 2_000;
const INTERVAL: u64 = 360;
/// The model's `BPS * BPY`: the denominator of one block of interest.
const DEN: u128 = 10_000 * 7_200 * 365;
/// The debt of the long the tests open.
const DEBT: u64 = 20 * RAO;

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

fn id(who: AccountId, side: Side) -> PositionId<AccountId> {
    (who, sn1(), side)
}

fn position(who: AccountId, side: Side) -> Position<u64> {
    Positions::<Test>::get(id(who, side)).expect("position is open")
}

fn custody(who: AccountId, side: Side) -> AccountId {
    Lending::custody_of(side, &who, sn1(), position(who, side).opened_at)
}

/// A short of 200 alpha: about 20 TAO of notional at spot 0.1.
fn open_short(who: AccountId) -> DispatchResult {
    Lending::open_short(RuntimeOrigin::signed(who), sn1(), alpha(200), tao(45), 0)
}

/// A long with `DEBT` lifted; its notional is the debt, whatever the price does.
fn open_long(who: AccountId) -> DispatchResult {
    Lending::open_long(RuntimeOrigin::signed(who), sn1(), DEBT, tao(12), 0)
}

fn healthy() {
    assert_ok!(Lending::do_try_state());
}

/// The interest charges of the current block: holder, side and amount.
fn charges() -> Vec<(AccountId, Side, u64)> {
    lending_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::InterestCharged {
                who, side, to_pool, ..
            } => Some((who, side, to_pool)),
            _ => None,
        })
        .collect()
}

/// The one charge of the current block.
fn charge() -> u64 {
    match charges().as_slice() {
        [(_, _, amount)] => *amount,
        other => panic!("one charge expected, found {other:?}"),
    }
}

/// The model's `_accrue`: the whole rao owed over `blocks` and the remainder carried on.
fn model_accrue(notional: u64, apr: u128, blocks: u64, remainder: u128) -> (u64, u128) {
    let num = u128::from(notional) * apr * u128::from(blocks) + remainder;
    (u64::try_from(num / DEN).expect("fits"), num % DEN)
}

/// The model's short notional at the TWAP the pallet reads now: `floor(q * TWAP_median)`.
fn short_notional(q: u64) -> u64 {
    let twap = Lending::price_refs(sn1()).expect("rings are seeded").twap;
    u64::try_from((u128::from(q) * twap.to_bits()) >> 64).expect("fits")
}

fn set_lending(change: impl FnOnce(&mut LendingParams)) {
    let mut params = LendingParams::default();
    change(&mut params);
    assert_ok!(Lending::set_lending_params(RuntimeOrigin::root(), params));
}

/// Runs `fault` on a state with one open short and expects try-state to fail with `message`.
fn assert_fault(message: &'static str, fault: impl FnOnce()) {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        healthy();
        fault();
        assert_eq!(
            Lending::do_try_state(),
            Err(TryRuntimeError::Other(message))
        );
    });
}

#[test]
fn both_sides_are_charged_every_360_blocks_from_the_open() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        assert_ok!(open_long(BOB));
        let opened = System::block_number();
        let first = opened + INTERVAL;
        for (who, side) in [(ALICE, Side::Short), (BOB, Side::Long)] {
            assert_eq!(position(who, side).charge_at, first);
            assert!(Due::<Test>::contains_key(first, id(who, side)));
        }
        assert_eq!(NextDue::<Test>::get(), Some(first));

        // Nothing is charged before the interval has passed.
        run_to(first - 1);
        assert_eq!(InterestTotals::<Test>::get(sn1()).charged, 0);
        healthy();

        let short_c = position(ALICE, Side::Short).collateral;
        let long_c = position(BOB, Side::Long).collateral;
        let pool = FakePool::pool_tao(sn1());
        let alpha_in = FakePool::subnet_alpha_in(sn1());
        let issuance = FakePool::alpha_issuance(sn1());
        run_to(first);
        let (short_owed, _) = model_accrue(short_notional(alpha(200)), APR, INTERVAL, 0);
        let (long_owed, _) = model_accrue(DEBT, APR, INTERVAL, 0);
        let mut charged = charges();
        charged.sort();
        assert_eq!(
            charged,
            vec![
                (ALICE, Side::Short, short_owed),
                (BOB, Side::Long, long_owed)
            ]
        );
        assert_eq!(
            position(ALICE, Side::Short).collateral,
            short_c - short_owed
        );
        assert_eq!(position(BOB, Side::Long).collateral, long_c - long_owed);
        // I19: the pool's TAO rises by exactly the interest, and nothing else moves.
        assert_eq!(FakePool::pool_tao(sn1()), pool + short_owed + long_owed);
        assert_eq!(FakePool::subnet_alpha_in(sn1()), alpha_in);
        assert_eq!(FakePool::alpha_issuance(sn1()), issuance);
        let total = InterestTotals::<Test>::get(sn1());
        assert_eq!(total.charged, u128::from(short_owed + long_owed));
        assert_eq!(total.to_pool, total.charged);

        // The next charge is one interval after this one, and none comes in between.
        for (who, side) in [(ALICE, Side::Short), (BOB, Side::Long)] {
            assert_eq!(position(who, side).charge_at, first + INTERVAL);
            assert!(!Due::<Test>::contains_key(first, id(who, side)));
            assert!(Due::<Test>::contains_key(first + INTERVAL, id(who, side)));
        }
        healthy();
        run_to(first + INTERVAL - 1);
        assert_eq!(InterestTotals::<Test>::get(sn1()).charged, total.charged);
        run_to(first + INTERVAL);
        assert_eq!(charges().len(), 2);
        healthy();
    });
}

/// Charges a long every `interval` blocks, `count` times, and checks that the sum equals the
/// closed form over the whole span to the rao.
fn charges_sum_to_the_closed_form(interval: u32, count: u64) {
    live().execute_with(|| {
        set_lending(|p| p.charge_interval_blocks = interval);
        next_block();
        assert_ok!(open_long(ALICE));
        let opened = System::block_number();
        let collateral = position(ALICE, Side::Long).collateral;
        let pool = FakePool::pool_tao(sn1());
        let mut sum = 0u64;
        for k in 1..=count {
            run_to(opened + k * u64::from(interval));
            sum += charge();
        }
        let span = count * u64::from(interval);
        let (total, remainder) = model_accrue(DEBT, APR, span, 0);
        assert_eq!(sum, total);
        // Flooring each charge on its own would have lost the carried fractions.
        assert!(count * model_accrue(DEBT, APR, u64::from(interval), 0).0 < total);
        let pos = position(ALICE, Side::Long);
        assert_eq!(pos.collateral, collateral - total);
        assert_eq!(pos.accrual.remainder, remainder);
        assert_eq!(pos.accrual.due, 0);
        assert_eq!(FakePool::pool_tao(sn1()), pool + total);
        assert_eq!(
            InterestTotals::<Test>::get(sn1()).charged,
            u128::from(total)
        );
        healthy();
    });
}

#[test]
fn per_block_charges_carry_the_remainder_to_the_closed_form_total() {
    charges_sum_to_the_closed_form(1, 5_000);
}

#[test]
fn charges_of_an_odd_interval_carry_the_remainder_to_the_closed_form_total() {
    charges_sum_to_the_closed_form(7, 1_000);
}

#[test]
fn an_apr_change_mid_period_applies_from_the_next_accrual() {
    live().execute_with(|| {
        assert_ok!(open_long(ALICE));
        assert_ok!(open_long(BOB));
        // Bob's long accrues at block 180 by a touch; Alice's is not accrued until its charge.
        run_to(180);
        assert_ok!(Lending::add_long_collateral(
            RuntimeOrigin::signed(BOB),
            sn1(),
            tao(1)
        ));
        let (bob_first, bob_rem) = model_accrue(DEBT, APR, 179, 0);
        assert_eq!(charges(), vec![(BOB, Side::Long, bob_first)]);
        set_lending(|p| p.apr_bps = 4_000);

        run_to(361);
        // Alice: the whole period accrues at the rate live at the accrual (the model's `_accrue`).
        let (alice_owed, _) = model_accrue(DEBT, 4_000, 360, 0);
        // Bob: 179 blocks were accrued at 20 %; the 181 since accrue at 40 %.
        let (bob_owed, _) = model_accrue(DEBT, 4_000, 181, bob_rem);
        let mut charged = charges();
        charged.sort();
        assert_eq!(
            charged,
            vec![(ALICE, Side::Long, alice_owed), (BOB, Side::Long, bob_owed)]
        );
        healthy();
    });
}

#[test]
fn a_charge_interval_change_applies_from_the_next_scheduled_charge() {
    live().execute_with(|| {
        assert_ok!(open_long(ALICE));
        run_to(100);
        set_lending(|p| p.charge_interval_blocks = 100);
        // The queued entry is not moved: the first charge stays at block 361.
        run_to(360);
        assert_eq!(InterestTotals::<Test>::get(sn1()).charged, 0);
        assert_eq!(position(ALICE, Side::Long).charge_at, 361);
        assert!(Due::<Test>::contains_key(361, id(ALICE, Side::Long)));
        run_to(361);
        assert_eq!(charge(), model_accrue(DEBT, APR, 360, 0).0);
        // From that charge on, the new interval schedules on the position's grid, `1 + k * 100`
        // (the open is at block 1): the first grid block after 361 is 401, not 461.
        assert_eq!(position(ALICE, Side::Long).charge_at, 401);
        run_to(400);
        assert!(charges().is_empty());
        run_to(401);
        assert_eq!(charges().len(), 1);
        assert_eq!(position(ALICE, Side::Long).charge_at, 501);
        healthy();
    });
}

/// Leaves `collateral` rao in a position and its custody, as after a long bleed.
fn leave_collateral(who: AccountId, side: Side, collateral: u64) {
    let pos = position(who, side);
    set_balance(
        custody(who, side),
        collateral + pos.leg.proceeds() + pos.deposit,
    );
    Positions::<Test>::insert(id(who, side), Position { collateral, ..pos });
}

/// The charge is capped at the collateral (SPEC I5); what is left unpaid stays due and is taken
/// once collateral is added.
fn charge_is_capped_at_the_collateral(side: Side, open: fn(AccountId) -> DispatchResult) {
    live().execute_with(|| {
        assert_ok!(open(ALICE));
        leave_collateral(ALICE, side, 1_000);
        let notional = |pos: &Position<u64>| match pos.leg {
            Leg::Short { minted, .. } => short_notional(minted),
            Leg::Long { debt, .. } => debt,
        };
        let pool = FakePool::pool_tao(sn1());

        run_to(361);
        let (owed, remainder) = model_accrue(notional(&position(ALICE, side)), APR, 360, 0);
        assert!(owed > 1_000);
        assert_eq!(charges(), vec![(ALICE, side, 1_000)]);
        let pos = position(ALICE, side);
        assert_eq!(pos.collateral, 0);
        assert_eq!(pos.accrual.due, owed - 1_000);
        assert_eq!(FakePool::pool_tao(sn1()), pool + 1_000);
        healthy();

        // With no collateral the next period charges nothing and stays due.
        run_to(721);
        let (more, _) = model_accrue(notional(&pos), APR, 360, remainder);
        assert!(charges().is_empty());
        let pos = position(ALICE, side);
        assert_eq!(pos.accrual.due, owed - 1_000 + more);
        assert_eq!(pos.charge_at, 1_081);
        healthy();

        // Added collateral pays what is due at the next charge.
        let add = match side {
            Side::Short => Lending::add_collateral,
            Side::Long => Lending::add_long_collateral,
        };
        assert_ok!(add(RuntimeOrigin::signed(ALICE), sn1(), tao(1)));
        run_to(1_081);
        let paid = charge();
        assert!(paid > pos.accrual.due);
        let pos = position(ALICE, side);
        assert_eq!(pos.accrual.due, 0);
        assert_eq!(pos.collateral, tao(1) - paid);
        assert_eq!(FakePool::pool_tao(sn1()), pool + 1_000 + paid);
        healthy();
    });
}

#[test]
fn a_short_charge_is_capped_at_the_collateral() {
    charge_is_capped_at_the_collateral(Side::Short, open_short);
}

#[test]
fn a_long_charge_is_capped_at_the_collateral() {
    charge_is_capped_at_the_collateral(Side::Long, open_long);
}

#[test]
fn a_backlog_of_due_charges_carries_over_across_blocks() {
    const HOLDERS: AccountId = 1_200;
    const BUDGET: usize = 512;
    live().execute_with(|| {
        // A pool deep enough for 1,200 minimum-size shorts in one block (SPEC P8, P23).
        set_pool(SN1, tao(1_500_000), alpha(15_000_000));
        for who in 100..100 + HOLDERS {
            set_balance(who, tao(100));
            assert_ok!(Lending::open_short(
                RuntimeOrigin::signed(who),
                sn1(),
                alpha(110),
                tao(40),
                0
            ));
        }
        assert_eq!(TotalPositions::<Test>::get(), 1_200);
        assert_eq!(PositionCount::<Test>::get(sn1()), 1_200);
        healthy();

        run_to(360);
        assert!(charges().is_empty());
        // 512 charges per block: 512, 512, then the last 176.
        run_to(361);
        assert_eq!(charges().len(), BUDGET);
        assert_eq!(NextDue::<Test>::get(), Some(361));
        healthy();
        run_to(362);
        assert_eq!(charges().len(), BUDGET);
        assert_eq!(NextDue::<Test>::get(), Some(361));
        healthy();
        run_to(363);
        assert_eq!(charges().len(), 1_200 - 2 * BUDGET);
        assert_eq!(NextDue::<Test>::get(), Some(364));
        assert_eq!(Due::<Test>::iter_prefix(361).count(), 0);
        healthy();
        run_to(364);
        assert!(charges().is_empty());

        // A charge that ran late rejoins the grid: every position is next due at the first grid
        // block after its late charge, `1 + 2 * INTERVAL` = 721, whichever block it ran in.
        let mut by_block = [0usize; 3];
        for (_, pos) in Positions::<Test>::iter() {
            assert_eq!(pos.charge_at, 721);
            assert_eq!((pos.charge_at - pos.opened_at) % INTERVAL, 0);
            let late = usize::try_from(pos.accrual.last_accrued - 361).expect("charged from 361");
            by_block[late] += 1;
        }
        assert_eq!(by_block, [BUDGET, BUDGET, 1_200 - 2 * BUDGET]);

        // The next round is spread over the same three blocks and fits each block's budget.
        run_to(720);
        assert!(charges().is_empty());
        for (block, due) in (721..).zip(by_block) {
            run_to(block);
            assert_eq!(charges().len(), due);
        }
        // The second round's late charges rejoin the grid as well: the next block is 1_081.
        for pos in Positions::<Test>::iter_values() {
            assert_eq!(pos.charge_at, 1_081);
        }
        healthy();
    });
}

#[test]
fn a_failed_charge_is_retried_and_charges_the_whole_span() {
    live().execute_with(|| {
        assert_ok!(open_long(ALICE));
        let before = position(ALICE, Side::Long);
        // The custody cannot pay: the credit to the pool fails.
        let custody = custody(ALICE, Side::Long);
        let held = FakePool::tao_balance(&custody);
        set_balance(custody, 0);

        run_to(361);
        assert!(charges().is_empty());
        let retry_at = 361 + u64::from(RETRY_DELAY_BLOCKS);
        assert_eq!(InterestRetry::<Test>::get(id(ALICE, Side::Long)), 1);
        assert_eq!(
            position(ALICE, Side::Long),
            Position {
                charge_at: retry_at,
                ..before
            }
        );
        assert!(Due::<Test>::contains_key(retry_at, id(ALICE, Side::Long)));
        assert_eq!(InterestTotals::<Test>::get(sn1()).charged, 0);

        set_balance(custody, held);
        run_to(retry_at - 1);
        assert!(charges().is_empty());
        run_to(retry_at);
        assert_eq!(charge(), model_accrue(DEBT, APR, retry_at - 1, 0).0);
        assert!(!InterestRetry::<Test>::contains_key(id(ALICE, Side::Long)));
        assert_eq!(
            position(ALICE, Side::Long).charge_at,
            before.opened_at + 2 * INTERVAL
        );
        healthy();
    });
}

#[test]
fn closing_removes_the_scheduled_charge_and_lowers_the_counts() {
    live().execute_with(|| {
        assert_ok!(open_short(ALICE));
        assert_ok!(open_long(BOB));
        assert_eq!(PositionCount::<Test>::get(sn1()), 2);
        assert_eq!(TotalPositions::<Test>::get(), 2);

        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            u64::MAX,
            u64::MAX
        ));
        assert!(!Due::<Test>::contains_key(361, id(ALICE, Side::Short)));
        assert!(Due::<Test>::contains_key(361, id(BOB, Side::Long)));
        assert_eq!(PositionCount::<Test>::get(sn1()), 1);
        assert_eq!(TotalPositions::<Test>::get(), 1);
        healthy();

        assert_ok!(Lending::close_long(
            RuntimeOrigin::signed(BOB),
            sn1(),
            u64::MAX,
            0
        ));
        assert_eq!(Due::<Test>::iter().count(), 0);
        assert!(!PositionCount::<Test>::contains_key(sn1()));
        assert_eq!(TotalPositions::<Test>::get(), 0);
        healthy();

        // The queue is empty: its block passes with no charge and the cursor is dropped.
        run_to(361);
        assert!(charges().is_empty());
        assert_eq!(NextDue::<Test>::get(), None);
        healthy();
    });
}

/// Ten days of scheduled charges on one position against the model's `_accrue` and `_charge`:
/// every charge, the collateral, the carried remainder and the pool's TAO, to the rao.
fn ten_days_match_the_model(side: Side, open: fn(AccountId) -> DispatchResult) {
    live().execute_with(|| {
        assert_ok!(open(ALICE));
        let opened = System::block_number();
        let start = position(ALICE, side);
        let pool = FakePool::pool_tao(sn1());
        let (mut collateral, mut due, mut remainder) = (start.collateral, 0u64, 0u128);
        let mut paid_total = 0u64;
        for k in 1..=200 {
            run_to(opened + k * INTERVAL);
            let notional = match start.leg {
                Leg::Short { minted, .. } => short_notional(minted),
                Leg::Long { debt, .. } => debt,
            };
            let (owed, carried) = model_accrue(notional, APR, INTERVAL, remainder);
            due += owed;
            remainder = carried;
            let paid = due.min(collateral);
            collateral -= paid;
            due -= paid;
            paid_total += paid;

            assert_eq!(charges(), vec![(ALICE, side, paid)]);
            let pos = position(ALICE, side);
            assert_eq!(pos.collateral, collateral);
            assert_eq!(pos.accrual.due, due);
            assert_eq!(pos.accrual.remainder, remainder);
            assert_eq!(FakePool::pool_tao(sn1()), pool + paid_total);
        }
        assert_eq!(System::block_number(), opened + 10 * 7_200);
        assert!(paid_total > 0);
        assert_eq!(
            InterestTotals::<Test>::get(sn1()).charged,
            u128::from(paid_total)
        );
        healthy();
    });
}

#[test]
fn ten_days_of_short_charges_match_the_model() {
    ten_days_match_the_model(Side::Short, open_short);
}

#[test]
fn ten_days_of_long_charges_match_the_model() {
    ten_days_match_the_model(Side::Long, open_long);
}

// Try-state faults (SPEC I5, I19, 7.5).

#[test]
fn i5_fails_for_a_position_without_a_scheduled_charge() {
    assert_fault("I5: position without a scheduled charge", || {
        Due::<Test>::remove(361, id(ALICE, Side::Short));
    });
}

#[test]
fn i5_fails_for_a_second_scheduled_charge() {
    assert_fault(
        "I5: Due entry is not at the position's charge block",
        || {
            Due::<Test>::insert(362, id(ALICE, Side::Short), ());
        },
    );
}

#[test]
fn i5_fails_for_a_scheduled_charge_without_a_position() {
    assert_fault("I5: Due entry without a position", || {
        Due::<Test>::insert(361, id(BOB, Side::Short), ());
    });
}

#[test]
fn i5_fails_for_a_scheduled_charge_behind_the_cursor() {
    assert_fault("I5: Due entry before NextDue", || {
        NextDue::<Test>::put(362);
    });
    assert_fault("I5: Due entry before NextDue", || {
        NextDue::<Test>::kill();
    });
}

#[test]
fn i5_fails_for_a_retry_count_without_a_position() {
    assert_fault("I5: retry count without a position", || {
        InterestRetry::<Test>::insert(id(BOB, Side::Long), 1);
    });
}

#[test]
fn i19_fails_when_the_pool_did_not_rise_by_the_interest_charged() {
    assert_fault("I19: pool TAO did not rise by the interest charged", || {
        InterestTotals::<Test>::mutate(sn1(), |total| total.charged += 1);
    });
}

#[test]
fn position_counts_fail_when_they_drift() {
    assert_fault("PositionCount is not the number of open positions", || {
        PositionCount::<Test>::insert(sn1(), 2);
    });
    assert_fault("TotalPositions is not the number of open positions", || {
        TotalPositions::<Test>::put(0);
    });
}
