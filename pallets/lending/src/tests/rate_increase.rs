//! The delay of a tightening (SPEC 2.1, I26): a `set_lending_params` call that raises the base
//! APR or the cap, changes the exponent or the TWAP ring, or shortens `rate_delay_blocks` writes
//! the whole new `LendingParams` to `PendingLendingParams`, due the delay live at the call (so a
//! shortening waits the delay it replaces). Any other call, a lengthening of the delay included,
//! is live from the next block, and a later call replaces whatever is pending. No setter touches
//! a position: each reads the group live at its accrual.
//!
//! Expected rates and amounts are computed with the plain-integer closed forms of
//! `tests::interest`, not with the pallet's own math.

use frame_support::{assert_noop, assert_ok, dispatch::DispatchResult};
use subtensor_runtime_common::NetUid;

use super::interest::{Curve, model_accrue, model_rate};
use super::mock::*;
use crate::{
    Error, Event, LendingParamsStore, PendingLendingParams, Positions,
    math::fixed::Price,
    params::{LendingParams, RAO},
    types::{Leg, Position, Side},
};

const ALICE: AccountId = 1;
const SN1: u16 = 1;
/// The launch `rate_delay_blocks`: 3 days.
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
/// with 2,000 TAO, on the launch parameters: a base APR of 2,000 bps and a delay of 21,600
/// blocks.
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
/// 0.25, where the rate is the base APR.
fn open_short() -> DispatchResult {
    Lending::open_short(origin(), sn1(), tao(200), tao(60), 0)
}

fn position() -> Position<u64> {
    Positions::<Test>::get((ALICE, sn1(), Side::Short)).expect("position is open")
}

fn live() -> LendingParams {
    LendingParamsStore::<Test>::get()
}

/// `set_lending_params` with `params`.
fn set(params: LendingParams) -> DispatchResult {
    Lending::set_lending_params(RuntimeOrigin::root(), params)
}

fn healthy() {
    assert_ok!(Lending::do_try_state());
}

/// `floor(alpha * price)`.
fn value_at(alpha: u64, price: Price) -> u64 {
    u64::try_from((u128::from(alpha) * price.to_bits()) >> 64).expect("fits")
}

/// The notional of a short: `floor(q * TWAP median)`.
fn notional(pos: &Position<u64>) -> u64 {
    pos.leg
        .notional(Lending::price_refs(sn1()).expect("rings are seeded").twap)
}

/// The rate curve of a group, as the model reads it.
fn as_curve(params: &LendingParams) -> Curve {
    (
        u128::from(params.apr_bps),
        u32::from(params.rate_k),
        u128::from(params.rate_cap_bps),
    )
}

/// The rate of the stored short on `params`' curve: its `floor(q * min(TWAP, SMA30)) / (C + P)`
/// (SPEC 2.1).
fn rate_on(params: &LendingParams, pos: &Position<u64>) -> u32 {
    let Leg::Short { minted, proceeds } = pos.leg else {
        panic!("a short")
    };
    let refs = Lending::price_refs(sn1()).expect("rings are seeded");
    model_rate(
        as_curve(params),
        u128::from(value_at(minted, refs.twap.min(refs.sma))),
        u128::from(pos.collateral) + u128::from(proceeds),
    )
}

/// The interest charges of the current block: amount and rate charged.
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

/// The group events of the current block.
fn group_events() -> Vec<Event<Test>> {
    lending_events()
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::LendingParamsSet { .. } | Event::LendingParamsScheduled { .. }
            )
        })
        .collect()
}

// The delay of a tightening.

#[test]
fn a_tightening_waits_exactly_the_delay() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        run_to(CALL_AT);
        let old = live();
        let new = LendingParams {
            apr_bps: 4_000,
            ..Default::default()
        };
        assert_ok!(set(new.clone()));
        let apply_at = CALL_AT + DELAY;
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsScheduled {
                old: old.clone(),
                new: new.clone(),
                apply_at
            }]
        );
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((new.clone(), apply_at))
        );
        healthy();

        // The next block applies nothing, and the setter touched no position.
        let before = position();
        next_block();
        assert!(group_events().is_empty());
        assert_eq!(live(), old);
        assert_eq!(position(), before);

        // One block before its due block the old group still sets the rate.
        run_to(apply_at - 1);
        assert_eq!(live(), old);
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((new.clone(), apply_at))
        );
        let old_rate = rate_on(&old, &position());
        assert_eq!(old_rate, 2_000);
        assert_eq!(touch(), old_rate);
        healthy();

        // Its due block applies it, with one event and without touching the position.
        let before = position();
        next_block();
        assert_eq!(System::block_number(), apply_at);
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsSet {
                old: old.clone(),
                new: new.clone()
            }]
        );
        assert_eq!(live(), new);
        assert_eq!(PendingLendingParams::<Test>::get(), None);
        assert_eq!(position(), before);
        // The next accrual is at the new group.
        let new_rate = rate_on(&new, &before);
        assert_eq!(new_rate, 4_000);
        assert_eq!(touch(), new_rate);
        healthy();

        // It is applied once.
        next_block();
        assert!(group_events().is_empty());
    });
}

#[test]
fn a_scheduled_charge_reads_the_group_live_at_its_block() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        // A delay of 400 blocks: the new APR lands between the first and the second charge.
        set_live_lending(|p| p.rate_delay_blocks = 400);
        run_to(CALL_AT);
        let old = live();
        let new = LendingParams {
            apr_bps: 4_000,
            rate_delay_blocks: 400,
            ..Default::default()
        };
        assert_ok!(set(new.clone()));
        let apply_at = CALL_AT + 400;

        // The first charge is before the group is live: the base APR for its 360 blocks.
        let first = position();
        assert!(first.charge_at < apply_at);
        run_to(first.charge_at);
        let (owed, _) = model_accrue(
            notional(&first),
            2_000,
            first.charge_at - first.accrual.last_accrued,
            first.accrual.remainder,
        );
        assert_eq!(rate_on(&old, &first), 2_000);
        assert_eq!(charges(), vec![(owed, 2_000)]);

        // The second charge is after it: the new APR for its whole period.
        let second = position();
        assert!(second.charge_at > apply_at);
        run_to(second.charge_at);
        assert_eq!(rate_on(&new, &second), 4_000);
        let (owed, _) = model_accrue(
            notional(&second),
            4_000,
            second.charge_at - second.accrual.last_accrued,
            second.accrual.remainder,
        );
        assert_eq!(charges(), vec![(owed, 4_000)]);
        healthy();
    });
}

#[test]
fn a_decrease_is_live_in_the_next_block() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        run_to(CALL_AT);
        let old = live();
        // The base APR and the cap lower; the exponent does not change.
        let new = LendingParams {
            apr_bps: 1_000,
            rate_cap_bps: 5_000,
            ..Default::default()
        };
        assert_ok!(set(new.clone()));
        assert!(group_events().is_empty());
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((new.clone(), CALL_AT + 1))
        );
        assert_eq!(live(), old);
        let before = position();

        next_block();
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsSet {
                old,
                new: new.clone()
            }]
        );
        assert_eq!(live(), new);
        assert_eq!(position(), before);
        // The accrual at the touch reads the lower group, for the blocks before the call too.
        assert_eq!(rate_on(&new, &before), 1_000);
        assert_eq!(touch(), 1_000);
        healthy();
    });
}

#[test]
fn only_the_listed_changes_are_tightenings() {
    // A shorter delay than the launch one keeps the run short: the tightenings wait 50 blocks.
    const WAIT: u32 = 50;
    type Change = (fn(&mut LendingParams), bool);
    let cases: [Change; 12] = [
        (|p: &mut LendingParams| p.apr_bps = 1_999, false),
        (|p: &mut LendingParams| p.rate_cap_bps = 19_999, false),
        (|p: &mut LendingParams| p.apr_bps = 2_001, true),
        (|p: &mut LendingParams| p.rate_cap_bps = 20_001, true),
        (|p: &mut LendingParams| p.rate_k = 2, true),
        (|p: &mut LendingParams| p.rate_delay_blocks = 100, false),
        (|p: &mut LendingParams| p.rate_delay_blocks = 49, true),
        (
            |p: &mut LendingParams| p.charge_interval_blocks = 359,
            false,
        ),
        (|p: &mut LendingParams| p.min_notional = 2 * RAO, false),
        (|p: &mut LendingParams| p.deposit = RAO, false),
        (|p: &mut LendingParams| p.twap_samples = 71, true),
        (|p: &mut LendingParams| p.twap_interval_blocks = 299, true),
    ];
    for (change, tightening) in cases {
        launch().execute_with(|| {
            set_live_lending(|p| p.rate_delay_blocks = WAIT);
            run_to(CALL_AT);
            let old = live();
            let mut new = old.clone();
            change(&mut new);
            assert_ok!(set(new.clone()));
            let apply_at = if tightening {
                CALL_AT + u64::from(WAIT)
            } else {
                CALL_AT + 1
            };
            if tightening {
                assert_eq!(
                    group_events(),
                    vec![Event::LendingParamsScheduled {
                        old: old.clone(),
                        new: new.clone(),
                        apply_at
                    }],
                    "{new:?}"
                );
            } else {
                assert!(group_events().is_empty(), "{new:?}");
            }
            assert_eq!(
                PendingLendingParams::<Test>::get(),
                Some((new.clone(), apply_at)),
                "{new:?}"
            );
            run_to(apply_at - 1);
            assert_eq!(live(), old, "{new:?}");
            run_to(apply_at);
            assert_eq!(live(), new, "{new:?}");
            assert_eq!(PendingLendingParams::<Test>::get(), None, "{new:?}");
            healthy();
        });
    }
}

#[test]
fn a_tightening_group_waits_with_its_other_fields() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        run_to(CALL_AT);
        let old = live();
        // The base APR rises and so does the deposit: the whole group waits for the delay.
        let new = LendingParams {
            apr_bps: 4_000,
            deposit: RAO,
            ..Default::default()
        };
        assert_ok!(set(new.clone()));
        let apply_at = CALL_AT + DELAY;
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsScheduled {
                old: old.clone(),
                new: new.clone(),
                apply_at
            }]
        );
        next_block();
        assert_eq!(live(), old);
        assert_eq!(live().deposit, old.deposit);
        run_to(apply_at);
        assert_eq!(live(), new);
        healthy();
    });
}

#[test]
fn any_change_of_the_exponent_is_delayed_in_either_direction() {
    launch().execute_with(|| {
        set_live_lending(|p| p.rate_k = 3);
        run_to(CALL_AT);
        let old = live();
        // A lower exponent alone is a change of the curve, so it waits too.
        let new = LendingParams {
            rate_k: 2,
            ..old.clone()
        };
        assert_ok!(set(new.clone()));
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsScheduled {
                old: old.clone(),
                new: new.clone(),
                apply_at: CALL_AT + DELAY
            }]
        );
        next_block();
        assert_eq!(live(), old);
        run_to(CALL_AT + DELAY);
        assert_eq!(live(), new);
        healthy();
    });
}

#[test]
fn a_later_tightening_replaces_the_pending_group_and_restarts_the_delay() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        let old = live();
        let first = LendingParams {
            apr_bps: 4_000,
            ..Default::default()
        };
        assert_ok!(set(first));
        run_to(CALL_AT + 100);
        // Compared with the live group, not with the pending one: still a tightening.
        let second = LendingParams {
            apr_bps: 3_000,
            ..Default::default()
        };
        assert_ok!(set(second.clone()));
        let apply_at = CALL_AT + 100 + DELAY;
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsScheduled {
                old: old.clone(),
                new: second.clone(),
                apply_at
            }]
        );
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((second.clone(), apply_at))
        );
        healthy();

        // The first call's due block passes with nothing applied.
        run_to(CALL_AT + DELAY);
        assert!(group_events().is_empty());
        assert_eq!(live(), old);

        run_to(apply_at);
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsSet {
                old,
                new: second.clone()
            }]
        );
        assert_eq!(live(), second);
        healthy();
    });
}

#[test]
fn a_call_that_does_not_tighten_replaces_a_pending_increase() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        let old = live();
        assert_ok!(set(LendingParams {
            apr_bps: 4_000,
            ..Default::default()
        }));
        run_to(CALL_AT + 100);
        // A call that tightens nothing replaces the pending group, the increase included.
        let lowered = LendingParams {
            deposit: RAO,
            ..old.clone()
        };
        assert_ok!(set(lowered.clone()));
        assert!(group_events().is_empty());
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((lowered.clone(), CALL_AT + 101))
        );
        next_block();
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsSet {
                old,
                new: lowered.clone()
            }]
        );
        assert_eq!(live(), lowered);
        // The increase is gone: the base APR stays 2,000 past its former due block.
        run_to(CALL_AT + DELAY);
        assert_eq!(live(), lowered);
        healthy();
    });
}

#[test]
fn a_decrease_after_an_increase_in_one_block_is_the_group_that_is_due() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        let old = live();
        assert_ok!(set(LendingParams {
            apr_bps: 1_000,
            rate_cap_bps: 5_000,
            ..Default::default()
        }));
        // An increase in the same block replaces the queued decrease: the live group stays
        // until the increase is due.
        let raised = LendingParams {
            apr_bps: 3_000,
            ..Default::default()
        };
        assert_ok!(set(raised.clone()));
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((raised.clone(), CALL_AT + DELAY))
        );
        next_block();
        assert!(group_events().is_empty());
        assert_eq!(live(), old);
        run_to(CALL_AT + DELAY);
        assert_eq!(live(), raised);
        healthy();
    });
}

#[test]
fn with_no_delay_a_tightening_applies_in_the_next_block() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        set_live_lending(|p| p.rate_delay_blocks = 0);
        let start = live();
        run_to(CALL_AT);
        let new = LendingParams {
            apr_bps: 4_000,
            ..start.clone()
        };
        assert_ok!(set(new.clone()));
        // Due in the next block: a call never changes the rate of its own block.
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsScheduled {
                old: start.clone(),
                new: new.clone(),
                apply_at: CALL_AT + 1
            }]
        );
        assert_eq!(live(), start);
        assert_eq!(touch(), 2_000);
        healthy();

        next_block();
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsSet {
                old: start,
                new: new.clone()
            }]
        );
        assert_eq!(live(), new);
        assert_eq!(touch(), 4_000);
        healthy();
    });
}

#[test]
fn shortening_the_delay_waits_the_old_delay() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        run_to(CALL_AT);
        let old = live();
        assert_eq!(u64::from(old.rate_delay_blocks), DELAY);
        // A shorter delay is a tightening-class change: root cannot cut the delay and raise
        // rates within one window, so the cut itself waits the delay it replaces.
        let shorter = LendingParams {
            rate_delay_blocks: 100,
            ..old.clone()
        };
        assert_ok!(set(shorter.clone()));
        let apply_at = CALL_AT + DELAY;
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsScheduled {
                old: old.clone(),
                new: shorter.clone(),
                apply_at
            }]
        );
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((shorter.clone(), apply_at))
        );
        healthy();

        // Still the old delay one block before it is due: nothing has been cut yet.
        run_to(apply_at - 1);
        assert_eq!(live(), old);
        assert_eq!(rate_on(&old, &position()), 2_000);

        run_to(apply_at);
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsSet {
                old,
                new: shorter.clone()
            }]
        );
        assert_eq!(live(), shorter);
        healthy();
    });
}

#[test]
fn lengthening_the_delay_applies_in_the_next_block() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        run_to(CALL_AT);
        let old = live();
        // A longer delay only delays future tightenings, so it is live in the next block.
        let longer = LendingParams {
            rate_delay_blocks: 216_000,
            ..old.clone()
        };
        assert_ok!(set(longer.clone()));
        assert!(group_events().is_empty());
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((longer.clone(), CALL_AT + 1))
        );
        next_block();
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsSet {
                old,
                new: longer.clone()
            }]
        );
        assert_eq!(live(), longer);
        assert_eq!(PendingLendingParams::<Test>::get(), None);
        healthy();
    });
}

#[test]
fn a_tightening_waits_the_changed_delay_once_it_is_live() {
    launch().execute_with(|| {
        assert_ok!(open_short());
        run_to(CALL_AT);
        // Shorten the delay to 50 (it waits the old 21,600 blocks).
        let shorter = LendingParams {
            rate_delay_blocks: 50,
            ..live()
        };
        assert_ok!(set(shorter.clone()));
        run_to(CALL_AT + DELAY);
        assert_eq!(live(), shorter);

        // The rate tightening now waits exactly the delay in force at its call: 50 blocks.
        // (The block's own `LendingParamsSet` is cleared so only this call's event is checked.)
        let call = System::block_number();
        let raised = LendingParams {
            apr_bps: 4_000,
            ..shorter.clone()
        };
        System::reset_events();
        assert_ok!(set(raised.clone()));
        assert_eq!(
            group_events(),
            vec![Event::LendingParamsScheduled {
                old: shorter.clone(),
                new: raised.clone(),
                apply_at: call + 50
            }]
        );
        run_to(call + 49);
        assert_eq!(live(), shorter);
        run_to(call + 50);
        assert_eq!(live(), raised);
        healthy();
    });
}

#[test]
fn a_shortening_replaces_a_pending_group_and_waits_the_old_delay() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        let old = live();
        // A rate increase is pending for the old delay.
        assert_ok!(set(LendingParams {
            apr_bps: 4_000,
            ..old.clone()
        }));
        run_to(CALL_AT + 1_000);
        // A later shortening replaces it: still a tightening, so it waits the live delay from
        // this call, not the 50 blocks it carries.
        let shorter = LendingParams {
            apr_bps: 4_000,
            rate_delay_blocks: 50,
            ..old.clone()
        };
        assert_ok!(set(shorter.clone()));
        let apply_at = CALL_AT + 1_000 + DELAY;
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((shorter.clone(), apply_at))
        );
        run_to(apply_at);
        assert_eq!(live(), shorter);
        healthy();
    });
}

#[test]
fn the_cap_stays_at_or_above_the_base_apr_in_live_and_pending() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        let old = live(); // apr 2,000, cap 20,000
        // A base APR rise that keeps the cap above it is pending for the delay.
        let raised = LendingParams {
            apr_bps: 4_000,
            ..old.clone()
        };
        assert_ok!(set(raised.clone()));
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((raised.clone(), CALL_AT + DELAY))
        );

        // A cap below the new base APR is refused: the cap must hold under both the live APR
        // (2,000) and the new one (4,000).
        assert_noop!(
            set(LendingParams {
                apr_bps: 4_000,
                rate_cap_bps: 3_999,
                ..old.clone()
            }),
            Error::<Test>::InvalidParams
        );
        // A cap below the live base APR is refused too, while the live APR is still in force.
        assert_noop!(
            set(LendingParams {
                apr_bps: 1_000,
                rate_cap_bps: 1_999,
                ..old.clone()
            }),
            Error::<Test>::InvalidParams
        );
        // The pending group is untouched by the refused calls.
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((raised.clone(), CALL_AT + DELAY))
        );

        // A cap that stays above both rates is accepted, and replaces the pending group.
        let capped = LendingParams {
            apr_bps: 4_000,
            rate_cap_bps: 4_000,
            ..old.clone()
        };
        assert_ok!(set(capped.clone()));
        assert_eq!(
            PendingLendingParams::<Test>::get(),
            Some((capped.clone(), CALL_AT + DELAY))
        );
        run_to(CALL_AT + DELAY);
        assert_eq!(live(), capped);
        healthy();
    });
}

#[test]
fn a_rejected_call_leaves_the_pending_group() {
    launch().execute_with(|| {
        run_to(CALL_AT);
        assert_ok!(set(LendingParams {
            apr_bps: 4_000,
            ..Default::default()
        }));
        let pending = PendingLendingParams::<Test>::get();
        for bad in [
            LendingParams {
                rate_k: 0,
                ..Default::default()
            },
            LendingParams {
                rate_k: 9,
                ..Default::default()
            },
            LendingParams {
                rate_cap_bps: 1_000_001,
                ..Default::default()
            },
            // Below the base APR of the live group.
            LendingParams {
                rate_cap_bps: 1_999,
                ..Default::default()
            },
            LendingParams {
                apr_bps: 0,
                ..Default::default()
            },
            LendingParams {
                rate_delay_blocks: 216_001,
                ..Default::default()
            },
        ] {
            assert_noop!(set(bad), Error::<Test>::InvalidParams);
        }
        assert_eq!(PendingLendingParams::<Test>::get(), pending);
    });
}
