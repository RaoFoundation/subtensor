//! Beta basket: direct deposits (`stake_into_basket`), ΔNAV minting, and root-slot yield
//! attribution.

use crate::tests::claim_root::{
    escrow_alpha, flush_baskets, fund_pool, fund_shares, has_fund, root_stake_of,
    set_root_weights_direct, zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::{BasketClaimed, DefaultMinStake, Error, StakingHotkeys, SubnetAlphaIn, SubnetTAO};
use approx::assert_abs_diff_eq;
use frame_support::traits::Get;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

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
    (owner_coldkey, hotkey, netuid)
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
/// Nobody needs root stake for any of it.
#[test]
fn test_stake_into_basket_round_trip_symmetric() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, netuid) = setup_stake_in_env();
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        let bob = U256::from(2001);
        let amount = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * amount));

        assert_ok!(SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(bob),
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

/// Par mint invariant on an empty fund: the first deposit mints exactly one share per TAO of
/// realizable value added, so `BasketShares == realizable NAV` to the rao. This is the ΔNAV
/// property in its purest form — the mint is priced at what the fund can actually redeem,
/// not at the TAO deployed.
#[test]
fn test_stake_into_basket_empty_fund_par_mint_equals_nav() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, netuid) = setup_stake_in_env();
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        let bob = U256::from(2001);
        let amount = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * amount));

        assert_ok!(SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(bob),
            hotkey,
            amount.into(),
        ));

        let shares = fund_shares(&hotkey);
        let nav = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        assert_eq!(
            shares, nav,
            "par mint must equal post-deposit realizable NAV"
        );
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

/// A direct deposit neither dilutes nor gifts existing dividend-accrued holders: their owed
/// payout and the fund's share price (N/P) are unchanged by someone else buying in, and
/// remain unchanged after that someone claims back out. `Σ owed == BasketShares` holds
/// throughout.
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

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
        assert_ok!(SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(bob),
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

/// Input validation: nonexistent hotkey, dust amounts, insufficient balance, and a weight
/// vector that filters to nothing are all rejected before any state changes.
#[test]
fn test_stake_into_basket_rejections() {
    new_test_ext(1).execute_with(|| {
        let (_owner, hotkey, netuid) = setup_stake_in_env();

        let bob = U256::from(2001);
        add_balance_to_coldkey_account(&bob, TaoBalance::from(50_000_000u64));

        // Hotkey with no account.
        assert_noop!(
            SubtensorModule::stake_into_basket(
                RuntimeOrigin::signed(bob),
                U256::from(777),
                10_000_000u64.into(),
            ),
            Error::<Test>::HotKeyAccountNotExists
        );

        // Below the minimum stake.
        let dust = DefaultMinStake::<Test>::get().to_u64().saturating_sub(1);
        assert_noop!(
            SubtensorModule::stake_into_basket(RuntimeOrigin::signed(bob), hotkey, dust.into(),),
            Error::<Test>::AmountTooLow
        );

        // No balance.
        let pauper = U256::from(2002);
        assert_noop!(
            SubtensorModule::stake_into_basket(
                RuntimeOrigin::signed(pauper),
                hotkey,
                10_000_000u64.into(),
            ),
            Error::<Test>::NotEnoughBalanceToStake
        );

        // Explicit weights that filter to nothing (nonexistent subnet): the fund is treated
        // as uncurated instead of erroring. With no holdings yet there is nothing to
        // mirror, so the deposit is held as the fund's root (TAO cash) slot at NAV.
        set_root_weights_direct(&hotkey, 0, &[(NetUid::from(99u16), u16::MAX)]);
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        let deposit = 10_000_000u64;
        assert_ok!(SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(bob),
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
        assert_ok!(SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(bob),
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

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
        assert_ok!(SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(bob),
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        let amount = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * amount));
        assert_ok!(SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(bob),
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

/// ΔNAV minting on a thin destination pool: the mint is priced at the realizable value the
/// deposit added (bounded by the TAO deployed), never above it, and the par-mint identity
/// `shares == NAV` holds exactly even when the buys move the pool by ~20%.
#[test]
fn test_basket_deposit_mints_delta_nav_on_thin_pool() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let origin_netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        let dest_owner = U256::from(1004);
        let dest_hotkey = U256::from(1005);
        let dest_netuid = add_dynamic_network(&dest_hotkey, &dest_owner);
        remove_owner_registration_stake(origin_netuid);
        remove_owner_registration_stake(dest_netuid);
        fund_pool(origin_netuid);
        // Thin destination pool: the deposit's buys are ~17% of the alpha reserve.
        SubnetTAO::<Test>::insert(dest_netuid, TaoBalance::from(10_000_000u64));
        SubnetAlphaIn::<Test>::insert(dest_netuid, AlphaBalance::from(10_000_000u64));

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            origin_netuid,
            10_000_000u64.into(),
        );
        // Route the whole basket into the thin pool.
        set_root_weights_direct(&hotkey, 0, &[(dest_netuid, u16::MAX)]);

        let dividend = 2_000_000u64;
        SubtensorModule::distribute_emission(
            origin_netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            dividend.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let shares = fund_shares(&hotkey);
        let nav = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        assert!(shares > 0);
        // Par mint: shares == post-deposit realizable NAV, to the rao.
        assert_eq!(shares, nav, "first deposit must mint exactly the ΔNAV");
        // The realizable delta can never exceed the TAO the dividend produced (~dividend at
        // the deep origin pool's ~1.0 price).
        assert!(
            shares <= dividend,
            "mint value must be bounded by the TAO deployed: {shares} > {dividend}"
        );
        // And the sole staker's claim realizes ~that value (the resell retraces the curve).
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
        // All-cash basket: every deposit is held as root stake (1:1, no swaps on the way
        // in), which makes the arithmetic exact.
        set_root_weights_direct(&hotkey, 0, &[(NetUid::ROOT, u16::MAX)]);

        // Dividend 1: the escrow root slot is empty, so the full value mints (par).
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        let e1 = escrow_alpha(&hotkey, NetUid::ROOT);
        assert!(e1 > 0);
        assert_eq!(fund_shares(&hotkey), e1, "first deposit mints at par");

        // Bob buys in directly: all-root basket at N/P = 1, so his TAO mints 1:1 exactly.
        let b = 10_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * b));
        assert_ok!(SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(bob),
            hotkey,
            b.into(),
        ));
        assert_eq!(SubtensorModule::get_basket_owed_shares(&hotkey, &bob), b);
        let bob_payout_before = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);
        assert_eq!(bob_payout_before, b, "N/P = 1: payout == shares == TAO in");

        // Dividend 2: the escrow root slot now holds e1 + b of the validator's root stake,
        // so only alice_root / (alice_root + e1 + b) of the value mints shares; the rest
        // raises N/P for every share holder.
        let escrow_root = escrow_alpha(&hotkey, NetUid::ROOT);
        assert_eq!(escrow_root, e1 + b);
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

        let delta2 = SubtensorModule::get_validator_basket_nav_tao(&hotkey)
            .to_u64()
            .saturating_sub(nav_before);
        assert!(delta2 > 0);
        let minted2 = fund_shares(&hotkey).saturating_sub(shares_before);

        // The mint is scaled by the stakers' attribution fraction (N/P was 1, so shares
        // track value 1:1 here).
        let expected_minted = (u128::from(delta2) * u128::from(alice_root)
            / u128::from(alice_root + escrow_root)) as u64;
        assert_abs_diff_eq!(
            minted2,
            expected_minted,
            epsilon = expected_minted / SLIPPAGE_EPS_DENOM + ROUNDING_EPS
        );
        assert!(
            minted2 < delta2,
            "part of the dividend must enter unminted: minted {minted2} of {delta2}"
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

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
        assert_ok!(SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(bob),
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
