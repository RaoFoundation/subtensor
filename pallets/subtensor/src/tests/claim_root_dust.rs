//! Beta basket: dust rows are not sold on root claims (spec 468).
//!
//! A claim redeems its pro-rata slice of every fund row. Two floors keep it from selling
//! rows that cost a swap each for a rounding-sized amount: a row whose whole holding is
//! worth less than `min(BasketClaimRowDustCapTao, BasketClaimRowDustBps × anchored NAV)`
//! (1 TAO cap, 0.1% of NAV), or whose slice for this claimant is worth less than
//! `BasketClaimSliceDustTao` (0.0001 TAO), both at the anchored mark, is left in the fund.
//! The claim burns the whole entitlement, so the skipped slices — each below the floor —
//! stay with the remaining holders. No price enters the share accounting: the mark only
//! decides whether a slice is sold. Zero floors restore the pre-468 behaviour.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use crate::migrations::migrate_seed_beta_basket::{
    SeedBetaBasketV2Migration, SeedBetaBasketV2Progress,
};
use crate::tests::claim_root::{
    escrow_alpha, fund_shares, register_on_root, root_stake_of, zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::weights::WeightInfo;
use crate::{
    BasketClaimRowDustBps, BasketClaimRowDustCapTao, BasketClaimSliceDustTao, BasketRate,
    BasketShares, DEFAULT_BASKET_CLAIM_ROW_DUST_BPS, DEFAULT_BASKET_CLAIM_ROW_DUST_CAP_TAO,
    DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO, Event, SubnetAlphaIn, SubnetFastMovingPrice,
    SubnetMovingPrice, SubnetTAO,
};
use frame_support::dispatch::{GetDispatchInfo, Pays};
use frame_support::{assert_ok, assert_storage_noop};
use sp_core::U256;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

const TAO: u64 = 1_000_000_000;
/// Fund shares per unit of `stakes` in [`setup_fund`].
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
    forfeited_tao_est: u64,
}

fn dust_skipped_events() -> Vec<Skipped> {
    System::events()
        .iter()
        .filter_map(|record| match &record.event {
            RuntimeEvent::SubtensorModule(Event::BasketClaimDustSkipped {
                coldkey,
                rows,
                forfeited_tao_est,
                ..
            }) => Some(Skipped {
                coldkey: *coldkey,
                rows: *rows,
                forfeited_tao_est: forfeited_tao_est.to_u64(),
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
/// redeemed pro-rata, the whole entitlement is burned and the event reports the slice left
/// behind.
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
        assert_eq!(DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO, 100_000);
        let anchored_nav: u64 = fund
            .netuids
            .iter()
            .map(|n| {
                let alpha = escrow_alpha(&fund.hotkey, *n);
                SubtensorModule::anchored_basket_holding_value(
                    *n,
                    alpha,
                    SubtensorModule::realizable_tao_for_alpha(*n, alpha),
                )
            })
            .sum();
        assert!(anchored_nav > 1_000 * TAO);
        assert_eq!(
            SubtensorModule::basket_claim_row_dust_floor(anchored_nav),
            TAO,
            "0.1% of the NAV is above the 1 TAO cap, so the cap binds"
        );
        let [big, dust, bigger] = [fund.netuids[0], fund.netuids[1], fund.netuids[2]];
        let alice = fund.stakers[0];
        let before = |n: NetUid| escrow_alpha(&fund.hotkey, n);
        let (big_before, dust_before, bigger_before) = (before(big), before(dust), before(bigger));
        let root_before = root_stake_of(&fund.hotkey, &alice);
        let shares_before = fund_shares(&fund.hotkey);

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
        assert_eq!(
            fund_shares(&fund.hotkey),
            shares_before - 1_000 * SHARE,
            "the whole entitlement is burned"
        );
        assert_eq!(owed(&fund, &alice), 0);
        let events = dust_skipped_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].coldkey, alice);
        assert_eq!(events[0].rows, 1);
        // Alice's half of the 0.5 TAO row, at the pre-sale quote, stays in the fund.
        let est = events[0].forfeited_tao_est;
        assert!(
            est > TAO / 4 - 2_000 && est <= TAO / 4,
            "forfeited est {est}"
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
        let nav = nav(&fund);
        let floor = SubtensorModule::basket_claim_row_dust_floor(nav);
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
        assert_eq!(SubtensorModule::basket_claim_row_dust_floor(nav), 0);
        set_dust_floors(TAO, 10_000, DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO);
        assert_eq!(SubtensorModule::basket_claim_row_dust_floor(nav), TAO);
    });
}

/// (2) A row above the row floor is still skipped when this claimant's slice of it is under
/// 0.0001 TAO; the final claimant on the same fund sells everything, dust included.
#[test]
fn slice_under_the_floor_is_skipped_for_the_small_claimant_only() {
    new_test_ext(1).execute_with(|| {
        // Small staker owns 1/100_000 of the fund. Slices: 20 TAO → 0.0002 (sold), 5 TAO →
        // 0.00005 (skipped), 200 TAO → 0.002 (sold).
        let fund = setup_fund(&[20 * TAO, 5 * TAO, 200 * TAO], &[1, 99_999]);
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
            "0.00005 TAO slice skipped"
        );
        assert!(
            escrow_alpha(&fund.hotkey, fund.netuids[0]) < first_before,
            "0.0002 TAO slice sold"
        );
        assert_eq!(owed(&fund, &small), 0, "the whole entitlement is burned");
        let events = dust_skipped_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].coldkey, small);
        assert_eq!(events[0].rows, 1);
        let est = events[0].forfeited_tao_est;
        assert!(
            est > 49_000 && est <= 50_000,
            "≈0.00005 TAO left behind, got {est}"
        );

        // The large holder is now the last holder: nothing is dust for them and the fund
        // fully drains, the small claimant's slice of the 5 TAO row included.
        System::reset_events();
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(large),
            fund.hotkey
        ));
        assert!(dust_skipped_events().is_empty());
        for netuid in &fund.netuids {
            assert_eq!(escrow_alpha(&fund.hotkey, *netuid), 0);
        }
        assert_eq!(fund_shares(&fund.hotkey), 0);
    });
}

/// A non-final large claimant on a fund with a dust row sells every row above the floors
/// and skips the one below; the mid row that was dust for the small claimant is sold.
#[test]
fn slice_floor_is_per_claimant() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[20 * TAO, 5 * TAO, 200 * TAO], &[1, 49_999, 50_000]);
        let large = fund.stakers[1];
        let mid = fund.netuids[1];
        let mid_before = escrow_alpha(&fund.hotkey, mid);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(large),
            fund.hotkey
        ));
        assert!(escrow_alpha(&fund.hotkey, mid) < mid_before);
        assert!(dust_skipped_events().is_empty());
        assert_eq!(owed(&fund, &large), 0);
    });
}

/// (3) Zero floors are today's behaviour: every priced row is redeemed pro-rata, no event.
#[test]
fn zero_floors_redeem_every_row() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[5 * TAO, TAO / 2, 20 * TAO, 5_000], &[1, 99_999]);
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
            // 1/100_000 of each row (floored) is taken; the 5_000-rao row owes nothing and
            // is left alone exactly as before.
            let take = before / 100_000;
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

/// (a) The skipped slice stays with the remaining holders: the other holder's entitlement
/// grows by exactly that slice (to rounding), the claimant is paid the rest, and the sum is
/// what they were owed. No share is minted or left owed.
#[test]
fn skipped_slice_stays_with_the_remaining_holders() {
    new_test_ext(1).execute_with(|| {
        // Alice owns 1/100_000: her 20 TAO slice (0.0002) is sold, her 5 TAO slice
        // (0.00005) is dust.
        let fund = setup_fund(&[20 * TAO, 5 * TAO], &[1, 99_999]);
        let alice = fund.stakers[0];
        let bob = fund.stakers[1];
        let bob_before = entitlement_tao(&fund, &bob);
        let alice_before = entitlement_tao(&fund, &alice);
        let alice_root_before = root_stake_of(&fund.hotkey, &alice);
        let shares_before = fund_shares(&fund.hotkey);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            fund.hotkey
        ));
        let events = dust_skipped_events();
        assert_eq!(events.len(), 1);
        let forfeited = events[0].forfeited_tao_est;
        assert!(forfeited > 49_000 && forfeited <= 50_000, "{forfeited}");

        assert_eq!(fund_shares(&fund.hotkey), shares_before - SHARE);
        assert_eq!(owed(&fund, &alice), 0);
        let paid = root_stake_of(&fund.hotkey, &alice) - alice_root_before;
        assert_close(
            u128::from(paid + forfeited),
            u128::from(alice_before),
            2_000,
            "paid + left behind vs owed",
        );
        let bob_gain = entitlement_tao(&fund, &bob) - bob_before;
        assert!(
            bob_gain + 2_000 >= forfeited && bob_gain <= forfeited + 2_000,
            "bob gains exactly the slice left behind: {bob_gain} vs {forfeited}"
        );
    });
}

/// (c) Two claimants in one block: each burns their whole entitlement, only the small one
/// hits the slice floor, and the untouched holder gains exactly what was left behind.
#[test]
fn two_claimants_in_one_block_keep_the_accounting_honest() {
    new_test_ext(1).execute_with(|| {
        // A: 1/100_000 (5 TAO row is dust: 0.00005). B: 3/100_000 (0.00015: sold). C: rest.
        let fund = setup_fund(&[20 * TAO, 5 * TAO, 200 * TAO], &[1, 3, 99_996]);
        let (a, b, c) = (fund.stakers[0], fund.stakers[1], fund.stakers[2]);
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
        let forfeited = events[0].forfeited_tao_est;
        assert_eq!(owed(&fund, &a), 0);
        assert_eq!(owed(&fund, &b), 0);
        assert_eq!(shares_before - fund_shares(&fund.hotkey), 4 * SHARE);

        let a_paid = root_stake_of(&fund.hotkey, &a) - a_root;
        let b_paid = root_stake_of(&fund.hotkey, &b) - b_root;
        assert_close(
            u128::from(a_paid + forfeited),
            u128::from(a_before),
            2_000,
            "A paid + left behind",
        );
        assert_close(u128::from(b_paid), u128::from(b_before), 2_000, "B paid");
        let c_gain = entitlement_tao(&fund, &c) - c_before;
        assert!(
            c_gain + 2_000 >= forfeited && c_gain <= forfeited + 2_000,
            "C gains what A left behind: {c_gain} vs {forfeited}"
        );
    });
}

/// A row on a young subnet: the slow EMA sits at 2% of spot (it starts at zero and takes a
/// month to reach half the price) while the fast anchor has caught up. At the slow mark
/// this claimant's slice would be 0.000006 TAO — dust — and the whole slice would be left
/// behind; at the fast anchor it is 0.0003 TAO and is sold.
#[test]
fn young_subnet_row_is_priced_at_the_fast_anchor_not_the_slow_ema() {
    new_test_ext(1).execute_with(|| {
        // Alice owns 1 ppm of a 1_000 TAO fund (owed ≈ 0.001 TAO).
        let fund = setup_fund(&[700 * TAO, 300 * TAO], &[1, 999_999]);
        let young = fund.netuids[1];
        SubnetMovingPrice::<Test>::insert(young, I96F32::from_num(0.02));
        let live = SubtensorModule::realizable_tao_for_alpha(young, 300 * TAO);
        assert!(
            SubtensorModule::guarded_basket_holding_value(young, 300 * TAO, live) < 7 * TAO,
            "the slow mark writes the row down to ~6 TAO"
        );
        assert!(
            SubtensorModule::anchored_basket_holding_value(young, 300 * TAO, live) > 299 * TAO,
            "the fast anchor tracks it"
        );
        let alice = fund.stakers[0];
        let alice_before = entitlement_tao(&fund, &alice);
        let alice_root_before = root_stake_of(&fund.hotkey, &alice);
        let young_before = escrow_alpha(&fund.hotkey, young);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            fund.hotkey
        ));

        assert!(
            escrow_alpha(&fund.hotkey, young) < young_before,
            "the 300 TAO row is sold, whatever the slow EMA says"
        );
        assert!(dust_skipped_events().is_empty());
        let paid = root_stake_of(&fund.hotkey, &alice) - alice_root_before;
        assert_close(
            u128::from(paid),
            u128::from(alice_before),
            5_000,
            "paid vs owed (nothing left behind)",
        );
    });
}

/// (c) A below-anchor pump of a skipped row's pool changes nothing for anyone: the dust
/// decision reads the anchored mark, the burn is the whole entitlement either way, and the
/// row's alpha stays in the fund untouched. After the pump unwinds every holder is exactly
/// where they would be without it.
#[test]
fn pumping_a_skipped_row_changes_nothing_for_anyone() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[20 * TAO, 5 * TAO], &[1, 99_999]);
        let alice = fund.stakers[0];
        let bob = fund.stakers[1];
        let dust = fund.netuids[1];
        let pool_tao_before = SubnetTAO::<Test>::get(dust);
        let bob_before = entitlement_tao(&fund, &bob);
        let alice_root_before = root_stake_of(&fund.hotkey, &alice);
        let shares_before = fund_shares(&fund.hotkey);

        // Pump: the dust row's pool now quotes 3× (live realizable ≈ 15 TAO); the fast
        // anchor still says 1.0.
        SubnetTAO::<Test>::insert(dust, TaoBalance::from(300_000 * TAO));
        let live = SubtensorModule::realizable_tao_for_alpha(dust, 5 * TAO);
        assert!(live > 14 * TAO, "pump lifted the live quote: {live}");
        assert_eq!(
            SubtensorModule::anchored_basket_holding_value(dust, 5 * TAO, live),
            5 * TAO,
            "the anchored value did not move"
        );

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            fund.hotkey
        ));
        let events = dust_skipped_events();
        assert_eq!(events.len(), 1, "still dust at the anchored mark");
        assert_eq!(escrow_alpha(&fund.hotkey, dust), 5 * TAO, "row untouched");
        assert_eq!(fund_shares(&fund.hotkey), shares_before - SHARE);
        assert_eq!(owed(&fund, &alice), 0);
        let paid = root_stake_of(&fund.hotkey, &alice) - alice_root_before;
        // Alice's 20 TAO slice only: the pumped row paid nothing.
        assert!(paid > 199_000 && paid <= 200_000, "paid {paid}");

        // Unwind the pump: bob owns the same shares of the same alpha rows as he would
        // without it — his gain is alice's un-pumped 5 TAO slice.
        SubnetTAO::<Test>::insert(dust, pool_tao_before);
        let bob_gain = entitlement_tao(&fund, &bob) - bob_before;
        assert!(
            bob_gain + 2_000 >= 50_000 && bob_gain <= 52_000,
            "bob gain {bob_gain}"
        );
    });
}

/// (4) A small finney claimant (P10 ≈ 0.015 TAO) against a fund spread over 125 rows whose
/// values tail off: the claim sells only the rows whose slice clears 0.0001 TAO, and payout
/// plus the reported forfeit add up to the entitlement (less pool slippage).
#[test]
fn small_claimant_on_a_wide_fund_sells_only_the_rows_that_carry_value() {
    new_test_ext(1).execute_with(|| {
        // Zipf-like fund of ~3_250 TAO over 125 rows: row i holds 600 / (i + 1) TAO. Every
        // row is above the 1 TAO row cap, so only the slice floor acts.
        let rows: Vec<u64> = (0..125u64).map(|i| 600 * TAO / (i + 1)).collect();
        let nav_alpha: u64 = rows.iter().sum();
        // Claimant owed 0.015 TAO ⇒ fraction f = 0.015 / nav.
        let claim = 15_000_000u64;
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
            sold_count < 40 && sold_count > 15,
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
        let forfeited = events[0].forfeited_tao_est;
        assert!(
            paid + forfeited <= entitlement && paid + forfeited > entitlement * 99 / 100,
            "paid {paid} + forfeited {forfeited} vs entitlement {entitlement}"
        );
        assert_eq!(owed(&fund, &claimant), 0);
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
/// burned, nothing left behind — the claimant keeps accruing and claims later.
#[test]
fn all_dust_claim_is_a_noop_that_burns_nothing() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[5 * TAO, 3 * TAO], &[1, 999_999]);
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
        let fund = setup_fund(&[5 * TAO], &[1, 99_999]);
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
        // 1/100_000 of 5 TAO cash = 0.00005 TAO: below the slice floor, still paid.
        assert_eq!(
            escrow_alpha(&fund.hotkey, NetUid::ROOT),
            cash_before - 50_000
        );
        assert!(root_stake_of(&fund.hotkey, &small) >= root_before + 50_000);
        // The 5 TAO alpha row's 0.00005 TAO slice is dust and stays; the entitlement is burned.
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuids[0]), 5 * TAO);
        assert_eq!(owed(&fund, &small), 0);
    });
}

/// A claim refused at admission keeps the declared envelope and touches nothing; a claim
/// that is admitted and then fails is billed the admission scan plus the work it did, and
/// less than the envelope.
#[test]
fn refund_on_failure_applies_after_admission_only() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1001);
        let escrow = SubtensorModule::get_beta_escrow_account_id();

        // Admission failure: 130 rows on the fund.
        let heavy = U256::from(1003);
        for raw_netuid in 1..=crate::MAX_ROOT_CLAIM_HOTKEY_WORK as u16 {
            crate::AlphaV2::<Test>::insert(
                (heavy, escrow, NetUid::from(raw_netuid)),
                share_pool::SafeFloat::from(1_u64),
            );
        }
        assert_storage_noop!({
            let err =
                SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(coldkey), heavy)
                    .expect_err("too many rows");
            assert_eq!(err.error, crate::Error::<Test>::RootClaimTooHeavy.into());
            assert_eq!(
                err.post_info.actual_weight, None,
                "admission failures keep the declared envelope"
            );
        });

        // Post-admission failure: the seed migration cursor is present, so the admitted
        // claim aborts before any redemption. Billed for the admission scan only.
        let fund = setup_fund(&[5 * TAO], &[1, 1]);
        SeedBetaBasketV2Migration::<Test>::put(SeedBetaBasketV2Progress::Convert {
            after: None,
            hotkey: None,
        });
        let err = SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.stakers[0]),
            fund.hotkey,
        )
        .expect_err("seed in progress");
        assert_eq!(
            err.error,
            crate::Error::<Test>::BetaBasketSeedInProgress.into()
        );
        let charged = err
            .post_info
            .actual_weight
            .expect("admitted failures report actual weight");
        let admission =
            SubtensorModule::root_claim_admission_weight(crate::MAX_ROOT_CLAIM_HOTKEY_WORK);
        assert!(charged.all_gte(admission), "{charged:?} < {admission:?}");
        assert!(charged.all_lt(SubtensorModule::root_claim_hotkey_declared_weight()));
    });
}
