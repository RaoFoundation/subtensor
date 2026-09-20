//! Beta basket: the cash-first claim path (spec 468).
//!
//! A single-hotkey claim on a fund whose TAO cash slot is ready declares only a scan plus
//! one transfer, pays the claimant from that cash at the guarded (fast-EMA-capped) mark,
//! and sells nothing. A claim that finds no ready cash declares the full 129-row envelope
//! and redeems pro-rata as before. A fund whose cash-path facts changed earlier in the
//! same block fails cheaply instead of running heavy work under a cheap declaration.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use crate::tests::claim_root::{
    allow_full_cash_claims, disable_cash_claims, enable_recommended_cash_claims, escrow_alpha,
    flush_baskets, fund_pool, fund_shares, register_on_root, root_stake_of, zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::weights::WeightInfo;
use crate::{
    BASKET_TRADE_REFILL_BLOCKS, BasketCashClaimBucket, BasketCashClaimCap, BasketCashNavAdjust,
    BasketCashTouchedBlock, Error, Event, RootClaimableThreshold, SubnetAlphaIn,
    SubnetFastMovingAlphaIn, SubnetFastMovingPrice, SubnetMovingPrice, SubnetTAO,
};
use frame_support::dispatch::GetDispatchInfo;
use frame_support::weights::Weight;
use frame_support::{assert_ok, dispatch::Pays};
use sp_core::U256;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

/// Dividend credited to the fund (alpha on subnet A, price ~1).
const DIVIDEND: u64 = 100_000_000;
/// TAO placed in the fund's root (cash) slot.
const CASH: u64 = 50_000_000;

struct Fund {
    hotkey: U256,
    /// Root stakers; `stakes[i]` units of root stake each.
    stakers: Vec<U256>,
    netuid: NetUid,
}

/// A root validator whose fund holds `DIVIDEND` alpha on one deep pool plus `cash` TAO in
/// its root slot, with `stakes` root stakers. The pool's moving prices are pinned at 1.0.
fn setup_fund(stakes: &[u64], cash: u64) -> Fund {
    let owner = U256::from(1001);
    let hotkey = U256::from(1002);
    let netuid = add_dynamic_network(&hotkey, &owner);
    remove_owner_registration_stake(netuid);
    fund_pool(netuid);
    SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(1));
    SubnetFastMovingPrice::<Test>::insert(netuid, U64F64::from_num(1));
    SubnetFastMovingAlphaIn::<Test>::insert(
        netuid,
        U64F64::from_num(SubnetAlphaIn::<Test>::get(netuid).to_u64()),
    );
    SubtensorModule::set_tao_weight(u64::MAX);
    zero_claim_threshold();
    // The path ships dark (default cap 0); these tests exercise it at the sized cap.
    enable_recommended_cash_claims();

    let stakers: Vec<U256> = stakes
        .iter()
        .enumerate()
        .map(|(i, stake)| {
            let staker = U256::from(5_000 + i as u32);
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &staker,
                NetUid::ROOT,
                (*stake).into(),
            );
            staker
        })
        .collect();
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &owner,
        netuid,
        10_000_000u64.into(),
    );
    register_on_root(&hotkey, 0);

    SubtensorModule::distribute_emission(
        netuid,
        AlphaBalance::ZERO,
        AlphaBalance::ZERO,
        DIVIDEND.into(),
        AlphaBalance::ZERO,
    );
    flush_baskets();
    assert!(escrow_alpha(&hotkey, netuid) > 0);
    assert!(fund_shares(&hotkey) > 0);

    if cash > 0 {
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            NetUid::ROOT,
            cash.into(),
        );
    }
    Fund {
        hotkey,
        stakers,
        netuid,
    }
}

fn claim_call(hotkey: U256) -> RuntimeCall {
    RuntimeCall::SubtensorModule(crate::Call::claim_root_with_hotkey { hotkey })
}

fn declared(hotkey: U256) -> Weight {
    claim_call(hotkey).get_dispatch_info().call_weight
}

/// What `get_dispatch_info` reports for a full-envelope claim: the 129-unit envelope
/// plus the coldkey-swap dispatch-extension fold every call carries.
fn full_envelope() -> Weight {
    SubtensorModule::root_claim_hotkey_declared_weight().saturating_add(extension())
}

fn extension() -> Weight {
    <Test as crate::Config>::WeightInfo::check_coldkey_swap_extension()
}

fn cash_claim_events() -> Vec<(U256, u64, u64)> {
    System::events()
        .iter()
        .filter_map(|record| match &record.event {
            RuntimeEvent::SubtensorModule(Event::BasketCashClaimed {
                coldkey,
                tao,
                shares,
                ..
            }) => Some((*coldkey, tao.to_u64(), *shares)),
            _ => None,
        })
        .collect()
}

fn next_block() {
    System::set_block_number(System::block_number() + 1);
}

/// (a) A claim the cash slot covers declares the cheap weight, pays from cash, sells
/// nothing, and reports an actual weight inside its cheap declaration.
#[test]
fn cash_covered_claim_declares_cheap_and_pays_from_cash() {
    new_test_ext(1).execute_with(|| {
        // 1% of the root stake belongs to the claimant, so their payout is a small slice
        // of the fund that the cash slot and the default 1% daily budget both cover.
        let fund = setup_fund(&[1, 199], CASH);
        let claimant = fund.stakers[0];

        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        let cheap = declared(fund.hotkey);
        assert_eq!(
            cheap,
            SubtensorModule::root_claim_cash_declared_weight(&fund.hotkey)
                .saturating_add(extension())
        );
        assert!(
            cheap.all_lt(full_envelope()),
            "cash-ready claim must declare below the envelope: {cheap:?} vs {:?}",
            full_envelope()
        );
        // The declaration is well under half the envelope: no sale per row is reserved.
        assert!(cheap.ref_time() * 2 < full_envelope().ref_time());

        let alpha_before = escrow_alpha(&fund.hotkey, fund.netuid);
        let cash_before = escrow_alpha(&fund.hotkey, NetUid::ROOT);
        let root_before = root_stake_of(&fund.hotkey, &claimant);
        let owed = SubtensorModule::get_basket_owed_shares(&fund.hotkey, &claimant);
        assert!(owed > 0);
        let expected_payout = SubtensorModule::basket_payout_from(
            owed,
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
            fund_shares(&fund.hotkey),
        );
        assert!(expected_payout > 0 && expected_payout < cash_before);

        let post =
            SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(claimant), fund.hotkey)
                .expect("cash claim succeeds");
        let actual = post.actual_weight.expect("claim reports actual weight");
        assert!(
            actual.all_lte(cheap),
            "actual {actual:?} within cheap {cheap:?}"
        );
        assert_eq!(post.pays_fee, Pays::Yes);

        let paid = root_stake_of(&fund.hotkey, &claimant) - root_before;
        assert_eq!(
            paid, expected_payout,
            "paid the guarded-mark share of the fund"
        );
        assert_eq!(escrow_alpha(&fund.hotkey, NetUid::ROOT), cash_before - paid);
        assert_eq!(
            escrow_alpha(&fund.hotkey, fund.netuid),
            alpha_before,
            "nothing was sold"
        );
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&fund.hotkey, &claimant),
            0
        );
        assert_eq!(cash_claim_events(), vec![(claimant, paid, owed)]);
        let (level, _) = BasketCashClaimBucket::<Test>::get(fund.hotkey).expect("bucket written");
        assert!(
            level
                < SubtensorModule::basket_cash_claim_budget_tao(
                    SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64()
                )
        );
    });
}

/// (b) Two claims in one block: the first exhausts the fund's cash budget, so the second
/// fails cheaply with `CashPathUnavailable` (no heavy work under a cheap declaration) and
/// a fresh declaration for it is already the full envelope. It succeeds next block.
#[test]
fn second_claim_in_block_after_cash_exhausted_fails_cheap() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[50, 50], CASH);
        let [alice, bob] = [fund.stakers[0], fund.stakers[1]];

        // Each staker's payout is far above the default 1% daily budget, so Alice's cash
        // claim pays only the budget and empties the bucket.
        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert!(declared(fund.hotkey).all_lt(full_envelope()));
        let alice_before = root_stake_of(&fund.hotkey, &alice);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            fund.hotkey
        ));
        let alice_paid = root_stake_of(&fund.hotkey, &alice) - alice_before;
        assert!(alice_paid > 0);
        assert!(
            SubtensorModule::get_basket_owed_shares(&fund.hotkey, &alice) > 0,
            "budget-limited: the rest stays owed"
        );
        assert!(!SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert_eq!(
            BasketCashTouchedBlock::<Test>::get(fund.hotkey),
            System::block_number()
        );
        // A claim submitted now is declared heavy again.
        assert_eq!(declared(fund.hotkey), full_envelope());

        let bob_owed = SubtensorModule::get_basket_owed_shares(&fund.hotkey, &bob);
        let alpha_before = escrow_alpha(&fund.hotkey, fund.netuid);
        let err = SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(bob), fund.hotkey)
            .expect_err("same-block claim on a touched fund fails cheap");
        assert_eq!(err.error, Error::<Test>::CashPathUnavailable.into());
        assert_eq!(
            err.post_info.actual_weight,
            Some(SubtensorModule::root_claim_precheck_weight(0)),
            "charged only the pre-check reads"
        );
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid), alpha_before);
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&fund.hotkey, &bob),
            bob_owed
        );
        assert_eq!(cash_claim_events().len(), 1);

        // Next block: the fund is no longer touched, cash is not ready, so Bob's claim
        // declares and runs the full redemption path.
        next_block();
        assert_eq!(declared(fund.hotkey), full_envelope());
        let post = SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(bob), fund.hotkey)
            .expect("redemption claim succeeds");
        assert!(post.actual_weight.expect("actual").all_lte(full_envelope()));
        assert!(
            escrow_alpha(&fund.hotkey, fund.netuid) < alpha_before,
            "redemption sold Bob's slice"
        );
        assert_eq!(cash_claim_events().len(), 1, "no cash claim for Bob");
    });
}

/// (c) A fund with no cash declares the full envelope and redeems pro-rata.
#[test]
fn no_cash_claim_declares_full_envelope_and_redeems() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 99], 0);
        let claimant = fund.stakers[0];
        assert!(!SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert_eq!(declared(fund.hotkey), full_envelope());

        let alpha_before = escrow_alpha(&fund.hotkey, fund.netuid);
        let root_before = root_stake_of(&fund.hotkey, &claimant);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(claimant),
            fund.hotkey
        ));
        assert!(root_stake_of(&fund.hotkey, &claimant) > root_before);
        assert!(escrow_alpha(&fund.hotkey, fund.netuid) < alpha_before);
        assert!(cash_claim_events().is_empty());
    });
}

/// Cash below the claim threshold does not open the cash path: the heavy path takes the
/// pro-rata slice of the slot as before.
#[test]
fn cash_below_threshold_is_not_ready() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 99], CASH);
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(CASH + 1));
        assert!(!SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert_eq!(declared(fund.hotkey), full_envelope());
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(CASH));
        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert!(declared(fund.hotkey).all_lt(full_envelope()));
    });
}

/// A zero cap turns the cash path off entirely (the emergency switch).
#[test]
fn zero_cap_disables_cash_path() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 99], CASH);
        disable_cash_claims();
        assert!(!SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert_eq!(declared(fund.hotkey), full_envelope());
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.stakers[0]),
            fund.hotkey
        ));
        assert!(cash_claim_events().is_empty());
    });
}

/// Partial cash: the claim pays what the cash slot holds, burns exactly the matching
/// shares, and leaves the rest owed; the next block's claim redeems the remainder on the
/// full path. Together the two claims pay the claimant the whole fund.
#[test]
fn partial_cash_pays_cash_then_remainder_redeems_next_block() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[100], CASH);
        allow_full_cash_claims();
        let claimant = fund.stakers[0];
        let shares_before = fund_shares(&fund.hotkey);
        let owed = SubtensorModule::get_basket_owed_shares(&fund.hotkey, &claimant);
        let guarded_nav =
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64();
        let payout = SubtensorModule::basket_payout_from(owed, guarded_nav, shares_before);
        assert!(payout > CASH, "the payout exceeds the cash on hand");

        let root_before = root_stake_of(&fund.hotkey, &claimant);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(claimant),
            fund.hotkey
        ));
        let paid = root_stake_of(&fund.hotkey, &claimant) - root_before;
        assert_eq!(paid, CASH, "the whole cash slot was paid");
        assert_eq!(escrow_alpha(&fund.hotkey, NetUid::ROOT), 0);
        let burned = shares_before - fund_shares(&fund.hotkey);
        let expected_burn =
            (u128::from(CASH) * u128::from(shares_before)).div_ceil(u128::from(guarded_nav)) as u64;
        assert!(
            burned.abs_diff(expected_burn) <= 1,
            "burn {burned} for {CASH} rao at NAV {guarded_nav}, expected {expected_burn}"
        );
        let remaining_owed = SubtensorModule::get_basket_owed_shares(&fund.hotkey, &claimant);
        // Fixed-point watermark rebase for the grown root stake rounds by at most one share.
        assert!(remaining_owed.abs_diff(owed - burned) <= 1);
        assert!(remaining_owed > 0);
        assert!(!SubtensorModule::root_claim_cash_ready(&fund.hotkey));

        next_block();
        assert_eq!(declared(fund.hotkey), full_envelope());
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(claimant),
            fund.hotkey
        ));
        assert!(SubtensorModule::get_basket_owed_shares(&fund.hotkey, &claimant) <= 1);
        assert!(escrow_alpha(&fund.hotkey, fund.netuid) <= 10);
        assert!(fund_shares(&fund.hotkey) <= 10);
        let total = root_stake_of(&fund.hotkey, &claimant) - root_before;
        // Cash at the guarded mark plus the alpha sold at its realizable quote: the whole
        // fund, within slippage.
        assert!(total >= guarded_nav * 99 / 100 && total <= guarded_nav);
    });
}

/// Pump-and-claim: inflating a held pool's spot inside the block raises the realizable
/// NAV but not the fast-EMA-capped mark, so a cash claim pays exactly what it would have
/// paid without the pump; and the daily budget caps what the cash path pays in a window.
#[test]
fn pump_does_not_inflate_cash_payout_and_budget_caps_the_window() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 199], CASH);
        let attacker = fund.stakers[0];
        let honest_mark = SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey);
        let honest_nav = SubtensorModule::get_validator_basket_nav_tao(&fund.hotkey);
        let owed = SubtensorModule::get_basket_owed_shares(&fund.hotkey, &attacker);
        let honest_payout = SubtensorModule::basket_payout_from(
            owed,
            honest_mark.to_u64(),
            fund_shares(&fund.hotkey),
        );

        // Pump: a hundredfold TAO reserve on the held pool. Spot and realizable soar; the
        // fast anchor (updated only between blocks) does not move.
        SubnetTAO::<Test>::mutate(fund.netuid, |tao| {
            *tao = TaoBalance::from(tao.to_u64().saturating_mul(100))
        });
        let pumped_nav = SubtensorModule::get_validator_basket_nav_tao(&fund.hotkey);
        assert!(pumped_nav.to_u64() > honest_nav.to_u64().saturating_mul(10));
        // The mark is a liquidation against anchored price and depth: it does not move.
        let pumped_mark = SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey);
        assert_eq!(pumped_mark, honest_mark, "the cash mark is anchored");
        let before = root_stake_of(&fund.hotkey, &attacker);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(attacker),
            fund.hotkey
        ));
        let paid = root_stake_of(&fund.hotkey, &attacker) - before;
        assert_eq!(paid, honest_payout, "the pump bought nothing");
        assert!(paid < CASH / 10);

        // Budget: whatever the mark, the cash path pays at most the daily cap per window.
        // Reset and let a big claimant try: the cap binds and the rest stays owed.
        next_block();
        BasketCashClaimBucket::<Test>::remove(fund.hotkey);
        let whale = fund.stakers[1];
        let budget = SubtensorModule::basket_cash_claim_budget_tao(honest_mark.to_u64());
        let whale_before = root_stake_of(&fund.hotkey, &whale);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(whale),
            fund.hotkey
        ));
        let whale_paid = root_stake_of(&fund.hotkey, &whale) - whale_before;
        assert!(
            whale_paid <= budget + budget / 10_000 + 1,
            "{whale_paid} exceeds the daily cash budget {budget}"
        );
        assert!(whale_paid > 0);
        assert!(SubtensorModule::get_basket_owed_shares(&fund.hotkey, &whale) > 0);
        // The bucket refills over a day; half a day later half the budget is back.
        System::set_block_number(System::block_number() + BASKET_TRADE_REFILL_BLOCKS / 2);
        let (level, last) = BasketCashClaimBucket::<Test>::get(fund.hotkey).expect("bucket");
        let room = SubtensorModule::basket_bucket_level_at(
            Some((level, last)),
            System::block_number(),
            budget,
        );
        assert!(room >= budget / 2 && room <= budget / 2 + budget / 100 + 1);
    });
}

/// The coldkey-wide `claim_root` keeps its full declaration but pays from cash when the
/// fund is ready, and is never refused for a same-block touch.
#[test]
fn coldkey_wide_claim_uses_cash_under_full_declaration() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 99], CASH);
        let claimant = fund.stakers[0];
        let call = RuntimeCall::SubtensorModule(crate::Call::claim_root {
            subnets: Default::default(),
        });
        assert_eq!(
            call.get_dispatch_info().call_weight,
            SubtensorModule::root_claim_declared_weight().saturating_add(extension())
        );
        SubtensorModule::mark_basket_cash_touched(&fund.hotkey);
        let alpha_before = escrow_alpha(&fund.hotkey, fund.netuid);
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(claimant),
            Default::default()
        ));
        assert_eq!(cash_claim_events().len(), 1);
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid), alpha_before);
    });
}

/// A hotkey swap moves rows, cash and queued credits between funds; both hotkeys are
/// marked touched so a same-block single-hotkey claim on either fails cheap.
#[test]
fn hotkey_swap_marks_both_funds_touched() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 99], CASH);
        let new_hotkey = U256::from(7_777);
        SubtensorModule::transfer_basket_for_new_hotkey(&fund.hotkey, &new_hotkey);
        for hotkey in [fund.hotkey, new_hotkey] {
            let err = SubtensorModule::claim_root_with_hotkey(
                RuntimeOrigin::signed(fund.stakers[0]),
                hotkey,
            )
            .expect_err("touched fund refuses same-block single-hotkey claims");
            assert_eq!(err.error, Error::<Test>::CashPathUnavailable.into());
        }
        // The stakers' root positions stay on the old hotkey in this direct call (a real
        // hotkey swap moves them too), so the moved fund has no claimant yet: the claims
        // are admitted again next block and no-op.
        next_block();
        for hotkey in [fund.hotkey, new_hotkey] {
            assert_ok!(SubtensorModule::claim_root_with_hotkey(
                RuntimeOrigin::signed(fund.stakers[0]),
                hotkey
            ));
        }
        assert!(cash_claim_events().is_empty());
        assert_eq!(escrow_alpha(&new_hotkey, NetUid::ROOT), CASH);
    });
}

/// The cash-path predicate reads the cap and threshold live and is monotone in cash.
#[test]
fn cash_ready_tracks_cap_threshold_and_cash() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 99], 0);
        assert!(!SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &fund.hotkey,
            &escrow,
            NetUid::ROOT,
            1_000_000u64.into(),
        );
        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        BasketCashClaimCap::<Test>::put(0);
        assert!(!SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        BasketCashClaimCap::<Test>::put(1);
        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
    });
}

/// Skeptic finding on #3184: the ready predicate is not monotone in cash once a bucket
/// level is stored (the quarter-budget bar scales with the cash slot, the stored level
/// does not), so a cash *inflow* can close the path. `batch(stake_into_basket, claim)`
/// would then run the heavy path under the cheap declaration computed at batch start —
/// unless the inflow is recorded. Every root-slot credit records the flip; the claim
/// fails cheap and sells nothing.
#[test]
fn cash_inflow_that_closes_the_path_marks_the_fund_touched() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 99], CASH);
        let depositor = U256::from(9_001);
        add_balance_to_coldkey_account(&depositor, TaoBalance::from(10 * CASH));

        // A partially depleted bucket: just enough for the quarter rule at today's cash.
        let budget_floor = SubtensorModule::basket_cash_claim_budget_tao(CASH);
        BasketCashClaimBucket::<Test>::insert(
            fund.hotkey,
            (budget_floor / 4 + 1, System::block_number()),
        );
        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert!(
            declared(fund.hotkey).all_lt(full_envelope()),
            "declared cheap at batch start"
        );
        assert!(!SubtensorModule::basket_cash_touched_this_block(
            &fund.hotkey
        ));

        // Inflow: a mirrored deposit credits the root slot pro-rata (the fund is ~1/3
        // cash), raising the cash-derived budget above four times the stored level.
        assert_ok!(SubtensorModule::do_stake_into_basket(
            depositor,
            fund.hotkey,
            (4 * CASH).into(),
        ));
        assert!(escrow_alpha(&fund.hotkey, NetUid::ROOT) > CASH);
        assert!(
            !SubtensorModule::root_claim_cash_ready(&fund.hotkey),
            "more cash, same bucket level: the quarter rule now fails"
        );
        assert!(
            SubtensorModule::basket_cash_touched_this_block(&fund.hotkey),
            "the inflow recorded the flip"
        );

        // The claim that was declared cheap at batch start must not redeem.
        let alpha_before = escrow_alpha(&fund.hotkey, fund.netuid);
        let err = SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.stakers[0]),
            fund.hotkey,
        )
        .expect_err("touched fund refuses the same-block single-hotkey claim");
        assert_eq!(err.error, Error::<Test>::CashPathUnavailable.into());
        assert_eq!(
            err.post_info.actual_weight,
            Some(SubtensorModule::root_claim_precheck_weight(0))
        );
        assert_eq!(
            escrow_alpha(&fund.hotkey, fund.netuid),
            alpha_before,
            "nothing sold"
        );
        assert!(cash_claim_events().is_empty());

        // Next block the fund is declared for the path it will take (heavy, bucket
        // below the bar) and redeems normally.
        next_block();
        assert_eq!(declared(fund.hotkey), full_envelope());
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.stakers[0]),
            fund.hotkey
        ));
        assert!(escrow_alpha(&fund.hotkey, fund.netuid) < alpha_before);
    });
}

/// The cash-path decision is taken on the state before the claim's own flush, i.e. the
/// state the declaration saw; a flush cannot move the claim onto the other path.
#[test]
fn path_is_decided_before_the_flush() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 199], CASH);
        // Queue a fresh dividend credit; the claim flushes it first.
        let credit = 1_000_000u64;
        crate::SubnetAlphaOut::<Test>::mutate(fund.netuid, |t| {
            *t = t.saturating_add(credit.into())
        });
        SubtensorModule::enqueue_basket_deposit(&fund.hotkey, fund.netuid, credit.into());
        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        let post = SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.stakers[0]),
            fund.hotkey,
        )
        .expect("cash claim");
        assert!(
            !crate::PendingBasketDeposits::<Test>::contains_key(fund.hotkey, fund.netuid),
            "the claim flushed the credit"
        );
        assert_eq!(cash_claim_events().len(), 1);
        assert!(
            post.actual_weight
                .expect("actual")
                .all_lte(declared(fund.hotkey))
        );
    });
}

/// Skeptic/auditor finding on #3184: with a holding that is a material share of its pool,
/// the realizable quote sits well below `alpha × EMA` because of slippage, and a same-block
/// buy (or liquidity add) can lift it toward that cap with the price anchor unchanged. The
/// mark now liquidates against anchored price *and* anchored depth, so neither move pays.
#[test]
fn material_holding_pump_or_liquidity_add_cannot_raise_the_cash_mark() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 199], CASH);
        // Thin pool: the fund's row is 10% of the alpha reserve (the liquidity cap).
        let reserve = 1_000_000_000_000u64;
        let holding = reserve / 10;
        SubnetTAO::<Test>::insert(fund.netuid, TaoBalance::from(reserve));
        SubnetAlphaIn::<Test>::insert(fund.netuid, AlphaBalance::from(reserve));
        SubnetFastMovingAlphaIn::<Test>::insert(fund.netuid, U64F64::from_num(reserve));
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        let held = escrow_alpha(&fund.hotkey, fund.netuid);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &fund.hotkey,
            &escrow,
            fund.netuid,
            (holding - held).into(),
        );
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid), holding);
        // Enough cash that the claimant's 0.5% share is fully cash-covered.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &fund.hotkey,
            &escrow,
            NetUid::ROOT,
            (holding / 10).into(),
        );

        let realizable = SubtensorModule::realizable_tao_for_alpha(fund.netuid, holding);
        // ~9% slippage discount below alpha × EMA on a 10%-of-pool row.
        assert!(realizable < holding * 92 / 100 && realizable > holding * 89 / 100);
        let honest_mark = SubtensorModule::anchored_liquidation_value(fund.netuid, holding);
        assert_eq!(
            honest_mark, realizable,
            "un-manipulated: mark is the liquidation quote"
        );
        let honest_payout = {
            let owed = SubtensorModule::get_basket_owed_shares(&fund.hotkey, &fund.stakers[0]);
            SubtensorModule::basket_payout_from(
                owed,
                SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
                fund_shares(&fund.hotkey),
            )
        };

        // Attack 1: a 50%-of-reserve buy. Spot 1.0 → 2.25; the row's liquidation quote
        // nearly doubles; the mark does not move at all.
        let bought = reserve / 2;
        SubnetTAO::<Test>::insert(fund.netuid, TaoBalance::from(reserve + bought));
        SubnetAlphaIn::<Test>::insert(
            fund.netuid,
            AlphaBalance::from(
                (reserve as u128 * reserve as u128 / (reserve + bought) as u128) as u64,
            ),
        );
        let pumped = SubtensorModule::realizable_tao_for_alpha(fund.netuid, holding);
        assert!(pumped > realizable * 18 / 10);
        assert_eq!(
            SubtensorModule::anchored_liquidation_value(fund.netuid, holding),
            honest_mark
        );

        // Attack 2: deepen the pool a hundredfold at the same price, erasing slippage. The
        // quote rises to alpha × price; the mark still does not move.
        SubnetTAO::<Test>::insert(fund.netuid, TaoBalance::from(reserve * 100));
        SubnetAlphaIn::<Test>::insert(fund.netuid, AlphaBalance::from(reserve * 100));
        let deep = SubtensorModule::realizable_tao_for_alpha(fund.netuid, holding);
        assert!(deep > holding * 99 / 100);
        assert_eq!(
            SubtensorModule::anchored_liquidation_value(fund.netuid, holding),
            honest_mark
        );

        // And the claim under attack 2 pays exactly the honest amount.
        let before = root_stake_of(&fund.hotkey, &fund.stakers[0]);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.stakers[0]),
            fund.hotkey
        ));
        assert_eq!(
            root_stake_of(&fund.hotkey, &fund.stakers[0]) - before,
            honest_payout
        );
        assert_eq!(cash_claim_events().len(), 1);

        // Skeptic follow-up: after a decline (live quote far below the lagging anchors) a
        // same-block buy or liquidity add must not move the mark either; the mark reads no
        // live figure, so a buy → cash claim → sell round trip pays exactly what an
        // un-manipulated claim pays and nets the attacker minus fees.
        SubnetTAO::<Test>::insert(fund.netuid, TaoBalance::from(reserve / 2));
        SubnetAlphaIn::<Test>::insert(fund.netuid, AlphaBalance::from(reserve * 2));
        let dumped = SubtensorModule::realizable_tao_for_alpha(fund.netuid, holding);
        assert!(
            dumped < honest_mark / 3,
            "the market fell well below the anchors"
        );
        assert_eq!(
            SubtensorModule::anchored_liquidation_value(fund.netuid, holding),
            honest_mark,
            "anchored: the mark does not follow the live quote"
        );
        let whale = fund.stakers[1];
        allow_full_cash_claims();
        let whale_owed = SubtensorModule::get_basket_owed_shares(&fund.hotkey, &whale);
        let unmanipulated_payout = SubtensorModule::basket_payout_from(
            whale_owed,
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
            fund_shares(&fund.hotkey),
        );
        // The "buy": lift spot back to the anchor and deepen the pool in the same block.
        SubnetTAO::<Test>::insert(fund.netuid, TaoBalance::from(reserve * 50));
        SubnetAlphaIn::<Test>::insert(fund.netuid, AlphaBalance::from(reserve * 50));
        let lifted = SubtensorModule::realizable_tao_for_alpha(fund.netuid, holding);
        assert!(lifted > dumped * 3, "the live quote was lifted back up");
        assert_eq!(
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
            SubtensorModule::basket_payout_from(
                fund_shares(&fund.hotkey),
                SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
                fund_shares(&fund.hotkey)
            ),
        );
        let cash_now = escrow_alpha(&fund.hotkey, NetUid::ROOT);
        let whale_before = root_stake_of(&fund.hotkey, &whale);
        next_block();
        BasketCashClaimBucket::<Test>::remove(fund.hotkey);
        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(whale),
            fund.hotkey
        ));
        assert_eq!(
            root_stake_of(&fund.hotkey, &whale) - whale_before,
            unmanipulated_payout.min(cash_now),
            "the lift bought nothing: paid the anchored mark"
        );

        // A row on a subnet with no anchors yet contributes nothing to the cash mark.
        SubnetFastMovingAlphaIn::<Test>::remove(fund.netuid);
        assert_eq!(
            SubtensorModule::anchored_liquidation_value(fund.netuid, holding),
            0
        );
    });
}

/// The reserve anchor advances once per block like the price anchor and starts at the
/// current reserve.
#[test]
fn fast_reserve_anchor_tracks_the_pool_between_blocks() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 199], 0);
        SubnetFastMovingAlphaIn::<Test>::remove(fund.netuid);
        let reserve = SubnetAlphaIn::<Test>::get(fund.netuid).to_u64();
        SubtensorModule::update_fast_moving_price(fund.netuid);
        assert_eq!(
            SubnetFastMovingAlphaIn::<Test>::get(fund.netuid),
            Some(U64F64::from_num(reserve)),
            "seeded at the current reserve"
        );
        SubnetAlphaIn::<Test>::insert(fund.netuid, AlphaBalance::from(reserve * 2));
        SubtensorModule::update_fast_moving_price(fund.netuid);
        let anchored = SubnetFastMovingAlphaIn::<Test>::get(fund.netuid).expect("anchor");
        assert!(anchored > U64F64::from_num(reserve));
        assert!(
            anchored < U64F64::from_num(reserve + reserve / 100),
            "one block moves ~0.1%"
        );
    });
}

/// Skeptic finding on #3184 (anchors-only variant): after a decline, a deposit priced at
/// the live NAV must not redeem immediately through the cash path for more than it paid.
/// The mark is capped by the live liquidation quote, so the fresh shares are worth at most
/// what they bought (minus the deposit's own slippage).
#[test]
fn deposit_then_cash_claim_cannot_exceed_the_deposit() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 199], CASH);
        allow_full_cash_claims();
        // The market halves while the anchors still say 1.0.
        let reserve = 1_000_000_000_000u64;
        SubnetTAO::<Test>::insert(fund.netuid, TaoBalance::from(reserve / 2));
        SubnetAlphaIn::<Test>::insert(fund.netuid, AlphaBalance::from(reserve));
        assert_eq!(
            SubnetFastMovingPrice::<Test>::get(fund.netuid),
            Some(U64F64::from_num(1))
        );
        let live_nav = SubtensorModule::get_validator_basket_nav_tao(&fund.hotkey).to_u64();
        let mark_nav =
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64();

        let arb = U256::from(9_100);
        let deposit = 10_000_000u64;
        add_balance_to_coldkey_account(&arb, TaoBalance::from(2 * deposit));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            arb,
            fund.hotkey,
            deposit.into()
        ));
        let root_before = root_stake_of(&fund.hotkey, &arb);
        // Same block: a deposit into a fund with a full bucket does not flip the predicate.
        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(arb),
            fund.hotkey
        ));
        let paid = root_stake_of(&fund.hotkey, &arb) - root_before;
        assert!(paid > 0);
        assert!(
            paid <= deposit + 2,
            "a deposit below the anchors redeems for at most what it paid: {paid} vs {deposit}"
        );
        // Mint priced at the anchored NAV (the higher one): the shares are worth, at the
        // anchored mark, exactly the live value the deposit added and no more.
        assert!(mark_nav > live_nav, "anchors above the market");
    });
}

/// The deposit half of the cash-mark design: a direct deposit's shares may claim no more
/// of the anchored NAV than the TAO it brought in. Above the anchors (a rally) the
/// live-priced mint is smaller and pricing is unchanged; below them the cap binds.
#[test]
fn deposit_mint_is_capped_by_its_claim_on_the_anchored_nav() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 199], CASH);
        let reserve = 1_000_000_000_000u64;
        let depositor = U256::from(9_200);
        let deposit = 10_000_000u64;
        add_balance_to_coldkey_account(&depositor, TaoBalance::from(10 * deposit));

        // Aligned anchors: the cap sits above the live-priced mint.
        let live = SubtensorModule::get_validator_basket_nav_tao(&fund.hotkey).to_u64();
        let anchored = SubtensorModule::anchored_basket_nav_tao(&fund.hotkey);
        assert!(anchored.abs_diff(live) <= live / 100_000);
        let shares = fund_shares(&fund.hotkey);
        assert!(
            SubtensorModule::basket_anchored_mint_cap(&fund.hotkey, deposit, shares)
                > SubtensorModule::mul_div_u64(deposit, shares, live)
        );

        // Rally: live above the anchors, the live-priced mint is the smaller one.
        SubnetTAO::<Test>::insert(fund.netuid, TaoBalance::from(reserve * 2));
        let rally_live = SubtensorModule::get_validator_basket_nav_tao(&fund.hotkey).to_u64();
        assert!(rally_live > anchored);
        assert!(
            SubtensorModule::basket_anchored_mint_cap(&fund.hotkey, deposit, shares)
                > SubtensorModule::mul_div_u64(deposit, shares, rally_live)
        );

        // Decline: alpha is cheap live but worth its anchored value at the cash mark, so
        // the cap binds and the depositor gets fewer shares than the live NAV would give.
        SubnetTAO::<Test>::insert(fund.netuid, TaoBalance::from(reserve / 2));
        let fall_live = SubtensorModule::get_validator_basket_nav_tao(&fund.hotkey).to_u64();
        assert!(fall_live < anchored);
        assert_ok!(SubtensorModule::do_stake_into_basket(
            depositor,
            fund.hotkey,
            deposit.into()
        ));
        let minted = fund_shares(&fund.hotkey) - shares;
        let live_priced = SubtensorModule::mul_div_u64(deposit, shares, fall_live);
        assert!(
            minted < live_priced,
            "minted {minted} < live-priced {live_priced}"
        );
        assert!(minted > 0);
        // The new shares are worth at most the deposit at the cash mark.
        let claim_at_mark = SubtensorModule::basket_payout_from(
            minted,
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
            fund_shares(&fund.hotkey),
        );
        assert!(claim_at_mark <= deposit, "{claim_at_mark} <= {deposit}");
    });
}

/// Skeptic finding on #3184 (mint cap alone): a depositor who already holds shares gains
/// `f × (ΔA − D)` on those shares when the mirror buys alpha below the anchors. The
/// cost-basis correction carries the purchase at cost, so the total cash claim after a
/// deposit is at most the original entitlement plus the deposit.
#[test]
fn deposit_with_existing_shares_cannot_cash_claim_more_than_entitlement_plus_deposit() {
    new_test_ext(1).execute_with(|| {
        // Two-thirds alpha, one-third cash, with enough cash to cover the whole claim.
        let fund = setup_fund(&[5, 995], CASH);
        allow_full_cash_claims();
        let holder = fund.stakers[0];
        let reserve = 1_000_000_000_000u64;

        // Original entitlement at the cash mark, anchors aligned.
        let owed_before = SubtensorModule::get_basket_owed_shares(&fund.hotkey, &holder);
        let entitlement = SubtensorModule::basket_payout_from(
            owed_before,
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
            fund_shares(&fund.hotkey),
        );
        assert!(entitlement > 0);

        // The market halves; the anchors still say 1.0. A deposit now buys alpha cheap.
        SubnetTAO::<Test>::insert(fund.netuid, TaoBalance::from(reserve / 2));
        SubnetAlphaIn::<Test>::insert(fund.netuid, AlphaBalance::from(reserve));
        let deposit = 10_000_000u64;
        add_balance_to_coldkey_account(&holder, TaoBalance::from(2 * deposit));
        let anchored_before = SubtensorModule::anchored_basket_nav_tao(&fund.hotkey);
        let cash_slot_before = escrow_alpha(&fund.hotkey, NetUid::ROOT);
        let cash_nav_before =
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64();
        assert_ok!(SubtensorModule::do_stake_into_basket(
            holder,
            fund.hotkey,
            deposit.into()
        ));
        let anchored_added = SubtensorModule::anchored_basket_nav_tao(&fund.hotkey)
            - anchored_before
            - (escrow_alpha(&fund.hotkey, NetUid::ROOT) - cash_slot_before);
        let alpha_cost = deposit - (escrow_alpha(&fund.hotkey, NetUid::ROOT) - cash_slot_before);
        assert!(
            anchored_added > alpha_cost + alpha_cost / 10,
            "the mirror bought alpha worth more at the anchors than it cost: {anchored_added} for {alpha_cost}"
        );
        let cash_nav_after =
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64();
        assert!(
            cash_nav_after.abs_diff(cash_nav_before + deposit) <= 2,
            "the cash NAV rose by the cost: {cash_nav_before} + {deposit} -> {cash_nav_after}"
        );
        // The correction sits on the alpha row: its anchored uplift minus the TAO the
        // mirror spent on it (the cash slice carries none).
        let (adjust, _) =
            BasketCashNavAdjust::<Test>::get(fund.hotkey, fund.netuid).expect("correction booked");
        assert!(BasketCashNavAdjust::<Test>::get(fund.hotkey, NetUid::ROOT).is_none());
        let cash_added = escrow_alpha(&fund.hotkey, NetUid::ROOT) - cash_slot_before;
        assert_eq!(
            adjust,
            i128::from(anchored_added) - i128::from(deposit - cash_added)
        );

        // Immediate cash claim (same block: the deposit did not flip the predicate).
        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        let root_before = root_stake_of(&fund.hotkey, &holder);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(holder),
            fund.hotkey
        ));
        let paid = root_stake_of(&fund.hotkey, &holder) - root_before;
        assert_eq!(cash_claim_events().len(), 1, "paid from cash");
        assert!(
            paid <= entitlement + deposit + 2,
            "no uplift: paid {paid} vs entitlement {entitlement} + deposit {deposit}"
        );
        assert!(paid > entitlement, "the deposit itself is redeemable");
    });
}

/// The correction decays on the fast-EMA schedule (half gone after one half-life), is
/// released pro-rata when the row is disposed of — both signs — and follows hotkey swaps.
#[test]
fn cost_basis_correction_decays_releases_pro_rata_and_follows_hotkey_swaps() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 199], CASH);
        let now = System::block_number();
        let row = fund.netuid;
        BasketCashNavAdjust::<Test>::insert(fund.hotkey, row, (1_000_000i128, now));
        assert_eq!(
            SubtensorModule::basket_row_cost_basis_at(&fund.hotkey, row, now),
            1_000_000
        );
        let half_life = crate::BASKET_FAST_EMA_HALF_LIFE_BLOCKS;
        let after_half =
            SubtensorModule::basket_row_cost_basis_at(&fund.hotkey, row, now + half_life);
        assert!(after_half > 490_000 && after_half < 510_000, "{after_half}");
        let after_ten =
            SubtensorModule::basket_row_cost_basis_at(&fund.hotkey, row, now + 10 * half_life);
        assert!(after_ten < 1_100, "{after_ten}");

        // A positive correction lowers the row's cash value; the anchored figure is unchanged.
        let anchored = SubtensorModule::anchored_basket_nav_tao(&fund.hotkey);
        assert_eq!(
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
            anchored - 1_000_000
        );

        // Disposing a quarter of the row releases a quarter of the correction.
        SubtensorModule::release_basket_cost_basis(&fund.hotkey, row, 250, 1_000);
        assert_eq!(
            SubtensorModule::basket_row_cost_basis_at(&fund.hotkey, row, now),
            750_000
        );
        // Disposing the rest clears it.
        SubtensorModule::release_basket_cost_basis(&fund.hotkey, row, 1_000, 1_000);
        assert!(BasketCashNavAdjust::<Test>::get(fund.hotkey, row).is_none());

        // Negative corrections (buys above the anchors) decay and release the same way.
        BasketCashNavAdjust::<Test>::insert(fund.hotkey, row, (-1_000_000i128, now));
        let neg_half =
            SubtensorModule::basket_row_cost_basis_at(&fund.hotkey, row, now + half_life);
        assert!(neg_half < -490_000 && neg_half > -510_000, "{neg_half}");
        SubtensorModule::release_basket_cost_basis(&fund.hotkey, row, 500, 1_000);
        assert_eq!(
            SubtensorModule::basket_row_cost_basis_at(&fund.hotkey, row, now),
            -500_000
        );
        assert_eq!(
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
            anchored + 500_000
        );

        // Booking on top merges with the decayed value; a fund that ends clears every row.
        SubtensorModule::note_basket_cost_basis(&fund.hotkey, row, 700_000, 100_000);
        assert_eq!(
            SubtensorModule::basket_row_cost_basis_at(&fund.hotkey, row, now),
            100_000
        );
        SubtensorModule::clear_basket_cost_basis(&fund.hotkey);
        assert!(BasketCashNavAdjust::<Test>::get(fund.hotkey, row).is_none());

        // Hotkey swap carries it row by row.
        BasketCashNavAdjust::<Test>::insert(fund.hotkey, row, (1_000_000i128, now));
        let new_hotkey = U256::from(7_778);
        SubtensorModule::transfer_basket_for_new_hotkey(&fund.hotkey, &new_hotkey);
        assert!(BasketCashNavAdjust::<Test>::get(fund.hotkey, row).is_none());
        assert_eq!(
            SubtensorModule::basket_row_cost_basis_at(&new_hotkey, row, now),
            1_000_000
        );
    });
}

/// Skeptic finding on #3184 (fid acf10d8e): a correction booked on purchase must leave with
/// the alpha. A validator round-trips a row through `swap_basket` (buy, then sell it all
/// back) with the market just off the anchors; the correction is booked on the buy, fully
/// released on the sell, and a cash claim afterwards pays no more than before the trades.
/// A partial sell releases partially.
#[test]
fn round_trip_trade_releases_the_cost_basis_and_cannot_lift_a_cash_claim() {
    new_test_ext(1).execute_with(|| {
        // 200 TAO of cash so the fund can buy; the alpha row is small next to the pool.
        let fund = setup_fund(&[1, 199], 200_000_000_000);
        let owner = U256::from(1001);
        crate::BasketTradingEnabled::<Test>::put(true);
        crate::BasketDailyTurnoverCap::<Test>::put(u16::MAX);
        allow_full_cash_claims();
        let claimant = fund.stakers[0];
        let row = fund.netuid;
        let reserve = 1_000_000_000_000u64;

        // Market 1.5% below the anchors (inside the 2% trade band): alpha is cheap live.
        SubnetTAO::<Test>::insert(row, TaoBalance::from(reserve * 985 / 1000));
        let entitlement_before = SubtensorModule::basket_payout_from(
            SubtensorModule::get_basket_owed_shares(&fund.hotkey, &claimant),
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
            fund_shares(&fund.hotkey),
        );
        let alpha_before = escrow_alpha(&fund.hotkey, row);

        // Buy: 5 TAO of cash into the row (0.5% of the pool, inside the band).
        let spend = 5_000_000_000u64;
        assert_ok!(SubtensorModule::do_swap_basket(
            owner,
            fund.hotkey,
            NetUid::ROOT,
            row,
            spend,
            0
        ));
        let bought = escrow_alpha(&fund.hotkey, row) - alpha_before;
        let now = System::block_number();
        let booked = SubtensorModule::basket_row_cost_basis_at(&fund.hotkey, row, now);
        assert!(booked > 0, "cheap alpha carries a positive correction: {booked}");
        let cash_nav_after_buy =
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64();

        // Sell half of what was bought: half the correction is released.
        assert_ok!(SubtensorModule::do_swap_basket(
            owner,
            fund.hotkey,
            row,
            NetUid::ROOT,
            bought / 2,
            0
        ));
        let held_after_buy = alpha_before + bought;
        let expected_half = booked - booked * (bought / 2) as i128 / held_after_buy as i128;
        let after_half_sell = SubtensorModule::basket_row_cost_basis_at(&fund.hotkey, row, now);
        assert!(
            after_half_sell.abs_diff(expected_half) <= 2,
            "pro-rata release: {after_half_sell} vs {expected_half}"
        );

        // Sell the rest of the purchase. The release is pro-rata to the row, not lot-tracked,
        // so what stays is the pre-existing alpha's share of the correction (the row is a
        // single position; the remaining alpha is worth the same anchored value either way).
        assert_ok!(SubtensorModule::do_swap_basket(
            owner,
            fund.hotkey,
            row,
            NetUid::ROOT,
            bought - bought / 2,
            0
        ));
        let after_full = SubtensorModule::basket_row_cost_basis_at(&fund.hotkey, row, now);
        let pre_existing_share = booked * alpha_before as i128 / held_after_buy as i128;
        assert!(
            after_full <= pre_existing_share + 2 && after_full >= 0,
            "round trip releases the purchase's share: {after_full} of {booked} (pre-existing share {pre_existing_share})"
        );

        // The cash claim pays no more than before the round trip (fees make it less).
        let entitlement_after = SubtensorModule::basket_payout_from(
            SubtensorModule::get_basket_owed_shares(&fund.hotkey, &claimant),
            SubtensorModule::get_validator_basket_cash_mark_nav_tao(&fund.hotkey).to_u64(),
            fund_shares(&fund.hotkey),
        );
        assert!(
            entitlement_after <= entitlement_before,
            "no phantom value: {entitlement_after} vs {entitlement_before} (mid-trip NAV {cash_nav_after_buy})"
        );
        next_block();
        BasketCashClaimBucket::<Test>::remove(fund.hotkey);
        let before = root_stake_of(&fund.hotkey, &claimant);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(claimant),
            fund.hotkey
        ));
        assert!(root_stake_of(&fund.hotkey, &claimant) - before <= entitlement_before);
    });
}

/// Spec 468 ships the cash path dark: with the default cap (zero) a cash-rich fund is not
/// cash-ready, `claim_root_with_hotkey` declares today's full envelope, and the claim
/// redeems pro-rata exactly as on spec 467. Governance opens the path with
/// `sudo_set_basket_cash_claim_cap`.
#[test]
fn cash_path_ships_dark_by_default() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[1, 199], CASH);
        BasketCashClaimCap::<Test>::put(crate::DEFAULT_BASKET_CASH_CLAIM_CAP);
        assert_eq!(crate::DEFAULT_BASKET_CASH_CLAIM_CAP, 0);
        assert!(!SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert_eq!(declared(fund.hotkey), full_envelope());
        let alpha_before = escrow_alpha(&fund.hotkey, fund.netuid);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.stakers[0]),
            fund.hotkey
        ));
        assert!(cash_claim_events().is_empty(), "no cash claim");
        assert!(
            escrow_alpha(&fund.hotkey, fund.netuid) < alpha_before,
            "pro-rata redemption"
        );

        // Governance opens it at the sized cap.
        BasketCashClaimCap::<Test>::put(crate::RECOMMENDED_BASKET_CASH_CLAIM_CAP);
        assert!(SubtensorModule::root_claim_cash_ready(&fund.hotkey));
        assert!(declared(fund.hotkey).all_lt(full_envelope()));
    });
}
