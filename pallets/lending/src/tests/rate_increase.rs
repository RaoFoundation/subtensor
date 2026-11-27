//! The minimum-rate floor and the delay of a rate increase (approved additions beyond
//! SPEC-v2.1): a `set_lending_params` call that raises the rate terms waits
//! `rate_increase_delay_blocks` in `PendingRateIncrease`, a call that lowers them applies in
//! the next block, and no setter touches a position.
//!
//! Expected rates and amounts are computed with the plain-integer closed forms of
//! `tests::interest`, not with the pallet's own math.

use frame_support::{assert_ok, dispatch::DispatchResult};
use sp_runtime::TryRuntimeError;
use subtensor_runtime_common::NetUid;

use super::interest::{Curve, model_accrue, model_rate};
use super::mock::*;
use crate::{
    Event, LendingParamsStore, PendingLendingParams, PendingRateIncrease, Positions,
    params::{LendingParams, RAO, RateTerms},
    types::{Leg, Position, Side},
};

const ALICE: AccountId = 1;
const SN1: u16 = 1;
/// The launch `rate_increase_delay_blocks`: 3 days.
const DELAY: u64 = 21_600;
/// The block of the setter calls in these tests.
const CALL_AT: u64 = 10;

fn sn1() -> NetUid {
    NetUid::from(SN1)
}

fn tao(n: u64) -> u64 {
    n * RAO
}

/// Subnet 1 with a deep pool (5,000 TAO / 50,000 alpha, spot 0.1), lending on, Alice funded
/// with 2,000 TAO, on the launch parameters: a floor of 2,000 bps and a delay of 21,600 blocks.
fn launch() -> sp_io::TestExternalities {
    let mut ext = migrated_ext();
    ext.execute_with(|| {
        set_pool(SN1, tao(5_000), tao(50_000));
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        set_balance(ALICE, tao(2_000));
    });
    ext
}

fn origin() -> RuntimeOrigin {
    RuntimeOrigin::signed(ALICE)
}

/// A short of 200 alpha (about 20 TAO of notional) holding 60 TAO of collateral: `u` near
/// 0.25, where the launch curve alone is 125 bps.
fn open_short() -> DispatchResult {
    Lending::open_short(origin(), sn1(), tao(200), tao(60), 0)
}

fn position() -> Position<u64> {
    Positions::<Test>::get((ALICE, sn1(), Side::Short)).expect("position is open")
}

fn live() -> LendingParams {
    LendingParamsStore::<Test>::get()
}

fn terms(apr_bps: u32, rate_k: u8, rate_cap_bps: u32, rate_floor_bps: u32) -> RateTerms {
    RateTerms {
        apr_bps,
        rate_k,
        rate_cap_bps,
        rate_floor_bps,
    }
}

/// The launch terms (SPEC 2.1 and the launch floor).
fn launch_terms() -> RateTerms {
    terms(2_000, 2, 10_000, 2_000)
}

fn as_curve(terms: RateTerms) -> Curve {
    (
        u128::from(terms.apr_bps),
        u32::from(terms.rate_k),
        u128::from(terms.rate_cap_bps),
        u128::from(terms.rate_floor_bps),
    )
}

/// `set_lending_params` with a change to the live group.
fn set_lending(change: impl FnOnce(&mut LendingParams)) {
    let mut params = live();
    change(&mut params);
    assert_ok!(Lending::set_lending_params(RuntimeOrigin::root(), params));
}

/// `set_lending_params` with the live group and `new` as its rate terms.
fn set_terms(new: RateTerms) {
    let params = live().with_rate_terms(new);
    assert_ok!(Lending::set_lending_params(RuntimeOrigin::root(), params));
}

fn healthy() {
    assert_ok!(Lending::do_try_state());
}

/// `floor(q * TWAP_median)` of the stored short at the TWAP the pallet reads now.
fn notional(pos: &Position<u64>) -> u64 {
    let twap = Lending::price_refs(sn1()).expect("rings are seeded").twap;
    u64::try_from((u128::from(pos.leg.alpha()) * twap.to_bits()) >> 64).expect("fits")
}

/// The rate of the stored short on `terms`: its `N / (C + P)` on the curve, lifted to the
/// floor.
fn rate_on(terms: RateTerms, pos: &Position<u64>) -> u32 {
    let Leg::Short { proceeds, .. } = pos.leg else {
        panic!("a short")
    };
    model_rate(
        as_curve(terms),
        u128::from(notional(pos)),
        u128::from(pos.collateral) + u128::from(proceeds),
    )
}

/// The interest charges of the current block: amount and the rate charged.
fn charges() -> Vec<(u64, u32)> {
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

/// Accrues the short by a touch (a top-up of 1 rao) and returns the rate it was charged at,
/// after checking the amount against the model for the span since its last accrual.
fn touch() -> u32 {
    let before = position();
    let span = System::block_number() - before.accrual.last_accrued;
    assert_ok!(Lending::add_collateral(origin(), sn1(), 1));
    let [(amount, rate)] = charges()[..] else {
        panic!("one charge expected, found {:?}", charges())
    };
    let (owed, _) = model_accrue(notional(&before), rate, span, before.accrual.remainder);
    assert_eq!(amount, owed);
    rate
}

/// The parameter events of the current block.
fn param_events() -> Vec<Event<Test>> {
    lending_events()
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::LendingParamsSet { .. }
                    | Event::RateIncreaseScheduled { .. }
                    | Event::RateIncreaseApplied { .. }
            )
        })
        .collect()
}

// The floor.

#[test]
fn a_tiny_u_short_pays_the_floor_and_its_due_grows() {
    launch().execute_with(|| {
        // 110 alpha (about 11 TAO of notional) against 1,000 TAO of collateral: u near 0.01,
        // where the curve alone, 2,000 * u^2, floors to zero.
        assert_ok!(Lending::open_short(
            origin(),
            sn1(),
            tao(110),
            tao(1_000),
            0
        ));
        let pos = position();
        let n = notional(&pos);
        let unfloored = (2_000, 2, 10_000, 0);
        let Leg::Short { proceeds, .. } = pos.leg else {
            panic!("a short")
        };
        let holdings = u128::from(pos.collateral) + u128::from(proceeds);
        assert_eq!(model_rate(unfloored, u128::from(n), holdings), 0);

        // On the launch parameters it pays the launch floor at its first scheduled charge.
        assert_eq!(rate_on(launch_terms(), &pos), 2_000);
        run_to(361);
        let (owed, rem) = model_accrue(n, 2_000, 360, 0);
        assert!(owed > 0);
        assert_eq!(charges(), vec![(owed, 2_000)]);
        assert_eq!(position().accrual.remainder, rem);
        healthy();

        // On the least floor the setter accepts, charged every block: one block of interest
        // is under a rao, so a block charges nothing, and the remainder it carries makes a
        // later block charge a rao. The rate is never zero and the total is the closed form.
        set_live_lending(|p| {
            p.rate_floor_bps = 1;
            p.charge_interval_blocks = 1;
        });
        let mut pos = position();
        assert_eq!(rate_on(live().rate_terms(), &pos), 1);
        let per_block = u128::from(n);
        assert!(per_block < 10_000 * 7_200 * 365);
        let (start, carried) = (System::block_number(), pos.accrual.remainder);
        assert_eq!(start, pos.accrual.last_accrued);
        // The charge already scheduled runs on the old cadence; the ones after it every block.
        run_to(pos.charge_at);
        let first = pos.charge_at - pos.accrual.last_accrued;
        let (owed, mut rem) = model_accrue(n, 1, first, pos.accrual.remainder);
        assert_eq!(charges(), vec![(owed, 1)]);
        let mut total = owed;
        let (mut empty, mut paying) = (0, 0);
        for _ in 0..100 {
            next_block();
            let (owed, left) = model_accrue(n, 1, 1, rem);
            rem = left;
            // A charge of zero rao is not announced.
            if owed == 0 {
                assert!(charges().is_empty());
                empty += 1;
            } else {
                assert_eq!(charges(), vec![(owed, 1)]);
                paying += 1;
            }
            total += owed;
            pos = position();
            assert_eq!(pos.accrual.remainder, rem);
            assert!(
                rem > 0,
                "the remainder carries what the block did not charge"
            );
        }
        assert!(empty > 0 && paying > 0);
        let blocks = System::block_number() - start;
        assert_eq!(blocks, first + 100);
        // The closed form over the whole span, from the remainder the span started with.
        let closed_form = (u128::from(n) * u128::from(blocks) + carried) / (10_000 * 7_200 * 365);
        assert_eq!(u128::from(total), closed_form);
        assert!(total > 0);
        healthy();
    });
}

// Delayed increases.

#[test]
fn an_increase_waits_exactly_the_delay() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        run_to(CALL_AT);
        let new = terms(4_000, 2, 10_000, 3_000);
        set_terms(new);
        let effective_at = CALL_AT + DELAY;
        assert_eq!(
            param_events(),
            vec![Event::RateIncreaseScheduled { new, effective_at }]
        );
        assert_eq!(
            PendingRateIncrease::<Test>::get(),
            Some((new, effective_at))
        );
        // Nothing else changed, so no group is queued.
        assert_eq!(PendingLendingParams::<Test>::get(), None);
        healthy();

        // The next block applies nothing and the setter touched no position.
        let before = position();
        next_block();
        assert!(param_events().is_empty());
        assert_eq!(live(), LendingParams::default());
        assert_eq!(position(), before);

        // One block before the increase is live the old terms still set the rate.
        run_to(effective_at - 1);
        assert_eq!(live().rate_terms(), launch_terms());
        assert_eq!(
            PendingRateIncrease::<Test>::get(),
            Some((new, effective_at))
        );
        let old_rate = rate_on(launch_terms(), &position());
        assert_eq!(old_rate, 2_000);
        assert_eq!(touch(), old_rate);
        healthy();

        // Its block applies it, with one event and without touching the position.
        let before = position();
        next_block();
        assert_eq!(System::block_number(), effective_at);
        assert_eq!(
            param_events(),
            vec![Event::RateIncreaseApplied {
                old: launch_terms(),
                new
            }]
        );
        assert_eq!(live(), LendingParams::default().with_rate_terms(new));
        assert_eq!(PendingRateIncrease::<Test>::get(), None);
        assert_eq!(position(), before);
        // The next accrual is at the new terms.
        let new_rate = rate_on(new, &before);
        assert_eq!(new_rate, 3_000);
        assert_eq!(touch(), new_rate);
        healthy();

        // It is applied once.
        next_block();
        assert!(param_events().is_empty());
    });
}

#[test]
fn a_scheduled_charge_reads_the_terms_live_at_its_block() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        let pos = position();
        // A delay of 400 blocks: the increase lands between the first and the second charge.
        set_live_lending(|p| p.rate_increase_delay_blocks = 400);
        run_to(CALL_AT);
        let new = terms(2_000, 2, 10_000, 5_000);
        set_terms(new);
        let effective_at = CALL_AT + 400;

        // First charge, before the increase: the old floor for its 360 blocks.
        run_to(pos.charge_at);
        assert!(pos.charge_at < effective_at);
        let (owed, rem) = model_accrue(notional(&pos), 2_000, 360, 0);
        assert_eq!(charges(), vec![(owed, 2_000)]);

        // Second charge, after it: the new floor for the whole period, also the blocks of it
        // that passed before the increase was live (a rate is read at the accrual).
        let pos = position();
        run_to(pos.charge_at);
        assert!(pos.charge_at > effective_at);
        assert_eq!(rate_on(new, &pos), 5_000);
        let (owed, _) = model_accrue(notional(&pos), 5_000, 360, rem);
        assert_eq!(charges(), vec![(owed, 5_000)]);
        healthy();
    });
}

#[test]
fn a_decrease_applies_in_the_next_block() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        run_to(CALL_AT);
        // The base APR, the cap and the floor each lower, the exponent as it is.
        let new = terms(1_000, 2, 5_000, 500);
        set_terms(new);
        assert!(param_events().is_empty());
        assert_eq!(PendingRateIncrease::<Test>::get(), None);
        assert_eq!(live(), LendingParams::default());
        let before = position();

        next_block();
        let lowered = LendingParams::default().with_rate_terms(new);
        assert_eq!(
            param_events(),
            vec![Event::LendingParamsSet {
                old: LendingParams::default(),
                new: lowered.clone()
            }]
        );
        assert_eq!(live(), lowered);
        assert_eq!(position(), before);
        // The next accrual is at the lower terms, also for the blocks before the call.
        assert_eq!(rate_on(new, &before), 500);
        assert_eq!(touch(), 500);
        healthy();
    });
}

#[test]
fn each_lower_term_alone_is_a_decrease_and_each_higher_one_an_increase() {
    let cases: [(RateTerms, bool); 8] = [
        (terms(1_999, 2, 10_000, 2_000), true),
        (terms(2_000, 2, 9_999, 2_000), true),
        (terms(2_000, 2, 10_000, 1_999), true),
        // The live terms again.
        (terms(2_000, 2, 10_000, 2_000), true),
        (terms(2_001, 2, 10_000, 2_000), false),
        (terms(2_000, 2, 10_001, 2_000), false),
        (terms(2_000, 2, 10_000, 2_001), false),
        // Everything lower, but the exponent moved.
        (terms(1, 3, 1, 1), false),
    ];
    for (new, decrease) in cases {
        launch().execute_with(|| {
            run_to(CALL_AT);
            set_terms(new);
            next_block();
            if decrease {
                assert_eq!(PendingRateIncrease::<Test>::get(), None, "{new:?}");
                assert_eq!(live().rate_terms(), new, "{new:?}");
            } else {
                assert_eq!(
                    PendingRateIncrease::<Test>::get(),
                    Some((new, CALL_AT + DELAY)),
                    "{new:?}"
                );
                assert_eq!(live().rate_terms(), launch_terms(), "{new:?}");
            }
            healthy();
        });
    }
}

#[test]
fn a_mixed_change_is_delayed_and_the_other_fields_apply_in_the_next_block() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        run_to(CALL_AT);
        // The base APR down, the cap up, and two fields that are not rate terms.
        let new = terms(1_000, 2, 20_000, 2_000);
        set_lending(|p| {
            *p = p.clone().with_rate_terms(new);
            p.charge_interval_blocks = 720;
            p.deposit = RAO;
        });
        let effective_at = CALL_AT + DELAY;
        assert_eq!(
            param_events(),
            vec![Event::RateIncreaseScheduled { new, effective_at }]
        );
        let before = position();

        // The next block: the other fields are live, on the old rate terms as a whole (the
        // lower base APR waits with the higher cap).
        next_block();
        let rest = LendingParams {
            charge_interval_blocks: 720,
            deposit: RAO,
            ..Default::default()
        };
        assert_eq!(
            param_events(),
            vec![Event::LendingParamsSet {
                old: LendingParams::default(),
                new: rest.clone()
            }]
        );
        assert_eq!(live(), rest);
        assert_eq!(live().rate_terms(), launch_terms());
        assert_eq!(position(), before);
        assert_eq!(touch(), 2_000);

        run_to(effective_at);
        assert_eq!(
            param_events(),
            vec![Event::RateIncreaseApplied {
                old: launch_terms(),
                new
            }]
        );
        assert_eq!(live(), rest.with_rate_terms(new));
        healthy();
    });
}

#[test]
fn a_change_of_the_exponent_is_delayed_in_either_direction() {
    for k in [1, 3] {
        launch().execute_with(|| {
            assert_ok!(open_short());
            run_to(CALL_AT);
            // Every other term lower: the exponent alone makes the change an increase.
            let new = terms(1_000, k, 5_000, 500);
            set_terms(new);
            let effective_at = CALL_AT + DELAY;
            assert_eq!(
                param_events(),
                vec![Event::RateIncreaseScheduled { new, effective_at }]
            );
            next_block();
            assert!(param_events().is_empty());
            assert_eq!(live(), LendingParams::default());

            run_to(effective_at - 1);
            assert_eq!(touch(), 2_000);
            next_block();
            assert_eq!(live().rate_terms(), new);
            assert_eq!(touch(), rate_on(new, &position()));
            healthy();
        });
    }
}

#[test]
fn a_later_increase_replaces_the_pending_one() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        let first = terms(4_000, 2, 10_000, 2_000);
        set_terms(first);
        run_to(CALL_AT + 100);
        // The second call is compared with the live terms, not with the pending ones: it is
        // below the first and still an increase.
        let second = terms(3_000, 2, 10_000, 2_000);
        set_terms(second);
        let effective_at = CALL_AT + 100 + DELAY;
        assert_eq!(
            param_events(),
            vec![Event::RateIncreaseScheduled {
                new: second,
                effective_at
            }]
        );
        assert_eq!(
            PendingRateIncrease::<Test>::get(),
            Some((second, effective_at))
        );
        healthy();

        // The first one's block passes with nothing applied.
        run_to(CALL_AT + DELAY);
        assert!(param_events().is_empty());
        assert_eq!(live().rate_terms(), launch_terms());

        run_to(effective_at);
        assert_eq!(
            param_events(),
            vec![Event::RateIncreaseApplied {
                old: launch_terms(),
                new: second
            }]
        );
        assert_eq!(live().rate_terms(), second);
        healthy();
    });
}

#[test]
fn a_later_decrease_clears_the_pending_increase() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        set_terms(terms(4_000, 2, 10_000, 2_000));
        assert!(PendingRateIncrease::<Test>::get().is_some());

        // A call that carries the live terms (here: to change another field) is a decrease.
        run_to(CALL_AT + 100);
        set_lending(|p| p.deposit = RAO);
        assert_eq!(PendingRateIncrease::<Test>::get(), None);
        assert!(param_events().is_empty());
        next_block();
        assert_eq!(live().deposit, RAO);
        assert_eq!(live().rate_terms(), launch_terms());

        // So is a call with lower terms, which are live in the next block.
        set_terms(terms(4_000, 2, 10_000, 2_000));
        assert!(PendingRateIncrease::<Test>::get().is_some());
        let lower = terms(1_500, 2, 10_000, 1_500);
        set_terms(lower);
        assert_eq!(PendingRateIncrease::<Test>::get(), None);
        next_block();
        assert_eq!(live().rate_terms(), lower);

        // Nothing is applied when the cleared increases would have been due.
        run_to(CALL_AT + 200 + DELAY);
        assert_eq!(live().rate_terms(), lower);
        healthy();
    });
}

#[test]
fn an_increase_after_a_decrease_in_one_block_keeps_the_live_terms() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        set_terms(terms(1_000, 2, 10_000, 1_000));
        let raised = terms(3_000, 2, 10_000, 2_000);
        set_terms(raised);
        // The queued decrease is replaced: the live terms stay until the increase is due.
        assert_eq!(PendingLendingParams::<Test>::get(), None);
        next_block();
        assert!(param_events().is_empty());
        assert_eq!(live(), LendingParams::default());
        run_to(CALL_AT + DELAY);
        assert_eq!(live().rate_terms(), raised);
        healthy();
    });
}

#[test]
fn with_no_delay_an_increase_applies_in_the_next_block() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        set_live_lending(|p| p.rate_increase_delay_blocks = 0);
        let start = live();
        run_to(CALL_AT);
        let new = terms(4_000, 2, 10_000, 3_000);
        set_terms(new);
        // Not in the block of the call: a call never changes the rate of its own block.
        assert_eq!(
            param_events(),
            vec![Event::RateIncreaseScheduled {
                new,
                effective_at: CALL_AT + 1
            }]
        );
        assert_eq!(live(), start);
        assert_eq!(touch(), 2_000);
        healthy();

        next_block();
        assert_eq!(
            param_events(),
            vec![Event::RateIncreaseApplied {
                old: launch_terms(),
                new
            }]
        );
        assert_eq!(live(), start.with_rate_terms(new));
        assert_eq!(touch(), 3_000);
        healthy();
    });
}

#[test]
fn a_new_delay_applies_in_the_next_block_and_only_to_later_schedules() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        // One call shortens the delay and raises the floor: the increase waits the delay in
        // force at the call.
        let first = terms(2_000, 2, 10_000, 3_000);
        set_lending(|p| {
            *p = p.clone().with_rate_terms(first);
            p.rate_increase_delay_blocks = 50;
        });
        assert_eq!(
            PendingRateIncrease::<Test>::get(),
            Some((first, CALL_AT + DELAY))
        );
        next_block();
        assert_eq!(live().rate_increase_delay_blocks, 50);
        assert_eq!(live().rate_terms(), launch_terms());
        // The new delay does not move the increase already scheduled.
        assert_eq!(
            PendingRateIncrease::<Test>::get(),
            Some((first, CALL_AT + DELAY))
        );
        run_to(CALL_AT + 51);
        assert_eq!(live().rate_terms(), launch_terms());

        // A later call is scheduled with it.
        let second = terms(2_000, 2, 10_000, 4_000);
        set_terms(second);
        let effective_at = CALL_AT + 51 + 50;
        assert_eq!(
            PendingRateIncrease::<Test>::get(),
            Some((second, effective_at))
        );
        run_to(effective_at - 1);
        assert_eq!(live().rate_terms(), launch_terms());
        next_block();
        assert_eq!(live().rate_terms(), second);
        assert_eq!(live().rate_increase_delay_blocks, 50);
        healthy();
    });
}

#[test]
fn a_rejected_call_leaves_the_pending_increase() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        let new = terms(4_000, 2, 10_000, 2_000);
        set_terms(new);
        let pending = PendingRateIncrease::<Test>::get();
        for bad in [
            terms(2_000, 2, 10_000, 0),
            terms(2_000, 2, 10_000, 10_001),
            terms(2_000, 9, 10_000, 2_000),
            terms(2_000, 2, 1_999, 1),
        ] {
            assert!(
                Lending::set_lending_params(RuntimeOrigin::root(), live().with_rate_terms(bad))
                    .is_err()
            );
            assert_eq!(PendingRateIncrease::<Test>::get(), pending);
        }
        assert!(
            Lending::set_lending_params(
                RuntimeOrigin::root(),
                LendingParams {
                    rate_increase_delay_blocks: 216_001,
                    ..live()
                }
            )
            .is_err()
        );
        assert_eq!(PendingRateIncrease::<Test>::get(), pending);
    });
}

// Try-state.

#[test]
fn try_state_fails_for_a_pending_increase_out_of_bounds_or_overdue() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        let fails = |entry: (RateTerms, u64), message: &'static str| {
            PendingRateIncrease::<Test>::put(entry);
            assert_eq!(
                Lending::do_try_state(),
                Err(TryRuntimeError::Other(message))
            );
        };
        let out_of_bounds = "I26: pending rate increase out of bounds";
        fails((terms(2_000, 2, 10_000, 0), CALL_AT + 1), out_of_bounds);
        fails(
            (terms(2_000, 2, 10_000, 10_001), CALL_AT + 1),
            out_of_bounds,
        );
        fails((terms(2_000, 9, 10_000, 2_000), CALL_AT + 1), out_of_bounds);
        fails((terms(2_000, 2, 1_999, 1), CALL_AT + 1), out_of_bounds);
        let overdue = "I26: pending rate increase is overdue";
        fails((terms(4_000, 2, 10_000, 2_000), CALL_AT), overdue);
        fails((terms(4_000, 2, 10_000, 2_000), CALL_AT - 1), overdue);
        PendingRateIncrease::<Test>::put((terms(4_000, 2, 10_000, 2_000), CALL_AT + 1));
        healthy();
    });
}

#[test]
fn try_state_fails_for_a_position_with_debt_below_the_floor() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        healthy();
        // A live floor above the cap is out of bounds (I26), which is what would let a rate
        // sit outside floor..=cap.
        set_live_lending(|p| p.rate_floor_bps = p.rate_cap_bps + 1);
        assert_eq!(
            Lending::do_try_state(),
            Err(TryRuntimeError::Other("I26: LendingParams out of bounds"))
        );
        set_live_lending(|p| p.rate_floor_bps = 0);
        assert_eq!(
            Lending::do_try_state(),
            Err(TryRuntimeError::Other("I26: LendingParams out of bounds"))
        );
    });
}
