#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used
)]

pub(crate) mod mock;

use frame_support::{assert_err, assert_ok};
use sp_core::U256;
use sp_runtime::Percent;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::Perquintill;

use crate::{Closer, Error, Event, Footprint, PalletHotkey, Params, Side, position::*};
use mock::*;

const POOL_TAO: u64 = 1_000 * TAO;
const POOL_ALPHA: u64 = 4_000 * TAO;
const DEPOSIT: u64 = 10 * TAO;
const DAY: u64 = 7_200;
const WEEK: u64 = 7 * DAY;

fn netuid() -> NetUid {
    NetUid::from(1u16)
}

fn alice() -> U256 {
    U256::from(1)
}
fn bob() -> U256 {
    U256::from(2)
}
fn alice_hotkey() -> U256 {
    U256::from(101)
}

fn setup() {
    add_dynamic_network(netuid(), POOL_TAO, POOL_ALPHA);
    add_balance(&alice(), 100 * TAO);
    add_balance(&bob(), 100 * TAO);
    let _ = SubtensorModule::create_account_if_non_existent(&alice(), &alice_hotkey());
}

/// Add at the side's maximum leverage: 1x for shorts, 2x for longs.
fn add(who: U256, side: Side, amount: u64) -> sp_runtime::DispatchResult {
    let leverage = match side {
        Side::Short => 100,
        Side::Long => 200,
    };
    add_at(who, side, amount, leverage)
}

fn add_at(who: U256, side: Side, amount: u64, leverage_percent: u16) -> sp_runtime::DispatchResult {
    Derivatives::add(
        RuntimeOrigin::signed(who),
        netuid(),
        side,
        amount.into(),
        leverage_percent,
    )
}

fn close(who: U256) -> sp_runtime::DispatchResult {
    Derivatives::close(RuntimeOrigin::signed(who), netuid())
}

/// Advance to `block` as the chain would: the interest queue is walked block by block, one
/// `on_initialize` per block, until it has caught up.
fn run_to(block: u64) {
    System::set_block_number(block);
    while crate::NextDue::<Test>::get() <= block {
        Derivatives::collect_due(block);
    }
}

/// Set the global interest. The default 25%/year takes a 1x position four years to starve; tests
/// about starvation raise it to 100%/year, which takes about a year.
fn set_interest(rate: Percent) {
    let mut params = Params::<Test>::get();
    params.interest_rate = rate;
    assert_ok!(Derivatives::sudo_set_params(RuntimeOrigin::root(), params));
}

type Pos = Position<u64>;

fn leverage_of(pos: &Pos) -> f64 {
    u64::from(pos.exposure_tao) as f64 / u64::from(pos.cushion) as f64
}

fn assert_close(a: u64, b: u64, tolerance: u64) {
    let diff = a.abs_diff(b);
    assert!(
        diff <= tolerance,
        "{a} vs {b}: differ by {diff} > {tolerance}"
    );
}

/// `(proceeds, debt, escrow)` as raw units, whichever side the position is.
fn legs(pos: &Pos) -> (u64, u64, u64) {
    match pos.legs {
        Legs::Short {
            proceeds,
            debt,
            escrow,
        } => (proceeds.into(), debt.into(), escrow.into()),
        Legs::Long {
            proceeds,
            debt,
            escrow,
        } => (proceeds.into(), debt.into(), escrow.into()),
    }
}

/// `(payout, interest_paid, shortfall)` of the latest `PositionClosed`.
fn last_closed_event() -> (u64, u64, u64) {
    System::events()
        .into_iter()
        .rev()
        .find_map(|record| match record.event {
            RuntimeEvent::Derivatives(Event::PositionClosed {
                payout,
                interest_paid,
                shortfall,
                ..
            }) => Some((
                payout.into(),
                interest_paid.into(),
                match shortfall {
                    Lent::Alpha(amount) => amount.into(),
                    Lent::Tao(amount) => amount.into(),
                },
            )),
            _ => None,
        })
        .expect("PositionClosed event")
}

/// `(fraction, payout, shortfall, exposure_tao)` of the latest `PositionReduced`.
fn last_reduced_event() -> (Perquintill, u64, u64, u64) {
    System::events()
        .into_iter()
        .rev()
        .find_map(|record| match record.event {
            RuntimeEvent::Derivatives(Event::PositionReduced {
                fraction,
                payout,
                shortfall,
                exposure_tao,
                ..
            }) => Some((
                fraction,
                payout.into(),
                match shortfall {
                    Lent::Alpha(amount) => amount.into(),
                    Lent::Tao(amount) => amount.into(),
                },
                exposure_tao.into(),
            )),
            _ => None,
        })
        .expect("PositionReduced event")
}

/// `closed_by` of the latest `PositionClosed`.
fn last_closer() -> Closer {
    System::events()
        .into_iter()
        .rev()
        .find_map(|record| match record.event {
            RuntimeEvent::Derivatives(Event::PositionClosed { closed_by, .. }) => Some(closed_by),
            _ => None,
        })
        .expect("PositionClosed event")
}

// ── Pallet hotkey ────────────────────────────────────────────────────────────

#[test]
fn upgrade_claims_a_fresh_hotkey_for_the_pallet_account() {
    new_test_ext().execute_with(|| {
        let hotkey = pallet_hotkey();
        assert_eq!(Some(hotkey), Derivatives::hotkey_candidate(0));
        assert!(SubtensorModule::coldkey_owns_hotkey(
            &pallet_account(),
            &hotkey
        ));

        // A second upgrade keeps the one already claimed.
        <Derivatives as frame_support::traits::OnRuntimeUpgrade>::on_runtime_upgrade();
        assert_eq!(pallet_hotkey(), hotkey);
    });
}

#[test]
fn claim_skips_a_hotkey_someone_registered_first() {
    new_test_ext().execute_with(|| {
        // A later upgrade block: different parent hash, different candidates.
        System::set_parent_hash(sp_core::H256::repeat_byte(7));
        PalletHotkey::<Test>::kill();
        let taken = Derivatives::hotkey_candidate(0).unwrap();
        let _ = SubtensorModule::create_account_if_non_existent(&alice(), &taken);

        Derivatives::claim_hotkey();

        let hotkey = pallet_hotkey();
        assert_eq!(Some(hotkey), Derivatives::hotkey_candidate(1));
        assert!(SubtensorModule::coldkey_owns_hotkey(
            &pallet_account(),
            &hotkey
        ));
        assert!(SubtensorModule::coldkey_owns_hotkey(&alice(), &taken));
    });
}

#[test]
fn nothing_opens_until_the_hotkey_is_claimed() {
    new_test_ext().execute_with(|| {
        setup();
        PalletHotkey::<Test>::kill();
        assert_err!(
            add(alice(), Side::Short, DEPOSIT),
            Error::<Test>::PalletHotkeyUnset
        );
        assert_eq!(balance(&alice()), 100 * TAO);
    });
}

// ── Open ─────────────────────────────────────────────────────────────────────

#[test]
fn open_short_lifts_and_sells() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());
        let stake0 = total_stake();
        let flow0 = tao_flow(netuid());

        assert_ok!(add(alice(), Side::Short, DEPOSIT));

        let pos = position(&alice(), netuid()).unwrap();
        let (proceeds, debt, escrow) = legs(&pos);
        assert!(matches!(pos.legs, Legs::Short { .. }));
        // 1x leverage: phi = 10 / 1000 = 1%.
        assert_eq!(escrow, POOL_TAO / 100);
        assert_eq!(debt, POOL_ALPHA / 100);
        assert_eq!(u64::from(pos.exposure_tao), POOL_TAO / 100);
        // Selling 1% of alpha into a pool that just lost 1% pays a bit under 1% of TAO.
        assert!(proceeds > 0 && proceeds < POOL_TAO / 100);
        assert_eq!(pos.since, 1);
        assert_eq!(pos.interest_owed, TaoBalance::ZERO);

        // Alice paid the deposit; the pallet holds deposit + escrow + proceeds.
        assert_eq!(balance(&alice()), 100 * TAO - DEPOSIT);
        assert_eq!(balance(&pallet_account()), DEPOSIT + escrow + proceeds);
        // The lifted alpha went back into the pool when sold: AlphaIn is whole, AlphaOut too.
        let (t1, a1) = reserves(netuid());
        assert_eq!(a1, a0);
        assert_eq!(alpha_out(netuid()), out0);
        assert_eq!(t1, t0 - escrow - proceeds);
        assert_eq!(total_stake(), stake0 - escrow - proceeds);
        assert_eq!(tao_flow(netuid()), flow0);
        assert_eq!(
            Footprint::<Test>::get(netuid(), Side::Short),
            escrow + proceeds
        );
    });
}

#[test]
fn open_long_lifts_and_buys() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());
        let stake0 = total_stake();

        assert_ok!(add(alice(), Side::Long, DEPOSIT));

        let pos = position(&alice(), netuid()).unwrap();
        let (proceeds, debt, escrow) = legs(&pos);
        assert!(matches!(pos.legs, Legs::Long { .. }));
        // Longs run at 2x: a 10 TAO cushion lifts 2% of the 1000 TAO pool.
        assert_eq!(debt, POOL_TAO / 50);
        assert_eq!(escrow, POOL_ALPHA / 50);
        assert!(proceeds > 0 && proceeds < POOL_ALPHA / 50);

        // Pallet holds the deposit in TAO and escrow + proceeds as stake.
        assert_eq!(balance(&pallet_account()), DEPOSIT);
        assert_eq!(
            stake(&pallet_account(), &pallet_hotkey(), netuid()),
            escrow + proceeds
        );
        // TAO went out and came straight back in; alpha left the pool.
        let (t1, a1) = reserves(netuid());
        assert_eq!(t1, t0);
        assert_eq!(total_stake(), stake0);
        assert_eq!(a1, a0 - escrow - proceeds);
        assert_eq!(alpha_out(netuid()), out0 + escrow + proceeds);
    });
}

#[test]
fn open_rejects_bad_inputs() {
    new_test_ext().execute_with(|| {
        setup();
        assert_err!(
            add(alice(), Side::Short, TAO / 100),
            Error::<Test>::DepositTooLow
        );
        assert_err!(add(alice(), Side::Short, 0), Error::<Test>::DepositTooLow);
        assert_err!(
            Derivatives::add(
                RuntimeOrigin::signed(alice()),
                NetUid::from(9u16),
                Side::Short,
                DEPOSIT.into(),
                100,
            ),
            Error::<Test>::SubnetNotDynamic
        );
    });
}

#[test]
fn owner_chooses_leverage_up_to_the_side_maximum() {
    new_test_ext().execute_with(|| {
        setup();
        assert_err!(
            add_at(alice(), Side::Short, DEPOSIT, 0),
            Error::<Test>::LeverageOutOfRange
        );
        assert_err!(
            add_at(alice(), Side::Short, DEPOSIT, 101),
            Error::<Test>::LeverageOutOfRange
        );
        assert_err!(
            add_at(alice(), Side::Long, DEPOSIT, 201),
            Error::<Test>::LeverageOutOfRange
        );

        // Half leverage on a short: a 10 TAO cushion lifts 0.5% of the 1000 TAO pool.
        assert_ok!(add_at(alice(), Side::Short, DEPOSIT, 50));
        let short = position(&alice(), netuid()).unwrap();
        assert_eq!(leverage_of(&short), 0.5);
        assert_eq!(u64::from(short.exposure_tao), POOL_TAO / 200);

        // 1.5x on a long, below the 2x maximum.
        assert_ok!(add_at(bob(), Side::Long, DEPOSIT, 150));
        let long = position(&bob(), netuid()).unwrap();
        assert_close((leverage_of(&long) * 1000.0) as u64, 1500, 1);
        let (_, debt, _) = legs(&long);
        assert_close(debt, POOL_TAO * 15 / 1000, 1);
    });
}

#[test]
fn pool_share_caps_each_side_and_zero_pauses_adds() {
    new_test_ext().execute_with(|| {
        setup();
        // kappa = 5% of the TAO reserve. Each 1x/10 TAO short takes ~2% (phi * (2 - phi)).
        let mut params = Params::<Test>::get();
        params.pool_share = Percent::from_percent(5);
        assert_ok!(Derivatives::sudo_set_params(RuntimeOrigin::root(), params));

        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add(bob(), Side::Short, DEPOSIT));
        let charlie = U256::from(3);
        add_balance(&charlie, 100 * TAO);
        // Third one would push the footprint over 5%.
        assert_err!(
            add(charlie, Side::Short, DEPOSIT),
            Error::<Test>::PoolCapExceeded
        );
        // A single oversized position is rejected outright, and so is the whole pool.
        assert_err!(
            add(charlie, Side::Long, 60 * TAO),
            Error::<Test>::PoolCapExceeded
        );
        add_balance(&charlie, 2_000 * TAO);
        assert_err!(
            add(charlie, Side::Short, 1_000 * TAO),
            Error::<Test>::PoolCapExceeded
        );

        // A zero share is the pause: nothing new opens, but everything open still settles.
        params.pool_share = Percent::zero();
        assert_ok!(Derivatives::sudo_set_params(RuntimeOrigin::root(), params));
        assert_err!(
            add(charlie, Side::Short, DEPOSIT),
            Error::<Test>::PoolCapExceeded
        );
        assert_err!(
            add(alice(), Side::Short, DEPOSIT),
            Error::<Test>::PoolCapExceeded
        );
        assert_ok!(add_at(alice(), Side::Long, DEPOSIT / 2, 100));
        assert!(position(&alice(), netuid()).is_some());
        assert_ok!(close(alice()));
        assert_ok!(close(bob()));
    });
}

#[test]
fn a_spot_move_in_the_same_block_buys_no_bigger_slice_and_no_more_room() {
    // What the slice and the cap look like on an untouched pool: a 10 TAO short lifts 1% of the
    // alpha reserve, and a 140 TAO short (14% of the pool, footprint ~26%) is over the cap.
    let honest_debt = new_test_ext().execute_with(|| {
        setup();
        settle_moving_price(netuid());
        assert_err!(
            add(bob(), Side::Short, 140 * TAO),
            Error::<Test>::PoolCapExceeded
        );
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let (_, debt, _) = legs(&position(&alice(), netuid()).unwrap());
        debt
    });

    new_test_ext().execute_with(|| {
        setup();
        settle_moving_price(netuid());
        // Bob dumps 15% of the pool's alpha in this block: the TAO reserve falls by an eighth,
        // the alpha reserve grows by the same. On live reserves a 10 TAO short would now lift a
        // bigger share of a thinner pool, and so about a third more alpha.
        dump_alpha(600 * TAO);
        let (t, a) = reserves(netuid());
        assert!(t < POOL_TAO * 9 / 10 && a > POOL_ALPHA * 11 / 10);
        // Sized at the smoothed price, the slice owes the same alpha the untouched pool would
        // have given, and lifts less TAO.
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let (_, debt, _) = legs(&position(&alice(), netuid()).unwrap());
        assert_close(debt, honest_debt, honest_debt / 200);
        assert!(u64::from(position(&alice(), netuid()).unwrap().exposure_tao) < DEPOSIT);
    });

    new_test_ext().execute_with(|| {
        setup();
        settle_moving_price(netuid());
        // Bob pumps in this block: the TAO reserve is up by a half. On live reserves the short
        // side's cap would be half again as big, and the 140 TAO short would fit (its
        // footprint on the pumped pool is about 267 TAO against a live cap of 375). The cap
        // holds at the smoothed reserve, 250, and it does not.
        add_balance(&bob(), 1_000 * TAO);
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(bob()),
            alice_hotkey(),
            netuid(),
            (500 * TAO).into()
        ));
        assert!(reserves(netuid()).0 > POOL_TAO * 14 / 10);
        assert_err!(
            add(alice(), Side::Short, 140 * TAO),
            Error::<Test>::PoolCapExceeded
        );
    });
}

#[test]
fn interest_is_the_rate_on_exposure_and_frozen_at_open() {
    new_test_ext().execute_with(|| {
        setup();
        let params = Params::<Test>::get();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add(bob(), Side::Long, DEPOSIT));

        // One rate, both sides: 25%/year of exposure. The short's exposure is its 10 TAO
        // deposit at 1x; the long's is 20 TAO at 2x, so it pays twice.
        let short = position(&alice(), netuid()).unwrap();
        let long = position(&bob(), netuid()).unwrap();
        assert_eq!(
            short.interest_per_year,
            params.interest_for(short.exposure_tao)
        );
        assert_eq!(u64::from(short.interest_per_year), DEPOSIT / 4);
        assert_eq!(
            long.interest_per_year,
            params.interest_for(long.exposure_tao)
        );
        assert_close(u64::from(long.interest_per_year), 2 * DEPOSIT / 4, 1);

        // Changing the rate after the open does not reprice a running position.
        set_interest(Percent::from_percent(50));
        assert_eq!(
            position(&alice(), netuid()).unwrap().interest_per_year,
            short.interest_per_year
        );
        // Nothing is booked up front: a close in the opening block pays no interest.
        assert_ok!(close(alice()));
        let (_, interest_paid, _) = last_closed_event();
        assert_eq!(interest_paid, 0);
    });
}

// ── Close ────────────────────────────────────────────────────────────────────

#[test]
fn short_held_a_day_returns_the_deposit_less_a_day_of_interest() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());
        let stake0 = total_stake();
        let p0 = price(netuid());

        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let pos = position(&alice(), netuid()).unwrap();
        System::set_block_number(1 + DAY);
        assert_ok!(close(alice()));

        let interest = u64::from(interest_for_blocks(pos.interest_per_year, DAY));
        assert_eq!(interest, DEPOSIT / 4 / 365);
        let (payout, interest_paid, shortfall) = last_closed_event();
        assert_eq!(interest_paid, interest);
        assert_eq!(shortfall, 0);
        // Buying back the debt costs almost exactly what selling it paid: the round trip loses
        // only rounding, so Alice gets her deposit back minus the interest.
        assert_close(payout, DEPOSIT - interest, 100);
        assert_eq!(balance(&alice()), 100 * TAO - DEPOSIT + payout);
        assert!(position(&alice(), netuid()).is_none());
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), 0);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);

        // The pool got its slice back and the interest as buy pressure: the interest is in the
        // TAO reserve, the alpha it bought is gone from the reserve and from circulation, and
        // the price is a little higher than where the round trip alone would have left it.
        let (t1, a1) = reserves(netuid());
        assert_close(t1, t0 + interest, 100);
        // Supply fell by the alpha bought: out of the reserve, through the pallet, recycled.
        let burned = a0 - a1;
        assert!(burned > 0);
        assert_close(alpha_out(netuid()), out0, 100);
        assert_close(a1 + alpha_out(netuid()), a0 + out0 - burned, 100);
        assert_close(total_stake(), stake0 + interest, 100);
        assert!(price(netuid()) > p0);
    });
}

#[test]
fn long_held_a_day_returns_the_deposit_less_a_day_of_interest() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());
        let stake0 = total_stake();

        assert_ok!(add(alice(), Side::Long, DEPOSIT));
        let pos = position(&alice(), netuid()).unwrap();
        System::set_block_number(1 + DAY);
        assert_ok!(close(alice()));

        let interest = u64::from(interest_for_blocks(pos.interest_per_year, DAY));
        let (payout, interest_paid, shortfall) = last_closed_event();
        assert_eq!(interest_paid, interest);
        assert_eq!(shortfall, 0);
        assert_close(payout, DEPOSIT - interest, 100);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);

        let (t1, a1) = reserves(netuid());
        assert_close(t1, t0 + interest, 100);
        // Supply fell by the alpha bought: out of the reserve, through the pallet, recycled.
        let burned = a0 - a1;
        assert!(burned > 0);
        assert_close(alpha_out(netuid()), out0, 100);
        assert_close(a1 + alpha_out(netuid()), a0 + out0 - burned, 100);
        assert_close(total_stake(), stake0 + interest, 100);
    });
}

#[test]
fn short_profits_when_price_falls_and_loses_when_it_rises() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        // Bob dumps alpha: price falls.
        give_stake(&bob(), &alice_hotkey(), netuid(), 400 * TAO);
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(bob()),
            alice_hotkey(),
            netuid(),
            (400 * TAO).into()
        ));
        assert_ok!(close(alice()));
        let (payout, _, shortfall) = last_closed_event();
        assert_eq!(shortfall, 0);
        assert!(payout > DEPOSIT, "short should profit: {payout}");
    });

    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        // Bob buys alpha: price rises.
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(bob()),
            alice_hotkey(),
            netuid(),
            (50 * TAO).into()
        ));
        assert_ok!(close(alice()));
        let (payout, _, shortfall) = last_closed_event();
        assert_eq!(shortfall, 0);
        assert!(payout < DEPOSIT, "short should lose: {payout}");
    });
}

#[test]
fn underwater_short_settles_with_shortfall_and_pool_is_never_short() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, _) = reserves(netuid());
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        // Price triples: alpha is now far more expensive than N + P can buy.
        let whale = U256::from(7);
        add_balance(&whale, 5_000 * TAO);
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(whale),
            alice_hotkey(),
            netuid(),
            (2_000 * TAO).into()
        ));
        assert_ok!(close(alice()));
        let (payout, interest_paid, shortfall) = last_closed_event();
        assert_eq!(payout, 0);
        assert_eq!(interest_paid, 0);
        assert!(shortfall > 0);
        // Everything the pallet held for the position went back to the pool.
        assert_eq!(balance(&pallet_account()), 0);
        assert!(reserves(netuid()).0 > t0);
        assert!(position(&alice(), netuid()).is_none());
    });
}

#[test]
fn long_at_two_x_survives_a_moderate_drop_and_the_pool_is_whole() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Long, DEPOSIT));
        // Price falls about 30%: the proceeds alpha sells for less than D, the cushion covers
        // the gap, the pool gets D back and Alice keeps the rest.
        give_stake(&bob(), &alice_hotkey(), netuid(), 800 * TAO);
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(bob()),
            alice_hotkey(),
            netuid(),
            (800 * TAO).into()
        ));
        System::set_block_number(1 + DAY);
        assert_ok!(close(alice()));
        let (payout, interest_paid, shortfall) = last_closed_event();
        assert_eq!(shortfall, 0);
        assert!(interest_paid > 0);
        assert!(payout > 0 && payout < DEPOSIT, "payout = {payout}");
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
    });
}

#[test]
fn long_at_two_x_is_underwater_once_the_price_halves() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Long, DEPOSIT));
        // At 2x the cushion is D / 2, so a collapse past a halving leaves a shortfall. Alice
        // gets nothing, the pool takes everything the pallet still holds.
        give_stake(&bob(), &alice_hotkey(), netuid(), 20_000 * TAO);
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(bob()),
            alice_hotkey(),
            netuid(),
            (20_000 * TAO).into()
        ));
        assert_ok!(close(alice()));
        let (payout, interest_paid, shortfall) = last_closed_event();
        assert_eq!(payout, 0);
        assert_eq!(interest_paid, 0);
        assert!(shortfall > 0);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
        assert!(position(&alice(), netuid()).is_none());
    });
}

// ── Add, reduce, flip ────────────────────────────────────────────────────────

#[test]
fn adding_on_the_same_side_sums_every_leg() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let first = position(&alice(), netuid()).unwrap();
        let (p1, d1, e1) = legs(&first);
        System::set_block_number(101);

        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let both = position(&alice(), netuid()).unwrap();
        let (p2, d2, e2) = legs(&both);

        // Two 1x tranches of 10 TAO: cushion and exposure double, each leg is the sum of two
        // lifts (the second a little smaller in TAO, the pool having lost 1% to the first).
        assert_eq!(both.cushion, TaoBalance::from(2 * DEPOSIT));
        assert_close(
            u64::from(both.exposure_tao),
            2 * POOL_TAO / 100,
            POOL_TAO / 1000,
        );
        assert!(p2 > p1 && d2 > d1 && e2 > e1);
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), p2 + e2);
        assert_eq!(balance(&pallet_account()), 2 * DEPOSIT + p2 + e2);

        // The interest clock moved, carrying the hundred blocks of interest on the first tranche.
        assert_eq!(both.since, 101);
        assert_eq!(
            both.interest_owed,
            interest_for_blocks(first.interest_per_year, 100)
        );

        System::assert_last_event(
            Event::PositionAdded {
                owner: alice(),
                netuid: netuid(),
                side: Side::Short,
                deposit: DEPOSIT.into(),
                leverage_percent: 100,
                legs: Legs::Short {
                    proceeds: (p2 - p1).into(),
                    debt: (d2 - d1).into(),
                    escrow: (e2 - e1).into(),
                },
                exposure_added: (u64::from(both.exposure_tao) - u64::from(first.exposure_tao))
                    .into(),
                exposure_tao: both.exposure_tao,
            }
            .into(),
        );
    });
}

#[test]
fn interest_accrues_per_block_and_every_settlement_pays_it_all() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let first = position(&alice(), netuid()).unwrap();
        let rate1 = u64::from(first.interest_per_year);
        assert_eq!(first.interest_owed, TaoBalance::ZERO);
        assert_eq!(u64::from(first.interest_due(1)), 0);
        assert_eq!(u64::from(first.interest_due(1 + BLOCKS_PER_YEAR)), rate1);

        // Half a year later, a second tranche: the first tranche's half year is carried, and
        // the rate becomes the sum.
        System::set_block_number(1 + BLOCKS_PER_YEAR / 2);
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let both = position(&alice(), netuid()).unwrap();
        let rate2 = u64::from(both.interest_per_year) - rate1;
        assert_eq!(u64::from(both.interest_owed), rate1 / 2);
        assert_eq!(both.since, 1 + BLOCKS_PER_YEAR / 2);

        // Close a year after that: everything owed reaches the pool as interest.
        System::set_block_number(1 + BLOCKS_PER_YEAR / 2 + BLOCKS_PER_YEAR);
        let owed = u64::from(both.interest_due(System::block_number()));
        assert_eq!(owed, rate1 / 2 + rate1 + rate2);
        assert_ok!(close(alice()));
        let (_, interest_paid, _) = last_closed_event();
        assert_eq!(interest_paid, owed);
        assert_eq!(balance(&pallet_account()), 0);
    });
}

#[test]
fn adding_the_other_side_reduces_pro_rata_and_pays_that_share_out() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        assert_ok!(add(alice(), Side::Short, 2 * DEPOSIT));
        let before = position(&alice(), netuid()).unwrap();
        let (p0, d0, e0) = legs(&before);
        let wallet = balance(&alice());

        // A 1x long of 10 TAO against a 1x short of 20 TAO: half the exposure comes off.
        assert_ok!(add_at(alice(), Side::Long, DEPOSIT, 100));

        let after = position(&alice(), netuid()).unwrap();
        assert_eq!(after.side(), Side::Short);
        let (p1, d1, e1) = legs(&after);
        assert_close(p1, p0 / 2, 1);
        assert_close(d1, d0 / 2, 1);
        assert_close(e1, e0 / 2, 1);
        assert_close(
            u64::from(after.exposure_tao),
            u64::from(before.exposure_tao) / 2,
            1,
        );
        assert_close(
            u64::from(after.interest_per_year),
            u64::from(before.interest_per_year) / 2,
            1,
        );
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), p1 + e1);
        assert_eq!(after.interest_owed, TaoBalance::ZERO);

        // Half the cushion came back; nothing was deposited for the long. It is a little more
        // than half: the open sold all 80 alpha down the curve, and buying the first 40 back
        // starts at the bottom of it. The other half pays that back when it closes.
        let back = balance(&alice()) - wallet;
        assert!(back > DEPOSIT, "back = {back}");
        assert_close(back, DEPOSIT, DEPOSIT / 50);
        assert_close(u64::from(after.cushion), DEPOSIT, 1);
        assert_eq!(
            balance(&pallet_account()),
            u64::from(after.cushion) + p1 + e1
        );
        let (t1, a1) = reserves(netuid());
        assert_close(t1 + p1 + e1, t0, DEPOSIT / 50);
        assert_close(a1, a0, 100);

        let (fraction, payout, shortfall, exposure_tao) = last_reduced_event();
        assert_eq!(fraction, Perquintill::from_percent(50));
        assert_eq!(payout, back);
        assert_eq!(shortfall, 0);
        assert_eq!(exposure_tao, u64::from(after.exposure_tao));

        // Closing the rest: over both halves the pool is whole. The second buyback ran against
        // a pool that already had the first half's slice back, so the two halves do not add up
        // to one buyback to the rao; the difference is one part in 10^4 of the deposit.
        assert_ok!(close(alice()));
        assert_close(balance(&alice()), wallet + 2 * DEPOSIT, DEPOSIT / 5_000);
        assert_eq!(balance(&pallet_account()), 0);
        let (t2, a2) = reserves(netuid());
        assert_close(t2, t0, DEPOSIT / 5_000);
        assert_close(a2, a0, 100);
    });
}

#[test]
fn adding_more_than_the_position_closes_it_and_opens_the_rest_on_the_other_side() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let short = position(&alice(), netuid()).unwrap();
        let wallet = balance(&alice());
        System::set_block_number(50);

        // A 2x long of 15 TAO asks for 30 TAO of exposure against a 10 TAO short: the short
        // closes, and the 20 TAO that remain open a long with a 10 TAO cushion at 2x. Alice is
        // only charged that 10 TAO, plus gets the short's payout.
        assert_ok!(add(alice(), Side::Long, 15 * TAO));

        let long = position(&alice(), netuid()).unwrap();
        assert_eq!(long.side(), Side::Long);
        assert_close(u64::from(long.cushion), DEPOSIT, 1);
        assert_close((leverage_of(&long) * 100.0).round() as u64, 200, 1);
        assert_eq!(long.since, 50);
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), 0);
        assert_eq!(
            Footprint::<Test>::get(netuid(), Side::Long),
            long.legs.footprint()
        );

        let (payout, interest_paid, shortfall) = last_closed_event();
        assert_eq!(shortfall, 0);
        // Forty-nine blocks of interest on the short.
        assert_eq!(interest_paid, u64::from(short.interest_due(50)));
        assert!(interest_paid > 0);
        assert_close(balance(&alice()), wallet + payout - DEPOSIT, 1);
    });
}

#[test]
fn flipping_by_exactly_the_position_or_by_dust_only_closes_it() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Long, DEPOSIT));
        let exposure = u64::from(position(&alice(), netuid()).unwrap().exposure_tao);
        // Exactly the exposure at 1x: closed flat, nothing opened.
        assert_ok!(add_at(alice(), Side::Short, exposure, 100));
        assert!(position(&alice(), netuid()).is_none());

        assert_ok!(add(alice(), Side::Long, DEPOSIT));
        let exposure = u64::from(position(&alice(), netuid()).unwrap().exposure_tao);
        // A hair over: the rest is below `MinDeposit`, so it is dropped, not opened.
        assert_ok!(add_at(alice(), Side::Short, exposure + TAO / 100, 100));
        assert!(position(&alice(), netuid()).is_none());
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), 0);
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Long), 0);
        assert_eq!(balance(&pallet_account()), 0);
    });
}

#[test]
fn reducing_an_underwater_position_forfeits_that_share_to_the_pool() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let before = position(&alice(), netuid()).unwrap();
        let whale = U256::from(7);
        add_balance(&whale, 5_000 * TAO);
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(whale),
            alice_hotkey(),
            netuid(),
            (2_000 * TAO).into()
        ));
        System::set_block_number(1 + DAY);
        let wallet = balance(&alice());
        let (t0, _) = reserves(netuid());

        assert_ok!(add_at(alice(), Side::Long, DEPOSIT / 2, 100));

        // Half is gone: Alice got nothing for it, the pool got everything the pallet held for
        // that half, and the other half is still open (and just as underwater). The interest, which
        // the forfeited half could not pay, came off the cushion that stays.
        assert_eq!(balance(&alice()), wallet);
        assert!(reserves(netuid()).0 > t0);
        let rest = position(&alice(), netuid()).unwrap();
        let interest = u64::from(before.interest_due(1 + DAY));
        assert!(interest > 0);
        assert_close(u64::from(rest.cushion), DEPOSIT / 2 - interest, 1);
        assert_eq!(rest.interest_owed, TaoBalance::ZERO);
        let (_, payout, shortfall, _) = last_reduced_event();
        assert_eq!(payout, 0);
        assert!(shortfall > 0);
    });
}

// ── Interest queue ───────────────────────────────────────────────────────────

/// Positions listed in the interest queue for `slot`.
fn due_at(slot: u64) -> Vec<U256> {
    let mut owners: Vec<U256> = crate::Due::<Test>::iter_key_prefix(slot)
        .map(|(owner, _)| owner)
        .collect();
    owners.sort();
    owners
}

#[test]
fn a_new_position_is_queued_a_week_out_and_collected_on_its_block() {
    new_test_ext().execute_with(|| {
        setup();
        assert_eq!(crate::NextDue::<Test>::get(), 1);
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add(bob(), Side::Long, DEPOSIT));
        let short = position(&alice(), netuid()).unwrap();
        let long = position(&bob(), netuid()).unwrap();
        assert_eq!(short.due, 1 + WEEK);
        assert_eq!(due_at(1 + WEEK), vec![alice(), bob()]);
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());
        let p0 = price(netuid());
        let pallet0 = balance(&pallet_account());

        // The day before, nothing is due: the pointer walks the empty slots and stops at now.
        run_to(WEEK);
        assert_eq!(position(&alice(), netuid()), Some(short.clone()));
        assert_eq!(crate::NextDue::<Test>::get(), WEEK + 1);

        // On the due block each cushion pays a week of its own interest. The interest buys
        // alpha, which is recycled: the TAO reserve grows by the interest, the alpha reserve
        // and the alpha in circulation shrink by what it bought, and the price rises. Both are
        // booked for the next week; the trades themselves do not move.
        run_to(1 + WEEK);
        let short_week = u64::from(interest_for_blocks(short.interest_per_year, WEEK));
        let long_week = u64::from(interest_for_blocks(long.interest_per_year, WEEK));
        assert!(short_week > 0 && long_week == 2 * short_week);
        let short1 = position(&alice(), netuid()).unwrap();
        let long1 = position(&bob(), netuid()).unwrap();
        assert_eq!(u64::from(short1.cushion), DEPOSIT - short_week);
        assert_eq!(u64::from(long1.cushion), DEPOSIT - long_week);
        assert_eq!(short1.interest_owed, TaoBalance::ZERO);
        assert_eq!(short1.since, 1 + WEEK);
        assert_eq!(short1.due, 1 + 2 * WEEK);
        assert_eq!(long1.due, 1 + 2 * WEEK);
        assert_eq!(short1.legs, short.legs);
        assert_eq!(long1.legs, long.legs);
        assert!(due_at(1 + WEEK).is_empty());
        assert_eq!(due_at(1 + 2 * WEEK), vec![alice(), bob()]);
        assert_eq!(crate::NextDue::<Test>::get(), 2 + WEEK);
        assert_eq!(balance(&pallet_account()), pallet0 - short_week - long_week);
        let (t1, a1) = reserves(netuid());
        assert_close(t1, t0 + short_week + long_week, 2);
        // Supply fell by the alpha bought: out of the reserve, through the pallet, recycled.
        let burned = a0 - a1;
        assert!(burned > 0);
        assert_eq!(alpha_out(netuid()), out0);
        assert_eq!(a1 + alpha_out(netuid()), a0 + out0 - burned);
        assert!(price(netuid()) > p0);
        // The pallet holds no alpha it did not hold before: everything bought was recycled.
        assert_eq!(
            stake(&pallet_account(), &pallet_hotkey(), netuid()),
            long.legs.footprint()
        );

        // Running the same block again does nothing: the slot is behind the pointer.
        Derivatives::collect_due(1 + WEEK);
        assert_eq!(position(&alice(), netuid()), Some(short1));
        assert_eq!(reserves(netuid()), (t1, a1));

        // A close right after a collection owes no more interest, and leaves the queue.
        assert_ok!(close(alice()));
        let (_, interest_paid, _) = last_closed_event();
        assert_eq!(interest_paid, 0);
        assert_eq!(due_at(1 + 2 * WEEK), vec![bob()]);
    });
}

#[test]
fn adds_and_reductions_keep_the_slot_and_a_flip_takes_a_new_one() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        System::set_block_number(100);
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add_at(alice(), Side::Long, DEPOSIT / 2, 100));
        assert_eq!(position(&alice(), netuid()).unwrap().due, 1 + WEEK);
        assert_eq!(due_at(1 + WEEK), vec![alice()]);

        // A flip closes the short and opens a long: the old slot is freed, a new one taken.
        assert_ok!(add(alice(), Side::Long, 30 * TAO));
        assert_eq!(position(&alice(), netuid()).unwrap().side(), Side::Long);
        assert!(due_at(1 + WEEK).is_empty());
        assert_eq!(due_at(100 + WEEK), vec![alice()]);
    });
}

#[test]
fn a_crowded_slot_is_finished_over_the_following_blocks() {
    new_test_ext().execute_with(|| {
        setup();
        let many: Vec<U256> = (10..10 + COLLECTIONS_PER_BLOCK as u64 + 5)
            .map(U256::from)
            .collect();
        for who in &many {
            add_balance(who, 10 * TAO);
            assert_ok!(add(*who, Side::Short, TAO));
        }
        assert_eq!(due_at(1 + WEEK).len(), many.len());
        let collected = || {
            many.iter()
                .filter(|who| position(who, netuid()).unwrap().since > 1)
                .count()
        };

        // The due block collects a block's worth and stays on the slot.
        run_to(WEEK);
        System::set_block_number(1 + WEEK);
        Derivatives::collect_due(1 + WEEK);
        assert_eq!(collected(), COLLECTIONS_PER_BLOCK as usize);
        assert_eq!(crate::NextDue::<Test>::get(), 1 + WEEK);
        assert_eq!(due_at(1 + WEEK).len(), 5);

        // The next block finishes it, exactly, and moves on.
        System::set_block_number(2 + WEEK);
        Derivatives::collect_due(2 + WEEK);
        assert_eq!(collected(), many.len());
        assert!(due_at(1 + WEEK).is_empty());
        assert_eq!(due_at(1 + 2 * WEEK).len(), COLLECTIONS_PER_BLOCK as usize);
        assert_eq!(due_at(2 + 2 * WEEK).len(), 5);
        assert_eq!(crate::NextDue::<Test>::get(), 3 + WEEK);
    });
}

#[test]
fn the_queue_catches_up_after_a_stall_and_charges_the_exact_time() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let short = position(&alice(), netuid()).unwrap();

        // No block ran for ten days. The pointer is far behind; catching up walks the empty
        // slots, reaches the missed one, and the collection charges the full time elapsed, not
        // a week. The next collection is a week after the late one.
        run_to(1 + WEEK + 3 * DAY);
        let later = position(&alice(), netuid()).unwrap();
        assert_eq!(
            later.cushion,
            TaoBalance::from(DEPOSIT)
                - interest_for_blocks(short.interest_per_year, WEEK + 3 * DAY)
        );
        assert_eq!(later.since, 1 + WEEK + 3 * DAY);
        assert_eq!(later.due, 1 + 2 * WEEK + 3 * DAY);
        assert_eq!(due_at(1 + 2 * WEEK + 3 * DAY), vec![alice()]);
    });
}

#[test]
fn a_starved_position_is_forfeited_to_the_pool_without_a_swap() {
    new_test_ext().execute_with(|| {
        setup();
        set_interest(Percent::one());
        let (t0, a0) = reserves(netuid());
        assert_ok!(add(alice(), Side::Short, DEPOSIT));

        // 100%/year on a 10 TAO cushion: week by week the collections drain it, and on the week
        // it cannot pay the position is forfeited. Roughly a year in. Each collection buys
        // alpha; the forfeit itself trades nothing.
        let mut week = 1;
        let mut price_before = price(netuid());
        while position(&alice(), netuid()).is_some() {
            price_before = price(netuid());
            run_to(1 + week * WEEK);
            week += 1;
        }
        assert!(week > 51 && week < 55, "starved in week {week}");

        // Gone. Every TAO the pallet held went back to the pool: the slice in kind plus the
        // whole cushion, most of it week by week as interest and the rest at the forfeit. Alice
        // was paid nothing, the forfeit moved no price, and the alpha the interest bought is
        // out of the reserve.
        assert_eq!(last_closer(), Closer::Starved);
        let (payout, interest_paid, shortfall) = last_closed_event();
        assert_eq!(payout, 0);
        assert!(interest_paid > 0 && interest_paid < 2 * DEPOSIT * 7 / 365);
        assert_eq!(shortfall, 0);
        assert_eq!(balance(&alice()), 100 * TAO - DEPOSIT);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
        let (t1, a1) = reserves(netuid());
        assert_close(t1, t0 + DEPOSIT, 100);
        assert!(a1 < a0);
        assert_close(
            price(netuid()).to_bits() as u64,
            price_before.to_bits() as u64,
            (price_before.to_bits() as u64) >> 30,
        );
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), 0);
        assert!(
            crate::OpenByNetuid::<Test>::iter_prefix(netuid())
                .next()
                .is_none()
        );
        assert!(crate::Due::<Test>::iter_keys().next().is_none());
    });
}

#[test]
fn a_starved_long_is_forfeited_in_kind_too() {
    new_test_ext().execute_with(|| {
        setup();
        set_interest(Percent::one());
        let (t0, _) = reserves(netuid());
        assert_ok!(add(alice(), Side::Long, DEPOSIT));
        let long = position(&alice(), netuid()).unwrap();

        // A 2x long pays 20 TAO a year on a 10 TAO cushion: starved in about half a year.
        let mut week = 1;
        while position(&alice(), netuid()).is_some() {
            run_to(1 + week * WEEK);
            week += 1;
        }
        assert!(week > 25 && week < 29, "starved in week {week}");

        // The pool has the long's alpha back in kind, TAO it never lost, and the cushion on
        // top, minus the alpha the interest bought and recycled.
        assert_eq!(last_closer(), Closer::Starved);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
        assert_close(reserves(netuid()).0, t0 + DEPOSIT, 100);
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Long), 0);
        assert!(long.legs.footprint() > 0);
    });
}

#[test]
fn a_starving_position_can_be_rescued_by_an_add() {
    new_test_ext().execute_with(|| {
        setup();
        // 100%/year: 10 TAO of interest a year on a 10 TAO cushion at 1x, gone in about a year.
        set_interest(Percent::one());
        assert_ok!(add(alice(), Side::Short, DEPOSIT));

        // Week 43: the collections so far have left about 1.75 TAO of cushion.
        run_to(1 + 43 * WEEK);
        let thin = position(&alice(), netuid()).unwrap();
        assert_close(
            u64::from(thin.cushion),
            DEPOSIT - 43 * 7 * DEPOSIT / 365,
            TAO / 100,
        );

        // Alice adds cushion. Nine weeks later the thin cushion alone would have run out; with
        // the add, the collections keep going.
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        run_to(1 + 57 * WEEK);
        let rescued = position(&alice(), netuid()).unwrap();
        assert_eq!(rescued.interest_owed, TaoBalance::ZERO);
        assert!(rescued.cushion > TaoBalance::from(DEPOSIT / 2));
        assert_ok!(close(alice()));
        assert_eq!(last_closer(), Closer::Owner);
    });
}

// ── No term ──────────────────────────────────────────────────────────────────

#[test]
fn a_position_has_no_term_and_only_its_owner_can_close_it() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));

        // Two years on at the default interest, with no sweep having run, a 1x position owes
        // half its cushion. Bob cannot touch it: `close` only ever settles the caller's own.
        System::set_block_number(1 + 2 * BLOCKS_PER_YEAR);
        let later = position(&alice(), netuid()).unwrap();
        let owed = u64::from(later.interest_due(1 + 2 * BLOCKS_PER_YEAR));
        assert_eq!(owed, DEPOSIT / 2);
        assert_err!(close(bob()), Error::<Test>::NoPosition);
        assert_eq!(position(&alice(), netuid()), Some(later));

        // A same-side add grows it in place: nothing settles, the interest so far is carried and
        // the clock restarts.
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let grown = position(&alice(), netuid()).unwrap();
        assert_eq!(grown.cushion, TaoBalance::from(2 * DEPOSIT));
        assert_eq!(grown.since, 1 + 2 * BLOCKS_PER_YEAR);
        assert_eq!(grown.interest_owed, TaoBalance::from(owed));
        assert!(System::events().iter().all(|record| {
            !matches!(
                record.event,
                RuntimeEvent::Derivatives(Event::PositionClosed { .. })
            )
        }));

        // The owner closes whenever they like and pays everything owed.
        assert_ok!(close(alice()));
        let (_, interest_paid, shortfall) = last_closed_event();
        assert_eq!(interest_paid, owed);
        assert_eq!(shortfall, 0);
        assert_eq!(last_closer(), Closer::Owner);
        assert!(position(&alice(), netuid()).is_none());
    });
}

#[test]
fn a_failed_add_leaves_the_position_untouched() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        let before = position(&alice(), netuid()).unwrap();
        let footprint = Footprint::<Test>::get(netuid(), Side::Short);
        let balance_before = balance(&alice());
        let pool_before = reserves(netuid());

        // A flip whose new long is over the pool cap: the settlement ran and rolled back.
        assert_err!(
            add(alice(), Side::Long, 150 * TAO),
            Error::<Test>::PoolCapExceeded
        );

        assert_eq!(position(&alice(), netuid()), Some(before));
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), footprint);
        assert_eq!(balance(&alice()), balance_before);
        assert_eq!(reserves(netuid()), pool_before);
    });
}

// ── Dissolution ──────────────────────────────────────────────────────────────

/// `alpha` in TAO at the price `tao / alpha`, rounded up as the pallet does for a debt.
fn tao_at_ceil(alpha: u64, (tao, alpha_total): (u64, u64)) -> u64 {
    ((alpha as u128 * tao as u128).div_ceil(alpha_total as u128)) as u64
}

/// `alpha` in TAO at the price `tao / alpha`, rounded down as the pallet does for a credit.
fn tao_at_floor(alpha: u64, (tao, alpha_total): (u64, u64)) -> u64 {
    ((alpha as u128 * tao as u128) / alpha_total as u128) as u64
}

/// `(tao, alpha)` of the latest `DissolutionPriced`.
fn dissolution_price_event() -> (u64, u64) {
    System::events()
        .into_iter()
        .rev()
        .find_map(|record| match record.event {
            RuntimeEvent::Derivatives(Event::DissolutionPriced { tao, alpha, .. }) => {
                Some((tao.into(), alpha.into()))
            }
            _ => None,
        })
        .expect("DissolutionPriced")
}

/// The pool's spot price as the pallet will fix it, as `(tao, alpha)`.
fn spot_pair() -> (u64, u64) {
    let (tao, alpha) = <SubtensorModule as subtensor_swap_interface::DerivativesPoolInterface<
        AccountId,
    >>::spot_price(netuid());
    (tao.into(), alpha.into())
}

/// Bob dumps `alpha` into the pool so the price falls.
fn dump_alpha(alpha: u64) {
    give_stake(&bob(), &alice_hotkey(), netuid(), alpha);
    assert_ok!(SubtensorModule::remove_stake(
        RuntimeOrigin::signed(bob()),
        alice_hotkey(),
        netuid(),
        alpha.into()
    ));
}

#[test]
fn dissolution_settles_every_position_at_the_spot_price_with_no_swap() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add(bob(), Side::Long, DEPOSIT));
        let short = position(&alice(), netuid()).unwrap();
        let long = position(&bob(), netuid()).unwrap();
        let (short_proceeds, short_debt, _) = legs(&short);
        let (long_proceeds, long_debt, _) = legs(&long);
        System::set_block_number(1 + DAY);
        let now = System::block_number();

        // Dissolve: the subnet is no longer "added" but its account and pool still exist. The
        // price every position settles at is the spot price at that moment.
        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));
        let spot = price(netuid());
        let price = spot_pair();
        assert_close(price.0, (spot * U64F64::from_num(1u64 << 32)).to_num(), 2);
        settle_all_for_dissolution(netuid());
        assert_eq!(dissolution_price_event(), price);
        assert!(crate::DissolutionPrice::<Test>::get(netuid()).is_none());

        // Short: cushion plus proceeds, less the debt at the price and the interest. Long:
        // cushion plus the alpha held at the price, less the TAO debt and the interest. Either
        // owner gets nothing when that is negative.
        let short_owed = tao_at_ceil(short_debt, price);
        let long_credit = tao_at_floor(long_proceeds, price);
        let short_value = (DEPOSIT + short_proceeds)
            .saturating_sub(short_owed)
            .saturating_sub(u64::from(short.interest_due(now)));
        let long_value = (DEPOSIT + long_credit)
            .saturating_sub(long_debt)
            .saturating_sub(u64::from(long.interest_due(now)));
        let alice_payout = balance(&alice()) - (100 * TAO - DEPOSIT);
        let bob_payout = balance(&bob()) - (100 * TAO - DEPOSIT);
        assert_eq!(alice_payout, short_value);
        assert_eq!(bob_payout, long_value);
        // The short pressed the price down, the long lifted it back; at one shared spot price
        // the short is a little down and the long a little up, both less a day of interest.
        assert!(alice_payout < DEPOSIT && alice_payout > DEPOSIT * 9 / 10);
        assert!(bob_payout > DEPOSIT && bob_payout < DEPOSIT * 11 / 10);

        // No swap happened: the pool has its own liquidity back, less what the winner took,
        // plus what the loser forfeited; alpha is whole; the pallet holds nothing.
        for who in [alice(), bob()] {
            assert!(position(&who, netuid()).is_none());
        }
        let (t1, a1) = reserves(netuid());
        assert_close(t1, t0 + 2 * DEPOSIT - alice_payout - bob_payout, 100);
        assert_close(a1, a0, 100);
        assert_close(alpha_out(netuid()), out0, 100);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
        for side in [Side::Short, Side::Long] {
            assert_eq!(Footprint::<Test>::get(netuid(), side), 0);
        }
        assert!(
            crate::OpenByNetuid::<Test>::iter_prefix(netuid())
                .next()
                .is_none()
        );
        assert!(crate::Due::<Test>::iter_keys().next().is_none());
        let closers: Vec<_> = System::events()
            .into_iter()
            .filter_map(|r| match r.event {
                RuntimeEvent::Derivatives(Event::PositionClosed { closed_by, .. }) => {
                    Some(closed_by)
                }
                _ => None,
            })
            .collect();
        assert_eq!(closers.len(), 2);
        assert!(closers.iter().all(|c| *c == Closer::Dissolution));
    });
}

#[test]
fn a_lone_short_that_moved_the_price_keeps_that_move_at_dissolution() {
    // A short sells its slice down the curve. Closing by hand climbs the same curve back, so
    // its own impact nets to nothing. Dissolution charges the debt at the spot price the pool
    // last showed, so the short keeps what its sale did to the price.
    let by_close = new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, 100 * TAO));
        assert_ok!(close(alice()));
        last_closed_event().0
    });
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, 100 * TAO));
        let (proceeds, debt, _) = legs(&position(&alice(), netuid()).unwrap());
        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));
        let price = spot_pair();
        settle_all_for_dissolution(netuid());
        let by_dissolution = last_closed_event().0;
        assert_eq!(
            by_dissolution,
            100 * TAO + proceeds - tao_at_ceil(debt, price)
        );
        // Closing by hand returns the cushion less rounding; dissolution pays the impact too.
        assert_close(by_close, 100 * TAO, TAO / 100);
        assert!(
            by_dissolution > by_close + TAO,
            "{by_dissolution} vs {by_close}"
        );
    });
}

#[test]
fn dissolution_pays_a_short_the_decline_and_charges_a_long_for_it() {
    for side in [Side::Short, Side::Long] {
        new_test_ext().execute_with(|| {
            setup();
            assert_ok!(add(alice(), side, DEPOSIT));
            // The market sells: alpha loses about a tenth of its price.
            dump_alpha(400 * TAO);
            assert_ok!(SubtensorModule::do_dissolve_network(netuid()));
            settle_all_for_dissolution(netuid());
            let payout = last_closed_event().0;
            match side {
                Side::Short => assert!(payout > DEPOSIT, "short should profit: {payout}"),
                Side::Long => assert!(
                    payout < DEPOSIT * 7 / 10 && payout > DEPOSIT / 2,
                    "2x long should lose about 2 x 10% of its exposure: {payout}"
                ),
            }
        });
    }
}

#[test]
fn dissolution_price_is_fixed_before_the_first_settlement() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add(bob(), Side::Long, DEPOSIT));
        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));
        let price = spot_pair();
        let price = (TaoBalance::from(price.0), AlphaBalance::from(price.1));

        // One position's worth of weight: the price is fixed and stored, nothing settles yet.
        let mut meter = frame_support::weights::WeightMeter::with_limit(
            <() as crate::weights::WeightInfo>::close(),
        );
        assert!(
            !<Derivatives as subtensor_runtime_common::SubnetDissolveHook>::on_subnet_dissolve(
                netuid(),
                &mut meter
            )
        );
        assert_eq!(crate::DissolutionPrice::<Test>::get(netuid()), Some(price));
        assert_eq!(
            crate::OpenByNetuid::<Test>::iter_prefix(netuid()).count(),
            2
        );

        // Settling the first position moves the reserves; the second still settles at the
        // stored price.
        let mut meter = frame_support::weights::WeightMeter::with_limit(
            <() as crate::weights::WeightInfo>::close(),
        );
        assert!(
            !<Derivatives as subtensor_runtime_common::SubnetDissolveHook>::on_subnet_dissolve(
                netuid(),
                &mut meter
            )
        );
        assert_eq!(
            crate::OpenByNetuid::<Test>::iter_prefix(netuid()).count(),
            1
        );
        assert_eq!(crate::DissolutionPrice::<Test>::get(netuid()), Some(price));

        settle_all_for_dissolution(netuid());
        assert!(crate::DissolutionPrice::<Test>::get(netuid()).is_none());
    });
}

#[test]
fn dissolution_hook_is_bounded_by_weight() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add(bob(), Side::Long, DEPOSIT));
        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));

        // Fixing the price costs one position's worth; then one position fits.
        let mut meter = frame_support::weights::WeightMeter::with_limit(
            <() as crate::weights::WeightInfo>::close().saturating_mul(2),
        );
        assert!(
            !<Derivatives as subtensor_runtime_common::SubnetDissolveHook>::on_subnet_dissolve(
                netuid(),
                &mut meter
            )
        );
        assert_eq!(
            crate::OpenByNetuid::<Test>::iter_prefix(netuid()).count(),
            1
        );
        settle_all_for_dissolution(netuid());
        assert_eq!(
            crate::OpenByNetuid::<Test>::iter_prefix(netuid()).count(),
            0
        );
    });
}

#[test]
fn opposite_positions_conserve_alpha() {
    new_test_ext().execute_with(|| {
        setup();
        let supply0 = reserves(netuid()).1 + alpha_out(netuid());
        assert_ok!(add(alice(), Side::Short, DEPOSIT));
        assert_ok!(add(bob(), Side::Long, DEPOSIT));
        assert_eq!(reserves(netuid()).1 + alpha_out(netuid()), supply0);
        assert_ok!(close(alice()));
        assert_ok!(close(bob()));
        assert_close(reserves(netuid()).1 + alpha_out(netuid()), supply0, 100);
        assert_eq!(tao_flow(netuid()), 0);
    });
}

/// A long lifts the spot price but not the price emission is weighted by; a short lowers both.
#[test]
fn longs_do_not_move_the_emission_price_but_shorts_do() {
    new_test_ext().execute_with(|| {
        setup();
        let emission_price = || SubtensorModule::get_emission_alpha_price(netuid());
        let p0 = price(netuid());
        assert_eq!(emission_price(), p0);

        assert_ok!(add(alice(), Side::Long, DEPOSIT));
        assert!(price(netuid()) > p0);
        let p1 = emission_price();
        let drift = if p1 > p0 { p1 - p0 } else { p0 - p1 } / p0;
        assert!(
            drift < U64F64::from_num(0.000_001),
            "emission price moved by {drift}"
        );

        assert_ok!(add(bob(), Side::Short, DEPOSIT));
        assert!(emission_price() < p0);
        assert_eq!(
            emission_price(),
            <Swap as subtensor_swap_interface::SwapHandler>::alpha_price_for_reserves(
                netuid(),
                (reserves(netuid()).1 + Footprint::<Test>::get(netuid(), Side::Long)).into(),
                reserves(netuid()).0.into(),
            )
        );

        assert_ok!(close(alice()));
        assert_eq!(emission_price(), price(netuid()));
    });
}

#[test]
fn set_params_requires_root() {
    new_test_ext().execute_with(|| {
        let params = Params::<Test>::get();
        assert_eq!(params, DerivativesParams::defaults());
        assert_eq!(params.pool_share, Percent::from_percent(25));
        assert_eq!(params.interest_rate, Percent::from_percent(25));
        assert_err!(
            Derivatives::sudo_set_params(RuntimeOrigin::signed(alice()), params),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_ok!(Derivatives::sudo_set_params(RuntimeOrigin::root(), params));
        System::assert_last_event(Event::ParamsSet { params }.into());
    });
}
