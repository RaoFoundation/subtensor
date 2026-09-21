//! Beta basket: dust rows are not sold on root claims (spec 468).
//!
//! A claim redeems its pro-rata slice of every fund row. Two floors keep it from selling
//! rows that cost a swap each for a rounding-sized amount: a row whose whole holding is
//! worth less than `min(BasketClaimRowDustCapTao, BasketClaimRowDustBps × anchored NAV)`
//! (1 TAO cap, 0.1% of NAV), or whose slice for this claimant is worth less than
//! `BasketClaimSliceDustTao` (0.0001 TAO), both at the anchored mark, is left in the fund —
//! provided the claimant's slice of it is at most `BasketClaimForfeitCapTao` (0.01 TAO).
//! The claim burns the whole entitlement, so the skipped slices — each at most the cap —
//! stay with the remaining holders. No price enters the share accounting: the mark only
//! decides whether a slice is sold. Zero floors restore the pre-468 behaviour.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use crate::RootClaimableThreshold;
use crate::migrations::migrate_seed_beta_basket::{
    SeedBetaBasketV2Migration, SeedBetaBasketV2Progress,
};
use crate::tests::claim_root::{
    escrow_alpha, fund_shares, register_on_root, root_stake_of, zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::weights::WeightInfo;
use crate::{
    BasketClaimForfeitCapTao, BasketClaimRowDustBps, BasketClaimRowDustCapTao,
    BasketClaimSliceDustTao, BasketRate, BasketShares, DEFAULT_BASKET_CLAIM_FORFEIT_CAP_TAO,
    DEFAULT_BASKET_CLAIM_ROW_DUST_BPS, DEFAULT_BASKET_CLAIM_ROW_DUST_CAP_TAO,
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
    assert_eq!(
        BasketClaimForfeitCapTao::<Test>::get(),
        DEFAULT_BASKET_CLAIM_FORFEIT_CAP_TAO
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
/// 1_025 TAO fund is more — is skipped only when the claimant's slice of it is within the
/// forfeit cap. Alice owns half the fund: her 0.25 TAO slice of the 0.5 TAO row is above
/// the 0.01 TAO cap, so the row is sold like any other. Carol, with 1% of the fund, has a
/// 0.005 TAO slice of the same row: skipped, and the whole entitlement is burned. With the
/// cap at zero the row rule never skips.
#[test]
fn row_under_one_tao_is_skipped_only_within_the_forfeit_cap() {
    new_test_ext(1).execute_with(|| {
        // Alice 50%, Bob 49%, Carol 1%.
        let fund = setup_fund(&[5 * TAO, TAO / 2, 1_020 * TAO], &[5_000, 4_900, 100]);
        assert_eq!(
            BasketClaimRowDustCapTao::<Test>::get(),
            DEFAULT_BASKET_CLAIM_ROW_DUST_CAP_TAO
        );
        assert_eq!(
            BasketClaimRowDustBps::<Test>::get(),
            DEFAULT_BASKET_CLAIM_ROW_DUST_BPS
        );
        assert_eq!(DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO, 100_000);
        assert_eq!(DEFAULT_BASKET_CLAIM_FORFEIT_CAP_TAO, 10_000_000);
        assert_eq!(
            SubtensorModule::basket_claim_row_dust_floor(nav(&fund)),
            TAO,
            "0.1% of the NAV is above the 1 TAO cap, so the cap binds"
        );
        let small_row = fund.netuids[1];
        let (alice, carol) = (fund.stakers[0], fund.stakers[2]);

        // (a) Alice's 0.25 TAO slice of the 0.5 TAO row exceeds the forfeit cap: sold.
        let small_before = escrow_alpha(&fund.hotkey, small_row);
        let alice_root_before = root_stake_of(&fund.hotkey, &alice);
        let post =
            SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(alice), fund.hotkey)
                .expect("claim runs");
        assert_eq!(post.pays_fee, Pays::Yes);
        assert_eq!(
            escrow_alpha(&fund.hotkey, small_row),
            small_before - small_before / 2,
            "a 0.25 TAO slice is never left behind, however small the row"
        );
        assert!(dust_skipped_events().is_empty());
        let paid = root_stake_of(&fund.hotkey, &alice) - alice_root_before;
        assert!(paid > 505 * TAO && paid < 513 * TAO, "paid {paid}");
        assert_eq!(owed(&fund, &alice), 0);

        // (b) Carol's slice of what is left of that row is ≈ 0.25 TAO × 1/50 = 0.005 TAO:
        // under the cap and on a sub-1-TAO row, so it is skipped; her entitlement is burned.
        System::reset_events();
        let small_before = escrow_alpha(&fund.hotkey, small_row);
        let shares_before = fund_shares(&fund.hotkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(carol),
            fund.hotkey
        ));
        assert_eq!(
            escrow_alpha(&fund.hotkey, small_row),
            small_before,
            "row left whole"
        );
        let events = dust_skipped_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].coldkey, carol);
        assert_eq!(events[0].rows, 1);
        let est = events[0].forfeited_tao_est;
        assert!(est > 4_900_000 && est <= 5_000_000, "forfeited est {est}");
        assert_eq!(fund_shares(&fund.hotkey), shares_before - 100 * SHARE);
        assert_eq!(owed(&fund, &carol), 0);

        // (c) Cap at zero: the row rule never skips. Rebuild Carol's position and claim again.
        BasketClaimForfeitCapTao::<Test>::put(0);
        BasketShares::<Test>::mutate(fund.hotkey, |p| *p += 100 * SHARE);
        crate::BasketClaimed::<Test>::mutate(fund.hotkey, carol, |c| *c -= i128::from(100 * SHARE));
        assert_eq!(owed(&fund, &carol), 100 * SHARE);
        System::reset_events();
        let small_before = escrow_alpha(&fund.hotkey, small_row);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(carol),
            fund.hotkey
        ));
        assert!(
            escrow_alpha(&fund.hotkey, small_row) < small_before,
            "cap 0: the sub-1-TAO row is sold"
        );
        assert!(dust_skipped_events().is_empty());
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

/// A zero forfeit cap is a hard off-switch for both rules, even for a slice whose anchored
/// value rounds to zero while its live entitlement is positive (auditor `484ac8f8`): the row
/// is sold. With the default cap the same slice is dust.
#[test]
fn zero_cap_sells_a_slice_whose_anchored_value_rounds_to_zero() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[20 * TAO, 5 * TAO], &[1, 99_999]);
        let alice = fund.stakers[0];
        let row = fund.netuids[1];
        // The row's fast anchor sits far below the live quote: anchored value 5 rao, so
        // Alice's 1/100_000 slice rounds to zero there while it is 0.00005 TAO live.
        SubnetFastMovingPrice::<Test>::insert(row, U64F64::from_num(0.000_000_001));
        let live = SubtensorModule::realizable_tao_for_alpha(row, 5 * TAO);
        let anchored = SubtensorModule::anchored_basket_holding_value(row, 5 * TAO, live);
        assert!(anchored <= 5, "anchored {anchored}");
        assert_eq!(
            SubtensorModule::basket_payout_from(SHARE, anchored, 100_000 * SHARE),
            0
        );
        assert!(SubtensorModule::basket_payout_from(SHARE, live, 100_000 * SHARE) > 0);

        // Default cap: the zero-valued slice is dust.
        assert!(SubtensorModule::basket_row_is_claim_dust(
            anchored,
            SHARE,
            100_000 * SHARE,
            0,
            DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO,
            DEFAULT_BASKET_CLAIM_FORFEIT_CAP_TAO,
        ));
        // Cap 0: never dust.
        assert!(!SubtensorModule::basket_row_is_claim_dust(
            anchored,
            SHARE,
            100_000 * SHARE,
            0,
            DEFAULT_BASKET_CLAIM_SLICE_DUST_TAO,
            0,
        ));

        BasketClaimForfeitCapTao::<Test>::put(0);
        let before = escrow_alpha(&fund.hotkey, row);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            fund.hotkey
        ));
        assert!(
            escrow_alpha(&fund.hotkey, row) < before,
            "cap 0: the row is sold"
        );
        assert!(dust_skipped_events().is_empty());
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

/// The claim preview applies the same dust rules as the claim: `redeemable_tao` excludes the
/// skipped slices, `accrued_tao` keeps the full entitlement, and `rows_to_sell` counts only
/// the rows the claim will sell. The claim then pays exactly what the preview said.
#[test]
fn claim_preview_matches_the_claim_on_mixed_dust() {
    new_test_ext(1).execute_with(|| {
        // Small staker owns 1/100_000: 20 TAO → 0.0002 (sold), 5 TAO → 0.00005 (dust),
        // 200 TAO → 0.002 (sold).
        let fund = setup_fund(&[20 * TAO, 5 * TAO, 200 * TAO], &[1, 99_999]);
        let small = fund.stakers[0];
        let preview = SubtensorModule::get_basket_claim_preview(&fund.hotkey, &small)
            .expect("owed shares ⇒ a preview");
        assert_eq!(preview.owed_shares, SHARE);
        assert_eq!(preview.rows, 3);
        assert_eq!(preview.rows_to_sell, 2);
        assert_eq!(preview.dust_rows, 1);
        let accrued = preview.accrued_tao.to_u64();
        let redeemable = preview.redeemable_tao.to_u64();
        let forfeited = preview.forfeited_tao_est.to_u64();
        assert!(
            accrued > 2_240_000 && accrued <= 2_250_000,
            "accrued {accrued}"
        );
        assert!(
            forfeited > 49_000 && forfeited <= 50_000,
            "forfeited {forfeited}"
        );
        // Per-row floors: the parts can differ from the whole by up to one rao per row.
        assert!(
            accrued.abs_diff(redeemable + forfeited) <= 3,
            "{accrued} vs {redeemable}+{forfeited}"
        );

        let previews = SubtensorModule::get_root_basket_claim_previews(&small);
        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0], preview);

        let root_before = root_stake_of(&fund.hotkey, &small);
        let post =
            SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(small), fund.hotkey)
                .expect("claim runs");
        let paid = root_stake_of(&fund.hotkey, &small) - root_before;
        assert_close(
            u128::from(paid),
            u128::from(redeemable),
            2_000,
            "paid vs preview",
        );
        assert!(post.actual_weight.is_some());
        assert!(
            SubtensorModule::get_basket_claim_preview(&fund.hotkey, &small).is_none(),
            "nothing owed after the claim"
        );
    });
}

/// An all-dust claim previews as a no-op: `redeemable_tao == 0`, `rows_to_sell == 0`, while
/// the full entitlement is still reported as accrued. The claim itself burns nothing.
#[test]
fn claim_preview_reports_an_all_dust_claim_as_a_noop() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[5 * TAO, 3 * TAO], &[1, 999_999]);
        let small = fund.stakers[0];
        let preview = SubtensorModule::get_basket_claim_preview(&fund.hotkey, &small).unwrap();
        assert_eq!(preview.redeemable_tao.to_u64(), 0);
        assert_eq!(preview.rows_to_sell, 0);
        assert_eq!(preview.dust_rows, 2);
        assert!(preview.accrued_tao.to_u64() > 7_900);
        assert!(
            preview
                .forfeited_tao_est
                .to_u64()
                .abs_diff(preview.accrued_tao.to_u64())
                <= 2
        );

        let shares_before = fund_shares(&fund.hotkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(small),
            fund.hotkey
        ));
        assert_eq!(fund_shares(&fund.hotkey), shares_before);
        assert_eq!(owed(&fund, &small), SHARE);
    });
}

/// Dust can push a claim below the threshold although the full entitlement is above it: the
/// preview's `redeemable_tao` is what the runtime compares, and the claim is a no-op.
#[test]
fn claim_preview_redeemable_is_what_the_threshold_applies_to() {
    new_test_ext(1).execute_with(|| {
        // 1/100_000 of [20, 5, 5, 5] TAO: accrued 0.00035 TAO, redeemable 0.0002 TAO (the
        // three 5 TAO slices are 0.00005 each: dust).
        let fund = setup_fund(&[20 * TAO, 5 * TAO, 5 * TAO, 5 * TAO], &[1, 99_999]);
        let small = fund.stakers[0];
        let preview = SubtensorModule::get_basket_claim_preview(&fund.hotkey, &small).unwrap();
        let accrued = preview.accrued_tao.to_u64();
        let redeemable = preview.redeemable_tao.to_u64();
        assert!(accrued > 340_000 && accrued <= 350_000, "{accrued}");
        assert!(
            redeemable > 190_000 && redeemable <= 200_000,
            "{redeemable}"
        );
        assert_eq!(preview.rows_to_sell, 1);
        assert_eq!(preview.dust_rows, 3);

        // Threshold between the two: the full entitlement clears it, the payout does not.
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(250_000));
        let shares_before = fund_shares(&fund.hotkey);
        let root_before = root_stake_of(&fund.hotkey, &small);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(small),
            fund.hotkey
        ));
        assert_eq!(
            fund_shares(&fund.hotkey),
            shares_before,
            "no-op: nothing burned"
        );
        assert_eq!(
            root_stake_of(&fund.hotkey, &small),
            root_before,
            "nothing paid"
        );
        assert_eq!(owed(&fund, &small), SHARE);

        // Below the payout the claim goes through and pays the previewed amount.
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(150_000));
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(small),
            fund.hotkey
        ));
        let paid = root_stake_of(&fund.hotkey, &small) - root_before;
        assert_close(
            u128::from(paid),
            u128::from(redeemable),
            2_000,
            "paid vs preview",
        );
    });
}

/// The preview prepares the fund exactly as the claim does — flushes the queued dividend
/// credit and consolidates the sub-threshold rows into root cash (which is never dust) —
/// under a rolled-back transaction, so `redeemable_tao` is what the claim then pays and
/// `rows` / `rows_to_sell` / `swept` / `flushed_credits` match the executed work. Without
/// that preparation the 20 tiny rows would preview as forfeited dust and the credit as
/// absent, and the preview would under-report the payout.
#[test]
fn claim_preview_flushes_and_consolidates_like_the_claim() {
    new_test_ext(1).execute_with(|| {
        // Two real rows plus 20 rows worth 0.0005 TAO each; a 10% claimant.
        let mut rows = vec![20 * TAO, 5 * TAO];
        rows.extend(core::iter::repeat_n(TAO / 2_000, 20));
        let fund = setup_fund(&rows, &[1, 9]);
        let claimant = fund.stakers[0];
        // A nonzero claim threshold (0.001 TAO) makes the tiny rows consolidatable.
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(1_000_000));
        // A queued dividend credit of 2 TAO alpha on the first row, not yet flushed.
        crate::PendingBasketDeposits::<Test>::insert(
            fund.hotkey,
            fund.netuids[0],
            AlphaBalance::from(2 * TAO),
        );
        assert!(
            SubtensorModule::is_hotkey_registered_on_network(NetUid::ROOT, &fund.hotkey),
            "flush requires a root-registered fund"
        );
        let holdings_before = SubtensorModule::get_basket_holdings(&fund.hotkey).len();
        assert_eq!(holdings_before, 22);

        let preview =
            SubtensorModule::get_basket_claim_preview(&fund.hotkey, &claimant).expect("preview");
        // The view changed nothing.
        assert_eq!(SubtensorModule::get_basket_holdings(&fund.hotkey).len(), 22);
        assert!(crate::PendingBasketDeposits::<Test>::contains_key(
            fund.hotkey,
            fund.netuids[0]
        ));
        assert_eq!(escrow_alpha(&fund.hotkey, NetUid::ROOT), 0);

        assert_eq!(preview.flushed_credits, 1);
        assert_eq!(
            preview.swept, 20,
            "the sub-threshold rows are consolidated first"
        );
        assert_eq!(
            preview.rows, 3,
            "two real rows plus the root cash they became"
        );
        assert_eq!(preview.rows_to_sell, 3);
        assert_eq!(preview.dust_rows, 0);
        let redeemable = preview.redeemable_tao.to_u64();
        // 10% of (22 + 5 + ~0.01) TAO — the flushed credit is in, nothing is forfeited.
        assert!(
            redeemable > 2_690_000_000 && redeemable < 2_710_000_000,
            "{redeemable}"
        );
        assert_eq!(preview.accrued_tao, preview.redeemable_tao);

        let root_before = root_stake_of(&fund.hotkey, &claimant);
        let post =
            SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(claimant), fund.hotkey)
                .expect("claim runs");
        let paid = root_stake_of(&fund.hotkey, &claimant) - root_before;
        assert_close(
            u128::from(paid),
            u128::from(redeemable),
            2_000,
            "paid vs preview",
        );
        assert!(post.actual_weight.is_some());
        // The executed claim left the fund as the preview described it.
        assert_eq!(SubtensorModule::get_basket_holdings(&fund.hotkey).len(), 3);
        assert!(!crate::PendingBasketDeposits::<Test>::contains_key(
            fund.hotkey,
            fund.netuids[0]
        ));
        assert!(dust_skipped_events().is_empty());
    });
}

/// A valuation that fails mid-plan (one row on a pool priced below the swap's minimum, an
/// unknown swap error) aborts the admitted claim; the refund must still charge the rows
/// scanned up to and including that one, plus the admission scan (skeptic `c8404e68`).
#[test]
fn failed_valuation_refund_charges_the_rows_it_scanned() {
    new_test_ext(1).execute_with(|| {
        let fund = setup_fund(&[20 * TAO, 5 * TAO, 5 * TAO, 5 * TAO], &[1, 9]);
        let claimant = fund.stakers[0];
        // Make the last row's pool quote below the swap's minimum price (1_000 rao TAO
        // reserve against a 1_000 TAO alpha reserve): the sell simulation fails with
        // `PriceLimitExceeded`, which the valuation does not classify as terminal.
        let broken = fund.netuids[3];
        SubnetTAO::<Test>::insert(broken, TaoBalance::from(1_000u64));
        SubnetAlphaIn::<Test>::insert(broken, AlphaBalance::from(1_000 * TAO));
        assert!(SubtensorModule::try_realizable_tao_for_alpha(broken, 5 * TAO).is_err());

        let err =
            SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(claimant), fund.hotkey)
                .expect_err("the valuation error aborts the claim");
        let charged = err
            .post_info
            .actual_weight
            .expect("admitted failures report actual weight");
        let scanned_all = crate::staking::RootClaimOutcome {
            tao: 0,
            rows: 4,
            realized: 0,
            swept: 0,
            flush: Default::default(),
        };
        let floor = SubtensorModule::root_claim_admission_weight(crate::MAX_ROOT_CLAIM_HOTKEY_WORK)
            .saturating_add(SubtensorModule::root_claim_actual_weight(
                1,
                0,
                &scanned_all,
            ));
        assert!(
            charged.all_gte(floor),
            "refund {charged:?} must charge admission + 4 row scans {floor:?}"
        );
        assert!(charged.all_lt(SubtensorModule::root_claim_hotkey_declared_weight()));
        // Nothing moved.
        assert_eq!(owed(&fund, &claimant), SHARE);
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuids[0]), 20 * TAO);
    });
}
