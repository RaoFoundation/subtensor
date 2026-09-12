//! The attacks the safety rules exist for, run against the real pool. Each test is one of the
//! sequences that paid before the rules (a sandwich of an owner's own close, a third party
//! front-running an honest close, a long that dumps into its own lifted price, a short that
//! prunes its own subnet, a long paid nothing because another drew the reserve first, a weekly
//! tick that forfeits a position dissolution was about to pay) and asserts it no longer does,
//! plus the mechanics the rules rely on: parked liquidity and the cap on the real footprint.
//!
//! The mock pool is 1000 TAO / 4000 alpha at equal balancer weights, and `y` below is its TAO
//! reserve. Attacker P&L is TAO in hand at the end against the honest alternative.

use frame_support::{assert_err, assert_ok, traits::Hooks, weights::WeightMeter};
use sp_core::U256;
use sp_runtime::Percent;
use sp_weights::Weight;
use subtensor_runtime_common::SubnetDissolveHook;
use subtensor_swap_interface::Perquintill;

use super::*;
use crate::{Parked, position::*};

fn w1() -> U256 {
    U256::from(11)
}
fn w2() -> U256 {
    U256::from(12)
}

/// A live pool with its moving price pinned to spot, as a quiet market would have it, and the
/// two attacker wallets registered.
fn setup_pinned() {
    setup();
    settle_moving_price(netuid());
    for who in [w1(), w2()] {
        let _ = SubtensorModule::create_account_if_non_existent(&who, &alice_hotkey());
    }
}

fn set_pool_share(percent: u8) {
    let mut params = Params::<Test>::get();
    params.pool_share = Percent::from_percent(percent);
    assert_ok!(Derivatives::sudo_set_params(RuntimeOrigin::root(), params));
}

/// Deposit that lifts just under the largest slice the cap admits on an untouched 0.5/0.5
/// pool: `phi (2 - phi) = pool_share`, deposit `= phi * TAO reserve / leverage`.
fn max_deposit(pool_share: u8, leverage_percent: u16) -> u64 {
    let kappa = pool_share as f64 / 100.0;
    let phi = (1.0 - (1.0 - kappa).sqrt()) * 0.999_99;
    let (tao, _) = reserves(netuid());
    (phi * tao as f64 * 100.0 / leverage_percent as f64) as u64
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

fn parked() -> (u64, u64) {
    let (tao, alpha) = Parked::<Test>::get(netuid());
    (tao.into(), alpha.into())
}

/// The pool's TAO, wherever it is: in the reserve or parked on its way back.
fn pool_tao_all() -> u64 {
    reserves(netuid()).0 + parked().0
}

fn pool_alpha_all() -> u64 {
    reserves(netuid()).1 + parked().1
}

fn events_of<F: Fn(&Event<Test>) -> bool>(pred: F) -> usize {
    System::events()
        .into_iter()
        .filter(|record| match &record.event {
            RuntimeEvent::Derivatives(event) => pred(event),
            _ => false,
        })
        .count()
}

/// Wallet 1 opens the largest 1x short the cap admits, wallet 2 buys `b * y` of alpha, wallet
/// 1 closes, wallet 2 sells the alpha back. Both wallets start in TAO. Returns
/// `(total, sandwich, position)` P&L in rao, plus whether the close was underwater.
fn short_sandwich(pool_share: u8, b: f64) -> (i128, i128, i128, bool) {
    new_test_ext().execute_with(|| {
        setup_pinned();
        set_pool_share(pool_share);
        let deposit = max_deposit(pool_share, 100);
        add_balance(&w1(), deposit);
        let w1_0 = balance(&w1());
        assert_ok!(add_at(w1(), Side::Short, deposit, 100));
        let tao_in = (b * POOL_TAO as f64) as u64;
        let alpha = buy(w2(), tao_in);
        let w2_0 = balance(&w2());
        assert_ok!(close(w1()));
        let underwater = last_closer() == Closer::Underwater;
        sell(w2(), alpha);
        let position = balance(&w1()) as i128 - w1_0 as i128;
        let sandwich = (balance(&w2()) - w2_0) as i128 - tao_in as i128;
        // Whatever was parked is still the pool's: it never left the pallet.
        assert!(pool_tao_all() as i128 >= POOL_TAO as i128 - (sandwich + position).max(0));
        (sandwich + position, sandwich, position, underwater)
    })
}

/// The long-side mirror: wallet 1 opens the largest long at `leverage`, wallet 2 sells `h * x`
/// alpha, wallet 1 closes, wallet 2 buys the alpha back so its alpha book is square.
fn long_sandwich(pool_share: u8, leverage: u16, h: f64) -> (i128, i128, i128, bool) {
    new_test_ext().execute_with(|| {
        setup_pinned();
        set_pool_share(pool_share);
        let deposit = max_deposit(pool_share, leverage);
        add_balance(&w1(), deposit);
        let w1_0 = balance(&w1());
        assert_ok!(add_at(w1(), Side::Long, deposit, leverage));
        let h_alpha = (h * POOL_ALPHA as f64) as u64;
        give_stake(&w2(), &alice_hotkey(), netuid(), h_alpha);
        sell(w2(), h_alpha);
        assert_ok!(close(w1()));
        let underwater = last_closer() == Closer::Underwater;
        // Buy back until wallet 2 holds `h_alpha` again: quote, top up for the fee and
        // rounding, sell any surplus.
        let need: u64 = <Swap as subtensor_swap_interface::SwapHandler>::tao_needed_for_alpha(
            netuid(),
            h_alpha.into(),
        )
        .into();
        let spend = need + need / 1_500 + 10;
        add_balance(&w2(), spend);
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(w2()),
            alice_hotkey(),
            netuid(),
            spend.into()
        ));
        let held = stake(&w2(), &alice_hotkey(), netuid());
        assert!(held >= h_alpha, "buy-back short: {held} < {h_alpha}");
        if held > h_alpha {
            sell(w2(), held - h_alpha);
        }
        let position = balance(&w1()) as i128 - w1_0 as i128;
        let sandwich = balance(&w2()) as i128 - spend as i128;
        (sandwich + position, sandwich, position, underwater)
    })
}

// ── Sandwiching one's own close ──────────────────────────────────────────────

#[test]
fn sandwiching_your_own_short_close_never_pays() {
    // Before the rules this paid +1.4% of y at b = 0.25 (solvent, the re-add leak alone) and
    // +22% at b = 4 (underwater, the pot swapped at the pumped price as well). Now the
    // underwater close swaps nothing and the solvent close's slice is parked, so the sandwich
    // wallet's round trip is a plain round trip and the position wallet pays for its own push.
    for b in [0.25, 0.5, 1.0, 2.0, 4.0] {
        let (total, sandwich, position, underwater) = short_sandwich(25, b);
        assert!(
            total <= 0,
            "b={b}: self-sandwich paid {total} (sandwich {sandwich}, position {position})"
        );
        if b >= 0.5 {
            assert!(underwater, "b={b}: the max short should be underwater");
        }
    }
    // The same at every share root might set.
    for pool_share in [5u8, 10, 50] {
        for b in [0.25, 1.0, 4.0] {
            let (total, ..) = short_sandwich(pool_share, b);
            assert!(total <= 0, "share {pool_share}% b={b}: paid {total}");
        }
    }
}

#[test]
fn sandwiching_your_own_long_close_never_pays() {
    // The mirror on the long side, at 1x and at the 1.5x ceiling: the leak was in the slice
    // size the cap sets, not in the leverage, and it is gone at both.
    for leverage in [100u16, 150] {
        for h in [0.25, 0.5, 0.87, 1.5, 3.0] {
            let (total, sandwich, position, _) = long_sandwich(25, leverage, h);
            assert!(
                total <= 0,
                "{leverage}% h={h}: self-sandwich paid {total} (sandwich {sandwich}, position {position})"
            );
        }
    }
}

#[test]
fn front_running_an_honest_close_cannot_take_from_the_pool() {
    // A third party pushes the price before someone else's max short closes and sells back
    // after. It can still hurt the closer, but every TAO it takes comes from the closer, none
    // from the pool: the pool ends with at least the TAO it started with, wherever it sits.
    let unsandwiched = new_test_ext().execute_with(|| {
        setup_pinned();
        let deposit = max_deposit(25, 100);
        add_balance(&w1(), deposit);
        let b0 = balance(&w1());
        assert_ok!(add_at(w1(), Side::Short, deposit, 100));
        assert_ok!(close(w1()));
        assert_eq!(parked(), (0, 0));
        balance(&w1()) as i128 - b0 as i128
    });
    // An honest round trip costs rounding only.
    assert!(unsandwiched <= 0 && unsandwiched > -1_000, "{unsandwiched}");
    for b in [0.05, 0.25, 0.5, 1.0] {
        new_test_ext().execute_with(|| {
            setup_pinned();
            let deposit = max_deposit(25, 100);
            add_balance(&w1(), deposit);
            let w1_0 = balance(&w1());
            assert_ok!(add_at(w1(), Side::Short, deposit, 100));
            let tao_in = (b * POOL_TAO as f64) as u64;
            let alpha = buy(w2(), tao_in);
            let w2_0 = balance(&w2());
            assert_ok!(close(w1()));
            sell(w2(), alpha);
            let closer = balance(&w1()) as i128 - w1_0 as i128;
            let sandwich = (balance(&w2()) - w2_0) as i128 - tao_in as i128;
            assert!(
                sandwich <= -closer,
                "b={b}: front-runner took {sandwich} but the closer only lost {}",
                -closer
            );
            assert!(
                pool_tao_all() >= POOL_TAO,
                "b={b}: pool TAO {} < {POOL_TAO}",
                pool_tao_all()
            );
            assert_eq!(pool_alpha_all(), POOL_ALPHA);
        });
    }
}

// ── Long, then dump ──────────────────────────────────────────────────────────

/// Sell `h_alpha` into the untouched pool.
fn honest_dump(h_alpha: u64) -> u64 {
    new_test_ext().execute_with(|| {
        setup_pinned();
        give_stake(&w1(), &alice_hotkey(), netuid(), h_alpha);
        sell(w1(), h_alpha)
    })
}

#[test]
fn long_then_dump_loses_at_the_leverage_ceiling() {
    // Wallet 1 holds `h * x` alpha outside, opens the largest long the cap admits, dumps the
    // alpha into its own lifted price, and closes (underwater past h ~ 0.33, so it walks away
    // from the debt). At 2x this paid +0.48% of y at h = 0.87; the bound is
    // `L <= 1 + sqrt(1 - pool_share)` = 1.87x. At the 1.5x ceiling it loses at every h.
    for h in [0.25, 0.5, 0.75, 0.87, 1.0, 1.5] {
        let h_alpha = (h * POOL_ALPHA as f64) as u64;
        let honest = honest_dump(h_alpha);
        new_test_ext().execute_with(|| {
            setup_pinned();
            let deposit = max_deposit(25, 150);
            add_balance(&w1(), deposit);
            give_stake(&w1(), &alice_hotkey(), netuid(), h_alpha);
            let b0 = balance(&w1());
            assert_ok!(add_at(w1(), Side::Long, deposit, 150));
            sell(w1(), h_alpha);
            assert_ok!(close(w1()));
            let attacker = balance(&w1()) as i128 - b0 as i128 - honest as i128;
            assert!(attacker < 0, "h={h}: long-then-dump paid {attacker}");
        });
    }
    // 2x is no longer on offer at all.
    new_test_ext().execute_with(|| {
        setup_pinned();
        add_balance(&w1(), 100 * TAO);
        assert_err!(
            add_at(w1(), Side::Long, DEPOSIT, 200),
            Error::<Test>::LeverageOutOfRange
        );
    });
}

// ── The cap on the real footprint ────────────────────────────────────────────

#[test]
fn pool_share_is_enforced_on_the_real_footprint_at_any_balancer_weight() {
    // On a TAO-heavy pool a long takes more alpha per TAO than the constant-product estimate
    // said: at a TAO weight of 0.7 the "25%" long the estimate admitted really took 38% of
    // the alpha reserve. The cap is now checked on `proceeds + escrow` after the swap.
    new_test_ext().execute_with(|| {
        setup();
        set_tao_weight(netuid(), Perquintill::from_percent(70));
        settle_moving_price(netuid());
        let (_, alpha0) = reserves(netuid());
        let cap = alpha0 / 4;

        // The deposit the equal-weight estimate would admit at the cap: phi = 13.4% at 1.5x.
        let over = max_deposit(25, 150);
        add_balance(&alice(), over);
        assert_err!(
            add_at(alice(), Side::Long, over, 150),
            Error::<Test>::PoolCapExceeded
        );
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Long), 0);

        // Grow the long until the cap refuses it: every add that landed kept the booked
        // footprint at or under the cap, and the sum of what the pool actually lost matches
        // the book.
        let step = 5 * TAO;
        add_balance(&alice(), 100 * step);
        let mut adds = 0;
        while add_at(alice(), Side::Long, step, 150).is_ok() {
            adds += 1;
            assert!(adds < 100);
        }
        assert!(adds > 0);
        let footprint = Footprint::<Test>::get(netuid(), Side::Long);
        assert!(footprint <= cap, "{footprint} > {cap}");
        assert!(footprint > cap * 8 / 10, "{footprint} is well under {cap}");
        let (_, alpha1) = reserves(netuid());
        assert_eq!(alpha0 - alpha1, footprint);
        // The refused add left nothing behind.
        assert_eq!(
            balance(&pallet_account()),
            u64::from(position(&alice(), netuid()).unwrap().cushion)
        );
    });
}

// ── Dissolution ──────────────────────────────────────────────────────────────

#[test]
fn dissolution_charges_a_self_impacting_short_at_the_moving_price() {
    // The attack: open the largest short on the subnet about to be pruned, trigger the prune,
    // and be charged the spot price the short itself pushed down. At 130 TAO this paid
    // +11.3% of the deposit. With a moving price on the subnet, the debt is charged at the
    // higher of spot and moving; the short loses its own impact instead of keeping it.
    for deposit in [10 * TAO, 50 * TAO, 100 * TAO, 130 * TAO] {
        new_test_ext().execute_with(|| {
            setup_pinned();
            add_balance(&alice(), deposit);
            let moving = moving_pair();
            assert!(moving.0 > 0);
            assert_ok!(add_at(alice(), Side::Short, deposit, 100));
            let (proceeds, debt, _) = legs(&position(&alice(), netuid()).unwrap());
            assert_ok!(SubtensorModule::do_dissolve_network(netuid()));
            let spot = spot_pair();
            assert!(
                spot.0 < moving.0,
                "the short should have pushed the spot down"
            );
            settle_all_for_dissolution(netuid());
            let (short_price, long_price) = dissolution_price_event();
            assert_eq!(short_price, moving);
            assert_eq!(long_price, spot);
            let payout = last_closed_event().0;
            assert_eq!(payout, deposit + proceeds - tao_at_ceil(debt, moving));
            assert!(
                payout < deposit,
                "deposit {deposit}: short kept its own impact, payout {payout}"
            );
        });
    }
}

#[test]
fn dissolution_credits_a_self_impacting_long_at_the_moving_price() {
    // The mirror: a long lifts the spot; at dissolution its alpha is credited at the lower of
    // spot and moving, so it does not get paid for its own lift.
    new_test_ext().execute_with(|| {
        setup_pinned();
        let moving = moving_pair();
        assert_ok!(add_at(alice(), Side::Long, 50 * TAO, 150));
        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));
        let spot = spot_pair();
        assert!(spot.0 > moving.0);
        settle_all_for_dissolution(netuid());
        let (short_price, long_price) = dissolution_price_event();
        assert_eq!(short_price, spot);
        assert_eq!(long_price, moving);
        assert!(last_closed_event().0 < 50 * TAO);
    });
}

#[test]
fn two_winning_longs_are_both_paid_when_the_reserve_cannot_cover_both() {
    // Two longs at a 75% share, the price pumped many times over, then dissolution. Before,
    // whichever the hash order visited first drew its whole credit in TAO and the second,
    // whose credit exceeded what was left, was paid nothing. Now each is paid in TAO as far
    // as the reserve goes and keeps the rest of its alpha as stake, so both are made whole
    // at the long price.
    new_test_ext().execute_with(|| {
        setup_pinned();
        let carol = U256::from(3);
        add_balance(&carol, 100 * TAO);
        set_pool_share(75);
        assert_ok!(add(bob(), Side::Long, 100 * TAO));
        assert_ok!(add(carol, Side::Long, 100 * TAO));
        // The market buys hard and stays there long enough for the moving price to follow.
        buy(U256::from(9), 3_000 * TAO);
        settle_moving_price(netuid());
        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));
        let spot = spot_pair();
        let moving = moving_pair();
        let long_price = if spot.0 < moving.0 { spot } else { moving };
        let reserve = reserves(netuid()).0;

        let mut fair = Vec::new();
        for who in [bob(), carol] {
            let pos = position(&who, netuid()).unwrap();
            let (proceeds, debt, _) = legs(&pos);
            fair.push((
                who,
                (u64::from(pos.cushion) + tao_at_floor(proceeds, long_price))
                    .saturating_sub(debt)
                    .saturating_sub(u64::from(pos.interest_due(System::block_number()))),
            ));
        }
        // The reserve cannot pay both credits in full.
        assert!(fair.iter().map(|(_, v)| v).sum::<u64>() > reserve);

        let before: Vec<u64> = fair.iter().map(|(who, _)| balance(who)).collect();
        settle_all_for_dissolution(netuid());

        let mut alpha_kept = 0;
        for ((who, fair), before) in fair.iter().zip(before) {
            let tao = balance(who) - before;
            let alpha = stake(who, &pallet_hotkey(), netuid());
            let value = tao + tao_at_floor(alpha, long_price);
            assert!(tao > 0 || alpha > 0, "{who:?} was paid nothing");
            // Each rounding step (debt in alpha up, credit down, alpha sold up, alpha kept
            // valued down) costs the owner at most a rao.
            assert_close(value, *fair, 10);
            assert!(value <= *fair);
            alpha_kept += alpha;
        }
        // One of them was paid partly in alpha: the reserve ran dry on the way, and what is
        // left in it is at most the last cushion handed back after that.
        assert!(alpha_kept > 0);
        assert!(reserves(netuid()).0 <= 100 * TAO);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
        assert_eq!(
            events_of(|e| matches!(
                e,
                Event::PositionClosed { payout, .. } if u64::from(*payout) == 0
            )),
            0
        );
    });
}

#[test]
fn an_interest_tick_during_dissolution_does_not_forfeit_a_position() {
    // A short in profit with a thin cushion, dissolved one block before its due block. The
    // hook does not reach the subnet that block; the next block's weekly collection used to
    // find `cushion < interest` and forfeit it, paying the owner nothing for a position the
    // settlement was about to pay. Now the collection leaves a dissolving subnet alone.
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        dump_alpha(400 * TAO);
        // An old position: thin cushion, large weekly interest.
        crate::Positions::<Test>::mutate(alice(), netuid(), |p| {
            let p = p.as_mut().unwrap();
            p.cushion = (TAO / 10).into();
            p.interest_per_year = (20 * TAO).into();
        });
        assert_ok!(SubtensorModule::transfer_tao(
            &pallet_account(),
            &U256::from(3),
            (DEPOSIT - TAO / 10).into()
        ));
        System::set_block_number(WEEK);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));

        // The due block comes and goes: nothing is collected, nothing is forfeited, and the
        // position is booked a week ahead with its clock untouched.
        run_to(1 + WEEK);
        let pos = position(&alice(), netuid()).expect("still open");
        assert_eq!(pos.since, 1);
        assert_eq!(pos.due, 1 + 2 * WEEK);
        assert_eq!(u64::from(pos.cushion), TAO / 10);
        assert_eq!(events_of(|e| matches!(e, Event::PositionClosed { .. })), 0);

        // The hook settles it, and the settlement pays.
        settle_all_for_dissolution(netuid());
        assert_eq!(last_closer(), Closer::Dissolution);
        assert!(last_closed_event().0 > 0);
        assert!(crate::Due::<Test>::iter_keys().next().is_none());
        assert_eq!(balance(&pallet_account()), 0);
    });
}

// ── Parked liquidity ─────────────────────────────────────────────────────────

/// A pumped pool with an underwater max short closed into it: the slice comes back while the
/// spot is far off the moving price, so it is parked. Returns what was parked.
fn park_a_short_close() -> (u64, u64) {
    setup_pinned();
    let deposit = max_deposit(25, 100);
    add_balance(&w1(), deposit);
    assert_ok!(add_at(w1(), Side::Short, deposit, 100));
    let (proceeds, _, escrow) = legs(&position(&w1(), netuid()).unwrap());
    buy(w2(), POOL_TAO);
    let (t_before, a_before) = reserves(netuid());
    assert_ok!(close(w1()));
    assert_eq!(last_closer(), Closer::Underwater);
    // Nothing reached the reserves; the whole in-kind return sits in the pallet.
    assert_eq!(reserves(netuid()), (t_before, a_before));
    assert_eq!(parked(), (deposit + proceeds + escrow, 0));
    assert_eq!(balance(&pallet_account()), deposit + proceeds + escrow);
    assert_eq!(events_of(|e| matches!(e, Event::LiquidityParked { .. })), 1);
    parked()
}

#[test]
fn liquidity_returned_at_a_pushed_price_is_parked_and_released_when_the_spot_is_back() {
    new_test_ext().execute_with(|| {
        let (tao, alpha) = park_a_short_close();

        // Still off: on_idle leaves it. Out of weight: on_idle leaves it.
        Derivatives::on_idle(2, Weight::MAX);
        assert_eq!(parked(), (tao, alpha));
        settle_moving_price(netuid());
        Derivatives::on_idle(2, Weight::zero());
        assert_eq!(parked(), (tao, alpha));

        // Back within the band, with weight to spare: released, price-neutrally.
        let (t0, _) = reserves(netuid());
        let p0 = price(netuid());
        let used = Derivatives::on_idle(3, Weight::MAX);
        assert!(used.ref_time() > 0);
        assert_eq!(parked(), (0, 0));
        assert!(!Parked::<Test>::contains_key(netuid()));
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(reserves(netuid()).0, t0 + tao);
        assert_close(
            price(netuid()).to_bits() as u64,
            p0.to_bits() as u64,
            (p0.to_bits() as u64) >> 40,
        );
        assert_eq!(
            events_of(|e| matches!(e, Event::LiquidityReleased { .. })),
            1
        );
    });
}

#[test]
fn parked_liquidity_goes_to_the_reserves_at_dissolution() {
    new_test_ext().execute_with(|| {
        let (tao, _) = park_a_short_close();
        // Someone else is still open when the subnet dissolves.
        assert_ok!(add(bob(), Side::Long, DEPOSIT));
        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));
        let (t0, _) = reserves(netuid());

        // The parked pair is the first thing the hook returns, before any position settles,
        // and it costs one release on the meter.
        let mut meter = WeightMeter::with_limit(
            <() as crate::weights::WeightInfo>::close()
                + <() as crate::weights::WeightInfo>::release_parked(),
        );
        assert!(!<Derivatives as SubnetDissolveHook>::on_subnet_dissolve(
            netuid(),
            &mut meter
        ));
        assert_eq!(parked(), (0, 0));
        assert_eq!(reserves(netuid()).0, t0 + tao);
        assert!(position(&bob(), netuid()).is_some());

        settle_all_for_dissolution(netuid());
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
        // Everything the pool is owed is in its reserves, which is what the stakers are paid
        // from: nothing is stranded on the pallet.
        assert_eq!(
            balance(&SubtensorModule::get_subnet_account_id(netuid()).unwrap()),
            reserves(netuid()).0
        );
    });
}

#[test]
fn parked_alpha_counts_as_outstanding_for_the_emission_price() {
    // A long forfeited in kind while the spot is off parks alpha. The emission price counts
    // that alpha back into the pool, as it does the open long's footprint, so a subnet cannot
    // be paid emission for alpha the pallet is holding for it.
    new_test_ext().execute_with(|| {
        setup_pinned();
        assert_ok!(add(alice(), Side::Long, 50 * TAO));
        let emission_before = SubtensorModule::get_emission_alpha_price(netuid());
        // The market dumps: the long is underwater, and the spot is far below the moving
        // price, so its alpha is parked.
        dump_alpha(20_000 * TAO);
        assert_ok!(close(alice()));
        assert_eq!(last_closer(), Closer::Underwater);
        let (_, alpha) = parked();
        assert!(alpha > 0);
        assert_eq!(
            <Derivatives as subtensor_runtime_common::DerivativesHook>::long_alpha_outstanding(
                netuid()
            ),
            alpha.into()
        );
        assert!(SubtensorModule::get_emission_alpha_price(netuid()) < emission_before);
        assert_eq!(
            SubtensorModule::get_emission_alpha_price(netuid()),
            <Swap as subtensor_swap_interface::SwapHandler>::alpha_price_for_reserves(
                netuid(),
                (reserves(netuid()).1 + alpha).into(),
                reserves(netuid()).0.into(),
            )
        );
    });
}

#[test]
fn an_honest_close_on_a_quiet_pool_parks_nothing() {
    new_test_ext().execute_with(|| {
        setup_pinned();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add(bob(), Side::Long, DEPOSIT));
        System::set_block_number(1 + DAY);
        assert_ok!(close(alice()));
        assert_ok!(close(bob()));
        assert_eq!(parked(), (0, 0));
        assert_eq!(events_of(|e| matches!(e, Event::LiquidityParked { .. })), 0);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
    });
}

#[test]
fn a_subnet_with_no_moving_price_never_parks() {
    // Until the first `update_moving_price`, the moving price is zero and the gate is off:
    // even an underwater close into a pumped pool goes straight back to the reserves.
    new_test_ext().execute_with(|| {
        setup();
        assert_eq!(moving_pair().0, 0);
        add_balance(&w1(), 200 * TAO);
        assert_ok!(add_at(w1(), Side::Short, 100 * TAO, 100));
        buy(w2(), POOL_TAO);
        let (t0, _) = reserves(netuid());
        assert_ok!(close(w1()));
        assert_eq!(last_closer(), Closer::Underwater);
        assert_eq!(parked(), (0, 0));
        assert!(reserves(netuid()).0 > t0);
        assert_eq!(balance(&pallet_account()), 0);
    });
}
