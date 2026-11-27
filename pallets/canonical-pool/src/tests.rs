#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use frame_support::{assert_noop, assert_ok, traits::fungible::Mutate};
use sp_core::H256;
use sp_runtime::{DispatchError, Permill};
use subtensor_runtime_common::{
    TaoBalance, TusdBalance,
    rails::{OriginChain, StableAsset},
};

use crate::{
    Accumulator, Caps, Enabled, Error, Event, Fee, MAX_FEE, MIN_FEE, PRICE_SCALE, RecentFlow,
    Reserves, Shares, Side, SwapCaps, TotalShares,
    math::{self, MINIMUM_LIQUIDITY},
    mock::*,
};

const ALICE: u64 = 1;
const BOB: u64 = 2;
const CAROL: u64 = 3;
const BRIDGE: u32 = 8453;

fn tao(amount: u64) -> TaoBalance {
    TaoBalance::new(amount)
}

fn tusd(amount: u64) -> TusdBalance {
    TusdBalance::new(amount)
}

/// Gives `who` TAO and tUSD on top of what it holds.
fn fund(who: u64, tao_amount: u64, tusd_amount: u64) {
    if !pallet_tusd::Bridges::<Test>::contains_key(BRIDGE) {
        assert_ok!(Tusd::set_bridge(
            RuntimeOrigin::root(),
            BRIDGE,
            pallet_tusd::BridgeRoute {
                origin: OriginChain::Evm(8453),
                domain: BRIDGE,
                portal: H256::repeat_byte(1),
                asset: StableAsset::Usdc,
            },
            tusd(u64::MAX),
            tusd(u64::MAX),
            1,
        ));
        assert_ok!(Tusd::set_bridge_enabled(
            RuntimeOrigin::root(),
            BRIDGE,
            true
        ));
    }
    let held = Balances::free_balance(who);
    Balances::set_balance(&who, held + tao(tao_amount));
    assert_ok!(Tusd::mint(BRIDGE, &who, tusd(tusd_amount)));
}

/// Enables the pool and has ALICE make the first deposit, at the oracle's 400 dollars.
fn seed(tao_amount: u64) {
    assert_ok!(Pool::set_enabled(RuntimeOrigin::root(), true));
    fund(ALICE, tao_amount + UNIT, 400 * tao_amount);
    assert_ok!(Pool::add_liquidity(
        RuntimeOrigin::signed(ALICE),
        tao(tao_amount),
        tusd(400 * tao_amount),
        0
    ));
}

/// Lifts the swap caps so a test can trade freely.
fn lift_caps() {
    assert_ok!(Pool::set_swap_caps(
        RuntimeOrigin::root(),
        SwapCaps {
            per_swap: Permill::one(),
            per_window: Permill::one(),
            window: 1,
        }
    ));
}

fn product() -> u128 {
    let reserves = Pool::reserves();
    u128::from(reserves.tao) * u128::from(reserves.tusd)
}

/// What a constant-product pool pays, computed without the pallet's math.
fn exact_out(reserve_in: u64, reserve_out: u64, amount_in: u64, fee: Permill) -> u64 {
    let net = u128::from(amount_in - fee.mul_ceil(amount_in));
    (u128::from(reserve_out) * net / (u128::from(reserve_in) + net)) as u64
}

fn assert_invariants() {
    Pool::do_try_state().unwrap();
    let held: u128 = Shares::<Test>::iter().map(|(_, shares)| shares).sum();
    let total = TotalShares::<Test>::get();
    if total != 0 {
        assert_eq!(held + MINIMUM_LIQUIDITY, total);
    }
}

// ---------------------------------------------------------------- launch switch

#[test]
fn pool_ships_disabled() {
    new_test_ext().execute_with(|| {
        assert!(!Enabled::<Test>::get());
        fund(ALICE, 100 * UNIT, 40_000 * UNIT);
        assert_noop!(
            Pool::add_liquidity(RuntimeOrigin::signed(ALICE), tao(UNIT), tusd(400 * UNIT), 0),
            Error::<Test>::PoolDisabled
        );
        assert_noop!(
            Pool::swap_tusd_for_tao(RuntimeOrigin::signed(ALICE), tusd(UNIT), tao(0)),
            Error::<Test>::PoolDisabled
        );
        assert_noop!(
            Pool::swap_tao_for_tusd(RuntimeOrigin::signed(ALICE), tao(UNIT), tusd(0)),
            Error::<Test>::PoolDisabled
        );
        assert_eq!(Pool::spot_price(), None);
        assert_eq!(Pool::twap_price(), None);
    });
}

#[test]
fn admin_enables_and_guardian_only_disables() {
    new_test_ext().execute_with(|| {
        for origin in [
            RuntimeOrigin::signed(ALICE),
            RuntimeOrigin::signed(GUARDIAN),
        ] {
            assert_noop!(Pool::set_enabled(origin, true), DispatchError::BadOrigin);
        }
        assert_ok!(Pool::set_enabled(RuntimeOrigin::root(), true));
        System::assert_last_event(Event::EnabledSet { enabled: true }.into());

        assert_noop!(
            Pool::set_enabled(RuntimeOrigin::signed(ALICE), false),
            DispatchError::BadOrigin
        );
        assert_ok!(Pool::set_enabled(RuntimeOrigin::signed(GUARDIAN), false));
        assert!(!Enabled::<Test>::get());
        assert_ok!(Pool::set_enabled(RuntimeOrigin::root(), false));
    });
}

// ---------------------------------------------------------------- liquidity

#[test]
fn first_deposit_sets_the_price_and_locks_minimum_liquidity() {
    new_test_ext().execute_with(|| {
        seed(10 * UNIT);

        assert_eq!(
            Pool::reserves(),
            Reserves {
                tao: 10 * UNIT,
                tusd: 4_000 * UNIT
            }
        );
        let shares = math::initial_shares(10 * UNIT, 4_000 * UNIT);
        assert_eq!(shares, 200 * u128::from(UNIT));
        assert_eq!(TotalShares::<Test>::get(), shares);
        assert_eq!(Shares::<Test>::get(ALICE), shares - MINIMUM_LIQUIDITY);
        assert_eq!(Pool::spot_price(), Some(400 * PRICE_SCALE));
        assert_eq!(Balances::free_balance(ALICE), tao(UNIT));
        assert_eq!(Tusd::balance(&ALICE), tusd(0));
        assert_eq!(Balances::free_balance(Pool::account_id()), tao(10 * UNIT));
        assert_eq!(Tusd::balance(&Pool::account_id()), tusd(4_000 * UNIT));
        System::assert_last_event(
            Event::LiquidityAdded {
                who: ALICE,
                tao: tao(10 * UNIT),
                tusd: tusd(4_000 * UNIT),
                shares: shares - MINIMUM_LIQUIDITY,
            }
            .into(),
        );
        assert_invariants();
    });
}

#[test]
fn first_deposit_must_price_tao_near_the_oracle() {
    new_test_ext().execute_with(|| {
        assert_ok!(Pool::set_enabled(RuntimeOrigin::root(), true));
        fund(ALICE, 100 * UNIT, 100_000 * UNIT);
        // 2% is the tolerance: 409 dollars is too high, 391 too low, 408 and 392 pass.
        for (price, ok) in [(409, false), (391, false), (1, false), (408, true)] {
            let result = Pool::add_liquidity(
                RuntimeOrigin::signed(ALICE),
                tao(10 * UNIT),
                tusd(price * 10 * UNIT),
                0,
            );
            if ok {
                assert_ok!(result);
            } else {
                assert_noop!(result, Error::<Test>::InitialPriceOutOfRange);
            }
        }
        assert_eq!(Pool::spot_price(), Some(408 * PRICE_SCALE));
    });
}

#[test]
fn first_deposit_needs_a_fresh_oracle_price() {
    new_test_ext().execute_with(|| {
        assert_ok!(Pool::set_enabled(RuntimeOrigin::root(), true));
        fund(ALICE, 100 * UNIT, 100_000 * UNIT);
        OraclePrice::set(None);
        assert_noop!(
            Pool::add_liquidity(
                RuntimeOrigin::signed(ALICE),
                tao(10 * UNIT),
                tusd(4_000 * UNIT),
                0
            ),
            Error::<Test>::OracleUnavailable
        );
        // Later deposits follow the pool's own ratio and need no oracle.
        OraclePrice::set(Some(400 * UNIT));
        seed(10 * UNIT);
        OraclePrice::set(None);
        fund(BOB, 10 * UNIT, 4_000 * UNIT);
        assert_ok!(Pool::add_liquidity(
            RuntimeOrigin::signed(BOB),
            tao(UNIT),
            tusd(400 * UNIT),
            0
        ));
    });
}

#[test]
fn first_deposit_must_exceed_the_minimums() {
    new_test_ext().execute_with(|| {
        assert_ok!(Pool::set_enabled(RuntimeOrigin::root(), true));
        fund(ALICE, 100 * UNIT, 100_000 * UNIT);
        // Fewer shares than are locked.
        assert_noop!(
            Pool::add_liquidity(RuntimeOrigin::signed(ALICE), tao(40), tusd(16_000), 0),
            Error::<Test>::AmountTooLow
        );
        // Enough shares, but the TAO reserve would be below the minimum.
        assert_noop!(
            Pool::add_liquidity(
                RuntimeOrigin::signed(ALICE),
                tao(MIN_RESERVE - 1),
                tusd(400 * (MIN_RESERVE - 1)),
                0
            ),
            Error::<Test>::ReserveTooLow
        );
    });
}

#[test]
fn later_deposits_follow_the_pool_ratio() {
    new_test_ext().execute_with(|| {
        seed(10 * UNIT);
        fund(BOB, 100 * UNIT, 10_000 * UNIT);
        let total_before = TotalShares::<Test>::get();

        // BOB offers more tUSD than 1 TAO pairs with: only 400 are taken.
        assert_ok!(Pool::add_liquidity(
            RuntimeOrigin::signed(BOB),
            tao(UNIT),
            tusd(5_000 * UNIT),
            0
        ));

        assert_eq!(Shares::<Test>::get(BOB), total_before / 10);
        assert_eq!(Balances::free_balance(BOB), tao(99 * UNIT));
        assert_eq!(Tusd::balance(&BOB), tusd(9_600 * UNIT));
        assert_eq!(Pool::spot_price(), Some(400 * PRICE_SCALE));

        // And the other way round: TAO is the excess.
        assert_ok!(Pool::add_liquidity(
            RuntimeOrigin::signed(BOB),
            tao(50 * UNIT),
            tusd(400 * UNIT),
            0
        ));
        assert_eq!(Shares::<Test>::get(BOB), total_before / 5);
        assert_eq!(Balances::free_balance(BOB), tao(98 * UNIT));
        assert_invariants();
    });
}

#[test]
fn deposit_respects_min_shares_and_balances() {
    new_test_ext().execute_with(|| {
        seed(10 * UNIT);
        fund(BOB, 2 * UNIT, 400 * UNIT);
        let expected = TotalShares::<Test>::get() / 10;
        assert_noop!(
            Pool::add_liquidity(
                RuntimeOrigin::signed(BOB),
                tao(UNIT),
                tusd(400 * UNIT),
                expected + 1
            ),
            Error::<Test>::SlippageExceeded
        );
        assert_noop!(
            Pool::add_liquidity(RuntimeOrigin::signed(BOB), tao(0), tusd(400 * UNIT), 0),
            Error::<Test>::AmountTooLow
        );
        // CAROL has nothing.
        assert!(
            Pool::add_liquidity(RuntimeOrigin::signed(CAROL), tao(UNIT), tusd(400 * UNIT), 0)
                .is_err()
        );
        assert_ok!(Pool::add_liquidity(
            RuntimeOrigin::signed(BOB),
            tao(UNIT),
            tusd(400 * UNIT),
            expected
        ));
        assert_invariants();
    });
}

#[test]
fn deposit_rounds_for_the_pool() {
    new_test_ext().execute_with(|| {
        seed(10 * UNIT);
        lift_caps();
        // Skew the reserves so the ratio is not round.
        fund(BOB, 100 * UNIT, 100_000 * UNIT);
        assert_ok!(Pool::swap_tusd_for_tao(
            RuntimeOrigin::signed(BOB),
            tusd(777 * UNIT + 13),
            tao(0)
        ));
        let before = Pool::reserves();
        let total_before = TotalShares::<Test>::get();

        assert_ok!(Pool::add_liquidity(
            RuntimeOrigin::signed(BOB),
            tao(3),
            tusd(10_000),
            0
        ));

        // Neither reserve per share went down.
        let after = Pool::reserves();
        let total_after = TotalShares::<Test>::get();
        assert!(u128::from(after.tao) * total_before >= u128::from(before.tao) * total_after);
        assert!(u128::from(after.tusd) * total_before >= u128::from(before.tusd) * total_after);
        assert_invariants();
    });
}

#[test]
fn shares_redeem_both_reserves_pro_rata() {
    new_test_ext().execute_with(|| {
        seed(10 * UNIT);
        fund(BOB, 11 * UNIT, 4_000 * UNIT);
        assert_ok!(Pool::add_liquidity(
            RuntimeOrigin::signed(BOB),
            tao(10 * UNIT),
            tusd(4_000 * UNIT),
            0
        ));
        let shares = Shares::<Test>::get(BOB);

        assert_ok!(Pool::remove_liquidity(
            RuntimeOrigin::signed(BOB),
            shares / 2,
            tao(0),
            tusd(0)
        ));
        assert_eq!(Balances::free_balance(BOB), tao(6 * UNIT));
        assert_eq!(Tusd::balance(&BOB), tusd(2_000 * UNIT));
        System::assert_last_event(
            Event::LiquidityRemoved {
                who: BOB,
                tao: tao(5 * UNIT),
                tusd: tusd(2_000 * UNIT),
                shares: shares / 2,
            }
            .into(),
        );

        assert_ok!(Pool::remove_liquidity(
            RuntimeOrigin::signed(BOB),
            shares / 2,
            tao(0),
            tusd(0)
        ));
        assert!(!Shares::<Test>::contains_key(BOB));
        assert_eq!(Pool::spot_price(), Some(400 * PRICE_SCALE));
        assert_invariants();
    });
}

#[test]
fn withdrawal_stays_open_while_the_pool_is_disabled() {
    new_test_ext().execute_with(|| {
        seed(10 * UNIT);
        assert_ok!(Pool::set_enabled(RuntimeOrigin::signed(GUARDIAN), false));
        let shares = Shares::<Test>::get(ALICE);
        let owed =
            math::redemption(Pool::reserves(), TotalShares::<Test>::get(), shares / 2).unwrap();
        assert_ok!(Pool::remove_liquidity(
            RuntimeOrigin::signed(ALICE),
            shares / 2,
            tao(0),
            tusd(0)
        ));
        assert!(owed.tao > 4 * UNIT && owed.tusd > 1_999 * UNIT);
        assert_eq!(Balances::free_balance(ALICE), tao(UNIT + owed.tao));
        assert_eq!(Tusd::balance(&ALICE), tusd(owed.tusd));
        // Deposits stay closed.
        assert_noop!(
            Pool::add_liquidity(RuntimeOrigin::signed(ALICE), tao(UNIT), tusd(400 * UNIT), 0),
            Error::<Test>::PoolDisabled
        );
        assert_invariants();
    });
}

#[test]
fn withdrawal_checks_shares_and_minimums() {
    new_test_ext().execute_with(|| {
        seed(10 * UNIT);
        let shares = Shares::<Test>::get(ALICE);
        assert_noop!(
            Pool::remove_liquidity(RuntimeOrigin::signed(ALICE), shares + 1, tao(0), tusd(0)),
            Error::<Test>::InsufficientShares
        );
        assert_noop!(
            Pool::remove_liquidity(RuntimeOrigin::signed(BOB), 1, tao(0), tusd(0)),
            Error::<Test>::InsufficientShares
        );
        assert_noop!(
            Pool::remove_liquidity(RuntimeOrigin::signed(ALICE), 0, tao(0), tusd(0)),
            Error::<Test>::AmountTooLow
        );
        assert_noop!(
            Pool::remove_liquidity(
                RuntimeOrigin::signed(ALICE),
                shares / 2,
                tao(5 * UNIT),
                tusd(0)
            ),
            Error::<Test>::SlippageExceeded
        );
        assert_noop!(
            Pool::remove_liquidity(
                RuntimeOrigin::signed(ALICE),
                shares / 2,
                tao(0),
                tusd(2_000 * UNIT)
            ),
            Error::<Test>::SlippageExceeded
        );
    });
}

#[test]
fn the_last_provider_leaves_the_minimum_reserve() {
    new_test_ext().execute_with(|| {
        seed(10 * UNIT);
        let shares = Shares::<Test>::get(ALICE);
        // Everything ALICE holds would leave only what the locked shares back: too little.
        assert_noop!(
            Pool::remove_liquidity(RuntimeOrigin::signed(ALICE), shares, tao(0), tusd(0)),
            Error::<Test>::ReserveTooLow
        );
        // She can take all but the minimum reserve.
        let keep = shares / 5_000;
        assert_ok!(Pool::remove_liquidity(
            RuntimeOrigin::signed(ALICE),
            shares - keep,
            tao(0),
            tusd(0)
        ));
        let reserves = Pool::reserves();
        assert!(reserves.tao >= MIN_RESERVE && reserves.tao < 3 * MIN_RESERVE);
        assert!(Pool::spot_price().unwrap().abs_diff(400 * PRICE_SCALE) < 1_000_000);
        assert_invariants();
    });
}

// ---------------------------------------------------------------- swaps

#[test]
fn swap_tusd_for_tao_pays_the_constant_product_amount() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        fund(BOB, UNIT, 4_000 * UNIT);
        let before = Pool::reserves();
        let k = product();
        let expected = exact_out(before.tusd, before.tao, 4_000 * UNIT, Fee::<Test>::get());

        assert_ok!(Pool::swap_tusd_for_tao(
            RuntimeOrigin::signed(BOB),
            tusd(4_000 * UNIT),
            tao(0)
        ));

        let received = u64::from(Balances::free_balance(BOB)) - UNIT;
        assert!(received == expected || received + 1 == expected);
        // 4,000 dollars at 400 with a 0.3% fee and 1% of the pool: a bit under 10 TAO.
        assert!(received > 9_800_000_000 && received < 9_900_000_000);
        let after = Pool::reserves();
        assert_eq!(after.tusd, before.tusd + 4_000 * UNIT);
        assert_eq!(after.tao, before.tao - received);
        assert!(product() > k, "the fee must grow the invariant");
        System::assert_last_event(
            Event::TusdSwappedForTao {
                who: BOB,
                tusd_in: tusd(4_000 * UNIT),
                tao_out: tao(received),
                fee: tusd(12 * UNIT),
            }
            .into(),
        );
        assert_invariants();
    });
}

#[test]
fn swap_tao_for_tusd_pays_the_constant_product_amount() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        fund(BOB, 11 * UNIT, 0);
        let before = Pool::reserves();
        let k = product();
        let expected = exact_out(before.tao, before.tusd, 10 * UNIT, Fee::<Test>::get());

        assert_ok!(Pool::swap_tao_for_tusd(
            RuntimeOrigin::signed(BOB),
            tao(10 * UNIT),
            tusd(0)
        ));

        let received = u64::from(Tusd::balance(&BOB));
        assert!(received == expected || received + 1 == expected);
        assert!(received > 3_940 * UNIT && received < 3_960 * UNIT);
        let after = Pool::reserves();
        assert_eq!(after.tao, before.tao + 10 * UNIT);
        assert_eq!(after.tusd, before.tusd - received);
        assert!(product() > k);
        System::assert_last_event(
            Event::TaoSwappedForTusd {
                who: BOB,
                tao_in: tao(10 * UNIT),
                tusd_out: tusd(received),
                fee: tao(30_000_000),
            }
            .into(),
        );
        assert_invariants();
    });
}

#[test]
fn quotes_match_swaps() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        fund(BOB, 20 * UNIT, 4_000 * UNIT);

        let quoted = Pool::quote_tusd_for_tao(tusd(1_000 * UNIT)).unwrap();
        let held = Balances::free_balance(BOB);
        assert_ok!(Pool::swap_tusd_for_tao(
            RuntimeOrigin::signed(BOB),
            tusd(1_000 * UNIT),
            quoted
        ));
        assert_eq!(Balances::free_balance(BOB), held + quoted);

        let quoted = Pool::quote_tao_for_tusd(tao(5 * UNIT)).unwrap();
        let held = Tusd::balance(&BOB);
        assert_ok!(Pool::swap_tao_for_tusd(
            RuntimeOrigin::signed(BOB),
            tao(5 * UNIT),
            quoted
        ));
        assert_eq!(Tusd::balance(&BOB), held + quoted);
    });
}

#[test]
fn swap_enforces_the_minimum_out() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        fund(BOB, 20 * UNIT, 4_000 * UNIT);
        let quoted = Pool::quote_tusd_for_tao(tusd(1_000 * UNIT)).unwrap();
        assert_noop!(
            Pool::swap_tusd_for_tao(
                RuntimeOrigin::signed(BOB),
                tusd(1_000 * UNIT),
                quoted + tao(1)
            ),
            Error::<Test>::SlippageExceeded
        );
        let quoted = Pool::quote_tao_for_tusd(tao(5 * UNIT)).unwrap();
        assert_noop!(
            Pool::swap_tao_for_tusd(RuntimeOrigin::signed(BOB), tao(5 * UNIT), quoted + tusd(1)),
            Error::<Test>::SlippageExceeded
        );
    });
}

#[test]
fn swap_needs_liquidity_and_funds() {
    new_test_ext().execute_with(|| {
        assert_ok!(Pool::set_enabled(RuntimeOrigin::root(), true));
        fund(BOB, 20 * UNIT, 4_000 * UNIT);
        assert_noop!(
            Pool::swap_tusd_for_tao(RuntimeOrigin::signed(BOB), tusd(UNIT), tao(0)),
            Error::<Test>::NoLiquidity
        );
        seed(1_000 * UNIT);
        assert_noop!(
            Pool::swap_tusd_for_tao(RuntimeOrigin::signed(BOB), tusd(0), tao(0)),
            Error::<Test>::AmountTooLow
        );
        // CAROL holds neither asset.
        assert_noop!(
            Pool::swap_tusd_for_tao(RuntimeOrigin::signed(CAROL), tusd(UNIT), tao(0)),
            pallet_tusd::Error::<Test>::InsufficientBalance
        );
        assert!(Pool::swap_tao_for_tusd(RuntimeOrigin::signed(CAROL), tao(UNIT), tusd(0)).is_err());
        assert_invariants();
    });
}

#[test]
fn a_payer_may_spend_its_whole_tao_balance() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        // BOB sells every rao he holds, and CAROL deposits every rao she holds: an account
        // that only passes funds on, like the hub of the rails, keeps nothing behind.
        fund(BOB, 5 * UNIT, 0);
        assert_ok!(Pool::swap_tao_for_tusd(
            RuntimeOrigin::signed(BOB),
            tao(5 * UNIT),
            tusd(0)
        ));
        assert_eq!(Balances::free_balance(BOB), tao(0));
        assert!(Tusd::balance(&BOB) > tusd(1_900 * UNIT));

        let reserves = Pool::reserves();
        let tusd_needed = (u128::from(reserves.tusd) * u128::from(5 * UNIT)
            / u128::from(reserves.tao)) as u64
            + UNIT;
        fund(CAROL, 5 * UNIT, tusd_needed);
        assert_ok!(Pool::add_liquidity(
            RuntimeOrigin::signed(CAROL),
            tao(5 * UNIT),
            tusd(tusd_needed),
            0
        ));
        assert_eq!(Balances::free_balance(CAROL), tao(0));
        assert!(Shares::<Test>::get(CAROL) > 0);
        assert_invariants();
    });
}

#[test]
fn fee_stays_in_the_pool_for_the_providers() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        lift_caps();
        fund(BOB, 1_000 * UNIT, 400_000 * UNIT);
        // BOB trades back and forth; ALICE is the only provider.
        for round in 0..20 {
            System::set_block_number(2 + round);
            let bought = Pool::quote_tusd_for_tao(tusd(40_000 * UNIT)).unwrap();
            assert_ok!(Pool::swap_tusd_for_tao(
                RuntimeOrigin::signed(BOB),
                tusd(40_000 * UNIT),
                tao(0)
            ));
            assert_ok!(Pool::swap_tao_for_tusd(
                RuntimeOrigin::signed(BOB),
                bought,
                tusd(0)
            ));
        }
        // ALICE's shares now redeem more than she put in.
        let shares = Shares::<Test>::get(ALICE);
        let redeem =
            math::redemption(Pool::reserves(), TotalShares::<Test>::get(), shares).unwrap();
        // Each round trip returns the TAO it took, so the TAO side is whole (but for the
        // locked shares) and the fees of both legs have gathered in tUSD: 20 round trips of
        // 40,000 dollars at 0.3% a leg leave a little under 4,800 dollars.
        assert!(redeem.tao > 1_000 * UNIT - 10_000);
        let gained = redeem.tusd - 400_000 * UNIT;
        assert!(gained > 4_000 * UNIT && gained < 4_800 * UNIT, "{gained}");
        assert_invariants();
    });
}

#[test]
fn fee_is_bounded_and_admin_only() {
    new_test_ext().execute_with(|| {
        assert_eq!(Fee::<Test>::get(), Permill::from_parts(3_000));
        assert_noop!(
            Pool::set_fee(RuntimeOrigin::signed(GUARDIAN), MIN_FEE),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Pool::set_fee(RuntimeOrigin::root(), Permill::from_parts(499)),
            Error::<Test>::FeeOutOfRange
        );
        assert_noop!(
            Pool::set_fee(RuntimeOrigin::root(), Permill::from_parts(10_001)),
            Error::<Test>::FeeOutOfRange
        );
        assert_ok!(Pool::set_fee(RuntimeOrigin::root(), MIN_FEE));
        assert_ok!(Pool::set_fee(RuntimeOrigin::root(), MAX_FEE));
        assert_eq!(Fee::<Test>::get(), MAX_FEE);

        seed(1_000 * UNIT);
        fund(BOB, UNIT, 4_000 * UNIT);
        let before = Pool::reserves();
        let expected = exact_out(before.tusd, before.tao, 4_000 * UNIT, MAX_FEE);
        let quoted = u64::from(Pool::quote_tusd_for_tao(tusd(4_000 * UNIT)).unwrap());
        assert!(quoted == expected || quoted + 1 == expected);
    });
}

// ---------------------------------------------------------------- caps

#[test]
fn one_swap_is_capped_by_depth() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        fund(BOB, 100_000 * UNIT, 40_000_000 * UNIT);
        // 2% of 400,000 tUSD and of 1,000 TAO.
        assert_noop!(
            Pool::swap_tusd_for_tao(RuntimeOrigin::signed(BOB), tusd(8_000 * UNIT + 1), tao(0)),
            Error::<Test>::SwapTooLarge
        );
        assert_noop!(
            Pool::swap_tao_for_tusd(RuntimeOrigin::signed(BOB), tao(20 * UNIT + 1), tusd(0)),
            Error::<Test>::SwapTooLarge
        );
        assert_ok!(Pool::swap_tusd_for_tao(
            RuntimeOrigin::signed(BOB),
            tusd(8_000 * UNIT),
            tao(0)
        ));

        // Ten times the depth admits ten times the swap: the cap loosens as the pool deepens.
        assert_ok!(Pool::add_liquidity(
            RuntimeOrigin::signed(BOB),
            tao(9_000 * UNIT),
            tusd(4_000_000 * UNIT),
            0
        ));
        assert_ok!(Pool::swap_tusd_for_tao(
            RuntimeOrigin::signed(BOB),
            tusd(80_000 * UNIT),
            tao(0)
        ));
        assert_invariants();
    });
}

#[test]
fn flow_each_way_is_rate_limited_and_refills() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        fund(BOB, 100_000 * UNIT, 40_000_000 * UNIT);
        // Isolate the window: 10% of the paying reserve per 100 blocks.
        assert_ok!(Pool::set_swap_caps(
            RuntimeOrigin::root(),
            SwapCaps {
                per_swap: Permill::one(),
                per_window: Permill::from_percent(10),
                window: 100,
            }
        ));
        let swap =
            |amount: u64| Pool::swap_tusd_for_tao(RuntimeOrigin::signed(BOB), tusd(amount), tao(0));
        // What the tUSD window still admits at `now`.
        let headroom = |now: u32| {
            let caps = Caps::<Test>::get();
            let limit = caps.per_window.mul_floor(Pool::reserves().tusd);
            RecentFlow::<Test>::get()
                .tusd_in
                .retargeted(now, limit, caps.window)
                .available_at(now)
        };

        assert_eq!(headroom(1), 40_000 * UNIT);
        assert_noop!(swap(40_000 * UNIT + 1), Error::<Test>::SwapRateLimited);
        assert_ok!(swap(40_000 * UNIT));
        // The deposit deepened the reserve by 40,000, which opens 10% of that.
        assert_eq!(headroom(1), 4_000 * UNIT);
        assert_noop!(swap(4_000 * UNIT + 1), Error::<Test>::SwapRateLimited);

        // Half a window gives back half of what was consumed.
        System::set_block_number(51);
        assert_eq!(headroom(51), 24_000 * UNIT);
        assert_ok!(swap(24_000 * UNIT));
        assert_noop!(swap(2_400 * UNIT + 1), Error::<Test>::SwapRateLimited);

        // The other direction has its own window.
        assert_ok!(Pool::swap_tao_for_tusd(
            RuntimeOrigin::signed(BOB),
            tao(50 * UNIT),
            tusd(0)
        ));

        // After a whole idle window the full limit is back.
        System::set_block_number(151);
        let limit = Permill::from_percent(10).mul_floor(Pool::reserves().tusd);
        assert_eq!(headroom(151), limit);
        assert_invariants();
    });
}

#[test]
fn caps_are_admin_only_and_validated() {
    new_test_ext().execute_with(|| {
        let caps = SwapCaps {
            per_swap: Permill::from_percent(5),
            per_window: Permill::from_percent(50),
            window: 600,
        };
        assert_noop!(
            Pool::set_swap_caps(RuntimeOrigin::signed(GUARDIAN), caps),
            DispatchError::BadOrigin
        );
        for invalid in [
            SwapCaps {
                per_swap: Permill::zero(),
                ..caps
            },
            SwapCaps {
                per_window: Permill::zero(),
                ..caps
            },
            SwapCaps { window: 0, ..caps },
        ] {
            assert_noop!(
                Pool::set_swap_caps(RuntimeOrigin::root(), invalid),
                Error::<Test>::InvalidCaps
            );
        }
        assert_ok!(Pool::set_swap_caps(RuntimeOrigin::root(), caps));
        assert_eq!(Caps::<Test>::get(), caps);
    });
}

#[test]
fn swap_cannot_drain_a_reserve_below_the_minimum() {
    new_test_ext().execute_with(|| {
        // The smallest pool the first deposit may open.
        assert_ok!(Pool::set_enabled(RuntimeOrigin::root(), true));
        fund(ALICE, UNIT, 400 * MIN_RESERVE);
        assert_ok!(Pool::add_liquidity(
            RuntimeOrigin::signed(ALICE),
            tao(MIN_RESERVE),
            tusd(400 * MIN_RESERVE),
            0
        ));
        lift_caps();
        fund(BOB, UNIT, 400 * UNIT);
        assert_noop!(
            Pool::swap_tusd_for_tao(RuntimeOrigin::signed(BOB), tusd(40 * MIN_RESERVE), tao(0)),
            Error::<Test>::ReserveTooLow
        );
        assert_invariants();
    });
}

#[test]
fn transfers_to_the_pool_account_do_not_move_the_price() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        fund(BOB, 500 * UNIT, 4_000 * UNIT);
        assert_ok!(Balances::transfer_allow_death(
            RuntimeOrigin::signed(BOB),
            Pool::account_id(),
            tao(400 * UNIT)
        ));
        assert_ok!(Tusd::transfer(
            RuntimeOrigin::signed(BOB),
            Pool::account_id(),
            tusd(1_000 * UNIT)
        ));
        assert_eq!(Pool::spot_price(), Some(400 * PRICE_SCALE));
        assert_eq!(Pool::reserves().tao, 1_000 * UNIT);
        assert_invariants();
    });
}

// ---------------------------------------------------------------- time-weighted price

#[test]
fn twap_needs_a_full_period_of_history() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        for block in 1..1 + u64::from(TWAP_PERIOD) {
            System::set_block_number(block);
            assert_eq!(Pool::twap_price(), None);
        }
        System::set_block_number(1 + u64::from(TWAP_PERIOD));
        assert_eq!(Pool::twap_price(), Some(400 * PRICE_SCALE));
        // An idle pool keeps reporting its price.
        System::set_block_number(10_000);
        assert_eq!(Pool::twap_price(), Some(400 * PRICE_SCALE));
    });
}

#[test]
fn twap_averages_prices_by_the_blocks_they_stood() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        lift_caps();
        fund(BOB, 10_000 * UNIT, 4_000_000 * UNIT);
        // 400 stands for blocks 1..11, then a swap moves the price for blocks 11..20.
        System::set_block_number(11);
        assert_ok!(Pool::swap_tusd_for_tao(
            RuntimeOrigin::signed(BOB),
            tusd(40_000 * UNIT),
            tao(0)
        ));
        let moved = Pool::spot_price().unwrap();
        assert!(moved > 480 * PRICE_SCALE);

        System::set_block_number(20);
        let twap = Pool::twap_price().unwrap();
        let expected = (10 * 400 * PRICE_SCALE + 9 * moved) / 19;
        assert!(twap.abs_diff(expected) <= 1, "{twap} != {expected}");

        // One block on, the reference moves up a period: only the moved price is in view.
        System::set_block_number(21);
        assert_eq!(Pool::twap_price(), Some(moved));
        assert_invariants();
    });
}

#[test]
fn a_swap_reversed_in_the_same_block_leaves_no_trace_in_the_twap() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        lift_caps();
        fund(BOB, 10_000 * UNIT, 4_000_000 * UNIT);
        System::set_block_number(50);
        let before = Pool::twap_price().unwrap();

        // Double the price and sell it back down inside one block.
        let bought = Pool::quote_tusd_for_tao(tusd(170_000 * UNIT)).unwrap();
        assert_ok!(Pool::swap_tusd_for_tao(
            RuntimeOrigin::signed(BOB),
            tusd(170_000 * UNIT),
            tao(0)
        ));
        assert!(Pool::spot_price().unwrap() > 780 * PRICE_SCALE);
        assert_ok!(Pool::swap_tao_for_tusd(
            RuntimeOrigin::signed(BOB),
            bought,
            tusd(0)
        ));
        assert_eq!(Pool::twap_price(), Some(before));

        // Later blocks see only the fee the round trip left behind: under 1%.
        System::set_block_number(50 + 3 * u64::from(TWAP_PERIOD));
        let after = Pool::twap_price().unwrap();
        assert!(after >= before && after < before + before / 100);
    });
}

#[test]
fn a_price_held_for_one_block_moves_the_twap_by_at_most_its_share_of_the_period() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        lift_caps();
        fund(BOB, 10_000 * UNIT, 4_000_000 * UNIT);
        System::set_block_number(50);
        // The last swap of block 50 doubles the price; the first of block 51 undoes it.
        let bought = Pool::quote_tusd_for_tao(tusd(170_000 * UNIT)).unwrap();
        assert_ok!(Pool::swap_tusd_for_tao(
            RuntimeOrigin::signed(BOB),
            tusd(170_000 * UNIT),
            tao(0)
        ));
        let spike = Pool::spot_price().unwrap() - 400 * PRICE_SCALE;
        System::set_block_number(51);
        assert_ok!(Pool::swap_tao_for_tusd(
            RuntimeOrigin::signed(BOB),
            bought,
            tusd(0)
        ));

        let twap = Pool::twap_price().unwrap();
        let moved = twap - 400 * PRICE_SCALE;
        assert!(
            moved <= spike / u64::from(TWAP_PERIOD) + 1,
            "{moved} > {spike} / period"
        );
    });
}

#[test]
fn twap_reference_is_between_one_and_two_periods_old() {
    new_test_ext().execute_with(|| {
        seed(1_000 * UNIT);
        fund(BOB, 10_000 * UNIT, 4_000_000 * UNIT);
        let period = u64::from(TWAP_PERIOD);
        // Touch the pool every block for many periods.
        for block in 2..=10 * period {
            System::set_block_number(block);
            assert_ok!(Pool::swap_tusd_for_tao(
                RuntimeOrigin::signed(BOB),
                tusd(UNIT),
                tao(0)
            ));
            let age = block - u64::from(Accumulator::<Test>::get().reference.0);
            if block > 1 + period {
                assert!(
                    age >= period && age < 2 * period,
                    "age {age} at block {block}"
                );
                assert!(Pool::twap_price().is_some());
            }
        }
    });
}

// ---------------------------------------------------------------- math

#[test]
fn quote_never_pays_more_than_the_constant_product_allows() {
    let fee = Permill::from_parts(3_000);
    let amounts = [1, 999, UNIT, 37 * UNIT + 11, 5_000_000 * UNIT];
    let reserves = [
        MIN_RESERVE,
        UNIT,
        1_234_567 * UNIT + 891,
        21_000_000 * UNIT,
        u64::MAX / 2,
    ];
    let mut checked = 0;
    for tao_reserve in reserves {
        for tusd_reserve in reserves {
            for amount_in in amounts {
                for side in [Side::TusdIn, Side::TaoIn] {
                    let pool = Reserves {
                        tao: tao_reserve,
                        tusd: tusd_reserve,
                    };
                    let (reserve_in, reserve_out) = pool.in_out(side);
                    let Some(quote) = math::quote(pool, side, amount_in, fee) else {
                        continue;
                    };
                    let exact = exact_out(reserve_in, reserve_out, amount_in, fee);
                    assert!(quote.amount_out <= exact, "{pool:?} {side:?} {amount_in}");
                    // The balancer resolves 1e-18 of the paying reserve.
                    let slack = 2 + reserve_out / 500_000_000_000_000_000;
                    assert!(
                        exact - quote.amount_out <= slack,
                        "{pool:?} {side:?} {amount_in}"
                    );
                    assert_eq!(quote.fee, fee.mul_ceil(amount_in));
                    let net = u128::from(amount_in - quote.fee);
                    assert!(
                        (u128::from(reserve_in) + net) * u128::from(reserve_out - quote.amount_out)
                            >= u128::from(reserve_in) * u128::from(reserve_out)
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 120, "{checked}");
}

#[test]
fn quote_of_nothing_or_of_an_empty_pool_is_none() {
    let fee = Permill::from_parts(3_000);
    let pool = Reserves {
        tao: UNIT,
        tusd: 400 * UNIT,
    };
    assert_eq!(math::quote(pool, Side::TusdIn, 0, fee), None);
    // A swap so small it pays nothing.
    assert_eq!(math::quote(pool, Side::TusdIn, 1, fee), None);
    for empty in [Reserves { tao: 0, ..pool }, Reserves { tusd: 0, ..pool }] {
        assert_eq!(math::quote(empty, Side::TusdIn, UNIT, fee), None);
        assert_eq!(math::quote(empty, Side::TaoIn, UNIT, fee), None);
    }
    assert_eq!(math::price(Reserves { tao: 0, tusd: UNIT }), 0);
}

// ---------------------------------------------------------------- everything at once

/// Random deposits, withdrawals and swaps by three accounts. Whatever succeeds or fails:
/// the pool account covers the reserves, swaps never lower the product of the reserves, and
/// liquidity changes never lower either reserve per share.
#[test]
fn pool_invariants_hold_under_random_operations() {
    new_test_ext().execute_with(|| {
        seed(5_000 * UNIT);
        for who in [ALICE, BOB, CAROL] {
            fund(who, 50_000 * UNIT, 20_000_000 * UNIT);
        }
        // Keep the default cap per swap, but let the window refill quickly so that swaps
        // keep succeeding for the whole walk.
        assert_ok!(Pool::set_swap_caps(
            RuntimeOrigin::root(),
            SwapCaps {
                per_window: Permill::one(),
                window: 10,
                ..Caps::<Test>::get()
            }
        ));
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |bound: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % bound.max(1)
        };
        let mut succeeded = [0u32; 4];
        for step in 0..3_000u64 {
            System::set_block_number(1 + step / 3);
            let who = 1 + next(3);
            let reserves = Pool::reserves();
            let total = TotalShares::<Test>::get();
            let k = product();
            let op = next(4) as usize;
            let result = match op {
                0 => Pool::add_liquidity(
                    RuntimeOrigin::signed(who),
                    tao(next(reserves.tao / 4)),
                    tusd(next(reserves.tusd / 4)),
                    0,
                ),
                1 => Pool::remove_liquidity(
                    RuntimeOrigin::signed(who),
                    u128::from(next(u64::try_from(Shares::<Test>::get(who)).unwrap() + 1)),
                    tao(0),
                    tusd(0),
                ),
                2 => Pool::swap_tusd_for_tao(
                    RuntimeOrigin::signed(who),
                    tusd(next(reserves.tusd / 40)),
                    tao(0),
                ),
                _ => Pool::swap_tao_for_tusd(
                    RuntimeOrigin::signed(who),
                    tao(next(reserves.tao / 40)),
                    tusd(0),
                ),
            };
            assert_invariants();
            if result.is_err() {
                assert_eq!(
                    Pool::reserves(),
                    reserves,
                    "a failed call moved the reserves"
                );
                continue;
            }
            succeeded[op] += 1;
            let after = Pool::reserves();
            let total_after = TotalShares::<Test>::get();
            if op >= 2 {
                assert!(
                    product() >= k,
                    "a swap lowered the invariant at step {step}"
                );
                assert_eq!(total_after, total);
            } else {
                assert!(
                    u128::from(after.tao) * total >= u128::from(reserves.tao) * total_after
                        && u128::from(after.tusd) * total
                            >= u128::from(reserves.tusd) * total_after,
                    "liquidity change lowered a reserve per share at step {step}"
                );
            }
        }
        assert!(succeeded.iter().all(|count| *count > 100), "{succeeded:?}");
        assert!(RecentFlow::<Test>::get().tusd_in.limit > 0);
    });
}
