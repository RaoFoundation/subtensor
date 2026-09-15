//! The two settlement rules added after the pre-release audit, run against the real pool.
//!
//! **Unwinding in steps is the same trade as unwinding in one.** Before, every partial
//! settlement handed its escrow and the alpha it bought straight back to the pool,
//! price-neutrally, at the price the position's own opening trade had pushed. That deepened
//! the pool between the buybacks, so each later buyback climbed a flatter curve, and a short
//! unwound in steps was paid more than one close would have paid: at zero interest, out of
//! the pool, repeatably. Now a partial settlement returns nothing; the pool's share is held on
//! the position and goes back once, at the full close, after the last buyback.
//!
//! **The owner sets the floor.** `close` and `add` take a `min_amount_out`. A settlement
//! executes at the live price, which anyone can move in the same block ahead of it; the floor
//! is what bounds what that can cost the owner, and a settlement below it fails whole.
//!
//! The mock pool is 1000 TAO / 4000 alpha at equal balancer weights; interest is zero within a
//! block, so a same-block round trip's net is the pure trading result.

use frame_support::{assert_err, assert_ok, traits::Hooks};
use sp_core::U256;
use sp_runtime::Percent;
use sp_weights::Weight;
use subtensor_swap_interface::DerivativesPoolInterface;

use super::*;
use crate::{Parked, position::*};

fn trader() -> U256 {
    U256::from(21)
}
fn pusher() -> U256 {
    U256::from(22)
}

/// A live pool with its moving price pinned to spot and the two wallets registered.
fn setup_pinned() {
    setup();
    settle_moving_price(netuid());
    for who in [trader(), pusher()] {
        let _ = SubtensorModule::create_account_if_non_existent(&who, &alice_hotkey());
    }
}

fn parked() -> (u64, u64) {
    let (tao, alpha) = Parked::<Test>::get(netuid());
    (tao.into(), alpha.into())
}

/// The pool's TAO, wherever it sits: in the reserve, in the balancer reservoir, or parked.
fn pool_tao_all() -> u64 {
    reserves(netuid()).0
        + u64::from(pallet_subtensor_swap::BalancerTaoReservoir::<Test>::get(
            netuid(),
        ))
        + parked().0
}

fn pool_alpha_all() -> u64 {
    reserves(netuid()).1
        + u64::from(pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::get(
            netuid(),
        ))
        + parked().1
}

fn quote_buy(alpha: u64) -> u64 {
    <SubtensorModule as DerivativesPoolInterface<AccountId>>::quote_buy(netuid(), alpha.into())
        .into()
}

/// `who` sells `alpha` into the pool; returns the TAO received.
fn sell(who: U256, alpha: u64) -> u64 {
    let before = balance(&who);
    assert_ok!(SubtensorModule::remove_stake(
        RuntimeOrigin::signed(who),
        alice_hotkey(),
        netuid(),
        alpha.into()
    ));
    balance(&who) - before
}

/// `who` is given `tao` and buys alpha with it; returns the alpha received.
fn buy(who: U256, tao: u64) -> u64 {
    let before = stake(&who, &alice_hotkey(), netuid());
    add_balance(&who, tao);
    assert_ok!(SubtensorModule::add_stake(
        RuntimeOrigin::signed(who),
        alice_hotkey(),
        netuid(),
        tao.into()
    ));
    stake(&who, &alice_hotkey(), netuid()) - before
}

fn other(side: Side) -> Side {
    match side {
        Side::Short => Side::Long,
        Side::Long => Side::Short,
    }
}

/// What a close of `who`'s short would pay right now, by the pool's own quote:
/// cushion + proceeds - the cost of buying the debt back - interest.
fn quoted_short_payout(who: U256) -> u64 {
    let pos = position(&who, netuid()).unwrap();
    let (proceeds, debt, _) = legs(&pos);
    let interest = u64::from(pos.interest_due(System::block_number()));
    (u64::from(pos.cushion) + proceeds)
        .saturating_sub(quote_buy(debt))
        .saturating_sub(interest)
}

fn assert_nothing_left_behind() {
    assert!(position(&trader(), netuid()).is_none());
    assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), 0);
    assert_eq!(Footprint::<Test>::get(netuid(), Side::Long), 0);
    assert_eq!(balance(&pallet_account()), parked().0);
    assert_eq!(
        stake(&pallet_account(), &pallet_hotkey(), netuid()),
        parked().1
    );
}

// ── Stepwise unwind ──────────────────────────────────────────────────────────

/// Open `deposit` at 1x on `side` and unwind it in `steps` equal-fraction reductions, the
/// last one a `close`, all in one block. Returns the trader's net TAO and the pool's TAO and
/// alpha deltas. Along the way: nothing reaches the pool from a partial except what its own
/// buyback spent, the escrow never moves, and the footprint says so.
fn unwind_in_steps(side: Side, deposit: u64, steps: u32, pinned: bool) -> (i128, i128, i128) {
    new_test_ext().execute_with(|| {
        setup();
        if pinned {
            settle_moving_price(netuid());
        }
        add_balance(&trader(), deposit);
        let wallet0 = balance(&trader());
        let (pool_tao0, pool_alpha0) = (pool_tao_all(), pool_alpha_all());
        assert_ok!(add_at(trader(), side, deposit, 100));
        let opened = position(&trader(), netuid()).unwrap();
        let (_, _, escrow0) = legs(&opened);

        let mut bought_back = 0u64;
        for i in 0..steps.saturating_sub(1) {
            let before = position(&trader(), netuid()).unwrap();
            let held = u64::from(before.exposure_tao);
            let (tao_before, alpha_before) = reserves(netuid());
            let slice = held / u64::from(steps - i);
            assert_ok!(add_at(trader(), other(side), slice, 100));
            let after = position(&trader(), netuid()).unwrap();
            assert_eq!(after.side(), side, "a reduction must not flip");
            let (_, debt_after, escrow_after) = legs(&after);
            let (tao_after, alpha_after) = reserves(netuid());
            // The escrow stays put and stays in the footprint; nothing is parked.
            assert_eq!(escrow_after, escrow0);
            assert_eq!(
                Footprint::<Test>::get(netuid(), side),
                after.legs.footprint()
            );
            assert_eq!(parked(), (0, 0));
            match side {
                Side::Short => {
                    // The buyback took alpha out of the pool and put TAO in; the pool was
                    // handed nothing else. The bought alpha is held for it.
                    let (_, debt_before, _) = legs(&before);
                    bought_back += debt_before - debt_after;
                    assert!(tao_after > tao_before);
                    assert!(alpha_after < alpha_before);
                    assert!(u64::from(after.held.1) >= bought_back);
                    assert_eq!(after.held.0, TaoBalance::ZERO);
                }
                Side::Long => {
                    // The sale put alpha into the pool and took TAO out; the repaid TAO is held.
                    assert!(alpha_after > alpha_before);
                    assert!(tao_after < tao_before);
                    assert!(after.held.0 > TaoBalance::ZERO);
                    assert_eq!(after.held.1, AlphaBalance::ZERO);
                }
            }
        }
        assert_ok!(close(trader()));
        assert_nothing_left_behind();
        (
            balance(&trader()) as i128 - wallet0 as i128,
            pool_tao_all() as i128 - pool_tao0 as i128,
            pool_alpha_all() as i128 - pool_alpha0 as i128,
        )
    })
}

#[test]
fn unwinding_a_short_in_steps_pays_no_more_than_one_close() {
    // DV-03 / DV-14: 25 TAO at 1x on a pinned pool, one block. Before the fix: 1 step -1 rao,
    // 5 steps +3,831,918, 25 steps +4,995,260, 100 steps +5,227,681, all out of the pool.
    for pinned in [true, false] {
        for deposit in [25 * TAO, 130 * TAO] {
            let (single, pool_tao_single, _) = unwind_in_steps(Side::Short, deposit, 1, pinned);
            assert!((-4..=0).contains(&single), "one close nets {single}");
            assert!(pool_tao_single >= 0);
            for steps in [2u32, 5, 25, 100] {
                let (net, pool_tao, pool_alpha) =
                    unwind_in_steps(Side::Short, deposit, steps, pinned);
                assert!(
                    net <= single + 1,
                    "pinned={pinned} deposit={deposit} steps={steps}: paid {net}, one close paid {single}"
                );
                // The pool ends whole: every rao the trader lost is the pool's, alpha to the
                // rao, and it never lost TAO.
                assert!(pool_tao >= 0, "steps={steps}: pool TAO {pool_tao}");
                assert_eq!(net + pool_tao, 0);
                assert_eq!(pool_alpha, 0);
            }
        }
    }
}

#[test]
fn unwinding_a_long_in_steps_pays_no_more_than_one_close() {
    // The long side leaked symmetrically (DV-14: 5 steps +3,769,804; 25 steps +4,931,786).
    for pinned in [true, false] {
        let (single, _, pool_alpha_single) = unwind_in_steps(Side::Long, 25 * TAO, 1, pinned);
        assert!((-4..=0).contains(&single), "one close nets {single}");
        assert!(pool_alpha_single >= 0);
        for steps in [2u32, 5, 25, 100] {
            let (net, pool_tao, pool_alpha) = unwind_in_steps(Side::Long, 25 * TAO, steps, pinned);
            assert!(
                net <= single + 1,
                "pinned={pinned} steps={steps}: paid {net}, one close paid {single}"
            );
            assert_eq!(net + pool_tao, 0);
            assert!(pool_alpha >= 0, "steps={steps}: pool alpha {pool_alpha}");
        }
    }
}

#[test]
fn stepwise_cycles_block_after_block_never_take_from_the_pool() {
    // DV-03's repeatable drain: 30 blocks, each an open of 25 TAO unwound in 5 steps, with
    // `on_idle` releasing parked pairs between blocks as the chain would. Before the fix the
    // trader netted +97,746,416 rao over the run. Now the pool's TAO never goes down.
    new_test_ext().execute_with(|| {
        setup_pinned();
        add_balance(&trader(), 1_000 * TAO);
        let wallet0 = balance(&trader());
        let pool0 = pool_tao_all();
        let mut pool_prev = pool0;
        for cycle in 0..30u64 {
            let now = 2 + cycle;
            System::set_block_number(now);
            assert_ok!(add_at(trader(), Side::Short, 25 * TAO, 100));
            for i in 0..4u32 {
                let held = u64::from(position(&trader(), netuid()).unwrap().exposure_tao);
                assert_ok!(add_at(trader(), Side::Long, held / u64::from(5 - i), 100));
            }
            assert_ok!(close(trader()));
            Derivatives::on_idle(now, Weight::MAX);
            assert!(
                pool_tao_all() >= pool_prev,
                "cycle {cycle}: pool TAO fell {pool_prev} -> {}",
                pool_tao_all()
            );
            pool_prev = pool_tao_all();
        }
        let gain = balance(&trader()) as i128 - wallet0 as i128;
        assert!(gain <= 0, "trader netted {gain} over 30 cycles");
        assert_eq!(gain + (pool_tao_all() as i128 - pool0 as i128), 0);
        assert_eq!(pool_alpha_all(), POOL_ALPHA);
        assert_nothing_left_behind();
    });
}

#[test]
fn a_forfeit_returns_what_partial_settlements_held() {
    // Reduce a short by half, then let the weekly collections starve the rest. The forfeit
    // hands back everything the pallet holds for the position: the rest in kind, the whole
    // escrow, and the alpha the reduction bought back.
    new_test_ext().execute_with(|| {
        setup_pinned();
        set_interest(Percent::one());
        let (t0, a0) = reserves(netuid());
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add_at(alice(), Side::Long, DEPOSIT / 2, 100));
        let rest = position(&alice(), netuid()).unwrap();
        let (_, _, escrow) = legs(&rest);
        assert!(rest.held.1 > AlphaBalance::ZERO);
        assert_eq!(escrow, POOL_TAO / 100);
        let paid_out = balance(&alice()) - (100 * TAO - DEPOSIT);

        let mut week = 1;
        let mut before_forfeit = None;
        while let Some(pos) = position(&alice(), netuid()) {
            before_forfeit = Some((
                reserves(netuid()),
                balance(&pallet_account()),
                u64::from(pos.held.1),
            ));
            run_to(1 + week * WEEK);
            week += 1;
        }
        assert_eq!(last_closer(), Closer::Starved);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), 0);
        // The forfeit week trades nothing: the pool got exactly what the pallet held, the TAO
        // in one piece and the held alpha with it. Over the whole life of the position the
        // pool has all the TAO but what the reduction paid out (the interest bought and
        // recycled alpha on top).
        let ((t_prev, a_prev), pallet_prev, held_alpha) = before_forfeit.unwrap();
        let (t1, a1) = reserves(netuid());
        assert_eq!(a1, a_prev + held_alpha);
        assert_eq!(t1, t_prev + pallet_prev);
        assert!(t1 >= t0 + DEPOSIT - paid_out - 2, "{t0} -> {t1}");
        assert!(a1 < a0);
    });
}

#[test]
fn dissolution_returns_what_partial_settlements_held() {
    new_test_ext().execute_with(|| {
        setup_pinned();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add_at(alice(), Side::Long, DEPOSIT / 2, 100));
        let rest = position(&alice(), netuid()).unwrap();
        let held_alpha = u64::from(rest.held.1);
        assert!(held_alpha > 0);
        let (t0, a0) = reserves(netuid());
        let pallet_tao = balance(&pallet_account());

        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));
        let wallet = balance(&alice());
        settle_all_for_dissolution(netuid());
        let payout = balance(&alice()) - wallet;
        assert!(position(&alice(), netuid()).is_none());
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
        // Everything the pallet held for the pool landed in the reserves: the held alpha with
        // the rest, all the TAO but the payout.
        let (t1, a1) = reserves(netuid());
        assert_eq!(a1, a0 + held_alpha);
        assert_eq!(t1, t0 + pallet_tao - payout);
    });
}

// ── The owner's floor ────────────────────────────────────────────────────────

#[test]
fn a_close_sandwiched_below_its_floor_fails_and_the_position_is_untouched() {
    // DV-08: a third party buys `b * y` ahead of a max short's close. Without a floor the
    // closer lost 13% of the cushion at b=0.05, 94% at b=0.3, and at b=0.5 the quote said
    // underwater and the whole cushion was forfeited in kind. With the floor at 1% under the
    // honest quote, every one of those closes fails and leaves the position as it was; once
    // the price is back, the same close goes through at or above the floor.
    let deposit = new_test_ext().execute_with(|| {
        setup_pinned();
        let phi = (1.0 - (1.0 - 0.25f64).sqrt()) * 0.999_99;
        (phi * reserves(netuid()).0 as f64) as u64
    });
    for b in [0.05, 0.3, 0.5] {
        new_test_ext().execute_with(|| {
            setup_pinned();
            add_balance(&trader(), deposit);
            assert_ok!(add_at(trader(), Side::Short, deposit, 100));
            let before = position(&trader(), netuid()).unwrap();
            let quoted = quoted_short_payout(trader());
            let floor = quoted - quoted / 100;
            let pallet_before = balance(&pallet_account());
            let stake_before = stake(&pallet_account(), &pallet_hotkey(), netuid());
            let wallet_before = balance(&trader());

            let alpha = buy(pusher(), (b * POOL_TAO as f64) as u64);
            let pushed = quoted_short_payout(trader());
            assert!(
                pushed < floor,
                "b={b}: the push did not move the quote below the floor"
            );

            assert_err!(
                close_min(trader(), floor),
                Error::<Test>::SettlementBelowMinimum
            );
            // Nothing moved: not the position, not a rao of the pallet's, not the pool's.
            assert_eq!(position(&trader(), netuid()), Some(before.clone()));
            assert_eq!(balance(&pallet_account()), pallet_before);
            assert_eq!(
                stake(&pallet_account(), &pallet_hotkey(), netuid()),
                stake_before
            );
            assert_eq!(balance(&trader()), wallet_before);
            assert_eq!(parked(), (0, 0));
            assert_eq!(
                Footprint::<Test>::get(netuid(), Side::Short),
                before.legs.footprint()
            );

            // The pusher unwinds; the price is back; the same floor is now met.
            sell(pusher(), alpha);
            let quoted_again = quoted_short_payout(trader());
            assert!(quoted_again >= floor);
            assert_ok!(close_min(trader(), floor));
            assert_eq!(last_closer(), Closer::Owner);
            let (payout, _, _) = last_closed_event();
            assert!(
                payout >= floor,
                "b={b}: paid {payout} under the floor {floor}"
            );
            assert_nothing_left_behind();
        });
    }
}

#[test]
fn a_zero_floor_is_no_floor_and_an_underwater_close_still_forfeits() {
    new_test_ext().execute_with(|| {
        setup_pinned();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        buy(pusher(), 2_000 * TAO);
        let (proceeds, debt, escrow) = legs(&position(&alice(), netuid()).unwrap());
        let (t0, a0) = (pool_tao_all(), pool_alpha_all());
        // With a floor, an underwater close is refused: nothing is forfeited.
        assert_err!(close_min(alice(), 1), Error::<Test>::SettlementBelowMinimum);
        assert!(position(&alice(), netuid()).is_some());
        // Without one, it is the in-kind forfeit it always was.
        assert_ok!(close_min(alice(), 0));
        assert_eq!(last_closer(), Closer::Underwater);
        let (payout, _, shortfall) = last_closed_event();
        assert_eq!(payout, 0);
        assert_eq!(shortfall, debt);
        // The pushed spot is off the moving price, so the pair is parked: the pool's either way.
        assert_eq!(pool_alpha_all(), a0);
        assert_eq!(pool_tao_all(), t0 + DEPOSIT + proceeds + escrow);
    });
}

#[test]
fn a_reduction_below_its_floor_fails_whole() {
    new_test_ext().execute_with(|| {
        setup_pinned();
        assert_ok!(add(alice(), Side::Short, 2 * DEPOSIT));
        let before = position(&alice(), netuid()).unwrap();
        let wallet = balance(&alice());
        // Half the position: about half the cushion comes back. Ask for more than that.
        assert_err!(
            add_at_min(alice(), Side::Long, DEPOSIT, 100, DEPOSIT + DEPOSIT / 10),
            Error::<Test>::SettlementBelowMinimum
        );
        assert_eq!(position(&alice(), netuid()), Some(before));
        assert_eq!(balance(&alice()), wallet);
        // Ask for a little less than that and it goes through, paying at least the floor.
        assert_ok!(add_at_min(
            alice(),
            Side::Long,
            DEPOSIT,
            100,
            DEPOSIT - DEPOSIT / 10
        ));
        let (_, payout, _, _) = last_reduced_event();
        assert!(payout >= DEPOSIT - DEPOSIT / 10);
        assert_eq!(balance(&alice()), wallet + payout);
    });
}

#[test]
fn a_flip_checks_the_floor_against_the_position_it_closes() {
    new_test_ext().execute_with(|| {
        setup_pinned();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let wallet = balance(&alice());
        // Closing the short pays about the deposit back; the rest opens a long.
        assert_err!(
            add_at_min(
                alice(),
                Side::Long,
                3 * DEPOSIT,
                100,
                DEPOSIT + DEPOSIT / 10
            ),
            Error::<Test>::SettlementBelowMinimum
        );
        assert_eq!(position(&alice(), netuid()).unwrap().side(), Side::Short);
        assert_eq!(balance(&alice()), wallet);

        assert_ok!(add_at_min(
            alice(),
            Side::Long,
            3 * DEPOSIT,
            100,
            DEPOSIT - DEPOSIT / 10
        ));
        let (payout, _, _) = last_closed_event();
        assert!(payout >= DEPOSIT - DEPOSIT / 10);
        let long = position(&alice(), netuid()).unwrap();
        assert_eq!(long.side(), Side::Long);
        assert_eq!(long.cushion, TaoBalance::from(2 * DEPOSIT));
        assert_eq!(balance(&alice()), wallet + payout - 2 * DEPOSIT);
    });
}

#[test]
fn a_floor_on_an_add_that_only_opens_or_grows_is_refused() {
    new_test_ext().execute_with(|| {
        setup_pinned();
        // Nothing is paid out by an open, so any floor is unmet.
        assert_err!(
            add_at_min(alice(), Side::Short, DEPOSIT, 100, 1),
            Error::<Test>::SettlementBelowMinimum
        );
        assert!(position(&alice(), netuid()).is_none());
        assert_ok!(add_at_min(alice(), Side::Short, DEPOSIT, 100, 0));
        // Same for a same-side add.
        assert_err!(
            add_at_min(alice(), Side::Short, DEPOSIT, 100, 1),
            Error::<Test>::SettlementBelowMinimum
        );
        assert_ok!(add_at_min(alice(), Side::Short, DEPOSIT, 100, 0));
        assert_eq!(
            position(&alice(), netuid()).unwrap().cushion,
            TaoBalance::from(2 * DEPOSIT)
        );
    });
}

#[test]
fn the_floor_is_on_the_payout_after_interest() {
    new_test_ext().execute_with(|| {
        setup_pinned();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let pos = position(&alice(), netuid()).unwrap();
        System::set_block_number(1 + 30 * DAY);
        let interest = u64::from(pos.interest_due(1 + 30 * DAY));
        assert!(interest > 0);
        let quoted = quoted_short_payout(alice());
        // A floor between the payout before and after interest is not met.
        assert_err!(
            close_min(alice(), quoted + interest / 2),
            Error::<Test>::SettlementBelowMinimum
        );
        assert_ok!(close_min(alice(), quoted - 10));
        let (payout, interest_paid, _) = last_closed_event();
        assert_eq!(interest_paid, interest);
        assert!(payout >= quoted - 10 && payout <= quoted);
    });
}

#[test]
fn the_floor_does_not_change_what_a_close_pays() {
    // The same close with and without a floor it clears pays the same to the rao.
    let without = new_test_ext().execute_with(|| {
        setup_pinned();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(close_min(alice(), 0));
        last_closed_event().0
    });
    new_test_ext().execute_with(|| {
        setup_pinned();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(close_min(alice(), without));
        assert_eq!(last_closed_event().0, without);
    });
}
