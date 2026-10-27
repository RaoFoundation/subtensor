//! Beta basket trading: the fast price anchor and the guarded NAV (swarm escalation E8,
//! findings BK-11 F1, BK-03 F1/F2/F3, BK-15 F2).
//!
//! The `swap_basket` band used to reference only the slow emission EMA (a ~monthly
//! half-life) and spot. After any real price move the slow EMA sits stale, and because the
//! band re-anchored to spot on every leg a compromised trader key could lift (or dump) spot
//! inside a block and have the fund fill leg after leg at the manipulated price, extracting
//! 1–7% of NAV per day on either leg. These tests replay those attack sequences against the
//! fixed band — `1.02 × min(slow, fast, spot)` / `0.98 × max(slow, fast, spot)` — and
//! against the guarded NAV the turnover budget and concentration cap are now measured
//! against, and assert the extraction is gone while honest trading still fills.
//!
//! Fixtures book holdings directly, as the PoCs did: a fund of cash in the root slot with
//! `BasketShares == NAV`; pools with both EMAs pinned. A same-block attack cannot move the
//! fast EMA (it is written from the previous block's closing spot), so the fast anchor is
//! pinned at the pre-attack spot and left there; the "unanchored" control re-pins it to the
//! live spot before every fund leg, which is exactly the old `min(slow, spot)` band.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use crate::tests::claim_root::{escrow_alpha, register_on_root};
use crate::tests::mock::*;
use crate::{
    BasketDailyTurnoverCap, BasketShares, BasketTradeBucket, BasketTradingEnabled, Error,
    FirstEmissionBlockNumber, NetworksAdded, Owner, SubnetAlphaIn, SubnetAlphaOut,
    SubnetFastMovingPrice, SubnetMovingPrice, SubnetTAO, TotalStake,
};
use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use sp_runtime::DispatchError;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

const TAO: u64 = 1_000_000_000;
const COLDKEY: u64 = 1;
const HOTKEY: u64 = 2;
const ATTACKER: u64 = 77;

/// The accepted key-compromise extraction ceiling the design was signed off under, in
/// basis points of NAV per day. Both attack legs must land far below it after the fix.
const ACCEPTED_CEILING_BPS: i128 = 130;

/// Attacker profit an attack may still show from integer rounding: 0.01% of NAV.
const ROUNDING_BPS: i128 = 1;

fn hotkey() -> U256 {
    U256::from(HOTKEY)
}

fn escrow() -> U256 {
    SubtensorModule::get_beta_escrow_account_id()
}

fn subnet_acct(netuid: NetUid) -> U256 {
    SubtensorModule::get_subnet_account_id(netuid).unwrap()
}

/// Pool with `tao` / `alpha` reserves, the slow EMA pinned at `slow` and the fast EMA at
/// `fast` (TAO per alpha). The runtime clamps the slow EMA at 1.0, so a stale-high slow
/// EMA is modelled as spot below 1.0.
fn make_pool(seed: u64, tao: u64, alpha: u64, slow: f64, fast: f64) -> NetUid {
    let netuid = add_dynamic_network(&U256::from(seed + 1), &U256::from(seed));
    remove_owner_registration_stake(netuid);
    SubnetTAO::<Test>::insert(netuid, TaoBalance::from(tao));
    SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(alpha));
    add_balance_to_coldkey_account(&subnet_acct(netuid), TaoBalance::from(tao));
    SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(slow));
    SubnetFastMovingPrice::<Test>::insert(netuid, U64F64::from_num(fast));
    netuid
}

/// Fund with `tao` of cash in its root slot, backed by `BasketShares == tao`, trading on.
fn make_fund_with_cash(tao: u64) {
    let coldkey = U256::from(COLDKEY);
    register_on_root(&hotkey(), 0);
    Owner::<Test>::insert(hotkey(), coldkey);
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey(),
        &escrow(),
        NetUid::ROOT,
        tao.into(),
    );
    SubnetTAO::<Test>::mutate(NetUid::ROOT, |t| *t = t.saturating_add(tao.into()));
    SubnetAlphaOut::<Test>::mutate(NetUid::ROOT, |t| *t = t.saturating_add(tao.into()));
    TotalStake::<Test>::mutate(|t| *t = t.saturating_add(tao.into()));
    BasketShares::<Test>::insert(hotkey(), tao);
    add_balance_to_coldkey_account(&subnet_acct(NetUid::ROOT), TaoBalance::from(tao));
    BasketTradingEnabled::<Test>::put(true);
    SubtensorModule::set_tao_weight(u64::MAX);
    add_balance_to_coldkey_account(&U256::from(MOCK_BLOCK_BUILDER), TaoBalance::from(TAO));
}

/// Book `alpha` on `netuid` for the fund's escrow row (as if bought earlier).
fn grant_fund_alpha(netuid: NetUid, alpha: u64) {
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey(),
        &escrow(),
        netuid,
        alpha.into(),
    );
    SubnetAlphaOut::<Test>::mutate(netuid, |t| *t = t.saturating_add(alpha.into()));
}

/// Alpha the attacker holds outside the pool.
fn grant_outside_alpha(netuid: NetUid, alpha: u64) {
    SubnetAlphaOut::<Test>::mutate(netuid, |t| *t = t.saturating_add(alpha.into()));
}

/// Enough subnets on chain for the default 1/16 concentration cap to bind.
fn make_cap_live() {
    for raw in 100u16..120 {
        NetworksAdded::<Test>::insert(NetUid::from(raw), true);
    }
    let available = SubtensorModule::get_all_subnet_netuids().len() as u64;
    assert!(SubtensorModule::binding_basket_concentration_cap(available).is_some());
}

fn nav() -> u64 {
    SubtensorModule::get_validator_basket_nav_tao(&hotkey()).to_u64()
}

fn guarded_nav() -> u64 {
    SubtensorModule::get_validator_basket_guarded_nav_tao(&hotkey()).to_u64()
}

fn spot(netuid: NetUid) -> U64F64 {
    <Test as crate::Config>::SwapInterface::current_alpha_price(netuid.into())
}

fn spot_f64(netuid: NetUid) -> f64 {
    spot(netuid).to_num::<f64>()
}

fn reserves(netuid: NetUid) -> (u64, u64) {
    (
        SubnetTAO::<Test>::get(netuid).to_u64(),
        SubnetAlphaIn::<Test>::get(netuid).to_u64(),
    )
}

fn fund_swap(from: NetUid, to: NetUid, amount: u64) -> Result<(), DispatchError> {
    SubtensorModule::do_swap_basket(U256::from(COLDKEY), hotkey(), from, to, amount, 0).map(|_| ())
}

/// Attacker market-buys alpha with `tao` (fees charged). Returns alpha received.
fn attacker_buy(netuid: NetUid, tao: u64) -> u64 {
    add_balance_to_coldkey_account(&subnet_acct(netuid), TaoBalance::from(tao));
    let out = SubtensorModule::swap_tao_for_alpha(
        netuid,
        tao.into(),
        <Test as crate::Config>::SwapInterface::max_price::<TaoBalance>(),
        false,
    )
    .unwrap();
    out.amount_paid_out.to_u64()
}

/// Attacker market-sells `alpha` (fees charged). Returns TAO received.
fn attacker_sell(netuid: NetUid, alpha: u64) -> u64 {
    let out = SubtensorModule::swap_alpha_for_tao(
        netuid,
        alpha.into(),
        <Test as crate::Config>::SwapInterface::min_price::<TaoBalance>(),
        false,
    )
    .unwrap();
    let tao = out.amount_paid_out.to_u64();
    SubtensorModule::transfer_tao_from_subnet(netuid, &U256::from(ATTACKER), tao.into()).unwrap();
    tao
}

/// TAO the attacker must pay (fees included) to buy back at least `alpha` right now.
fn tao_to_buy_alpha(netuid: NetUid, alpha: u64) -> u64 {
    let mut lo = 0u64;
    let mut hi = SubnetTAO::<Test>::get(netuid)
        .to_u64()
        .saturating_mul(4)
        .max(TAO);
    for _ in 0..60 {
        let mid = lo + (hi - lo) / 2;
        let got = with_transaction(|| {
            let got = if mid == 0 {
                0
            } else {
                attacker_buy(netuid, mid)
            };
            TransactionOutcome::Rollback(Ok::<u64, DispatchError>(got))
        })
        .unwrap();
        if got >= alpha {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    hi
}

/// Whether the fast anchor is left where the block opened (the fixed runtime) or re-pinned
/// to the live spot before every fund leg (the old `min(slow, spot)` band, for the control
/// measurement).
#[derive(Clone, Copy, PartialEq)]
enum FastAnchor {
    Held,
    Neutralized,
}

fn prepare_leg(netuid: NetUid, anchor: FastAnchor) {
    if anchor == FastAnchor::Neutralized {
        SubnetFastMovingPrice::<Test>::insert(netuid, spot(netuid));
    }
}

/// Fund chain-buys `netuid` from its cash slot in ~0.95%-of-reserve legs until a guardrail
/// refuses or `budget` is spent. Returns `(spent, legs)`.
fn fund_chain_buy(netuid: NetUid, budget: u64, anchor: FastAnchor) -> (u64, u32) {
    let mut spent = 0u64;
    let mut legs = 0u32;
    loop {
        let remaining = budget.saturating_sub(spent);
        if remaining < TAO {
            return (spent, legs);
        }
        let leg = remaining.min(SubnetTAO::<Test>::get(netuid).to_u64() / 105);
        prepare_leg(netuid, anchor);
        match fund_swap(NetUid::ROOT, netuid, leg) {
            Ok(()) => {
                spent += leg;
                legs += 1;
            }
            Err(_) => return (spent, legs),
        }
    }
}

/// Fund chain-sells its `netuid` holding into cash in legs of `leg` alpha until a guardrail
/// refuses or `max_alpha` is sold. Returns `(alpha sold, TAO received, legs)`.
fn fund_chain_sell(
    netuid: NetUid,
    leg: u64,
    max_alpha: u64,
    anchor: FastAnchor,
) -> (u64, u64, u32) {
    let mut sold = 0u64;
    let mut received = 0u64;
    let mut legs = 0u32;
    loop {
        if sold + leg > max_alpha {
            return (sold, received, legs);
        }
        let cash_before = escrow_alpha(&hotkey(), NetUid::ROOT);
        prepare_leg(netuid, anchor);
        match fund_swap(netuid, NetUid::ROOT, leg) {
            Ok(()) => {
                sold += leg;
                received += escrow_alpha(&hotkey(), NetUid::ROOT) - cash_before;
                legs += 1;
            }
            Err(_) => return (sold, received, legs),
        }
    }
}

fn bps(part: i128, whole: u64) -> i128 {
    part * 10_000 / whole as i128
}

/// Outcome of one attack run: attacker cash profit and the fund's realizable NAV loss, both
/// in basis points of the fund's pre-attack NAV, plus the fund legs that filled.
struct AttackResult {
    profit_bps: i128,
    fund_loss_bps: i128,
    legs: u32,
}

/// Stale-HIGH slow EMA, buy side (BK-11 F1 / BK-03 F2): the key holder lifts spot toward
/// the slow EMA, has the fund chain-buy from cash, then dumps. Best lift over a grid.
fn best_stale_high_buy_attack(netuid: NetUid, budget: u64, anchor: FastAnchor) -> AttackResult {
    let (pool_tao, _) = reserves(netuid);
    let mut best: Option<AttackResult> = None;
    for lift_pct in [0u64, 5, 10, 15, 20, 25, 30, 40, 50, 60, 80, 100] {
        let lift = pool_tao * lift_pct / 100;
        let res = with_transaction(|| {
            BasketTradeBucket::<Test>::remove(hotkey());
            let nav0 = nav();
            let bought = if lift > 0 {
                attacker_buy(netuid, lift)
            } else {
                0
            };
            let (_, legs) = fund_chain_buy(netuid, budget, anchor);
            let proceeds = if bought > 0 {
                attacker_sell(netuid, bought)
            } else {
                0
            };
            let profit = proceeds as i128 - lift as i128;
            let loss = nav0 as i128 - nav() as i128;
            TransactionOutcome::Rollback(Ok::<_, DispatchError>(AttackResult {
                profit_bps: bps(profit, nav0),
                fund_loss_bps: bps(loss, nav0),
                legs,
            }))
        })
        .unwrap();
        if best
            .as_ref()
            .map(|b| res.profit_bps > b.profit_bps)
            .unwrap_or(true)
        {
            best = Some(res);
        }
    }
    best.unwrap()
}

/// Stale-LOW slow EMA, sell side (BK-03 F1): the key holder front-runs with a pre-held
/// alpha dump, has the fund chain-sell its holding toward the floor, then buys the alpha
/// back. Best dump over a grid.
fn best_stale_low_sell_attack(netuid: NetUid, holding: u64, anchor: FastAnchor) -> AttackResult {
    let (pool_tao, _) = reserves(netuid);
    let mut best: Option<AttackResult> = None;
    for dump_pct in [0u64, 1, 2, 5, 10, 15, 20, 30, 40, 50, 75, 100, 150, 200] {
        let dump = pool_tao * dump_pct / 100;
        let res = with_transaction(|| {
            BasketTradeBucket::<Test>::remove(hotkey());
            let nav0 = nav();
            grant_outside_alpha(netuid, dump);
            let proceeds = if dump > 0 {
                attacker_sell(netuid, dump)
            } else {
                0
            };
            let (_, _, legs) = fund_chain_sell(netuid, holding / 100, holding, anchor);
            let cost = if dump > 0 {
                let tao = tao_to_buy_alpha(netuid, dump);
                attacker_buy(netuid, tao);
                tao
            } else {
                0
            };
            let profit = proceeds as i128 - cost as i128;
            let loss = nav0 as i128 - nav() as i128;
            TransactionOutcome::Rollback(Ok::<_, DispatchError>(AttackResult {
                profit_bps: bps(profit, nav0),
                fund_loss_bps: bps(loss, nav0),
                legs,
            }))
        })
        .unwrap();
        if best
            .as_ref()
            .map(|b| res.profit_bps > b.profit_bps)
            .unwrap_or(true)
        {
            best = Some(res);
        }
    }
    best.unwrap()
}

// =============================================================================
// E8, buy side: stale-high slow EMA
// =============================================================================

/// With the slow EMA stale above spot by ×1.35, ×2 and ×4, a same-block lift-buy-dump used
/// to extract 1.1% / 4.1% / 6.9% of NAV per day (the fund chain-bought near the slow EMA).
/// Anchored to the fast EMA the fund fills at most one in-band leg at the pre-lift price
/// and the attacker cannot profit; the control with the fast anchor neutralized reproduces
/// the old extraction, well above the accepted ceiling.
#[test]
fn test_e8_stale_high_ema_buy_extraction_closed_by_fast_anchor() {
    new_test_ext(1).execute_with(|| {
        let fund_nav = 100_000 * TAO;
        make_fund_with_cash(fund_nav);
        let budget = SubtensorModule::basket_trade_budget_tao(guarded_nav());
        assert!(budget > fund_nav / 11 && budget <= fund_nav / 10);
        let pool_tao = 100_000 * TAO;
        for (i, m) in [1.35f64, 2.0, 4.0].iter().enumerate() {
            // Slow EMA 1.0 (clamped), spot 1/m, fast anchor at the spot the block opened at.
            let alpha = (pool_tao as f64 * m) as u64;
            let x = make_pool(100 + 10 * i as u64, pool_tao, alpha, 1.0, 1.0 / m);
            let p0 = spot_f64(x);
            assert!((p0 * m - 1.0).abs() < 0.001);

            let fixed = best_stale_high_buy_attack(x, budget, FastAnchor::Held);
            let control = best_stale_high_buy_attack(x, budget, FastAnchor::Neutralized);
            println!(
                "E8 buy m={m:.2}: fixed profit {} bps NAV / fund loss {} bps / legs {}; \
                 unanchored control profit {} bps / fund loss {} bps / legs {}",
                fixed.profit_bps,
                fixed.fund_loss_bps,
                fixed.legs,
                control.profit_bps,
                control.fund_loss_bps,
                control.legs
            );

            assert!(
                fixed.profit_bps <= ROUNDING_BPS,
                "m={m}: attacker still profits {} bps of NAV",
                fixed.profit_bps
            );
            assert!(
                fixed.fund_loss_bps <= 10,
                "m={m}: fund lost {} bps of NAV to a lift-buy-dump",
                fixed.fund_loss_bps
            );
            assert!(
                fixed.legs <= 1,
                "m={m}: the fund filled {} legs against a lifted spot",
                fixed.legs
            );
            // The control must still show the vulnerability the anchor closes: at m >= 2
            // the old band leaked more than the accepted ceiling in a single block.
            if *m >= 2.0 {
                assert!(
                    control.profit_bps > ACCEPTED_CEILING_BPS,
                    "m={m}: control leaked only {} bps; the harness no longer reproduces E8",
                    control.profit_bps
                );
            }
            assert!(control.profit_bps > fixed.profit_bps + 50);
        }
    });
}

// =============================================================================
// E8, sell side: stale-low slow EMA (BK-03 F1)
// =============================================================================

/// The mirror: with spot above a stale-low slow EMA by ×1.35, ×2 and ×4, a pre-held dump
/// used to let the fund chain-sell its holding toward `0.98 × slow EMA` while the attacker
/// bought the alpha back below fair (1.05% / 3.05% / 4.71% of NAV). Anchored to the fast
/// EMA the fund sells nothing below the pre-dump price; the neutralized control reproduces
/// the leak.
#[test]
fn test_e8_stale_low_ema_sell_extraction_closed_by_fast_anchor() {
    new_test_ext(1).execute_with(|| {
        let cash = 90_000 * TAO;
        make_fund_with_cash(cash);
        let pool_tao = 100_000 * TAO;
        let holding = 10_000 * TAO;
        for (i, m) in [1.35f64, 2.0, 4.0].iter().enumerate() {
            // Spot 1.0, slow EMA 1/m (the pool rallied m-fold within the EMA's memory),
            // fast anchor at the spot the block opened at.
            let x = make_pool(300 + 10 * i as u64, pool_tao, pool_tao, 1.0 / m, 1.0);
            grant_fund_alpha(x, holding);

            let fixed = best_stale_low_sell_attack(x, holding, FastAnchor::Held);
            let control = best_stale_low_sell_attack(x, holding, FastAnchor::Neutralized);
            println!(
                "E8 sell m={m:.2}: fixed profit {} bps NAV / fund loss {} bps / legs {}; \
                 unanchored control profit {} bps / fund loss {} bps / legs {}",
                fixed.profit_bps,
                fixed.fund_loss_bps,
                fixed.legs,
                control.profit_bps,
                control.fund_loss_bps,
                control.legs
            );

            assert!(
                fixed.profit_bps <= ROUNDING_BPS,
                "m={m}: attacker still profits {} bps of NAV",
                fixed.profit_bps
            );
            assert!(
                fixed.fund_loss_bps <= 10,
                "m={m}: fund lost {} bps of NAV to a dump-sell-buyback",
                fixed.fund_loss_bps
            );
            if *m >= 2.0 {
                assert!(
                    control.profit_bps > ACCEPTED_CEILING_BPS,
                    "m={m}: control leaked only {} bps; the harness no longer reproduces BK-03 F1",
                    control.profit_bps
                );
            }
            assert!(control.profit_bps > fixed.profit_bps + 50);

            // The fund's holding is intact: nothing was sold below the anchor.
            assert_eq!(escrow_alpha(&hotkey(), x), holding);
        }
    });
}

// =============================================================================
// Honest trading still fills
// =============================================================================

/// When the anchors agree with spot an in-band trade fills on both legs, and when the fast
/// EMA has followed a genuine move it lets the fund trade at the new level even though the
/// slow EMA is still stale (the slow EMA remains the level cap in the direction it always
/// was).
#[test]
fn test_e8_honest_trade_at_spot_still_fills() {
    new_test_ext(1).execute_with(|| {
        make_fund_with_cash(100_000 * TAO);
        // Deep pool, every anchor at spot: a 1%-impact buy and the matching sell both fill.
        let x = make_pool(100, 1_000_000 * TAO, 1_000_000 * TAO, 1.0, 1.0);
        assert_ok!(fund_swap(NetUid::ROOT, x, 5_000 * TAO));
        let held = escrow_alpha(&hotkey(), x);
        assert!(held > 0);
        assert_ok!(fund_swap(x, NetUid::ROOT, held / 2));

        // A pool that genuinely halved two hours ago: the fast EMA has followed spot down
        // while the slow EMA is still at 1.0. Buying the dip at the new level is allowed —
        // the fast anchor does not lock trading out once it has caught up.
        let y = make_pool(200, 500_000 * TAO, 1_000_000 * TAO, 1.0, 0.5);
        assert!((spot_f64(y) - 0.5).abs() < 0.001);
        BasketTradeBucket::<Test>::remove(hotkey());
        assert_ok!(fund_swap(NetUid::ROOT, y, 2_000 * TAO));

        // ...but a pool whose spot was lifted this block above where it opened (fast
        // anchor 0.4, spot 0.5, slow EMA still permissive at 1.0) is refused: nothing may
        // fill more than 2% above the price the block opened at.
        let z = make_pool(300, 500_000 * TAO, 1_000_000 * TAO, 1.0, 0.4);
        assert_noop!(
            fund_swap(NetUid::ROOT, z, 2_000 * TAO),
            Error::<Test>::SlippageTooHigh
        );
    });
}

// =============================================================================
// Fast EMA maintenance
// =============================================================================

/// The fast EMA seeds at spot on its first update, then closes half the distance to a new
/// spot every `BASKET_FAST_EMA_HALF_LIFE_BLOCKS` updates, and is advanced by the same
/// per-block hook that advances the slow EMA.
#[test]
fn test_fast_moving_price_bootstraps_and_halves_distance_per_half_life() {
    new_test_ext(1).execute_with(|| {
        let x = make_pool(100, 1_000_000 * TAO, 1_000_000 * TAO, 1.0, 1.0);
        SubnetFastMovingPrice::<Test>::remove(x);
        assert!(SubnetFastMovingPrice::<Test>::get(x).is_none());

        // First update seeds at spot (1.0).
        SubtensorModule::update_fast_moving_price(x);
        let seeded = SubnetFastMovingPrice::<Test>::get(x)
            .unwrap()
            .to_num::<f64>();
        assert!((seeded - 1.0).abs() < 1e-9, "seeded at {seeded}");

        // Spot jumps to 2.0 and holds: after one half-life the fast EMA is at ~1.5, after
        // two at ~1.75.
        SubnetTAO::<Test>::insert(x, TaoBalance::from(2_000_000 * TAO));
        let half_life = crate::BASKET_FAST_EMA_HALF_LIFE_BLOCKS;
        for _ in 0..half_life {
            SubtensorModule::update_fast_moving_price(x);
        }
        let after_one = SubnetFastMovingPrice::<Test>::get(x)
            .unwrap()
            .to_num::<f64>();
        assert!(
            (after_one - 1.5).abs() < 0.01,
            "after one half-life: {after_one}"
        );
        for _ in 0..half_life {
            SubtensorModule::update_fast_moving_price(x);
        }
        let after_two = SubnetFastMovingPrice::<Test>::get(x)
            .unwrap()
            .to_num::<f64>();
        assert!(
            (after_two - 1.75).abs() < 0.01,
            "after two half-lives: {after_two}"
        );

        // The slow-EMA hook advances the fast series too (the fast series is not clamped
        // at parity the way the slow one is).
        FirstEmissionBlockNumber::<Test>::insert(x, 1);
        SubtensorModule::update_moving_price(x);
        let after_hook = SubnetFastMovingPrice::<Test>::get(x)
            .unwrap()
            .to_num::<f64>();
        assert!(
            after_hook > after_two && after_hook <= 2.0,
            "hook advanced to {after_hook}"
        );
        assert!(SubnetMovingPrice::<Test>::get(x).to_num::<f64>() <= 1.0);
    });
}

// =============================================================================
// BK-15 F2 / BK-03 F3: the guarded NAV cannot be pumped
// =============================================================================

/// The concentration cap and the turnover budget are measured against a NAV that marks each
/// holding at `min(realizable, alpha × slow EMA)`. A same-block pump of a held thin pool
/// used to mark that holding at roughly the pump size (a 1,500 TAO buy refused by the cap
/// became admitted at 13.85% of real NAV; a 9,500 TAO buy refused by the budget became
/// admitted at 87.7% of NAV into one pool). Both stay refused now, and the fund's reported
/// budget does not move with the pump.
#[test]
fn test_bk15_pumped_thin_holding_cannot_admit_oversize_buy() {
    new_test_ext(1).execute_with(|| {
        // Fund: 10k TAO cash + 900 alpha of a thin 10k/10k pool (9% of its alpha, inside
        // the liquidity cap), and a deep destination pool D.
        let x = make_pool(100, 10_000 * TAO, 10_000 * TAO, 1.0, 1.0);
        let d = make_pool(200, 1_000_000 * TAO, 1_000_000 * TAO, 1.0, 1.0);
        make_fund_with_cash(10_000 * TAO);
        grant_fund_alpha(x, 900 * TAO);
        make_cap_live();
        let nav_real = nav();
        let guarded_real = guarded_nav();
        assert_eq!(guarded_real, nav_real, "unpumped: both marks agree");
        let honest_budget = SubtensorModule::basket_trade_budget_tao(guarded_real);

        // --- A: cap alone (turnover budget lifted): a 1,500 TAO buy is over 1/16 of NAV.
        with_transaction(|| {
            BasketDailyTurnoverCap::<Test>::put(u16::MAX);
            let trade = 1_500 * TAO;
            assert_noop!(
                fund_swap(NetUid::ROOT, d, trade),
                Error::<Test>::BasketConcentrationCapExceeded
            );

            // Same block: pump X with 100k TAO. The realizable NAV balloons, the guarded
            // NAV does not, and the same buy stays refused.
            let pump = 100_000 * TAO;
            let got = attacker_buy(x, pump);
            let nav_pumped = nav();
            let guarded_pumped = guarded_nav();
            println!(
                "BK-15 A: pump {} TAO -> realizable NAV {} -> {} TAO, guarded NAV {} -> {} TAO",
                pump / TAO,
                nav_real / TAO,
                nav_pumped / TAO,
                guarded_real / TAO,
                guarded_pumped / TAO
            );
            assert!(nav_pumped > nav_real * 5, "the realizable mark must be pumpable");
            assert!(
                guarded_pumped <= guarded_real + 900 * TAO,
                "guarded NAV moved to {guarded_pumped}"
            );
            assert_noop!(
                fund_swap(NetUid::ROOT, d, trade),
                Error::<Test>::BasketConcentrationCapExceeded
            );
            attacker_sell(x, got);
            TransactionOutcome::Rollback(Ok::<(), DispatchError>(()))
        })
        .unwrap();

        // --- B: default 10% turnover budget: a 9,500 TAO buy is over budget, before and
        // after a 1M TAO pump; the reported budget is unchanged by the pump.
        let trade = 9_500 * TAO;
        assert_noop!(
            fund_swap(NetUid::ROOT, d, trade),
            Error::<Test>::BasketTurnoverBudgetExceeded
        );
        let pump = 1_000_000 * TAO;
        let got = attacker_buy(x, pump);
        let status = SubtensorModule::get_basket_trading_status(&hotkey());
        println!(
            "BK-15 B: pump {} TAO -> realizable NAV {} TAO, guarded NAV {} TAO, budget {} TAO (honest {} TAO)",
            pump / TAO,
            nav() / TAO,
            guarded_nav() / TAO,
            status.budget_tao.to_u64() / TAO,
            honest_budget / TAO
        );
        assert!(status.budget_tao.to_u64() <= honest_budget + 90 * TAO);
        assert_noop!(
            fund_swap(NetUid::ROOT, d, trade),
            Error::<Test>::BasketTurnoverBudgetExceeded
        );
        attacker_sell(x, got);
        assert_eq!(escrow_alpha(&hotkey(), d), 0, "nothing was bought on D");

        // A trade inside the honest budget and cap still fills.
        assert_ok!(fund_swap(NetUid::ROOT, d, 500 * TAO));
    });
}

/// The guarded mark is one-sided: a holding trading below its slow EMA is marked at the
/// (lower) realizable quote, one trading above it at the (lower) EMA value, and root cash
/// passes through 1:1.
#[test]
fn test_guarded_holding_value_takes_the_lower_mark() {
    new_test_ext(1).execute_with(|| {
        let alpha = 1_000 * TAO;
        assert_eq!(
            SubtensorModule::guarded_basket_holding_value(NetUid::ROOT, alpha, alpha),
            alpha
        );
        // Below the EMA: realizable (900) < EMA value (1,000).
        let below = make_pool(100, 900_000 * TAO, 1_000_000 * TAO, 1.0, 0.9);
        let realizable = SubtensorModule::realizable_tao_for_alpha(below, alpha);
        assert!(realizable < alpha);
        assert_eq!(
            SubtensorModule::guarded_basket_holding_value(below, alpha, realizable),
            realizable
        );
        // Above the EMA: realizable (~1,500) > EMA value (1,000).
        let above = make_pool(200, 1_500_000 * TAO, 1_000_000 * TAO, 1.0, 1.5);
        let realizable = SubtensorModule::realizable_tao_for_alpha(above, alpha);
        assert!(realizable > alpha);
        assert_eq!(
            SubtensorModule::guarded_basket_holding_value(above, alpha, realizable),
            alpha
        );
    });
}
