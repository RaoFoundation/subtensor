//! Beta basket: validator-directed rebalancing (`swap_basket`).
//!
//! Trades change only the fund's composition. Shares, the claimable rate, and every
//! staker's watermark are untouched; NAV moves only by fees and slippage; root reserves
//! stay in lockstep; the block author receives the AMM fee; both legs are booked as
//! protocol flow. Every gate, guardrail, and rollback path is pinned here.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::CheckColdkeySwap;
use crate::migrations::migrate_seed_beta_basket::kickoff_seed_beta_basket_v2;
use crate::tests::claim_root::{
    escrow_alpha, flush_baskets, fund_pool, fund_shares, register_on_root, root_stake_of,
    zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::{
    BASKET_TRADE_WINDOW_BLOCKS, BasketClaimed, BasketDailyTurnoverCap, BasketRate,
    BasketTradeWindow, BasketTradingEnabled, BasketTradingFrozen, ColdkeySwapAnnouncements,
    DEFAULT_BASKET_DAILY_TURNOVER_CAP, DefaultMinStake, Error, Event, RootWeightsCap,
    SubnetAlphaIn, SubnetAlphaOut, SubnetMovingPrice, SubnetProtocolFlow, SubnetTAO, SubnetTaoFlow,
    SubtokenEnabled, TotalStake, Uids,
};
use codec::Encode;
use frame_support::dispatch::DispatchResultWithPostInfo;
use frame_support::traits::{ExtendedDispatchable, Get};
use frame_support::weights::Weight;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use sp_runtime::traits::Hash;
use substrate_fixed::types::I96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

type HashingOf<T> = <T as frame_system::Config>::Hashing;

/// Economic bound: a trade may only cost AMM fees and slippage, so NAV must stay within
/// this percentage of its pre-trade value on deep pools.
const FEE_TOLERANCE_PCT: u64 = 5;

/// Dividend credited to the fund in the standard playground (alpha on subnet A, price ~1).
const DIVIDEND: u64 = 100_000_000;

/// A trade comfortably above `DefaultMinStake` (2 TAO in the mock) and well inside the
/// default 10% turnover budget of a `DIVIDEND`-sized fund.
const TRADE: u64 = 4_000_000;

struct Fund {
    /// Owns `hotkey`; the account that signs trades.
    coldkey: U256,
    /// Root-registered validator whose basket is traded.
    hotkey: U256,
    /// Root staker entitled to the fund's dividends.
    staker: U256,
    /// Dividend origin; the fund's initial holding lives here.
    netuid_a: NetUid,
    /// Second deep pool to trade into.
    netuid_b: NetUid,
}

/// A root validator whose fund holds `DIVIDEND` alpha on subnet A, a second deep pool B,
/// EMA prices pinned at 1.0, trading enabled, and the turnover budget lifted to 100% so
/// happy-path tests can move whole holdings. Guardrail tests tighten what they need.
fn setup_fund() -> Fund {
    let coldkey = U256::from(1001);
    let hotkey = U256::from(1002);
    let staker = U256::from(1003);
    let owner_b = U256::from(2001);
    let hotkey_b = U256::from(2002);

    let netuid_a = add_dynamic_network(&hotkey, &coldkey);
    let netuid_b = add_dynamic_network(&hotkey_b, &owner_b);
    remove_owner_registration_stake(netuid_a);
    fund_pool(netuid_a);
    fund_pool(netuid_b);
    SubnetMovingPrice::<Test>::insert(netuid_a, I96F32::from_num(1));
    SubnetMovingPrice::<Test>::insert(netuid_b, I96F32::from_num(1));

    SubtensorModule::set_tao_weight(u64::MAX);
    zero_claim_threshold();

    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &staker,
        NetUid::ROOT,
        2_000_000u64.into(),
    );
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &coldkey,
        netuid_a,
        10_000_000u64.into(),
    );
    register_on_root(&hotkey, 0);

    SubtensorModule::distribute_emission(
        netuid_a,
        AlphaBalance::ZERO,
        AlphaBalance::ZERO,
        DIVIDEND.into(),
        AlphaBalance::ZERO,
    );
    flush_baskets();
    assert!(escrow_alpha(&hotkey, netuid_a) > 0);
    assert!(fund_shares(&hotkey) > 0);

    BasketTradingEnabled::<Test>::put(true);
    BasketDailyTurnoverCap::<Test>::put(u16::MAX);

    Fund {
        coldkey,
        hotkey,
        staker,
        netuid_a,
        netuid_b,
    }
}

fn swap(fund: &Fund, origin: NetUid, dest: NetUid, amount: u64) -> DispatchResultWithPostInfo {
    SubtensorModule::swap_basket(
        RuntimeOrigin::signed(fund.coldkey),
        fund.hotkey,
        origin,
        dest,
        amount.into(),
    )
}

fn nav(hotkey: &U256) -> u64 {
    SubtensorModule::get_validator_basket_nav_tao(hotkey).to_u64()
}

fn author_balance() -> u64 {
    SubtensorModule::get_coldkey_balance(&U256::from(MOCK_BLOCK_BUILDER)).to_u64()
}

/// Forget the fund's turnover window so consecutive whole-holding trades in one test are
/// not limited by the budget (budget behaviour has its own tests).
fn reset_turnover_window(hotkey: &U256) {
    BasketTradeWindow::<Test>::remove(hotkey);
}

/// `(alpha_sold, tao_mid, alpha_bought)` of the most recent `BasketSwapped` event.
fn last_swap_event() -> (u64, u64, u64) {
    System::events()
        .iter()
        .rev()
        .find_map(|record| match &record.event {
            RuntimeEvent::SubtensorModule(Event::BasketSwapped {
                alpha_sold,
                tao_mid,
                alpha_bought,
                ..
            }) => Some((alpha_sold.to_u64(), tao_mid.to_u64(), alpha_bought.to_u64())),
            _ => None,
        })
        .expect("a BasketSwapped event was emitted")
}

/// Snapshot of everything a trade must leave untouched.
#[derive(PartialEq, Debug)]
struct Entitlements {
    shares: u64,
    rate: I96F32,
    claimed: i128,
    owed: u64,
    root_stake: u64,
}

fn entitlements(fund: &Fund) -> Entitlements {
    Entitlements {
        shares: fund_shares(&fund.hotkey),
        rate: BasketRate::<Test>::get(fund.hotkey),
        claimed: BasketClaimed::<Test>::get(fund.hotkey, fund.staker),
        owed: SubtensorModule::get_basket_owed_shares(&fund.hotkey, &fund.staker),
        root_stake: root_stake_of(&fund.hotkey, &fund.staker),
    }
}

fn assert_nav_within_fees(before: u64, after: u64) {
    assert!(
        after <= before,
        "a trade cannot create value: {before} -> {after}"
    );
    assert!(
        after >= before * (100 - FEE_TOLERANCE_PCT) / 100,
        "a trade may only cost fees and slippage: {before} -> {after}"
    );
}

// =============================================================================
// Happy paths
// =============================================================================

/// Alpha -> alpha: the holding moves, entitlements are untouched, NAV moves only by fees,
/// TotalStake drops by exactly the block author's fee, the event names both legs.
#[test]
fn test_swap_basket_alpha_to_alpha_is_composition_only() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        let held = escrow_alpha(&fund.hotkey, fund.netuid_a);
        let before = entitlements(&fund);
        let nav_before = nav(&fund.hotkey);
        let ts_before = TotalStake::<Test>::get().to_u64();
        let author_before = author_balance();

        assert_ok!(swap(&fund, fund.netuid_a, fund.netuid_b, TRADE));

        let (alpha_sold, tao_mid, alpha_bought) = last_swap_event();
        assert_eq!(alpha_sold, TRADE);
        assert!(tao_mid > 0 && tao_mid <= TRADE, "tao_mid = {tao_mid}");
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_a), held - TRADE);
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_b), alpha_bought);
        assert!(alpha_bought > 0);

        assert_eq!(entitlements(&fund), before);
        assert_nav_within_fees(nav_before, nav(&fund.hotkey));

        // The block author is paid on both legs. Only the sell leg's fee (alpha sold
        // fee-free for TAO that leaves the pool) reduces `TotalStake`; the buy leg's fee is
        // TAO already counted in by `swap_tao_for_alpha` and merely moves pot -> author,
        // exactly as in `stake_into_subnet`. The sell-leg fee is what the origin pool
        // booked as protocol outflow beyond `tao_mid`.
        let author_fee = author_balance() - author_before;
        assert!(
            author_fee > 0,
            "the block author must receive the swap fees"
        );
        let sell_fee_outflow = (-SubnetProtocolFlow::<Test>::get(fund.netuid_a)) as u64 - tao_mid;
        assert!(sell_fee_outflow > 0 && sell_fee_outflow < author_fee);
        assert_eq!(
            TotalStake::<Test>::get().to_u64(),
            ts_before - sell_fee_outflow
        );
    });
}

/// Alpha -> TAO (destination 0): the proceeds become root cash 1:1, the root reserves are
/// credited by exactly `tao_mid`, and the cash slot is worth exactly its face value.
#[test]
fn test_swap_basket_alpha_to_root_credits_reserves_in_lockstep() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        let held = escrow_alpha(&fund.hotkey, fund.netuid_a);
        let root_tao_before = SubnetTAO::<Test>::get(NetUid::ROOT).to_u64();
        let root_alpha_out_before = SubnetAlphaOut::<Test>::get(NetUid::ROOT).to_u64();
        let nav_before = nav(&fund.hotkey);
        let before = entitlements(&fund);

        // The whole holding.
        assert_ok!(swap(&fund, fund.netuid_a, NetUid::ROOT, held));

        let (alpha_sold, tao_mid, alpha_bought) = last_swap_event();
        assert_eq!(alpha_sold, held);
        assert_eq!(alpha_bought, tao_mid, "root cash is TAO 1:1");
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_a), 0);
        assert_eq!(escrow_alpha(&fund.hotkey, NetUid::ROOT), tao_mid);
        assert_eq!(
            SubnetTAO::<Test>::get(NetUid::ROOT).to_u64(),
            root_tao_before + tao_mid
        );
        assert_eq!(
            SubnetAlphaOut::<Test>::get(NetUid::ROOT).to_u64(),
            root_alpha_out_before + tao_mid
        );
        assert_eq!(nav(&fund.hotkey), tao_mid, "cash values at face");
        assert_eq!(entitlements(&fund), before);
        assert_nav_within_fees(nav_before, nav(&fund.hotkey));
    });
}

/// TAO -> alpha (origin 0): the cash slot is debited, the root reserves unwind exactly to
/// where they started, and no block-author fee is taken on the root leg.
#[test]
fn test_swap_basket_root_to_alpha_debits_reserves_in_lockstep() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        let root_tao_start = SubnetTAO::<Test>::get(NetUid::ROOT).to_u64();
        let root_alpha_out_start = SubnetAlphaOut::<Test>::get(NetUid::ROOT).to_u64();
        assert_ok!(swap(
            &fund,
            fund.netuid_a,
            NetUid::ROOT,
            escrow_alpha(&fund.hotkey, fund.netuid_a)
        ));
        let cash = escrow_alpha(&fund.hotkey, NetUid::ROOT);
        assert!(cash > 0);
        reset_turnover_window(&fund.hotkey);

        let before = entitlements(&fund);
        let nav_before = nav(&fund.hotkey);
        let ts_before = TotalStake::<Test>::get().to_u64();
        let author_before = author_balance();

        assert_ok!(swap(&fund, NetUid::ROOT, fund.netuid_b, cash));

        let (alpha_sold, tao_mid, alpha_bought) = last_swap_event();
        assert_eq!(alpha_sold, cash);
        assert_eq!(tao_mid, cash, "selling cash is TAO 1:1 with no fee");
        assert_eq!(escrow_alpha(&fund.hotkey, NetUid::ROOT), 0);
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_b), alpha_bought);
        assert!(alpha_bought > 0);
        assert_eq!(
            SubnetTAO::<Test>::get(NetUid::ROOT).to_u64(),
            root_tao_start
        );
        assert_eq!(
            SubnetAlphaOut::<Test>::get(NetUid::ROOT).to_u64(),
            root_alpha_out_start
        );
        // Root leg is fee-free and the buy leg's fee stays inside `TotalStake` accounting
        // (it is TAO moved from the pot to the author, already counted as staked).
        assert_eq!(TotalStake::<Test>::get().to_u64(), ts_before);
        assert!(
            author_balance() > author_before,
            "buy-leg fee goes to the author"
        );
        assert_eq!(entitlements(&fund), before);
        assert_nav_within_fees(nav_before, nav(&fund.hotkey));
    });
}

/// Both legs are booked as protocol flow, never user flow: the sell leg is an outflow of
/// `tao_mid` plus the author fee, the buy leg an inflow bounded by `tao_mid`.
#[test]
fn test_swap_basket_books_protocol_flow_not_user_flow() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        assert_eq!(SubnetProtocolFlow::<Test>::get(fund.netuid_a), 0);
        assert_eq!(SubnetProtocolFlow::<Test>::get(fund.netuid_b), 0);
        let user_a = SubnetTaoFlow::<Test>::get(fund.netuid_a);
        let user_b = SubnetTaoFlow::<Test>::get(fund.netuid_b);
        let author_before = author_balance();

        assert_ok!(swap(&fund, fund.netuid_a, fund.netuid_b, TRADE));

        let (_, tao_mid, _) = last_swap_event();
        let author_fee = (author_balance() - author_before) as i64;
        let flow_a = SubnetProtocolFlow::<Test>::get(fund.netuid_a);
        let flow_b = SubnetProtocolFlow::<Test>::get(fund.netuid_b);
        // Sell leg: outflow of the TAO through the middle plus the author's fee share.
        let sell_fee_outflow = -flow_a - tao_mid as i64;
        assert!(
            sell_fee_outflow > 0,
            "sell outflow must include the author fee: {flow_a}"
        );
        assert!(
            sell_fee_outflow < author_fee,
            "author is also paid on the buy leg"
        );
        // Buy leg: inflow is what entered the pool, fee excluded.
        assert!(
            flow_b > 0 && flow_b < tao_mid as i64,
            "buy inflow = {flow_b}"
        );
        assert!(tao_mid as i64 - flow_b < author_fee);

        assert_eq!(SubnetTaoFlow::<Test>::get(fund.netuid_a), user_a);
        assert_eq!(SubnetTaoFlow::<Test>::get(fund.netuid_b), user_b);
    });
}

/// After a rebalance the staker still redeems ~the same value: composition is not
/// entitlement.
#[test]
fn test_swap_basket_then_claim_pays_the_same_value() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        let nav_before = nav(&fund.hotkey);
        let owed_before = SubtensorModule::get_basket_owed_shares(&fund.hotkey, &fund.staker);

        assert_ok!(swap(
            &fund,
            fund.netuid_a,
            fund.netuid_b,
            escrow_alpha(&fund.hotkey, fund.netuid_a)
        ));
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&fund.hotkey, &fund.staker),
            owed_before
        );

        let root_before = root_stake_of(&fund.hotkey, &fund.staker);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.staker),
            fund.hotkey
        ));
        let gain = root_stake_of(&fund.hotkey, &fund.staker) - root_before;
        assert_nav_within_fees(nav_before, gain);
    });
}

/// The actual weight scales with the fund's holding count and never exceeds the declared
/// 256-row cap.
#[test]
fn test_swap_basket_post_dispatch_weight_is_bounded() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        let post = swap(&fund, fund.netuid_a, fund.netuid_b, TRADE).expect("trade succeeds");
        let actual = post.actual_weight.expect("trade reports its actual weight");
        // Two rows after the trade (A remainder + B).
        assert_eq!(actual, SubtensorModule::swap_basket_weight(2));
        assert!(actual.all_lt(SubtensorModule::swap_basket_weight(256)));
    });
}

// =============================================================================
// Gates and errors
// =============================================================================

#[test]
fn test_swap_basket_rejects_when_trading_disabled() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        BasketTradingEnabled::<Test>::put(false);
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::BasketTradingDisabled
        );
    });
}

#[test]
fn test_swap_basket_rejects_when_frozen() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        BasketTradingFrozen::<Test>::insert(fund.hotkey, ());
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::BasketTradingFrozen
        );
        BasketTradingFrozen::<Test>::remove(fund.hotkey);
        assert_ok!(swap(&fund, fund.netuid_a, fund.netuid_b, TRADE));
    });
}

#[test]
fn test_swap_basket_rejects_same_subnet() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_a, TRADE),
            Error::<Test>::BasketSameSubnet
        );
        assert_noop!(
            swap(&fund, NetUid::ROOT, NetUid::ROOT, TRADE),
            Error::<Test>::BasketSameSubnet
        );
    });
}

#[test]
fn test_swap_basket_rejects_non_owner_coldkey() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        let stranger = U256::from(777);
        assert_noop!(
            SubtensorModule::swap_basket(
                RuntimeOrigin::signed(stranger),
                fund.hotkey,
                fund.netuid_a,
                fund.netuid_b,
                TRADE.into(),
            ),
            Error::<Test>::NonAssociatedColdKey
        );
        // A hotkey with no account at all is also "not owned".
        assert_noop!(
            SubtensorModule::swap_basket(
                RuntimeOrigin::signed(fund.coldkey),
                U256::from(778),
                fund.netuid_a,
                fund.netuid_b,
                TRADE.into(),
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

#[test]
fn test_swap_basket_rejects_hotkey_not_on_root() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        Uids::<Test>::remove(NetUid::ROOT, fund.hotkey);
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::HotKeyNotRegisteredInSubNet
        );
    });
}

#[test]
fn test_swap_basket_rejects_while_seed_migration_in_progress() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        kickoff_seed_beta_basket_v2::<Test>();
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::BetaBasketSeedInProgress
        );
    });
}

/// The `CheckColdkeySwap` dispatch extension refuses the trade while the signing coldkey
/// has a swap announced (the same guard every other signed call gets).
#[test]
fn test_swap_basket_rejects_while_coldkey_swap_announced() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        let call = RuntimeCall::SubtensorModule(crate::Call::swap_basket {
            hotkey: fund.hotkey,
            origin_netuid: fund.netuid_a,
            destination_netuid: fund.netuid_b,
            amount: TRADE.into(),
        });
        let dispatch = |call: RuntimeCall| {
            <CheckColdkeySwap<Test> as ExtendedDispatchable<RuntimeCall>>::dispatch_with_extension(
                RuntimeOrigin::signed(fund.coldkey),
                call,
            )
        };

        let hash = HashingOf::<Test>::hash_of(&U256::from(42));
        ColdkeySwapAnnouncements::<Test>::insert(fund.coldkey, (System::block_number(), hash));
        let held = escrow_alpha(&fund.hotkey, fund.netuid_a);
        assert_eq!(
            dispatch(call.clone()).unwrap_err().error,
            Error::<Test>::ColdkeySwapAnnounced.into()
        );
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_a), held);

        ColdkeySwapAnnouncements::<Test>::remove(fund.coldkey);
        assert_ok!(dispatch(call));
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_a), held - TRADE);
    });
}

#[test]
fn test_swap_basket_rejects_nonexistent_subnets() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        let missing = NetUid::from(99u16);
        assert_noop!(
            swap(&fund, fund.netuid_a, missing, TRADE),
            Error::<Test>::SubnetNotExists
        );
        assert_noop!(
            swap(&fund, missing, fund.netuid_a, TRADE),
            Error::<Test>::SubnetNotExists
        );
    });
}

#[test]
fn test_swap_basket_rejects_subtoken_disabled_destination() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        SubtokenEnabled::<Test>::insert(fund.netuid_b, false);
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::SubtokenDisabled
        );
    });
}

#[test]
fn test_swap_basket_rejects_zero_and_dust_amounts() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, 0),
            Error::<Test>::AmountTooLow
        );
        // Positive, but the TAO through the middle lands below `DefaultMinStake`.
        let dust = DefaultMinStake::<Test>::get().to_u64() / 2;
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, dust),
            Error::<Test>::AmountTooLow
        );
    });
}

#[test]
fn test_swap_basket_rejects_more_than_held() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        let held = escrow_alpha(&fund.hotkey, fund.netuid_a);
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, held + 1),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
        // An empty origin (no cash yet) fails the same way.
        assert_noop!(
            swap(&fund, NetUid::ROOT, fund.netuid_b, TRADE),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
    });
}

// =============================================================================
// Guardrail: per-leg slippage band
// =============================================================================

/// Buy leg, EMA anchor: a pre-trade pump (spot above the moving price by more than 2%)
/// is refused before any leg runs.
#[test]
fn test_swap_basket_buy_refused_when_spot_above_ema_band() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        // Spot is 1.0; a moving price of 0.9 puts the ceiling at 0.918.
        SubnetMovingPrice::<Test>::insert(fund.netuid_b, I96F32::from_num(0.9));
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::SlippageTooHigh
        );
    });
}

/// Sell leg, EMA anchor: a pre-trade dump (spot below the moving price by more than 2%)
/// is refused.
#[test]
fn test_swap_basket_sell_refused_when_spot_below_ema_band() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        // Spot is 1.0; a moving price of 1.1 puts the floor at 1.078.
        SubnetMovingPrice::<Test>::insert(fund.netuid_a, I96F32::from_num(1.1));
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::SlippageTooHigh
        );
    });
}

/// A subnet with no moving price yet cannot be traded on either leg.
#[test]
fn test_swap_basket_refused_without_moving_price() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        SubnetMovingPrice::<Test>::insert(fund.netuid_b, I96F32::from_num(0));
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::SlippageTooHigh
        );
        SubnetMovingPrice::<Test>::insert(fund.netuid_b, I96F32::from_num(1));
        SubnetMovingPrice::<Test>::insert(fund.netuid_a, I96F32::from_num(0));
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::SlippageTooHigh
        );
    });
}

/// Buy leg, spot anchor: on a thin destination pool the trade's own price impact would
/// exceed 2%, so the leg cannot fill fully within the ceiling and the trade is refused.
#[test]
fn test_swap_basket_buy_refused_when_own_impact_exceeds_band() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        // 10 TAO / 10 alpha: a 4 TAO buy would move the price ~96%.
        SubnetTAO::<Test>::insert(fund.netuid_b, TaoBalance::from(10_000_000u64));
        SubnetAlphaIn::<Test>::insert(fund.netuid_b, AlphaBalance::from(10_000_000u64));
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::SlippageTooHigh
        );
    });
}

/// Sell leg, spot anchor: on a thin origin pool the sale's own impact exceeds 2%.
#[test]
fn test_swap_basket_sell_refused_when_own_impact_exceeds_band() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        SubnetTAO::<Test>::insert(fund.netuid_a, TaoBalance::from(10_000_000u64));
        SubnetAlphaIn::<Test>::insert(fund.netuid_a, AlphaBalance::from(10_000_000u64));
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::SlippageTooHigh
        );
    });
}

/// Fills that sit just inside the band on both legs pass: spot 1% away from the moving
/// price on each side, and a trade small enough to leave less than 1% of impact.
#[test]
fn test_swap_basket_fills_just_inside_band() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        // Sell floor = max(1.01, 1.0) * 0.98 = 0.9898 < spot; buy ceiling = min(0.99, 1.0)
        // * 1.02 = 1.0098 > spot.
        SubnetMovingPrice::<Test>::insert(fund.netuid_a, I96F32::from_num(1.01));
        SubnetMovingPrice::<Test>::insert(fund.netuid_b, I96F32::from_num(0.99));
        assert_ok!(swap(&fund, fund.netuid_a, fund.netuid_b, TRADE));
        let (alpha_sold, tao_mid, alpha_bought) = last_swap_event();
        assert_eq!(alpha_sold, TRADE);
        assert!(tao_mid > 0 && alpha_bought > 0);
    });
}

/// A `swap_basket` refusal after the sell leg has already executed rolls the whole trade
/// back: holdings, reserves, TotalStake, author fee, and the turnover window are untouched.
#[test]
fn test_swap_basket_failed_second_leg_leaves_no_partial_state() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        // The sell leg on A is fine; the buy leg on a thin B fails.
        SubnetTAO::<Test>::insert(fund.netuid_b, TaoBalance::from(10_000_000u64));
        SubnetAlphaIn::<Test>::insert(fund.netuid_b, AlphaBalance::from(10_000_000u64));

        let held = escrow_alpha(&fund.hotkey, fund.netuid_a);
        let tao_a = SubnetTAO::<Test>::get(fund.netuid_a);
        let alpha_in_a = SubnetAlphaIn::<Test>::get(fund.netuid_a);
        let ts = TotalStake::<Test>::get();
        let author = author_balance();
        let window = BasketTradeWindow::<Test>::get(fund.hotkey);
        let flow_a = SubnetProtocolFlow::<Test>::get(fund.netuid_a);
        let before = entitlements(&fund);

        assert!(swap(&fund, fund.netuid_a, fund.netuid_b, TRADE).is_err());

        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_a), held);
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_b), 0);
        assert_eq!(SubnetTAO::<Test>::get(fund.netuid_a), tao_a);
        assert_eq!(SubnetAlphaIn::<Test>::get(fund.netuid_a), alpha_in_a);
        assert_eq!(TotalStake::<Test>::get(), ts);
        assert_eq!(
            author_balance(),
            author,
            "rolled-back fee must not reach the author"
        );
        assert_eq!(BasketTradeWindow::<Test>::get(fund.hotkey), window);
        assert_eq!(SubnetProtocolFlow::<Test>::get(fund.netuid_a), flow_a);
        assert_eq!(entitlements(&fund), before);
        assert!(!System::events().iter().any(|e| matches!(
            e.event,
            RuntimeEvent::SubtensorModule(Event::BasketSwapped { .. })
        )));
    });
}

// =============================================================================
// Guardrail: turnover budget
// =============================================================================

/// The TAO through the middle accumulates in the window, refuses at the cap, and the
/// window rolls after `BASKET_TRADE_WINDOW_BLOCKS`.
#[test]
fn test_swap_basket_turnover_budget_accumulates_refuses_and_rolls() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        BasketDailyTurnoverCap::<Test>::put(DEFAULT_BASKET_DAILY_TURNOVER_CAP);
        let budget = SubtensorModule::basket_trade_budget_tao(nav(&fund.hotkey));
        // 10% of a ~100 TAO fund: two 4 TAO trades fit, a third does not.
        assert!(
            budget > 2 * TRADE && budget < 3 * TRADE,
            "budget = {budget}"
        );
        let start = System::block_number();
        assert!(
            start > 0,
            "the first trade must open the window at the current block"
        );
        assert_eq!(BasketTradeWindow::<Test>::get(fund.hotkey), (0, 0));
        assert_eq!(
            SubtensorModule::get_basket_trading_status(&fund.hotkey).window_start_block,
            start
        );

        assert_ok!(swap(&fund, fund.netuid_a, fund.netuid_b, TRADE));
        let (_, mid_1, _) = last_swap_event();
        assert_eq!(BasketTradeWindow::<Test>::get(fund.hotkey), (start, mid_1));

        assert_ok!(swap(&fund, fund.netuid_a, fund.netuid_b, TRADE));
        let (_, mid_2, _) = last_swap_event();
        assert_eq!(
            BasketTradeWindow::<Test>::get(fund.hotkey),
            (start, mid_1 + mid_2),
            "tao_used is the sum of tao_mid across the window"
        );

        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::BasketTurnoverBudgetExceeded
        );
        // The view agrees with what a trade would be charged against.
        let status = SubtensorModule::get_basket_trading_status(&fund.hotkey);
        assert_eq!(status.window_start_block, start);
        assert_eq!(status.tao_used.to_u64(), mid_1 + mid_2);
        assert!(status.enabled && !status.frozen);

        // One block short of the roll: still refused.
        System::set_block_number(start + BASKET_TRADE_WINDOW_BLOCKS - 1);
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, TRADE),
            Error::<Test>::BasketTurnoverBudgetExceeded
        );

        // At the roll a fresh window opens, charged only with this trade.
        let rolled = start + BASKET_TRADE_WINDOW_BLOCKS;
        System::set_block_number(rolled);
        assert_ok!(swap(&fund, fund.netuid_a, fund.netuid_b, TRADE));
        let (_, mid_3, _) = last_swap_event();
        assert_eq!(BasketTradeWindow::<Test>::get(fund.hotkey), (rolled, mid_3));
    });
}

/// Root cash is TAO 1:1, so a root-origin trade charges exactly its amount.
#[test]
fn test_swap_basket_turnover_charges_root_origin_at_face() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        assert_ok!(swap(&fund, fund.netuid_a, NetUid::ROOT, 3 * TRADE));
        let start = System::block_number();
        BasketTradeWindow::<Test>::remove(fund.hotkey);

        assert_ok!(swap(&fund, NetUid::ROOT, fund.netuid_b, TRADE));
        assert_eq!(BasketTradeWindow::<Test>::get(fund.hotkey), (start, TRADE));
    });
}

// =============================================================================
// Guardrail: concentration cap
// =============================================================================

/// With enough destinations on chain (root + A + B = 3, cap 1/2) the destination holding
/// may not end above the cap share of NAV; smaller trades pass.
#[test]
fn test_swap_basket_refuses_destination_over_concentration_cap() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        assert_eq!(SubtensorModule::get_all_subnet_netuids().len(), 3);
        RootWeightsCap::<Test>::insert(NetUid::ROOT, u16::MAX / 2);
        let held = escrow_alpha(&fund.hotkey, fund.netuid_a);

        // 60% of the fund into B: over the cap.
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, held * 6 / 10),
            Error::<Test>::RootWeightCapExceeded
        );
        // 40% is fine.
        assert_ok!(swap(&fund, fund.netuid_a, fund.netuid_b, held * 4 / 10));
        // Topping B up past the cap is refused even though this trade alone is small.
        assert_noop!(
            swap(&fund, fund.netuid_a, fund.netuid_b, held * 2 / 10),
            Error::<Test>::RootWeightCapExceeded
        );
        // The cash slot is a destination like any other.
        assert_noop!(
            swap(&fund, fund.netuid_a, NetUid::ROOT, held * 6 / 10),
            Error::<Test>::RootWeightCapExceeded
        );
    });
}

/// Selling out of a holding that is already over the cap is always allowed; only the
/// destination is checked.
#[test]
fn test_swap_basket_allows_selling_out_of_over_cap_position() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        // Build the over-cap position while the cap is not binding (default 1/16 needs 16
        // destinations), then make it binding.
        assert_ok!(swap(
            &fund,
            fund.netuid_a,
            fund.netuid_b,
            escrow_alpha(&fund.hotkey, fund.netuid_a)
        ));
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_a), 0);
        reset_turnover_window(&fund.hotkey);
        RootWeightsCap::<Test>::insert(NetUid::ROOT, u16::MAX / 2);
        let held_b = escrow_alpha(&fund.hotkey, fund.netuid_b);

        // B holds 100% (> 50%): selling 30% of it back into A is allowed ...
        assert_ok!(swap(&fund, fund.netuid_b, fund.netuid_a, held_b * 3 / 10));
        // ... but moving 60% would put A over the cap.
        assert_noop!(
            swap(&fund, fund.netuid_b, fund.netuid_a, held_b * 6 / 10),
            Error::<Test>::RootWeightCapExceeded
        );
    });
}

/// Young-chain softening: with fewer destinations than the cap demands (3 < 16 at the
/// default 1/16), the concentration rule is skipped and a whole holding can move.
#[test]
fn test_swap_basket_concentration_cap_skipped_on_young_chain() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        assert!(
            SubtensorModule::binding_root_weights_cap(
                SubtensorModule::get_all_subnet_netuids().len() as u64
            )
            .is_none()
        );
        let held = escrow_alpha(&fund.hotkey, fund.netuid_a);
        assert_ok!(swap(&fund, fund.netuid_a, fund.netuid_b, held));
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_a), 0);
    });
}

// =============================================================================
// Guardrails follow the fund on hotkey swap
// =============================================================================

#[test]
fn test_swap_basket_freeze_and_window_follow_hotkey_swap() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund();
        let new_hotkey = U256::from(10030);
        // The swap target must be a hotkey the same coldkey owns (a subnet-scoped swap does
        // not create ownership).
        let _ = SubtensorModule::create_account_if_non_existent(&fund.coldkey, &new_hotkey);

        assert_ok!(swap(&fund, fund.netuid_a, fund.netuid_b, TRADE));
        let window = BasketTradeWindow::<Test>::get(fund.hotkey);
        assert!(window.1 > 0);
        BasketTradingFrozen::<Test>::insert(fund.hotkey, ());

        let mut weight = Weight::zero();
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_one_subnet(
            &fund.hotkey,
            &new_hotkey,
            &mut weight,
            NetUid::ROOT,
            false,
        ));

        // The freeze is copied (the old key stays frozen too), the window moves.
        assert!(BasketTradingFrozen::<Test>::contains_key(new_hotkey));
        assert!(BasketTradingFrozen::<Test>::contains_key(fund.hotkey));
        assert_eq!(BasketTradeWindow::<Test>::get(new_hotkey), window);
        assert_eq!(BasketTradeWindow::<Test>::get(fund.hotkey), (0, 0));

        // `register_on_root` only writes `Uids` (no `Keys` row), so the subnet-scoped swap
        // above cannot carry the root seat over; give the new hotkey its seat directly.
        register_on_root(&new_hotkey, 0);

        // The new hotkey is frozen: no trades until governance lifts it.
        let new_fund = Fund {
            hotkey: new_hotkey,
            ..fund
        };
        assert_noop!(
            swap(&new_fund, new_fund.netuid_b, new_fund.netuid_a, TRADE),
            Error::<Test>::BasketTradingFrozen
        );
        BasketTradingFrozen::<Test>::remove(new_hotkey);
        // And the moved window is what the next trade is charged against.
        BasketDailyTurnoverCap::<Test>::put(DEFAULT_BASKET_DAILY_TURNOVER_CAP);
        let held_b = escrow_alpha(&new_hotkey, new_fund.netuid_b);
        assert_ok!(swap(
            &new_fund,
            new_fund.netuid_b,
            new_fund.netuid_a,
            held_b
        ));
        let (_, mid, _) = last_swap_event();
        assert_eq!(
            BasketTradeWindow::<Test>::get(new_hotkey),
            (window.0, window.1 + mid)
        );
    });
}

// =============================================================================
// Event index stability
// =============================================================================

/// `BasketSwapped` must be appended after the last pre-existing event so historical
/// event indices stay stable for decoders.
#[test]
fn regression_basket_swapped_event_index_is_appended() {
    let prior_tail = Event::<Test>::BasketAlphaWrittenOff {
        hotkey: U256::from(1),
        netuid: NetUid::from(1),
        alpha: AlphaBalance::from(1),
    }
    .encode();
    let swapped = Event::<Test>::BasketSwapped {
        hotkey: U256::from(1),
        origin_netuid: NetUid::from(1),
        destination_netuid: NetUid::from(2),
        alpha_sold: AlphaBalance::from(1),
        tao_mid: TaoBalance::from(1),
        alpha_bought: AlphaBalance::from(1),
    }
    .encode();
    assert_eq!(prior_tail[0], 148);
    assert_eq!(swapped[0], 149);
}
