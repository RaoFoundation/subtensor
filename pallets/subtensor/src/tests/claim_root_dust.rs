//! Beta basket: dust rows are not sold on root claims (spec 468).
//!
//! A claim redeems its pro-rata slice of every fund row. Two floors keep it from selling
//! rows that cost a swap each for a rounding-sized amount: a row whose whole holding is
//! worth less than `min(BasketClaimRowDustCapTao, BasketClaimRowDustBps × guarded NAV)`
//! (1 TAO cap, 0.1% of NAV), or whose slice for this claimant is worth less than
//! `BasketClaimSliceDustTao` (0.001 TAO), both at the guarded mark, is left in the fund.
//! Nothing is forfeited: the claim burns only the shares matching what it redeemed, so the
//! claimant keeps the shares for the skipped slices and redeems them in a later, larger
//! claim, and NAV per share is unchanged for everyone else. Zero floors restore the
//! pre-468 behaviour.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use crate::tests::claim_root::{
    escrow_alpha, fund_shares, register_on_root, root_stake_of, zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::weights::WeightInfo;
use crate::{
    BasketClaimRowDustBps, BasketClaimRowDustCapTao, BasketClaimSliceDustTao, BasketClaimed,
    BasketRate, BasketShares, DEFAULT_BASKET_CLAIM_ROW_DUST_BPS,
    DEFAULT_BASKET_CLAIM_ROW_DUST_CAP_TAO, DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO, Event,
    SubnetAlphaIn, SubnetFastMovingPrice, SubnetMovingPrice, SubnetTAO,
};
use frame_support::assert_ok;
use frame_support::dispatch::{GetDispatchInfo, Pays};
use sp_core::U256;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

const TAO: u64 = 1_000_000_000;
/// Fund shares per unit of `stakes` in [`setup_fund`]: enough resolution that a retained
/// fraction of a claim is a whole number of shares.
const SHARE: u64 = 1_000_000;

struct Fund {
    hotkey: U256,
    /// One subnet per row, in `rows` order.
    netuids: Vec<NetUid>,
    /// Root stakers, one per `stakes` entry; owed shares equal `stakes[i] × SHARE`.
    stakers: Vec<U256>,
}

/// A 100k-TAO balanced pool: rows of hundreds of TAO sell with slippage well under 1%, so
/// a row's realizable value is its alpha to within that.
fn deep_pool(netuid: NetUid) {
    let tao = TaoBalance::from(100_000 * TAO);
    SubnetTAO::<Test>::insert(netuid, tao);
    SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(100_000 * TAO));
    if let Some(subnet_account) = SubtensorModule::get_subnet_account_id(netuid) {
        add_balance_to_coldkey_account(&subnet_account, tao);
    }
}

/// A root validator whose fund holds `rows[i]` alpha on a deep pool priced at 1 (spot, fast
/// anchor and slow EMA), so a row's realizable and anchored values are both about `rows[i]`.
/// `stakes` root stakers each hold `stakes[i] × SHARE` fund shares (`BasketRate` = 1,
/// shares outstanding = the sum), so staker `i`'s fraction is `stakes[i] / Σ stakes`.
fn setup_fund(rows: &[u64], stakes: &[u64]) -> Fund {
    let hotkey = U256::from(1002);
    let escrow = SubtensorModule::get_beta_escrow_account_id();
    zero_claim_threshold();
    // The mock starts with the floors off (see `new_test_ext`); these tests run at the
    // chain defaults unless they say otherwise.
    set_dust_floors(
        DEFAULT_BASKET_CLAIM_ROW_DUST_CAP_TAO,
        DEFAULT_BASKET_CLAIM_ROW_DUST_BPS,
        DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO,
    );
    register_on_root(&hotkey, 0);

    let netuids: Vec<NetUid> = rows
        .iter()
        .enumerate()
        .map(|(i, alpha)| {
            let subnet_owner = U256::from(20_000 + 2 * i as u32);
            let subnet_hotkey = U256::from(20_001 + 2 * i as u32);
            // The lock cost doubles per same-block registration; pin it so 125 subnets fit.
            SubtensorModule::set_network_last_lock(TaoBalance::from(TAO));
            let netuid = add_dynamic_network(&subnet_hotkey, &subnet_owner);
            remove_owner_registration_stake(netuid);
            deep_pool(netuid);
            SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(1));
            SubnetFastMovingPrice::<Test>::insert(netuid, U64F64::from_num(1));
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &escrow,
                netuid,
                (*alpha).into(),
            );
            netuid
        })
        .collect();

    let stakers: Vec<U256> = stakes
        .iter()
        .enumerate()
        .map(|(i, stake)| {
            let staker = U256::from(5_000 + i as u32);
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &staker,
                NetUid::ROOT,
                (*stake * SHARE).into(),
            );
            staker
        })
        .collect();
    BasketShares::<Test>::insert(hotkey, stakes.iter().sum::<u64>() * SHARE);
    BasketRate::<Test>::insert(hotkey, I96F32::from_num(1));
    for (staker, stake) in stakers.iter().zip(stakes) {
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, staker),
            *stake * SHARE
        );
    }
    Fund {
        hotkey,
        netuids,
        stakers,
    }
}

fn set_dust_floors(row_cap: u64, row_bps: u16, slice: u64) {
    BasketClaimRowDustCapTao::<Test>::put(row_cap);
    BasketClaimRowDustBps::<Test>::put(row_bps);
    BasketClaimSliceDustTao::<Test>::put(slice);
}

struct Skipped {
    coldkey: U256,
    rows: u32,
    retained_shares: u64,
    retained_tao_est: u64,
}

fn dust_skipped_events() -> Vec<Skipped> {
    System::events()
        .iter()
        .filter_map(|record| match &record.event {
            RuntimeEvent::SubtensorModule(Event::BasketClaimDustSkipped {
                coldkey,
                rows,
                retained_shares,
                retained_tao_est,
                ..
            }) => Some(Skipped {
                coldkey: *coldkey,
                rows: *rows,
                retained_shares: *retained_shares,
                retained_tao_est: retained_tao_est.to_u64(),
            }),
            _ => None,
        })
        .collect()
}

fn claim_call(hotkey: U256) -> RuntimeCall {
    RuntimeCall::SubtensorModule(crate::Call::claim_root_with_hotkey { hotkey })
}

fn owed(fund: &Fund, who: &U256) -> u64 {
    SubtensorModule::get_basket_owed_shares(&fund.hotkey, who)
}

fn nav(fund: &Fund) -> u64 {
    SubtensorModule::get_validator_basket_nav_tao(&fund.hotkey).to_u64()
}

/// Realizable NAV per share, scaled by 1e9 for integer comparison.
fn nav_per_share_e9(fund: &Fund) -> u128 {
    u128::from(nav(fund)) * 1_000_000_000 / u128::from(fund_shares(&fund.hotkey))
}

/// What `who` could take out of the fund right now at the pre-sale quote: owed × NAV / P.
fn entitlement_tao(fund: &Fund, who: &U256) -> u64 {
    SubtensorModule::basket_payout_from(owed(fund, who), nav(fund), fund_shares(&fund.hotkey))
}

fn assert_close(a: u128, b: u128, rel_ppm: u128, what: &str) {
    let diff = a.abs_diff(b);
    assert!(
        diff * 1_000_000 <= a.max(b) * rel_ppm,
        "{what}: {a} vs {b} differ by more than {rel_ppm} ppm"
    );
}

/// (1) A row whose whole holding is under the row floor — 1 TAO here, since 0.1% of this
/// 1_025 TAO fund is more — is not sold: its alpha stays in the fund, every other row is
/// redeemed pro-rata, the claim burns only the shares it redeemed and reports what the
/// claimant still owns.
#[test]
fn row_under_one_tao_is_skipped() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[5 * TAO, TAO / 2, 1_020 * TAO], &[1_000, 1_000]);
        assert_eq!(
            BasketClaimRowDustCapTao::<Test>::get(),
            DEFAULT_BASKET_CLAIM_ROW_DUST_CAP_TAO
        );
        assert_eq!(
            BasketClaimRowDustBps::<Test>::get(),
            DEFAULT_BASKET_CLAIM_ROW_DUST_BPS
        );
        let guarded_nav = SubtensorModule::get_validator_basket_guarded_nav_tao(&fund.hotkey);
        assert!(guarded_nav.to_u64() > 1_000 * TAO);
        assert_eq!(
            SubtensorModule::basket_claim_row_dust_floor(guarded_nav.to_u64()),
            TAO,
            "0.1% of the NAV is above the 1 TAO cap, so the cap binds"
        );
        let [big, dust, bigger] = [fund.netuids[0], fund.netuids[1], fund.netuids[2]];
        let alice = fund.stakers[0];
        let before = |n: NetUid| escrow_alpha(&fund.hotkey, n);
        let (big_before, dust_before, bigger_before) = (before(big), before(dust), before(bigger));
        let root_before = root_stake_of(&fund.hotkey, &alice);
        let shares_before = fund_shares(&fund.hotkey);
        let owed_before = owed(&fund, &alice);
        let nav_before = nav(&fund);

        let post =
            SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(alice), fund.hotkey)
                .expect("claim runs");
        assert_eq!(post.pays_fee, Pays::Yes);

        assert_eq!(before(dust), dust_before, "the sub-1-TAO row is left whole");
        assert_eq!(
            before(big),
            big_before - big_before / 2,
            "5 TAO row: half sold"
        );
        assert_eq!(before(bigger), bigger_before - bigger_before / 2);
        let paid = root_stake_of(&fund.hotkey, &alice) - root_before;
        // Half of 1_025 TAO less the pool's slippage (about 0.5% on the big row); nothing
        // from the skipped row.
        assert!(
            paid > 505 * TAO && paid < 512 * TAO + TAO / 2,
            "paid {paid}"
        );

        // Burned = owed × (NAV − 0.5 TAO) / NAV, rounded up; the rest stays owed.
        let dust_value = SubtensorModule::realizable_tao_for_alpha(dust, dust_before);
        let expected_burn =
            SubtensorModule::mul_div_u64_ceil(owed_before, nav_before - dust_value, nav_before);
        assert_eq!(fund_shares(&fund.hotkey), shares_before - expected_burn);
        let retained = owed(&fund, &alice);
        assert_eq!(retained, owed_before - expected_burn);
        assert!(retained > 0, "the skipped slice stays owed as shares");
        assert!(
            retained > owed_before / 2_100 && retained < owed_before / 2_000,
            "retained {retained} ≈ owed × 0.5 / 1025.5"
        );
        let events = dust_skipped_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].coldkey, alice);
        assert_eq!(events[0].rows, 1);
        assert_eq!(events[0].retained_shares, retained);
        // Alice's half of the 0.5 TAO row, at the pre-sale quote.
        let est = events[0].retained_tao_est;
        assert!(
            est > TAO / 4 - 2_000 && est <= TAO / 4,
            "retained est {est}"
        );
    });
}

/// The row floor scales with the fund: on a 25 TAO fund it is 0.025 TAO, so a 0.5 TAO row
/// that a fixed 1 TAO floor would skip is redeemed, and only a row under 0.025 TAO is dust.
#[test]
fn row_floor_is_relative_for_small_funds() {
    new_test_ext(1).execute_with(|| {
        // 20 + 5 + 0.5 + 0.02 TAO ≈ 25.5 TAO ⇒ floor = 0.0255 TAO.
        let fund = setup_fund(&[20 * TAO, 5 * TAO, TAO / 2, TAO / 50], &[1_000, 1_000]);
        let guarded_nav = SubtensorModule::get_validator_basket_guarded_nav_tao(&fund.hotkey);
        let floor = SubtensorModule::basket_claim_row_dust_floor(guarded_nav.to_u64());
        assert!(floor > TAO / 40 && floor < TAO / 39, "floor {floor}");
        let alice = fund.stakers[0];
        let half_before = escrow_alpha(&fund.hotkey, fund.netuids[2]);
        let tiny_before = escrow_alpha(&fund.hotkey, fund.netuids[3]);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            fund.hotkey
        ));
        assert!(
            escrow_alpha(&fund.hotkey, fund.netuids[2]) < half_before,
            "0.5 TAO row is redeemed on a small fund"
        );
        assert_eq!(
            escrow_alpha(&fund.hotkey, fund.netuids[3]),
            tiny_before,
            "0.02 TAO row is under 0.1% of NAV"
        );
        let events = dust_skipped_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].rows, 1);

        // Either knob at zero turns the row skip off; at 100% the cap alone applies.
        set_dust_floors(TAO, 0, DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO);
        assert_eq!(
            SubtensorModule::basket_claim_row_dust_floor(guarded_nav.to_u64()),
            0
        );
        set_dust_floors(TAO, 10_000, DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO);
        assert_eq!(
            SubtensorModule::basket_claim_row_dust_floor(guarded_nav.to_u64()),
            TAO
        );
    });
}

/// (2) A row above the row floor is still skipped when this claimant's slice of it is under
/// 0.001 TAO; a larger claimant on the same fund sells it. The small claimant's slice of
/// the skipped row stays owed, so the large claimant is no longer the last holder and
/// leaves exactly that fraction of every row behind.
#[test]
fn slice_under_a_thousandth_tao_is_skipped_for_the_small_claimant_only() {
    new_test_ext(1).execute_with(|| {
        // Small staker owns 1/10_000 of the fund. Slices: 20 TAO → 0.002 (sold), 5 TAO →
        // 0.0005 (skipped), 200 TAO → 0.02 (sold).
        let fund = setup_fund(&[20 * TAO, 5 * TAO, 200 * TAO], &[1, 9_999]);
        assert_eq!(
            BasketClaimSliceDustTao::<Test>::get(),
            DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO
        );
        let small = fund.stakers[0];
        let large = fund.stakers[1];
        let mid = fund.netuids[1];
        let mid_before = escrow_alpha(&fund.hotkey, mid);
        let first_before = escrow_alpha(&fund.hotkey, fund.netuids[0]);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(small),
            fund.hotkey
        ));
        assert_eq!(
            escrow_alpha(&fund.hotkey, mid),
            mid_before,
            "0.0005 TAO slice skipped"
        );
        assert!(
            escrow_alpha(&fund.hotkey, fund.netuids[0]) < first_before,
            "0.002 TAO slice sold"
        );
        let events = dust_skipped_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].coldkey, small);
        assert_eq!(events[0].rows, 1);
        let est = events[0].retained_tao_est;
        assert!(
            est > 490_000 && est <= 500_000,
            "≈0.0005 TAO still owed, got {est}"
        );
        // About 5 / 225 of the small claimant's shares stay owed (slightly more: the sold
        // rows realize a little under their alpha).
        let retained = owed(&fund, &small);
        assert_eq!(retained, events[0].retained_shares);
        assert!(
            retained > SHARE * 5 / 226 && retained < SHARE * 5 / 222,
            "{retained}"
        );

        // The large holder sells everything it owns; nothing is dust at that size. The
        // small claimant's retained fraction of every row stays in the fund.
        System::reset_events();
        let shares_total = fund_shares(&fund.hotkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(large),
            fund.hotkey
        ));
        assert!(dust_skipped_events().is_empty());
        assert_eq!(owed(&fund, &large), 0);
        assert_eq!(fund_shares(&fund.hotkey), retained);
        for (netuid, row) in fund.netuids.iter().zip([20 * TAO, 5 * TAO, 200 * TAO]) {
            let left = escrow_alpha(&fund.hotkey, *netuid);
            assert!(
                left > 0,
                "row {netuid:?} keeps the small claimant's fraction"
            );
            // The small claimant's fraction of the row (the 5 TAO row also keeps the slice
            // the small claim itself did not sell), plus rounding.
            let bound = row * (retained + 1) / shares_total + row / 10_000 + 2;
            assert!(left <= bound, "row {netuid:?} left {left} > {bound}");
        }
    });
}

/// A non-final large claimant on a fund with a dust row sells every row above the floors
/// and skips the one below; the mid row that was dust for the small claimant is sold.
#[test]
fn slice_floor_is_per_claimant() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[20 * TAO, 5 * TAO, 200 * TAO], &[1, 4_999, 5_000]);
        let large = fund.stakers[1];
        let mid = fund.netuids[1];
        let mid_before = escrow_alpha(&fund.hotkey, mid);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(large),
            fund.hotkey
        ));
        assert!(escrow_alpha(&fund.hotkey, mid) < mid_before);
        assert!(dust_skipped_events().is_empty());
        assert_eq!(
            owed(&fund, &large),
            0,
            "no dust ⇒ the whole entitlement is burned"
        );
    });
}

/// (3) Zero floors are today's behaviour: every priced row is redeemed pro-rata, the whole
/// entitlement is burned, no event.
#[test]
fn zero_floors_redeem_every_row() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[5 * TAO, TAO / 2, 20 * TAO, 5_000], &[1, 9_999]);
        set_dust_floors(0, 0, 0);
        let small = fund.stakers[0];
        let befores: Vec<u64> = fund
            .netuids
            .iter()
            .map(|n| escrow_alpha(&fund.hotkey, *n))
            .collect();
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(small),
            fund.hotkey
        ));
        for (netuid, before) in fund.netuids.iter().zip(befores) {
            // 1/10_000 of each row (floored) is taken; the 5_000-rao row owes nothing and
            // is left alone exactly as before.
            let take = before / 10_000;
            assert_eq!(
                escrow_alpha(&fund.hotkey, *netuid),
                before - take,
                "row {netuid:?}"
            );
        }
        assert!(dust_skipped_events().is_empty());
        assert_eq!(owed(&fund, &small), 0);
    });
}

/// (a) Skipped value is retained as shares, not forfeited: the other holder's entitlement
/// and the fund's NAV per share are unchanged by the claim, and the claimant's payout plus
/// what they still own adds up to what they were owed.
#[test]
fn skipped_slices_are_retained_as_shares_not_forfeited() {
    new_test_ext(1).execute_with(|| {
        // Alice owns 1/10_000: her 20 TAO slice (0.002) is sold, her 5 TAO slice (0.0005) is
        // dust. Sales this small move a 100k pool by nothing measurable.
        let fund = setup_fund(&[20 * TAO, 5 * TAO], &[1, 9_999]);
        let alice = fund.stakers[0];
        let bob = fund.stakers[1];
        let nav_per_share_before = nav_per_share_e9(&fund);
        let bob_before = entitlement_tao(&fund, &bob);
        let alice_before = entitlement_tao(&fund, &alice);
        let alice_root_before = root_stake_of(&fund.hotkey, &alice);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            fund.hotkey
        ));
        assert_eq!(dust_skipped_events().len(), 1);

        assert_close(
            nav_per_share_e9(&fund),
            nav_per_share_before,
            100,
            "NAV per share",
        );
        assert_close(
            u128::from(entitlement_tao(&fund, &bob)),
            u128::from(bob_before),
            100,
            "the other holder's entitlement",
        );
        let paid = root_stake_of(&fund.hotkey, &alice) - alice_root_before;
        let still_owned = entitlement_tao(&fund, &alice);
        assert!(
            still_owned > 490_000 && still_owned <= 500_000,
            "{still_owned}"
        );
        assert_close(
            u128::from(paid + still_owned),
            u128::from(alice_before),
            2_000,
            "paid + retained vs owed",
        );

        // Bob claims after Alice. Nothing is dust at his size, but he is not the last
        // holder any more, so exactly Alice's retained fraction of every row stays behind.
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            fund.hotkey
        ));
        assert_eq!(owed(&fund, &bob), 0);
        assert_eq!(fund_shares(&fund.hotkey), owed(&fund, &alice));
        assert_close(
            u128::from(entitlement_tao(&fund, &alice)),
            u128::from(still_owned),
            2_000,
            "alice's retained value survives bob's claim",
        );
    });
}

/// (b) A later, larger claim redeems the retained slices once they clear the floor.
#[test]
fn later_claim_redeems_retained_slices_once_above_the_floor() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[20 * TAO, 5 * TAO, 200 * TAO], &[1, 9_999]);
        let small = fund.stakers[0];
        let mid = fund.netuids[1];
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(small),
            fund.hotkey
        ));
        let retained = owed(&fund, &small);
        assert!(retained > 0);
        assert_eq!(escrow_alpha(&fund.hotkey, mid), 5 * TAO);
        // Still dust at this size: claiming again sells nothing and burns nothing.
        System::reset_events();
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(small),
            fund.hotkey
        ));
        assert_eq!(owed(&fund, &small), retained);
        assert!(dust_skipped_events().is_empty());

        // Dividends accrue: the claimant is credited 3 × SHARE more shares (the fund grows
        // by the matching value, as a dividend deposit would). The slice of the 5 TAO row is
        // now ≈ 3e-4 × 5 TAO = 0.0015 TAO, above the floor.
        let accrued = 3 * SHARE;
        let value =
            SubtensorModule::basket_payout_from(accrued, nav(&fund), fund_shares(&fund.hotkey));
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &fund.hotkey,
            &escrow,
            fund.netuids[2],
            value.into(),
        );
        BasketShares::<Test>::mutate(fund.hotkey, |p| *p += accrued);
        BasketClaimed::<Test>::mutate(fund.hotkey, small, |c| *c -= i128::from(accrued));
        assert_eq!(owed(&fund, &small), retained + accrued);

        let mid_before = escrow_alpha(&fund.hotkey, mid);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(small),
            fund.hotkey
        ));
        assert!(
            escrow_alpha(&fund.hotkey, mid) < mid_before,
            "the retained slice is sold"
        );
        assert!(dust_skipped_events().is_empty());
        assert_eq!(
            owed(&fund, &small),
            0,
            "everything owed is redeemed and burned"
        );
    });
}

/// (c) Two claimants in one block keep the accounting honest: shares burned equal the
/// value redeemed for each, NAV per share is unchanged, the untouched holder's entitlement
/// is unchanged, and each claimant ends up with exactly their own retained slice.
#[test]
fn two_claimants_in_one_block_keep_the_accounting_honest() {
    new_test_ext(1).execute_with(|| {
        // A: 1/10_000 (5 TAO row is dust: 0.0005). B: 3/10_000 (0.0015: sold). C: the rest.
        let fund = setup_fund(&[20 * TAO, 5 * TAO, 200 * TAO], &[1, 3, 9_996]);
        let (a, b, c) = (fund.stakers[0], fund.stakers[1], fund.stakers[2]);
        let nav_per_share_before = nav_per_share_e9(&fund);
        let c_before = entitlement_tao(&fund, &c);
        let a_before = entitlement_tao(&fund, &a);
        let b_before = entitlement_tao(&fund, &b);
        let shares_before = fund_shares(&fund.hotkey);
        let a_root = root_stake_of(&fund.hotkey, &a);
        let b_root = root_stake_of(&fund.hotkey, &b);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(a),
            fund.hotkey
        ));
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(b),
            fund.hotkey
        ));

        let events = dust_skipped_events();
        assert_eq!(events.len(), 1, "only A hit the slice floor");
        assert_eq!(events[0].coldkey, a);
        let a_retained = owed(&fund, &a);
        assert_eq!(a_retained, events[0].retained_shares);
        assert!(a_retained > 0);
        assert_eq!(owed(&fund, &b), 0);

        assert_close(
            nav_per_share_e9(&fund),
            nav_per_share_before,
            100,
            "NAV per share",
        );
        assert_close(
            u128::from(entitlement_tao(&fund, &c)),
            u128::from(c_before),
            100,
            "C's entitlement",
        );
        let a_paid = root_stake_of(&fund.hotkey, &a) - a_root;
        let b_paid = root_stake_of(&fund.hotkey, &b) - b_root;
        assert_close(
            u128::from(a_paid + entitlement_tao(&fund, &a)),
            u128::from(a_before),
            200,
            "A paid + retained",
        );
        assert_close(u128::from(b_paid), u128::from(b_before), 200, "B paid");
        // Shares burned = A's owed − A's retained + all of B's.
        assert_eq!(
            shares_before - fund_shares(&fund.hotkey),
            SHARE - a_retained + 3 * SHARE
        );
        // A's retained slice of the 5 TAO row is worth what A left there.
        assert_close(
            u128::from(entitlement_tao(&fund, &a)),
            u128::from(events[0].retained_tao_est),
            2_000,
            "A's retained value",
        );
    });
}

/// A row on a young subnet: the slow EMA sits at 2% of spot (it starts at zero and takes a
/// month to reach half the price) while the fast anchor has caught up. The row must not be
/// written down to the slow mark: it is not dust at 300 TAO, the claimant is paid for it,
/// and nothing is forfeited. (The local skeptic/auditor found the slow-EMA version of this
/// forfeiting ~30% of such a claim.)
#[test]
fn young_subnet_row_is_neither_dust_nor_forfeited() {
    new_test_ext(1).execute_with(|| {
        // Alice owns 1 bp of a 1_000 TAO fund (owed ≈ 0.1 TAO).
        let fund = setup_fund(&[700 * TAO, 300 * TAO], &[1, 9_999]);
        let young = fund.netuids[1];
        SubnetMovingPrice::<Test>::insert(young, I96F32::from_num(0.02));
        let alice = fund.stakers[0];
        let bob = fund.stakers[1];
        let nav_per_share_before = nav_per_share_e9(&fund);
        let bob_before = entitlement_tao(&fund, &bob);
        let alice_before = entitlement_tao(&fund, &alice);
        let alice_root_before = root_stake_of(&fund.hotkey, &alice);
        let young_before = escrow_alpha(&fund.hotkey, young);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            fund.hotkey
        ));

        assert!(
            escrow_alpha(&fund.hotkey, young) < young_before,
            "a 300 TAO row is sold, whatever the slow EMA says"
        );
        assert!(dust_skipped_events().is_empty());
        assert_eq!(owed(&fund, &alice), 0);
        assert_close(
            nav_per_share_e9(&fund),
            nav_per_share_before,
            100,
            "NAV per share",
        );
        assert_close(
            u128::from(entitlement_tao(&fund, &bob)),
            u128::from(bob_before),
            100,
            "the other holder's entitlement",
        );
        let paid = root_stake_of(&fund.hotkey, &alice) - alice_root_before;
        assert_close(
            u128::from(paid),
            u128::from(alice_before),
            2_000,
            "paid vs owed (nothing forfeited)",
        );
    });
}

/// A same-block pump of a skipped row lifts its live quote but not its anchored value, so
/// the retained fraction does not grow: the claimant keeps the same shares as without the
/// pump, and the other holder is unaffected.
#[test]
fn pumping_a_skipped_row_does_not_grow_the_retained_shares() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[20 * TAO, 5 * TAO], &[1, 9_999]);
        let alice = fund.stakers[0];
        let bob = fund.stakers[1];
        let dust = fund.netuids[1];
        // Without a pump the 5 TAO row is 1/5 of the fund: ≈ 1/5 of the owed shares stay.
        let expected_retained = SHARE * 5 / 25;

        // Pump: the dust row's pool now quotes 3× (live realizable ≈ 15 TAO), the fast
        // anchor still says 1.0.
        SubnetTAO::<Test>::insert(dust, TaoBalance::from(300_000 * TAO));
        let live = SubtensorModule::realizable_tao_for_alpha(dust, 5 * TAO);
        assert!(live > 14 * TAO, "pump lifted the live quote: {live}");
        assert_eq!(
            SubtensorModule::anchored_basket_holding_value(dust, 5 * TAO, live),
            5 * TAO,
            "the anchored value did not move"
        );
        let bob_before = entitlement_tao(&fund, &bob);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            fund.hotkey
        ));
        let retained = owed(&fund, &alice);
        assert!(
            retained <= expected_retained + expected_retained / 100,
            "retained {retained} must not exceed the un-pumped {expected_retained}"
        );
        assert!(retained > expected_retained * 98 / 100, "{retained}");
        // Bob's entitlement at the (pumped) live quote is not reduced by Alice's claim.
        assert!(entitlement_tao(&fund, &bob) >= bob_before - 2);
    });
}

/// (4) The median finney claimant: about 0.30 TAO owed against a fund spread over 125 rows
/// whose values tail off. Under the default floors the claim sells only the rows carrying
/// the payout, the tail stays owed as shares, and payout plus the retained value add up to
/// the entitlement (less pool slippage).
#[test]
fn median_claimant_sells_far_fewer_rows() {
    new_test_ext(1).execute_with(|| {
        // Zipf-like fund of ~3_250 TAO over 125 rows: row i holds 600 / (i + 1) TAO. Every
        // row is above the 1 TAO row cap, so only the slice floor acts.
        let rows: Vec<u64> = (0..125u64).map(|i| 600 * TAO / (i + 1)).collect();
        let nav_alpha: u64 = rows.iter().sum();
        // Claimant owed 0.30 TAO ⇒ fraction f = 0.30 / nav.
        let claim = 300_000_000u64;
        let f_den = nav_alpha / claim;
        let fund = setup_fund(&rows, &[1, f_den - 1]);
        let claimant = fund.stakers[0];
        assert_eq!(fund_shares(&fund.hotkey), f_den * SHARE);

        let sold_expected: Vec<bool> = rows
            .iter()
            .map(|row| row / f_den >= DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO)
            .collect();
        let sold_count = sold_expected.iter().filter(|s| **s).count();
        assert!(
            sold_count < 70 && sold_count > 40,
            "sold {sold_count} of 125"
        );

        let entitlement = entitlement_tao(&fund, &claimant);
        let root_before = root_stake_of(&fund.hotkey, &claimant);
        let post =
            SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(claimant), fund.hotkey)
                .expect("claim runs");
        let paid = root_stake_of(&fund.hotkey, &claimant) - root_before;

        for ((netuid, row), sold) in fund.netuids.iter().zip(&rows).zip(&sold_expected) {
            let after = escrow_alpha(&fund.hotkey, *netuid);
            if *sold {
                assert!(after < *row, "row {netuid:?} should have been sold");
            } else {
                assert_eq!(after, *row, "row {netuid:?} should have been skipped");
            }
        }
        let events = dust_skipped_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].rows as usize, 125 - sold_count);
        let retained = events[0].retained_tao_est;
        // The skipped tail is well under a fifth of the entitlement, and it is still owed.
        assert!(
            retained * 5 < entitlement,
            "retained {retained} of {entitlement}"
        );
        assert_close(
            u128::from(entitlement_tao(&fund, &claimant)),
            u128::from(retained),
            20_000,
            "still owed vs reported",
        );
        assert!(
            paid + retained <= entitlement && paid + retained > entitlement * 99 / 100,
            "paid {paid} + retained {retained} vs entitlement {entitlement}"
        );
        // Post-dispatch weight prices the skipped rows as scans, not redemptions.
        let actual = post.actual_weight.expect("claim reports actual weight");
        let declared = claim_call(fund.hotkey).get_dispatch_info().call_weight;
        assert!(actual.all_lt(declared));
    });
}

/// (5) The declared weight is the same envelope as before: the floors change what a claim
/// does, not what it reserves. Both calls declare their fixed envelopes regardless of state.
#[test]
fn declared_weight_is_the_unchanged_envelope() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1002);
        let single = claim_call(hotkey).get_dispatch_info().call_weight;
        let wide = RuntimeCall::SubtensorModule(crate::Call::claim_root {
            subnets: Default::default(),
        })
        .get_dispatch_info()
        .call_weight;
        let extension = <Test as crate::Config>::WeightInfo::check_coldkey_swap_extension();
        assert_eq!(
            single,
            SubtensorModule::root_claim_hotkey_declared_weight().saturating_add(extension)
        );
        assert_eq!(
            wide,
            SubtensorModule::root_claim_declared_weight().saturating_add(extension)
        );

        // Neither a fund's state nor the floors move the declaration.
        let fund = setup_fund(&[5 * TAO, TAO / 2], &[1, 1]);
        assert_eq!(
            claim_call(fund.hotkey).get_dispatch_info().call_weight,
            single
        );
        set_dust_floors(0, 0, 0);
        assert_eq!(
            claim_call(fund.hotkey).get_dispatch_info().call_weight,
            single
        );
    });
}

/// A claim whose every row is dust for this claimant is a no-op: nothing sold, nothing
/// burned — the claimant keeps accruing and claims later.
#[test]
fn all_dust_claim_is_a_noop_that_burns_nothing() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[5 * TAO, 3 * TAO], &[1, 99_999]);
        let small = fund.stakers[0];
        let shares_before = fund_shares(&fund.hotkey);
        let root_before = root_stake_of(&fund.hotkey, &small);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(small),
            fund.hotkey
        ));
        assert_eq!(fund_shares(&fund.hotkey), shares_before);
        assert_eq!(root_stake_of(&fund.hotkey, &small), root_before);
        assert_eq!(owed(&fund, &small), SHARE);
        assert!(dust_skipped_events().is_empty());
    });
}

/// The root cash slot is TAO 1:1 with no swap: it is never treated as dust, however small
/// the slice.
#[test]
fn root_cash_slot_is_never_dust() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[5 * TAO], &[1, 9_999]);
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &fund.hotkey,
            &escrow,
            NetUid::ROOT,
            (5 * TAO).into(),
        );
        let small = fund.stakers[0];
        let root_before = root_stake_of(&fund.hotkey, &small);
        let cash_before = escrow_alpha(&fund.hotkey, NetUid::ROOT);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(small),
            fund.hotkey
        ));
        // 1/10_000 of 5 TAO cash = 0.0005 TAO: below the slice floor, still paid.
        assert_eq!(
            escrow_alpha(&fund.hotkey, NetUid::ROOT),
            cash_before - 500_000
        );
        assert!(root_stake_of(&fund.hotkey, &small) >= root_before + 500_000);
        // The 5 TAO alpha row's 0.0005 TAO slice is dust and stays; half the shares stay owed.
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuids[0]), 5 * TAO);
        let retained = owed(&fund, &small);
        assert!(
            retained > SHARE * 49 / 100 && retained <= SHARE / 2,
            "{retained}"
        );
    });
}

/// A failed claim is billed the work it did, not the declared envelope.
#[test]
fn failed_claim_reports_actual_weight() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1003);
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        for raw_netuid in 1..=crate::MAX_ROOT_CLAIM_HOTKEY_WORK as u16 {
            crate::AlphaV2::<Test>::insert(
                (hotkey, escrow, NetUid::from(raw_netuid)),
                share_pool::SafeFloat::from(1_u64),
            );
        }
        let err = SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(U256::from(1001)),
            hotkey,
        )
        .expect_err("too many rows");
        assert_eq!(err.error, crate::Error::<Test>::RootClaimTooHeavy.into());
        let charged = err
            .post_info
            .actual_weight
            .expect("failure reports actual weight");
        assert_eq!(
            charged,
            SubtensorModule::root_claim_precheck_weight(crate::MAX_ROOT_CLAIM_HOTKEY_WORK)
        );
        assert!(charged.all_lt(SubtensorModule::root_claim_hotkey_declared_weight()));
    });
}
