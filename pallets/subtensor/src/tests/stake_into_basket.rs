//! Beta basket: direct deposits (`stake_into_basket`), ΔNAV minting, and root-slot yield
//! attribution.
#![allow(clippy::indexing_slicing, clippy::unwrap_used)]

use crate::staking::BasketFlushWork;
use crate::tests::claim_root::{
    escrow_alpha, flush_baskets, fund_pool, fund_shares, has_fund, register_on_root, root_stake_of,
    zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::{
    BasketClaimed, BasketShares, DefaultMinStake, Error, PendingBasketDeposits, StakingHotkeys,
    SubnetAlphaIn, SubnetAlphaOut, SubnetTAO, TotalStake,
};
use approx::assert_abs_diff_eq;
use frame_support::dispatch::GetDispatchInfo;
use frame_support::traits::Get;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

/// Economic bound: a value round trip (or entry) may only cost swap fees, so recovered
/// values must land within this percentage of the input.
const FEE_TOLERANCE_PCT: u64 = 5;

/// Economic bound: a marked payout may drift by at most `payout / PAYOUT_EPS_DENOM` (2%)
/// under unrelated operations, from fee/slippage residue.
const PAYOUT_EPS_DENOM: u64 = 50;

/// Tight economic bound (1%): for values that should match up to residual slippage.
const SLIPPAGE_EPS_DENOM: u64 = 100;

/// Arithmetic slack for integer floor rounding, in rao / shares. Distinct from the economic
/// bounds above: loosening this to paper over a fee regression is a bug.
const ROUNDING_EPS: u64 = 3;

/// Standard playground for direct-deposit tests: a validator with a root uid and a deep,
/// balanced pool on its subnet; tao_weight maxed and the claim threshold zeroed.
fn setup_stake_in_env() -> (U256, U256, NetUid) {
    let owner_coldkey = U256::from(1001);
    let hotkey = U256::from(1002);
    let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
    remove_owner_registration_stake(netuid);
    fund_pool(netuid);
    SubtensorModule::set_tao_weight(u64::MAX);
    zero_claim_threshold();
    register_on_root(&hotkey, 0);
    (owner_coldkey, hotkey, netuid)
}

/// Alpha of the opening holding `open_fund_with_alpha` seeds: dust next to every deposit in
/// this file (well inside the swap fees any round trip pays), but enough to make the fund
/// mirror a subnet instead of holding deposits as cash.
const OPENING_ALPHA: u64 = 100;

/// Open `hotkey`'s fund with a dust holding on `netuid` and no shares outstanding, so direct
/// deposits buy that subnet's alpha through the AMM instead of being held as the root cash
/// slot an empty fund starts with. With no shares the first deposit still mints at par and
/// owns the whole fund (the dust is a gift far below any bound asserted here), so
/// `Σ owed == BasketShares` keeps holding exactly. Root-registers the hotkey.
fn open_fund_with_alpha(hotkey: &U256, netuid: NetUid) {
    register_on_root(hotkey, 0);
    let escrow = SubtensorModule::get_beta_escrow_account_id();
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey,
        &escrow,
        netuid,
        OPENING_ALPHA.into(),
    );
}

/// `Σ owed == BasketShares` for a known set of stakers: every outstanding share is claimable
/// by exactly one coldkey (no stranded or double-counted entitlement).
fn assert_shares_fully_owed(hotkey: &U256, coldkeys: &[U256], epsilon: u64) {
    let total_owed: u64 = coldkeys
        .iter()
        .map(|ck| SubtensorModule::get_basket_owed_shares(hotkey, ck))
        .sum();
    assert_abs_diff_eq!(total_owed, fund_shares(hotkey), epsilon = epsilon);
}

/// A direct deposit followed by a claim is symmetric: the staker recovers ~their TAO
/// (minus real swap fees), the fund drains, and the watermark returns to exactly zero.
/// Nobody needs root stake for any of it. The fund is opened with a subnet holding first so
/// the round trip really crosses the AMM (an empty fund would hold the deposit as cash).
#[test]
fn test_stake_into_basket_round_trip_symmetric() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, netuid) = setup_stake_in_env();
        open_fund_with_alpha(&hotkey, netuid);

        let bob = U256::from(2001);
        let amount = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * amount));

        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            amount.into(),
        ));

        // Shares were credited through the signed watermark: owed == minted, watermark is
        // exactly -minted, and the claim path can find the position.
        let minted = fund_shares(&hotkey);
        assert!(minted > 0);
        assert_eq!(
            BasketClaimed::<Test>::get(hotkey, bob),
            -(i128::from(minted))
        );
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &bob),
            minted
        );
        assert!(StakingHotkeys::<Test>::get(bob).contains(&hotkey));

        // Claim it all back. The proceeds are staked on root for bob.
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));
        let recovered = root_stake_of(&hotkey, &bob);
        assert!(
            recovered <= amount,
            "round trip must not create value: recovered {recovered} of {amount}"
        );
        assert!(
            recovered >= amount * (100 - FEE_TOLERANCE_PCT) / 100,
            "round trip should only cost swap fees: recovered {recovered} of {amount}"
        );

        // Fund fully drained, watermark settled to exactly zero, nothing owed.
        assert!(fund_shares(&hotkey) <= 10, "fund should be drained");
        assert_eq!(BasketClaimed::<Test>::get(hotkey, bob), 0);
        assert_eq!(SubtensorModule::get_basket_owed_shares(&hotkey, &bob), 0);
    });
}

/// A direct deposit first flushes the validator's queued dividend credits; that work is
/// charged into the post-dispatch weight through the same `basket_flush_weight` model as
/// `swap_basket` and `claim_root`, the declared weight carries the flat flush allowance,
/// and the actual weight refunds below it.
#[test]
fn test_stake_into_basket_declared_weight_covers_flush_and_refunds() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, netuid) = setup_stake_in_env();
        register_on_root(&hotkey, 0);
        let alice = U256::from(2002);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            10_000_000_000u64.into(),
        );

        let bob = U256::from(2001);
        let amount = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(4 * amount));

        let declared = RuntimeCall::SubtensorModule(crate::Call::stake_into_basket {
            hotkey,
            amount_staked: amount.into(),
        })
        .get_dispatch_info()
        .call_weight;
        assert!(declared.all_gte(SubtensorModule::stake_into_basket_declared_weight()));

        // Empty queue: the bare deposit weight (one slot — the root cash slot an empty fund
        // opens with), no flush term.
        let Ok(bare) = SubtensorModule::do_stake_into_basket(bob, hotkey, amount.into()) else {
            panic!("bare deposit succeeds");
        };
        assert_eq!(bare, SubtensorModule::stake_into_basket_weight(1, 1));

        // Queue a dividend credit: the next deposit flushes it first. Quotes: scan 1 + the
        // NAV sweep over the one (cash) holding + two quotes for the credit. Rows: one
        // in-place credit.
        let credit = 1_000_000u64;
        SubnetAlphaOut::<Test>::mutate(netuid, |t| *t = t.saturating_add(credit.into()));
        SubtensorModule::enqueue_basket_deposit(&hotkey, netuid, credit.into());
        let holdings = SubtensorModule::get_basket_holdings(&hotkey).len() as u64;
        assert_eq!(holdings, 1);
        let expected_flush_work = BasketFlushWork::new(1 + holdings + 2, 1);

        let Ok(charged) = SubtensorModule::do_stake_into_basket(bob, hotkey, amount.into()) else {
            panic!("deposit after flush succeeds");
        };
        assert!(
            !PendingBasketDeposits::<Test>::contains_key(hotkey, netuid),
            "the deposit flushed the queued credit"
        );
        // The flushed credit opened a second holding, so the deposit mirrors two slots over
        // two holdings (each slot may open a row: 2 + 2 rows swept), plus the flush.
        assert_eq!(
            SubtensorModule::get_basket_holdings(&hotkey).len(),
            2,
            "the flushed credit landed as a holding on its origin"
        );
        assert_eq!(
            charged,
            SubtensorModule::stake_into_basket_weight(2, 4)
                .saturating_add(SubtensorModule::basket_flush_weight(expected_flush_work))
        );
        assert!(
            charged.all_lt(declared),
            "actual {charged:?} must refund below declared {declared:?}"
        );
    });
}

/// Par mint invariant on a fund with no shares outstanding: the deposit mints one share per
/// TAO of realizable value added, so `BasketShares == realizable NAV` to the rao (up to the
/// opening dust the depositor inherits). This is the ΔNAV property in its purest form — the
/// mint is priced at what the fund can actually redeem (the deposit buys alpha and bears its
/// own fees), not at the TAO deployed.
#[test]
fn test_stake_into_basket_par_mint_equals_nav() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, netuid) = setup_stake_in_env();
        open_fund_with_alpha(&hotkey, netuid);

        let bob = U256::from(2001);
        let amount = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * amount));

        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            amount.into(),
        ));

        let shares = fund_shares(&hotkey);
        let nav = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        assert_abs_diff_eq!(shares, nav, epsilon = ROUNDING_EPS + OPENING_ALPHA);
        assert!(
            shares <= amount,
            "mint value can never exceed the TAO brought in"
        );
        assert!(
            shares >= amount * (100 - FEE_TOLERANCE_PCT) / 100,
            "entry cost should be fees-only"
        );
    });
}

/// In a deep, balanced pool, a direct deposit leaves existing dividend-accrued holders' payout
/// and the fund's share price (N/P) unchanged. They remain unchanged after the depositor claims
/// back out, and `Σ owed == BasketShares` holds throughout.
#[test]
fn test_stake_into_basket_does_not_dilute_existing_holders() {
    new_test_ext(1).execute_with(|| {
        let (owner_coldkey, hotkey, netuid) = setup_stake_in_env();
        let alice = U256::from(2001);
        let bob = U256::from(2002);

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        // Alice accrues via a dividend.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        let alice_payout_before = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        assert!(alice_payout_before > 0);

        let share_price = |hk: &U256| -> f64 {
            let n = SubtensorModule::get_validator_basket_nav_tao(hk).to_u64() as f64;
            let p = fund_shares(hk) as f64;
            n / p
        };
        let price_before = share_price(&hotkey);

        // Bob (no root stake at all) buys in directly.
        let amount = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * amount));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            amount.into(),
        ));

        // Alice's marked payout and the share price are untouched by bob's entry.
        let alice_payout_mid = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        assert_abs_diff_eq!(
            alice_payout_mid,
            alice_payout_before,
            epsilon = alice_payout_before / PAYOUT_EPS_DENOM
        );
        assert_abs_diff_eq!(share_price(&hotkey), price_before, epsilon = 0.02);
        assert_shares_fully_owed(&hotkey, &[alice, bob], ROUNDING_EPS);

        // Bob exits. Alice is still whole.
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));
        let alice_payout_after = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        assert_abs_diff_eq!(
            alice_payout_after,
            alice_payout_before,
            epsilon = alice_payout_before / PAYOUT_EPS_DENOM
        );
        assert_shares_fully_owed(&hotkey, &[alice, bob], ROUNDING_EPS);
    });
}

/// Regression: a direct depositor cannot capture the concavity premium from selling a
/// proportional alpha slice. Shares are minted against full-liquidation NAV, so the claim
/// pays that same NAV-priced fraction and retains any larger raw sale proceeds as fund cash.
/// The remaining holder therefore keeps their pre-claim marked value.
#[test]
fn test_stake_into_basket_claim_retains_concavity_surplus_for_existing_holders() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, netuid) = setup_stake_in_env();
        open_fund_with_alpha(&hotkey, netuid);

        // Make the pool deliberately thin so the old raw-alpha-fraction redemption
        // overpayment is large and this test is sensitive to the exploit.
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(100_000_000u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(100_000_000u64));

        let alice = U256::from(2001);
        let bob = U256::from(2002);
        let alice_deposit = 50_000_000u64;
        let bob_deposit = 10_000_000u64;
        add_balance_to_coldkey_account(&alice, TaoBalance::from(2 * alice_deposit));
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * bob_deposit));

        assert_ok!(SubtensorModule::do_stake_into_basket(
            alice,
            hotkey,
            alice_deposit.into(),
        ));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            bob_deposit.into(),
        ));

        let alice_payout_before = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        let bob_payout_before = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);
        let bob_shares = SubtensorModule::get_basket_owed_shares(&hotkey, &bob);
        let shares_total = fund_shares(&hotkey);
        let holding = escrow_alpha(&hotkey, netuid);
        let raw_take = SubtensorModule::mul_div_u64(holding, bob_shares, shares_total);
        let uncapped_raw_sale = SubtensorModule::realizable_tao_for_alpha(netuid, raw_take);

        assert!(
            uncapped_raw_sale > bob_payout_before,
            "test setup must expose the concavity premium: raw={uncapped_raw_sale}, nav-priced={bob_payout_before}"
        );
        assert_eq!(
            SubtensorModule::get_basket_subnet_payout_tao(&hotkey, &bob, netuid),
            bob_payout_before,
            "subnet payout view must quote the same NAV-priced entitlement as claim"
        );

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));

        let bob_received = root_stake_of(&hotkey, &bob);
        assert!(
            bob_received <= bob_payout_before,
            "claim paid more than the depositor's NAV-priced entitlement: {bob_received} > {bob_payout_before}"
        );

        let alice_payout_after = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        assert!(
            alice_payout_after.saturating_add(ROUNDING_EPS) >= alice_payout_before,
            "earlier holder lost value across the deposit/claim cycle: before={alice_payout_before}, after={alice_payout_after}",
        );
    });
}

/// Regression for interleaved accounting after a concavity-capped claim: the retained sale
/// surplus becomes root cash, the first claimant's newly received root stake earns only a
/// subsequent dividend, and both the old and newly accrued shares can then drain the fund
/// without stranding cash or changing TotalStake.
#[test]
fn test_retained_concavity_cash_balances_with_new_root_entitlement() {
    new_test_ext(1).execute_with(|| {
        let (owner, hotkey, netuid) = setup_stake_in_env();
        open_fund_with_alpha(&hotkey, netuid);

        // A thin pool makes Bob's partial alpha sale realize a measurable concavity surplus.
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(100_000_000u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(100_000_000u64));
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner,
            netuid,
            10_000_000u64.into(),
        );

        let alice = U256::from(2001);
        let bob = U256::from(2002);
        let alice_deposit = 50_000_000u64;
        let bob_deposit = 10_000_000u64;
        add_balance_to_coldkey_account(&alice, TaoBalance::from(2 * alice_deposit));
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * bob_deposit));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            alice,
            hotkey,
            alice_deposit.into(),
        ));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            bob_deposit.into(),
        ));

        // From this point onward claims and dividend deployment only move existing stake.
        let total_stake_before_claims = TotalStake::<Test>::get();
        let bob_first_quote = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);
        let bob_root_before = root_stake_of(&hotkey, &bob);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));
        let bob_first_gain = root_stake_of(&hotkey, &bob).saturating_sub(bob_root_before);
        assert!(bob_first_gain <= bob_first_quote);
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &bob),
            0,
            "claim payout must not retroactively recreate Bob's consumed entitlement"
        );

        let retained_root = escrow_alpha(&hotkey, NetUid::ROOT);
        assert!(
            retained_root > 0,
            "partial claim must retain a measurable concavity surplus as root cash"
        );
        let alice_before_dividend = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);

        // Bob's claimed root stake now legitimately earns a new dividend. The retained escrow
        // root slot earns its own fraction unminted, so Alice's pre-existing shares are not
        // diluted when Bob receives the newly minted entitlement.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            5_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        assert!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &bob) > 0,
            "Bob's root stake must earn shares from the later dividend"
        );
        let alice_after_dividend = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        assert!(
            alice_after_dividend.saturating_add(ROUNDING_EPS) >= alice_before_dividend,
            "newly minted root entitlement diluted the retained cash belonging to Alice"
        );
        assert_shares_fully_owed(&hotkey, &[alice, bob], ROUNDING_EPS);

        let nav = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        let owed_value = alice_after_dividend
            .saturating_add(SubtensorModule::get_basket_payout_tao(&hotkey, &bob));
        assert_abs_diff_eq!(owed_value, nav, epsilon = ROUNDING_EPS);

        // Alice redeems the old position, then Bob redeems his later-earned shares as the final
        // holder. The latter must receive all remaining alpha and retained root cash.
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            hotkey
        ));
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));

        assert!(
            escrow_alpha(&hotkey, netuid) <= 10,
            "subnet holding remained after the final claim"
        );
        assert!(
            escrow_alpha(&hotkey, NetUid::ROOT) <= 10,
            "retained root cash remained after the final claim"
        );
        assert!(
            fund_shares(&hotkey) <= 10,
            "shares remained after final claim"
        );
        assert_eq!(
            TotalStake::<Test>::get(),
            total_stake_before_claims,
            "claim/dividend interleaving changed TotalStake"
        );
    });
}

/// Input validation: nonexistent hotkey, a hotkey that exists but is not on root,
/// dust amounts, and insufficient balance are rejected before any state changes.
#[test]
fn test_stake_into_basket_rejections() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, netuid) = setup_stake_in_env();

        let bob = U256::from(2001);
        add_balance_to_coldkey_account(&bob, TaoBalance::from(50_000_000u64));

        // Hotkey with no account.
        assert_noop!(
            SubtensorModule::do_stake_into_basket(bob, U256::from(777), 10_000_000u64.into(),),
            Error::<Test>::HotKeyAccountNotExists
        );

        // Hotkey exists (another subnet owner) but is not registered on root.
        let other_owner = U256::from(3001);
        let other_hotkey = U256::from(3002);
        add_dynamic_network(&other_hotkey, &other_owner);
        assert_noop!(
            SubtensorModule::do_stake_into_basket(bob, other_hotkey, 10_000_000u64.into(),),
            Error::<Test>::HotKeyNotRegisteredInSubNet
        );

        // Below the minimum stake.
        let dust = DefaultMinStake::<Test>::get().to_u64().saturating_sub(1);
        assert_noop!(
            SubtensorModule::do_stake_into_basket(bob, hotkey, dust.into(),),
            Error::<Test>::AmountTooLow
        );

        // No balance.
        let pauper = U256::from(2002);
        assert_noop!(
            SubtensorModule::do_stake_into_basket(pauper, hotkey, 10_000_000u64.into(),),
            Error::<Test>::NotEnoughBalanceToStake
        );

        // Explicit weights that filter to nothing (nonexistent subnet): the fund is treated
        // as uncurated instead of erroring. With no holdings yet there is nothing to
        // mirror, so the deposit is held as the fund's root (TAO cash) slot at NAV.
        register_on_root(&hotkey, 0);
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        let deposit = 10_000_000u64;
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            deposit.into(),
        ));
        let root_slot = |hotkey: &U256| {
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                &escrow,
                NetUid::ROOT,
            )
            .to_u64()
        };
        assert_eq!(
            root_slot(&hotkey),
            deposit,
            "deposit into an empty uncurated fund must land in the root (TAO cash) slot 1:1"
        );

        // Once the uncurated fund holds something, a deposit mirrors it: with the root slot
        // and an equally-valued alpha holding (price ~1, deep pool), a new deposit must
        // split ~50/50 between them instead of piling into cash.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid,
            deposit.into(),
        );
        let alpha_before = escrow_alpha(&hotkey, netuid);
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            deposit.into(),
        ));
        let root_gain = root_slot(&hotkey).saturating_sub(deposit);
        let alpha_gain = escrow_alpha(&hotkey, netuid).saturating_sub(alpha_before);
        assert_abs_diff_eq!(
            root_gain,
            deposit / 2,
            epsilon = deposit / 2 * FEE_TOLERANCE_PCT / 100
        );
        assert_abs_diff_eq!(
            alpha_gain,
            deposit / 2,
            epsilon = deposit / 2 * FEE_TOLERANCE_PCT / 100
        );
    });
}

/// Regression: depressing a stale holding, depositing at the lower NAV, claiming, and restoring
/// the attacker's alpha inventory must not create TAO profit or transfer value from the basket.
#[test]
fn test_stake_into_basket_sell_deposit_claim_buyback_cannot_profit() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, netuid) = setup_stake_in_env();
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        let existing_holder = U256::from(2001);
        let attacker = U256::from(2002);
        let unit = 1_000_000_000u64;

        // Reviewer reproduction: a 1,000 TAO / 1,000 alpha pool and a basket holding
        // 1,000 alpha plus 100 TAO cash, with 600 shares outstanding at its 600 TAO NAV.
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000 * unit));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000 * unit));
        register_on_root(&hotkey, 0);
        add_balance_to_coldkey_account(&existing_holder, TaoBalance::from(200 * unit));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            existing_holder,
            hotkey,
            TaoBalance::from(100 * unit),
        ));
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid,
            AlphaBalance::from(1_000 * unit),
        );
        BasketShares::<Test>::insert(hotkey, 600 * unit);
        let basket_nav_before =
            SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        assert_abs_diff_eq!(basket_nav_before, 600 * unit, epsilon = ROUNDING_EPS);

        // The attacker depresses the alpha price, then deposits into the basket at that mark.
        let alpha_sold = 500 * unit;
        let sale = SubtensorModule::swap_alpha_for_tao(
            netuid,
            AlphaBalance::from(alpha_sold),
            TaoBalance::ZERO,
            false,
        )
        .unwrap();
        let sale_proceeds = sale.amount_paid_out.to_u64();
        let alpha_before_deposit = escrow_alpha(&hotkey, netuid);
        let deposit = 100 * unit;
        add_balance_to_coldkey_account(&attacker, TaoBalance::from(2 * deposit));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            attacker,
            hotkey,
            deposit.into(),
        ));
        let alpha_after_deposit = escrow_alpha(&hotkey, netuid);
        let alpha_bought_by_deposit = alpha_after_deposit.saturating_sub(alpha_before_deposit);

        // Claim immediately. Quantity-covered minting must prevent the claim from selling more
        // alpha than the deposit acquired, even though the NAV-priced share count is larger.
        let root_before_claim = root_stake_of(&hotkey, &attacker);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(attacker),
            hotkey,
        ));
        let claim_payout = root_stake_of(&hotkey, &attacker).saturating_sub(root_before_claim);
        let alpha_redeemed = alpha_after_deposit.saturating_sub(escrow_alpha(&hotkey, netuid));
        assert!(
            alpha_redeemed <= alpha_bought_by_deposit,
            "claim sold {alpha_redeemed} alpha after the deposit bought only {alpha_bought_by_deposit}"
        );

        // Find and execute the cheapest gross-TAO buy that restores the 500-alpha inventory.
        let mut low = 1u64;
        let mut high = 500 * unit;
        while low < high {
            let mid = low.saturating_add(high).saturating_div(2);
            let bought = crate::tests::mock::swap_tao_to_alpha(netuid, mid.into()).0;
            if bought.to_u64() >= alpha_sold {
                high = mid;
            } else {
                low = mid.saturating_add(1);
            }
        }
        let buyback_cost = low;
        let bought_back = SubtensorModule::swap_tao_for_alpha(
            netuid,
            buyback_cost.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap()
        .amount_paid_out
        .to_u64();
        assert!(
            bought_back >= alpha_sold,
            "buyback did not restore the attacker's alpha inventory"
        );

        let attacker_inflow = sale_proceeds.saturating_add(claim_payout);
        let attacker_outflow = deposit.saturating_add(buyback_cost);
        assert!(
            attacker_inflow <= attacker_outflow,
            "full cycle created TAO profit: inflow={attacker_inflow}, outflow={attacker_outflow}"
        );
        let basket_nav_after =
            SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        assert!(
            basket_nav_after.saturating_add(ROUNDING_EPS) >= basket_nav_before,
            "full cycle transferred basket NAV: before={basket_nav_before}, after={basket_nav_after}"
        );
    });
}

/// Two more deep pools next to the playground's, so a fund can hold several subnets.
fn add_deep_pools(seed: u64) -> (NetUid, NetUid) {
    let owner_b = U256::from(seed);
    let hotkey_b = U256::from(seed.saturating_add(1));
    let owner_c = U256::from(seed.saturating_add(2));
    let hotkey_c = U256::from(seed.saturating_add(3));
    let netuid_b = add_dynamic_network(&hotkey_b, &owner_b);
    let netuid_c = add_dynamic_network(&hotkey_c, &owner_c);
    remove_owner_registration_stake(netuid_b);
    remove_owner_registration_stake(netuid_c);
    fund_pool(netuid_b);
    fund_pool(netuid_c);
    (netuid_b, netuid_c)
}

/// Give `hotkey`'s fund a 3:1 holding on `(major, minor)` (equal-depth pools, price ~1) with
/// shares outstanding at par, so a deposit has a live portfolio to mirror.
fn hold_three_to_one(hotkey: &U256, major: NetUid, minor: NetUid) -> (u64, u64) {
    let escrow = SubtensorModule::get_beta_escrow_account_id();
    let major_alpha = 3_000_000u64;
    let minor_alpha = 1_000_000u64;
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey,
        &escrow,
        major,
        major_alpha.into(),
    );
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey,
        &escrow,
        minor,
        minor_alpha.into(),
    );
    BasketShares::<Test>::insert(hotkey, major_alpha.saturating_add(minor_alpha));
    (major_alpha, minor_alpha)
}

/// Deposit `amount` TAO from a fresh coldkey and return each subnet's alpha growth.
fn deposit_and_measure(hotkey: &U256, coldkey: U256, amount: u64, netuids: &[NetUid]) -> Vec<u64> {
    let before: Vec<u64> = netuids.iter().map(|n| escrow_alpha(hotkey, *n)).collect();
    add_balance_to_coldkey_account(&coldkey, TaoBalance::from(amount.saturating_mul(2)));
    assert_ok!(SubtensorModule::do_stake_into_basket(
        coldkey,
        *hotkey,
        amount.into(),
    ));
    netuids
        .iter()
        .zip(before)
        .map(|(n, b)| escrow_alpha(hotkey, *n).saturating_sub(b))
        .collect()
}

/// A direct deposit mirrors the fund's *holdings* (3:1 across the two held subnets) and buys
/// nothing else: composition is untouched by inflows, and only `swap_basket` changes it.
#[test]
fn test_stake_into_basket_mirrors_holdings_and_preserves_composition() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, other) = setup_stake_in_env();
        let (major, minor) = add_deep_pools(3001);
        let (major_alpha, minor_alpha) = hold_three_to_one(&hotkey, major, minor);
        assert_eq!(escrow_alpha(&hotkey, other), 0);

        let amount = 4_000_000u64;
        let grew = deposit_and_measure(
            &hotkey,
            U256::from(2001),
            amount,
            &[major, minor, other, NetUid::ROOT],
        );

        assert_eq!(grew[2], 0, "a non-held subnet must receive nothing");
        assert_eq!(
            grew[3], 0,
            "a fund with holdings is mirrored, never seeded as cash"
        );
        assert!(grew[0] > 0 && grew[1] > 0, "every holding must be bought");
        // Deep pools at price ~1: alpha bought tracks TAO spent, so the 3:1 value split
        // shows up as a 3:1 alpha split (fees/slippage are symmetric on equal pools).
        let ratio_bps = grew[0] * 10_000 / grew[1];
        let target_bps = major_alpha * 10_000 / minor_alpha;
        assert!(
            ratio_bps.abs_diff(target_bps) <= target_bps / SLIPPAGE_EPS_DENOM,
            "deposit must mirror holdings 3:1, got {ratio_bps} bps vs {target_bps}"
        );
        assert!(
            grew[0] + grew[1] >= amount * (100 - FEE_TOLERANCE_PCT) / 100,
            "the whole deposit must be deployed"
        );
        // Composition (by alpha, equal pools) is unchanged by the inflow.
        let after_bps = escrow_alpha(&hotkey, major) * 10_000 / escrow_alpha(&hotkey, minor);
        assert!(
            after_bps.abs_diff(target_bps) <= target_bps / SLIPPAGE_EPS_DENOM,
            "inflow must not move composition: {after_bps} bps vs {target_bps}"
        );
    });
}

/// A fund with no holdings has nothing to mirror, so its first deposit is held as the root
/// (TAO cash) slot at par — no pool is touched. The fund is then mirrored, so a second
/// deposit keeps buying the cash slot until the validator trades or dividends land.
#[test]
fn test_stake_into_empty_basket_opens_as_root_cash() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, netuid) = setup_stake_in_env();
        assert!(SubtensorModule::get_basket_holdings(&hotkey).is_empty());
        let pool_before = (
            SubnetTAO::<Test>::get(netuid),
            SubnetAlphaIn::<Test>::get(netuid),
        );

        let amount = 4_000_000u64;
        let grew = deposit_and_measure(&hotkey, U256::from(2001), amount, &[NetUid::ROOT, netuid]);
        assert_eq!(grew[0], amount, "first deposit is held as cash");
        assert_eq!(grew[1], 0, "no subnet is bought");
        assert_eq!(fund_shares(&hotkey), amount, "cash mints at par");
        assert_eq!(
            (
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid)
            ),
            pool_before,
            "opening a fund moves no pool"
        );

        let grew = deposit_and_measure(&hotkey, U256::from(2002), amount, &[NetUid::ROOT]);
        assert_eq!(
            grew[0], amount,
            "a cash-only fund keeps mirroring its cash slot"
        );
        assert_eq!(fund_shares(&hotkey), 2 * amount);
    });
}

/// Spec 468: a fully drained holding (alpha with zero realizable value) is left out of the
/// mirror instead of blocking every deposit into the fund with `AmountTooLow`. It
/// contributes nothing to the NAV the shares are priced at, so the new shares are owed
/// nothing from it. A barely priceable holding whose deposit slice rounds to zero still
/// rejects the deposit: it has value the depositor would otherwise not buy.
#[test]
fn test_stake_into_basket_skips_unpriceable_holding_and_rejects_zero_slices() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, stale_netuid) = setup_stake_in_env();
        let current_owner = U256::from(3001);
        let current_hotkey = U256::from(3002);
        let current_netuid = add_dynamic_network(&current_hotkey, &current_owner);
        remove_owner_registration_stake(current_netuid);
        fund_pool(current_netuid);

        open_fund_with_alpha(&hotkey, stale_netuid);
        let alice = U256::from(2001);
        let seed = 200_000_000u64;
        add_balance_to_coldkey_account(&alice, TaoBalance::from(2 * seed));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            alice,
            hotkey,
            seed.into(),
        ));
        assert!(escrow_alpha(&hotkey, stale_netuid) > OPENING_ALPHA);

        // The fund has since gained a healthy holding elsewhere (say, dividends earned on
        // another subnet), while the old pool has no TAO left with which to price or redeem
        // the basket's retained alpha.
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            current_netuid,
            200_000_000_000u64.into(),
        );
        let current_before = escrow_alpha(&hotkey, current_netuid);
        SubnetTAO::<Test>::insert(stale_netuid, TaoBalance::ZERO);
        assert_eq!(
            SubtensorModule::realizable_tao_for_alpha(
                stale_netuid,
                escrow_alpha(&hotkey, stale_netuid),
            ),
            0
        );

        let bob = U256::from(2002);
        let deposit = 100_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(4 * deposit));
        let stale_before = escrow_alpha(&hotkey, stale_netuid);
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            deposit.into()
        ));
        assert!(
            escrow_alpha(&hotkey, current_netuid) > current_before,
            "the deposit mirrors the priceable holding"
        );
        assert_eq!(
            escrow_alpha(&hotkey, stale_netuid),
            stale_before,
            "the worthless row is left out of the mirror"
        );
        assert!(SubtensorModule::get_basket_owed_shares(&hotkey, &bob) > 0);
        let current_before = escrow_alpha(&hotkey, current_netuid);

        // A barely priceable holding is also unsafe when its proportional slice rounds to
        // zero. It must remain part of the deposit or the mint must be rejected.
        let current_value = SubtensorModule::realizable_tao_for_alpha(
            current_netuid,
            escrow_alpha(&hotkey, current_netuid),
        );
        let stale_value = [100_000u64, 1_000_000, 10_000_000, 100_000_000]
            .into_iter()
            .find_map(|tao_reserve| {
                SubnetTAO::<Test>::insert(stale_netuid, TaoBalance::from(tao_reserve));
                let value = SubtensorModule::realizable_tao_for_alpha(
                    stale_netuid,
                    escrow_alpha(&hotkey, stale_netuid),
                );
                (value > 0
                    && SubtensorModule::mul_div_u64(
                        deposit,
                        value,
                        value.saturating_add(current_value),
                    ) == 0)
                    .then_some(value)
            })
            .unwrap_or_default();
        assert!(
            stale_value > 0,
            "test setup must produce a positive quote whose deposit slice is zero"
        );
        assert_noop!(
            SubtensorModule::do_stake_into_basket(bob, hotkey, deposit.into()),
            Error::<Test>::AmountTooLow
        );
        assert_eq!(escrow_alpha(&hotkey, current_netuid), current_before);
    });
}

/// The watermark credit survives root-stake churn: adding or removing root stake rebases the
/// watermark by `rate * delta`, which is additive and cannot touch the `-minted` credit. The
/// direct shares stay exactly owed through the churn, and dividend accrual on the new root
/// stake stacks on top.
#[test]
fn test_stake_into_basket_credit_survives_stake_changes() {
    new_test_ext(1).execute_with(|| {
        let (owner_coldkey, hotkey, netuid) = setup_stake_in_env();
        let alice = U256::from(2001);
        let bob = U256::from(2002);

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        // A dividend so the fund has a non-zero rate (the rebase path is live).
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        assert!(has_fund(&hotkey));

        // Bob buys in directly with zero root stake.
        let amount = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * amount));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            amount.into(),
        ));
        let minted = SubtensorModule::get_basket_owed_shares(&hotkey, &bob);
        assert!(minted > 0);

        // Bob adds root stake (mirroring the real add_stake path: stake + watermark rebase).
        let stake = 1_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            stake.into(),
        );
        SubtensorModule::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(&hotkey, &bob, stake);
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &bob),
            minted,
            "adding root stake must not change direct-share credit"
        );

        // Bob removes half again (mirroring the real remove_stake path).
        let removed = stake / 2;
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            removed.into(),
        );
        SubtensorModule::remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(
            &hotkey,
            &bob,
            removed.into(),
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &bob),
            minted,
            epsilon = ROUNDING_EPS
        );

        // A new dividend accrues on bob's remaining root stake ON TOP of the credit.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        assert!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &bob) > minted,
            "dividend accrual must stack on top of the direct-share credit"
        );
        assert_shares_fully_owed(&hotkey, &[alice, bob], ROUNDING_EPS);
    });
}

/// Direct shares buy fund exposure, not dividend flow: with zero root stake, a direct
/// depositor's owed share count is bit-for-bit unchanged by subsequent dividend deposits
/// (which mint at NAV, so their marked payout is also preserved).
#[test]
fn test_stake_into_basket_gets_no_dividend_accrual() {
    new_test_ext(1).execute_with(|| {
        let (owner_coldkey, hotkey, netuid) = setup_stake_in_env();
        let alice = U256::from(2001);
        let bob = U256::from(2002);

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        open_fund_with_alpha(&hotkey, netuid);

        let amount = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * amount));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            amount.into(),
        ));
        let bob_shares = SubtensorModule::get_basket_owed_shares(&hotkey, &bob);
        let bob_payout_before = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);
        let alice_owed_before = SubtensorModule::get_basket_owed_shares(&hotkey, &alice);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        // Bob's share count is exactly unchanged; the dividend's shares went to root stakers.
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &bob),
            bob_shares
        );
        assert!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &alice) > alice_owed_before,
            "the root staker must capture the dividend accrual"
        );
        // Deposit-at-NAV: bob's marked payout is preserved (not diluted).
        let bob_payout_after = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);
        assert_abs_diff_eq!(
            bob_payout_after,
            bob_payout_before,
            epsilon = bob_payout_before / PAYOUT_EPS_DENOM
        );
    });
}

/// ΔNAV minting on a thin pool: a direct deposit into a fund holding a thin subnet is priced
/// at the realizable value the deposit added (bounded by the TAO deployed), never above it,
/// and the par-mint identity `shares == NAV` holds even when the buy moves the pool by ~17%.
#[test]
fn test_basket_deposit_mints_delta_nav_on_thin_pool() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let thin_netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(thin_netuid);
        // Thin pool: the deposit's buy is ~17% of the alpha reserve.
        SubnetTAO::<Test>::insert(thin_netuid, TaoBalance::from(10_000_000u64));
        SubnetAlphaIn::<Test>::insert(thin_netuid, AlphaBalance::from(10_000_000u64));

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();
        open_fund_with_alpha(&hotkey, thin_netuid);

        let deposit = 2_000_000u64;
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(2 * deposit));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            coldkey,
            hotkey,
            deposit.into(),
        ));

        let shares = fund_shares(&hotkey);
        let nav = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        assert!(shares > 0);
        // Par mint: shares == post-deposit realizable NAV (up to the opening dust the
        // depositor inherits).
        assert_abs_diff_eq!(shares, nav, epsilon = OPENING_ALPHA);
        // The realizable delta can never exceed the TAO deployed (the full-liquidation
        // quote retraces the buy's own price impact, so only fees are lost).
        assert!(
            shares <= deposit,
            "mint value must be bounded by the TAO deployed: {shares} > {deposit}"
        );
        assert!(shares >= deposit * (100 - FEE_TOLERANCE_PCT) / 100);
        // And the sole holder's claim realizes ~that value (the resell retraces the curve).
        let payout = SubtensorModule::get_basket_payout_tao(&hotkey, &coldkey);
        assert_abs_diff_eq!(payout, nav, epsilon = nav / SLIPPAGE_EPS_DENOM);
    });
}

/// The root-slot yield leak is fixed: the slice of each dividend attributable to the fund's
/// own root-slot (escrow) position enters the fund WITHOUT minting shares, so it accrues to
/// existing share holders through N/P. A pure share holder (zero root stake) captures their
/// pro-rata slice of the fund's cash yield — under the old full-mint behavior their payout
/// could never grow from root-slot earnings.
#[test]
fn test_root_slot_yield_accrues_to_share_holders() {
    new_test_ext(1).execute_with(|| {
        let (owner_coldkey, hotkey, netuid) = setup_stake_in_env();
        let alice = U256::from(2001);
        let bob = U256::from(2002);

        let alice_root = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            alice_root.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        // Bob opens the empty fund: his deposit is held as root cash (1:1, no swap on the
        // way in), which makes the arithmetic exact — N/P = 1, his TAO mints 1:1.
        let b = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * b));
        assert_ok!(SubtensorModule::do_stake_into_basket(bob, hotkey, b.into(),));
        assert_eq!(escrow_alpha(&hotkey, NetUid::ROOT), b);
        assert_eq!(fund_shares(&hotkey), b, "cash mints at par");
        assert_eq!(SubtensorModule::get_basket_owed_shares(&hotkey, &bob), b);
        let bob_payout_before = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);
        assert_eq!(bob_payout_before, b, "N/P = 1: payout == shares == TAO in");

        // A dividend lands while the escrow root slot holds b of the validator's root
        // stake, so only alice_root / (alice_root + b) of the value mints shares; the rest
        // raises N/P for every share holder.
        let escrow_root = escrow_alpha(&hotkey, NetUid::ROOT);
        let shares_before = fund_shares(&hotkey);
        let nav_before = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let delta = SubtensorModule::get_validator_basket_nav_tao(&hotkey)
            .to_u64()
            .saturating_sub(nav_before);
        assert!(delta > 0);
        let minted = fund_shares(&hotkey).saturating_sub(shares_before);

        // The mint is scaled by the stakers' attribution fraction (N/P was 1, so shares
        // track value 1:1 here).
        let expected_minted = (u128::from(delta) * u128::from(alice_root)
            / u128::from(alice_root + escrow_root)) as u64;
        assert_abs_diff_eq!(
            minted,
            expected_minted,
            epsilon = expected_minted / SLIPPAGE_EPS_DENOM + ROUNDING_EPS
        );
        assert!(
            minted < delta,
            "part of the dividend must enter unminted: minted {minted} of {delta}"
        );

        // Bob's payout GREW from the unminted slice — the fund's cash yield reached a pure
        // share holder. This is exactly the transfer the old behavior leaked to root stakers.
        let bob_payout_after = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);
        assert!(
            bob_payout_after > bob_payout_before,
            "share holder must capture root-slot yield: {bob_payout_after} <= {bob_payout_before}"
        );

        // Everything still adds up: every share is owed by exactly one of the two, and the
        // two payouts together drain the whole fund.
        assert_shares_fully_owed(&hotkey, &[alice, bob], ROUNDING_EPS);
        let alice_payout = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        let nav = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        assert_abs_diff_eq!(alice_payout + bob_payout_after, nav, epsilon = ROUNDING_EPS);
    });
}

/// A dividend deposit with a compounded fund (N/P > 1) still prices direct deposits
/// correctly: the direct depositor buys in at the compounded share price and cannot skim
/// the fund's past growth (the direct-deposit analog of the late-staker test).
#[test]
fn test_stake_into_basket_cannot_skim_compounding() {
    new_test_ext(1).execute_with(|| {
        let (owner_coldkey, hotkey, netuid) = setup_stake_in_env();
        let alice = U256::from(2001);
        let bob = U256::from(2002);

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        // The basket compounds hard: escrow value grows 4x, shares unchanged (N/P ~4).
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        let e0 = escrow_alpha(&hotkey, netuid);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid,
            (3 * e0).into(),
        );
        let alice_payout_before = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);

        // Bob buys in at the compounded price: his TAO mints ~amount / (N/P) shares, and
        // his immediate payout is ~his TAO — none of alice's compounding.
        let amount = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * amount));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            amount.into(),
        ));

        let bob_payout = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);
        assert_abs_diff_eq!(
            bob_payout,
            amount,
            epsilon = amount * FEE_TOLERANCE_PCT / 100
        );

        // Alice keeps her compounding.
        let alice_payout_after = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        assert_abs_diff_eq!(
            alice_payout_after,
            alice_payout_before,
            epsilon = alice_payout_before / PAYOUT_EPS_DENOM
        );
        assert_shares_fully_owed(&hotkey, &[alice, bob], ROUNDING_EPS);
    });
}
