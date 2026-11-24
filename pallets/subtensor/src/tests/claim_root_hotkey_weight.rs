//! `claim_root_with_hotkey` declares the weight of the validator's own fund (spec 495).
//!
//! Before spec 495 every single-hotkey claim declared one fixed envelope: 130 claim units
//! plus the coldkey-wide flush allowance (512 rows and 2,560 quotes). The declaration now
//! comes from the fund's stored escrow rows and its queued dividend credits. It stays
//! between a floor (the shape the fee allowance prices) and a ceiling (a fund of
//! `MAX_ROOT_CLAIM_HOTKEY_WORK` claim units, which is 256 subnets, the root cash slot and
//! the validator).
//!
//! A batch or proxy computes the declaration before any inner call runs. The fund's rows
//! and queued credits cannot grow in a transaction, except through a `swap_basket` buy or
//! a hotkey swap. A claim after one of those, in the same transaction, is refused.
//!
//! A claim starts with the settlement of the claimant's deferred-dividend debts. A debt can
//! outlive the queued credit that made it, so the fund's rows and queued credits do not
//! bound the debts. Each declaration has a fixed part for them: the settlement of the most
//! debts that one claimant can hold on one validator (`MAX_ROOT_CLAIM_WORK`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use crate::staking::{
    BasketFlushWork, DeferredBasketConversion, DeferredBasketDebt, RootClaimHotkeyWork,
};
use crate::tests::claim_root::{
    escrow_alpha, flush_baskets, fund_pool, fund_shares, register_on_root, root_stake_of,
    zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::weights::WeightInfo;
use crate::{
    AlphaV2, BasketClaimed, BasketDailyTurnoverCap, BasketRate, BasketRowsOpenedAt, BasketShares,
    BasketTradingEnabled, DeferredBasketConversions, DeferredBasketDebts, DeferredBasketRates,
    Error, MAX_ROOT_CLAIM_HOTKEY_WORK, MAX_ROOT_CLAIM_WORK, MIN_ROOT_CLAIM_HOTKEY_WORK,
    PendingBasketDeposits, RootClaimableThreshold, SubnetAlphaIn, SubnetAlphaOut,
    SubnetFastMovingPrice, SubnetLimit, SubnetMovingPrice, SubnetTAO,
};
use frame_support::dispatch::{DispatchClass, GetDispatchInfo};
use frame_support::weights::Weight;
use frame_support::{BoundedBTreeMap, assert_ok, assert_storage_noop};
use pallet_subtensor_utility as pallet_utility;
use sp_core::U256;
use sp_runtime::traits::Dispatchable;
use std::collections::BTreeMap;
use substrate_fixed::types::{I64F64, I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

pub(super) const TAO: u64 = 1_000_000_000;
/// Fund shares per unit of `stakes` in [`real_fund`].
pub(super) const SHARE: u64 = 1_000_000;

/// Dispatch-extension weight that FRAME folds into every declared call weight.
fn extension() -> Weight {
    <Test as crate::Config>::WeightInfo::check_coldkey_swap_extension()
}

fn claim_call(hotkey: U256) -> RuntimeCall {
    RuntimeCall::SubtensorModule(crate::Call::claim_root_with_hotkey { hotkey })
}

/// What a claim on `hotkey` declares now, without the folded extension weight.
fn declared(hotkey: U256) -> Weight {
    claim_call(hotkey)
        .get_dispatch_info()
        .call_weight
        .saturating_sub(extension())
}

/// The bound for `work`, the reads that count it, and the debt settlement bound: what a
/// fund above the floor declares.
fn sized(work: RootClaimHotkeyWork) -> Weight {
    SubtensorModule::root_claim_hotkey_weight_for_work(work.units(), work.queued)
        .saturating_add(SubtensorModule::root_claim_hotkey_sizing_weight(
            work.rows,
            work.queued,
        ))
        .saturating_add(SubtensorModule::root_claim_debt_settlement_weight_bound())
}

fn max_extrinsic() -> Weight {
    BlockWeights::get()
        .get(DispatchClass::Normal)
        .max_extrinsic
        .expect("normal extrinsics have a configured maximum")
}

/// A stored escrow row with no pool behind it. It sizes the declaration like a real row.
fn add_raw_row(hotkey: U256, netuid: u16) {
    AlphaV2::<Test>::insert(
        (
            hotkey,
            SubtensorModule::get_beta_escrow_account_id(),
            NetUid::from(netuid),
        ),
        share_pool::SafeFloat::from(1_u64),
    );
}

/// A queued dividend credit with no pool behind it.
fn queue_raw_credit(hotkey: U256, netuid: u16) {
    PendingBasketDeposits::<Test>::insert(hotkey, NetUid::from(netuid), AlphaBalance::from(1u64));
}

/// Puts the test inside user transaction `index` of the current block. The mock is
/// outside a transaction by default: the frame executive marks inherents as applied
/// before it runs the first user transaction of a block.
fn enter_transaction(index: u32) {
    System::note_inherents_applied();
    System::set_extrinsic_index(index);
}

fn this_transaction(index: u32) -> (u64, u32) {
    (SubtensorModule::get_current_block_as_u64(), index)
}

fn assert_refused_as_too_heavy(claimant: U256, hotkey: U256) {
    assert_storage_noop!({
        let err = SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(claimant), hotkey)
            .expect_err("the claim must be refused");
        assert_eq!(err.error, Error::<Test>::RootClaimTooHeavy.into());
        assert_eq!(
            err.post_info.actual_weight, None,
            "a claim refused at admission keeps its declaration"
        );
    });
}

/// The declared weight scales with the fund's real rows and queued credits.
#[test]
fn declared_weight_scales_with_the_fund_rows_and_queued_credits() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1002);
        let floor = SubtensorModule::root_claim_hotkey_min_declared_weight();
        let ceiling = SubtensorModule::root_claim_hotkey_declared_weight();
        let wide = RuntimeCall::SubtensorModule(crate::Call::claim_root {
            subnets: Default::default(),
        });
        let wide_declared = wide.get_dispatch_info().call_weight;

        // No fund: the smallest declaration.
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&hotkey),
            Some(RootClaimHotkeyWork::default())
        );
        assert_eq!(declared(hotkey), floor);
        assert_eq!(MIN_ROOT_CLAIM_HOTKEY_WORK, 4);

        // Ten rows: eleven claim units, above the floor.
        for netuid in 1..=10 {
            add_raw_row(hotkey, netuid);
        }
        let ten_rows = RootClaimHotkeyWork {
            rows: 10,
            queued: 0,
            new_rows: 0,
        };
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&hotkey),
            Some(ten_rows)
        );
        assert_eq!(ten_rows.units(), 11);
        let with_ten_rows = declared(hotkey);
        assert_eq!(with_ten_rows, sized(ten_rows));
        assert!(floor.all_lt(with_ten_rows));

        // Thirty rows declare more than ten.
        for netuid in 11..=30 {
            add_raw_row(hotkey, netuid);
        }
        let with_thirty_rows = declared(hotkey);
        assert!(with_ten_rows.all_lt(with_thirty_rows));

        // A queued credit on a subnet the fund holds adds flush work, and no claim unit.
        for netuid in 1..=5 {
            queue_raw_credit(hotkey, netuid);
        }
        let held_credits = RootClaimHotkeyWork {
            rows: 30,
            queued: 5,
            new_rows: 0,
        };
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&hotkey),
            Some(held_credits)
        );
        assert_eq!(held_credits.units(), 31);
        let with_held_credits = declared(hotkey);
        assert_eq!(with_held_credits, sized(held_credits));
        assert!(with_thirty_rows.all_lt(with_held_credits));

        // A queued credit on a subnet the fund does not hold can open a row: one more
        // claim unit for each.
        for netuid in 101..=103 {
            queue_raw_credit(hotkey, netuid);
        }
        let new_credits = RootClaimHotkeyWork {
            rows: 30,
            queued: 8,
            new_rows: 3,
        };
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&hotkey),
            Some(new_credits)
        );
        assert_eq!(new_credits.units(), 34);
        let with_new_credits = declared(hotkey);
        assert_eq!(with_new_credits, sized(new_credits));
        assert!(with_held_credits.all_lt(with_new_credits));

        // Every declaration stays under the ceiling, far under it for a fund this size.
        assert!(with_new_credits.all_lt(ceiling));
        assert!(with_new_credits.ref_time() < ceiling.ref_time() / 8);

        // The coldkey-wide claim keeps its fixed envelope.
        assert_eq!(wide.get_dispatch_info().call_weight, wide_declared);
    });
}

/// A fund above the ceiling is refused, and declares the ceiling so the refusal is paid for.
#[test]
fn fund_above_the_ceiling_is_refused_and_declares_the_ceiling() {
    new_test_ext(1).execute_with(|| {
        let claimant = U256::from(1001);
        let hotkey = U256::from(1002);
        let ceiling = SubtensorModule::root_claim_hotkey_declared_weight();
        let limit = MAX_ROOT_CLAIM_HOTKEY_WORK as u16;

        // One validator unit and `limit - 1` rows: exactly at the ceiling.
        for netuid in 1..limit {
            add_raw_row(hotkey, netuid);
        }
        let full = RootClaimHotkeyWork {
            rows: u32::from(limit) - 1,
            queued: 0,
            new_rows: 0,
        };
        assert_eq!(SubtensorModule::root_claim_hotkey_work(&hotkey), Some(full));
        assert_eq!(full.units(), MAX_ROOT_CLAIM_HOTKEY_WORK);
        assert!(SubtensorModule::root_claim_hotkey_fits_declared_budget(
            &hotkey
        ));
        assert!(declared(hotkey).all_lt(ceiling));

        // A queued credit for each of those rows adds no claim unit. This fund declares
        // exactly the ceiling.
        for netuid in 1..limit {
            queue_raw_credit(hotkey, netuid);
        }
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&hotkey),
            Some(RootClaimHotkeyWork {
                queued: u32::from(limit) - 1,
                ..full
            })
        );
        assert_eq!(declared(hotkey), ceiling);

        // One queued credit on a subnet the fund does not hold is one unit too many.
        queue_raw_credit(hotkey, limit + 100);
        assert_eq!(SubtensorModule::root_claim_hotkey_work(&hotkey), None);
        assert!(!SubtensorModule::root_claim_hotkey_fits_declared_budget(
            &hotkey
        ));
        assert_eq!(declared(hotkey), ceiling);
        assert_refused_as_too_heavy(claimant, hotkey);

        // So is one more stored row, with nothing queued.
        let rows_only = U256::from(1003);
        for netuid in 1..=limit {
            add_raw_row(rows_only, netuid);
        }
        assert_eq!(SubtensorModule::root_claim_hotkey_work(&rows_only), None);
        assert_eq!(declared(rows_only), ceiling);
        assert_refused_as_too_heavy(claimant, rows_only);
    });
}

/// A full fund (a row for every subnet, and the root cash slot) with a full queue (a
/// credit for every subnet) is admitted, fits its declaration, and fits a block. This
/// holds at the default subnet limit and at a limit of 256.
#[test]
fn full_fund_with_a_full_queue_is_admitted_and_fits_its_declaration() {
    new_test_ext(1).execute_with(|| {
        let claimant = U256::from(1001);
        let default_limit = SubnetLimit::<Test>::get();
        assert_eq!(default_limit, 128);
        for (hotkey, subnets) in [
            (U256::from(1002), default_limit),
            (U256::from(1003), 256u16),
        ] {
            for netuid in 1..=subnets {
                add_raw_row(hotkey, netuid);
                queue_raw_credit(hotkey, netuid);
            }
            add_raw_row(hotkey, 0);

            let work = RootClaimHotkeyWork {
                rows: u32::from(subnets) + 1,
                queued: u32::from(subnets),
                new_rows: 0,
            };
            assert_eq!(SubtensorModule::root_claim_hotkey_work(&hotkey), Some(work));
            assert_eq!(work.units(), u32::from(subnets) + 2);
            assert!(work.units() <= MAX_ROOT_CLAIM_HOTKEY_WORK);

            let declared_before = declared(hotkey);
            assert_eq!(declared_before, sized(work));
            assert!(
                declared_before
                    .saturating_add(extension())
                    .all_lte(max_extrinsic()),
                "{subnets} subnets: {declared_before:?} must fit a block"
            );

            let post =
                SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(claimant), hotkey)
                    .expect("a full fund with a full queue is claimable");
            let actual = post.actual_weight.expect("claim reports actual weight");
            assert!(
                actual.all_lte(declared_before),
                "{subnets} subnets: actual {actual:?} above declared {declared_before:?}"
            );
            // The claim settled the whole queue.
            assert_eq!(
                PendingBasketDeposits::<Test>::iter_key_prefix(hotkey).count(),
                0
            );
        }
    });
}

/// The largest declaration fits the largest extrinsic, and a fund at the ceiling in
/// storage declares exactly it.
#[test]
fn worst_case_at_the_ceiling_fits_the_max_extrinsic() {
    new_test_ext(1).execute_with(|| {
        assert_eq!(MAX_ROOT_CLAIM_HOTKEY_WORK, 258);
        // 256 subnets, the root cash slot, and the validator.
        assert_eq!(MAX_ROOT_CLAIM_HOTKEY_WORK, 256 + 2);

        let ceiling = SubtensorModule::root_claim_hotkey_declared_weight();
        assert!(
            ceiling.saturating_add(extension()).all_lte(max_extrinsic()),
            "ceiling {ceiling:?} exceeds max extrinsic {:?}",
            max_extrinsic()
        );
        // The ceiling is the bound for 258 claim units with 257 queued credits.
        assert_eq!(
            ceiling,
            sized(RootClaimHotkeyWork {
                rows: 257,
                queued: 257,
                new_rows: 0,
            })
        );
        assert!(SubtensorModule::root_claim_hotkey_min_declared_weight().all_lt(ceiling));

        // Queued credits with no row behind them reach the same ceiling.
        let hotkey = U256::from(1002);
        for netuid in 1..=257 {
            queue_raw_credit(hotkey, netuid);
        }
        let queue_only = RootClaimHotkeyWork {
            rows: 0,
            queued: 257,
            new_rows: 257,
        };
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&hotkey),
            Some(queue_only)
        );
        assert_eq!(queue_only.units(), MAX_ROOT_CLAIM_HOTKEY_WORK);
        assert!(declared(hotkey).all_lte(ceiling));
        assert!(
            claim_call(hotkey)
                .get_dispatch_info()
                .call_weight
                .all_lte(max_extrinsic())
        );
    });
}

// =============================================================================
// Real funds: the declaration made before a claim covers the work of the claim.
// =============================================================================

pub(super) struct RealFund {
    pub(super) hotkey: U256,
    /// One subnet per row, in `rows` order.
    pub(super) netuids: Vec<NetUid>,
    /// Root stakers, one per `stakes` entry.
    pub(super) stakers: Vec<U256>,
}

/// A new subnet with a 100k-TAO balanced pool priced at 1 (spot, fast anchor, slow EMA).
pub(super) fn priced_subnet(index: u32) -> NetUid {
    let subnet_owner = U256::from(20_000 + 2 * index);
    let subnet_hotkey = U256::from(20_001 + 2 * index);
    // The lock cost doubles per same-block registration; pin it so many subnets fit.
    SubtensorModule::set_network_last_lock(TaoBalance::from(TAO));
    let netuid = add_dynamic_network(&subnet_hotkey, &subnet_owner);
    remove_owner_registration_stake(netuid);
    let tao = TaoBalance::from(100_000 * TAO);
    SubnetTAO::<Test>::insert(netuid, tao);
    SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(100_000 * TAO));
    if let Some(subnet_account) = SubtensorModule::get_subnet_account_id(netuid) {
        add_balance_to_coldkey_account(&subnet_account, tao);
    }
    SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(1));
    SubnetFastMovingPrice::<Test>::insert(netuid, U64F64::from_num(1));
    netuid
}

/// A root validator whose fund holds `rows[i]` alpha on its own priced subnet. `stakes`
/// root stakers each hold `stakes[i] × SHARE` fund shares.
pub(super) fn real_fund(rows: &[u64], stakes: &[u64]) -> RealFund {
    let hotkey = U256::from(1002);
    let escrow = SubtensorModule::get_beta_escrow_account_id();
    zero_claim_threshold();
    register_on_root(&hotkey, 0);

    let netuids: Vec<NetUid> = rows
        .iter()
        .enumerate()
        .map(|(index, alpha)| {
            let netuid = priced_subnet(index as u32);
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
        .map(|(index, stake)| {
            let staker = U256::from(5_000 + index as u32);
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
    RealFund {
        hotkey,
        netuids,
        stakers,
    }
}

/// Queue a dividend credit the way an epoch does (the alpha is already issued).
fn queue_credit(hotkey: &U256, netuid: NetUid, alpha: u64) {
    SubnetAlphaOut::<Test>::mutate(netuid, |out| *out = out.saturating_add(alpha.into()));
    SubtensorModule::enqueue_basket_deposit(hotkey, netuid, alpha.into());
}

/// Runs the claim and returns its post-dispatch weight.
fn claim(claimant: U256, hotkey: U256) -> Weight {
    SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(claimant), hotkey)
        .expect("claim succeeds")
        .actual_weight
        .expect("claim reports actual weight")
}

/// Queued credits deposit into the fund (some open a new row), and the claimant redeems a
/// slice of every row. The declaration made before the claim covers all of it.
#[test]
fn declaration_covers_a_claim_that_settles_its_queue_and_redeems_every_row() {
    new_test_ext(1).execute_with(|| {
        let fund = real_fund(&[5 * TAO; 12], &[3, 1]);
        let claimant = fund.stakers[0];
        // Four credits on subnets the fund holds, two on subnets it does not.
        for netuid in fund.netuids.iter().take(4) {
            queue_credit(&fund.hotkey, *netuid, TAO);
        }
        let new_subnets = [priced_subnet(100), priced_subnet(101)];
        for netuid in new_subnets {
            queue_credit(&fund.hotkey, netuid, TAO);
        }

        let work = RootClaimHotkeyWork {
            rows: 12,
            queued: 6,
            new_rows: 2,
        };
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&fund.hotkey),
            Some(work)
        );
        let declared_before = declared(fund.hotkey);
        assert_eq!(declared_before, sized(work));

        let root_before = root_stake_of(&fund.hotkey, &claimant);
        let actual = claim(claimant, fund.hotkey);

        // The queue settled into the fund: both new subnets are rows now.
        assert_eq!(
            PendingBasketDeposits::<Test>::iter_key_prefix(fund.hotkey).count(),
            0
        );
        for netuid in new_subnets {
            assert!(escrow_alpha(&fund.hotkey, netuid) > 0);
        }
        // Fourteen subnet rows, and the root cash row when a sale left a surplus in the fund.
        let rows_after = SubtensorModule::root_claim_hotkey_work(&fund.hotkey)
            .expect("the fund fits")
            .rows;
        assert!((14..=15).contains(&rows_after), "rows after: {rows_after}");
        assert!(root_stake_of(&fund.hotkey, &claimant) > root_before);
        assert!(
            actual.all_lte(declared_before),
            "actual {actual:?} above declared {declared_before:?}"
        );
        // The claim did real work on all 14 rows and 6 credits: far above the claim-unit
        // part of the floor. The claimant has no debts, so the claim reports no settlement.
        let floor_work = SubtensorModule::root_claim_hotkey_min_declared_weight()
            .saturating_sub(SubtensorModule::root_claim_debt_settlement_weight_bound());
        assert!(actual.ref_time() > floor_work.ref_time());
    });
}

/// A deposit that a price move defers is charged as attempted, and the claimant then
/// sells its own part of each credit directly. That is two rows of work for each queued
/// credit. One row for each credit would be less than the claim reports.
#[test]
fn deferred_deposit_needs_two_rows_for_each_queued_credit() {
    new_test_ext(1).execute_with(|| {
        let fund = real_fund(&[5 * TAO; 12], &[1]);
        let claimant = fund.stakers[0];
        // A credit on every subnet the fund holds, then every price moves out of the
        // dividend band (spot halves, the fast anchor stays).
        for netuid in fund.netuids.iter() {
            queue_credit(&fund.hotkey, *netuid, TAO);
            SubnetTAO::<Test>::mutate(*netuid, |tao| *tao = tao.saturating_div(2u64.into()));
        }

        let work = RootClaimHotkeyWork {
            rows: 12,
            queued: 12,
            new_rows: 0,
        };
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&fund.hotkey),
            Some(work)
        );
        let declared_before = declared(fund.hotkey);
        assert_eq!(declared_before, sized(work));

        let shares_before = fund_shares(&fund.hotkey);
        let root_before = root_stake_of(&fund.hotkey, &claimant);
        let actual = claim(claimant, fund.hotkey);

        // The deposit did not mint: every credit was deferred, and the claimant, who owns
        // all the root stake, sold all of it directly. The fund redemption then burned
        // every share.
        assert!(fund_shares(&fund.hotkey) < shares_before);
        assert_eq!(
            DeferredBasketRates::<Test>::iter_prefix(fund.hotkey).count(),
            0
        );
        assert!(root_stake_of(&fund.hotkey, &claimant) > root_before + 12 * TAO / 4);

        assert!(
            actual.all_lte(declared_before),
            "actual {actual:?} above declared {declared_before:?}"
        );
        // The same bound with one row for each queued credit is too small.
        let one_row_per_credit = SubtensorModule::root_claim_weight_for_work(
            work.units(),
            BasketFlushWork::new(
                u64::from(work.queued) * 4 + u64::from(work.units()) * 2,
                u64::from(work.queued),
            ),
        );
        assert!(
            actual.ref_time() > one_row_per_credit.ref_time(),
            "actual {actual:?} must exceed the one-row bound {one_row_per_credit:?}"
        );
    });
}

/// A claim first sweeps dust rows into the root cash slot, which it opens. Swept rows and
/// redeemed rows together are as many as the claim units: the validator unit covers the
/// new root row.
#[test]
fn declaration_covers_a_claim_that_sweeps_dust_and_opens_the_root_slot() {
    new_test_ext(1).execute_with(|| {
        let mut rows = vec![5 * TAO; 10];
        rows.extend([500_000u64; 4]);
        let fund = real_fund(&rows, &[1]);
        let claimant = fund.stakers[0];
        // Rows worth less than 0.001 TAO are dust for the fund.
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(1_000_000));
        assert_eq!(escrow_alpha(&fund.hotkey, NetUid::ROOT), 0);

        let work = RootClaimHotkeyWork {
            rows: 14,
            queued: 0,
            new_rows: 0,
        };
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&fund.hotkey),
            Some(work)
        );
        let declared_before = declared(fund.hotkey);
        assert_eq!(declared_before, sized(work));

        let actual = claim(claimant, fund.hotkey);

        // The four dust rows are gone. The sole claimant redeemed everything else,
        // the root cash that the sweep created included.
        for netuid in fund.netuids.iter() {
            assert_eq!(escrow_alpha(&fund.hotkey, *netuid), 0);
        }
        assert_eq!(fund_shares(&fund.hotkey), 0);
        assert!(
            actual.all_lte(declared_before),
            "actual {actual:?} above declared {declared_before:?}"
        );
        // Four swept rows, ten redeemed rows and the redeemed root row: fifteen full
        // claim units, exactly the fund's count.
        assert_eq!(work.units(), 15);
        let fifteen_units = <Test as crate::Config>::WeightInfo::claim_root(15);
        assert!(actual.ref_time() >= fifteen_units.ref_time());
    });
}

// =============================================================================
// Deferred-dividend debts: the settlement that starts a claim.
// =============================================================================

/// The debt settlement bound is one read and one write for each debt that one claimant
/// can hold on one validator, and three more of each for the debt map and the two
/// watermarks. Every declaration has it, and the largest one still fits a block.
#[test]
fn every_declaration_has_the_debt_settlement_bound() {
    new_test_ext(1).execute_with(|| {
        let db = <Test as frame_system::Config>::DbWeight::get();
        let bound = SubtensorModule::root_claim_debt_settlement_weight_bound();
        assert_eq!(MAX_ROOT_CLAIM_WORK, 256);
        assert_eq!(bound, db.reads_writes(259, 259));
        assert_eq!(
            bound,
            SubtensorModule::root_claim_debt_settlement_weight(MAX_ROOT_CLAIM_WORK)
        );
        assert_eq!(
            SubtensorModule::root_claim_debt_settlement_weight(0),
            Weight::zero()
        );
        assert_eq!(
            SubtensorModule::root_claim_debt_settlement_weight(1),
            db.reads_writes(4, 4)
        );

        // The floor and the ceiling are the two work shapes plus the bound.
        let units = MIN_ROOT_CLAIM_HOTKEY_WORK;
        let floor_work = SubtensorModule::root_claim_weight_for_work(
            units,
            BasketFlushWork::new(u64::from(units) * 6, u64::from(units)),
        )
        .saturating_add(SubtensorModule::root_claim_hotkey_sizing_weight(
            units, units,
        ));
        assert_eq!(
            SubtensorModule::root_claim_hotkey_min_declared_weight(),
            floor_work.saturating_add(bound)
        );
        let ceiling_work = SubtensorModule::root_claim_hotkey_weight_for_work(258, 257)
            .saturating_add(SubtensorModule::root_claim_hotkey_sizing_weight(257, 257));
        let ceiling = SubtensorModule::root_claim_hotkey_declared_weight();
        assert_eq!(ceiling, ceiling_work.saturating_add(bound));
        // The combined worst case: the largest fund, a queued credit for each of its rows,
        // and the most debts.
        assert!(ceiling.saturating_add(extension()).all_lte(max_extrinsic()));
    });
}

/// What [`debts_of_settled_credits`] reports.
struct DebtRun {
    /// Debts that the claimant held when the claim started.
    debts: u32,
    declared: Weight,
    actual: Weight,
    /// Root stake that the claim paid to the claimant.
    paid: u64,
}

/// Twelve dividend credits are deferred (the prices are out of the band). With
/// `stake_change`, the claimant then adds root stake, which makes one debt for each
/// deferred credit. The prices return, and the queue settles into the fund. The claim
/// runs after that, with nothing queued.
fn debts_of_settled_credits(stake_change: bool) -> DebtRun {
    let mut run = None;
    new_test_ext(1).execute_with(|| {
        let fund = real_fund(&[5 * TAO; 12], &[3, 1]);
        let claimant = fund.stakers[0];
        for netuid in fund.netuids.iter() {
            queue_credit(&fund.hotkey, *netuid, TAO);
            SubnetTAO::<Test>::mutate(*netuid, |tao| *tao = tao.saturating_div(2u64.into()));
        }
        assert_ok!(SubtensorModule::snapshot_deferred_dividends(&fund.hotkey));
        assert_eq!(
            DeferredBasketRates::<Test>::iter_prefix(fund.hotkey).count(),
            12
        );

        if stake_change {
            let added = SHARE;
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &fund.hotkey,
                &claimant,
                NetUid::ROOT,
                added.into(),
            );
            assert_ok!(
                SubtensorModule::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                    &fund.hotkey,
                    &claimant,
                    added
                )
            );
            assert_eq!(
                SubtensorModule::root_claim_debts(&fund.hotkey, &claimant),
                12
            );
        }

        // The prices return to the band, and the queue settles. The credits are gone.
        for netuid in fund.netuids.iter() {
            SubnetTAO::<Test>::mutate(*netuid, |tao| *tao = tao.saturating_mul(2u64.into()));
        }
        flush_baskets();
        assert_eq!(
            PendingBasketDeposits::<Test>::iter_key_prefix(fund.hotkey).count(),
            0
        );
        assert_eq!(
            DeferredBasketRates::<Test>::iter_prefix(fund.hotkey).count(),
            0
        );

        // The debts outlived their credits: each one has a conversion record now.
        let debts = SubtensorModule::root_claim_debts(&fund.hotkey, &claimant);
        assert_eq!(debts, if stake_change { 12 } else { 0 });
        assert_eq!(
            DeferredBasketConversions::<Test>::iter_prefix((fund.hotkey,)).count() as u32,
            debts
        );
        // Neither the rows nor the queue of the fund show them.
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&fund.hotkey),
            Some(RootClaimHotkeyWork {
                rows: 12,
                queued: 0,
                new_rows: 0,
            })
        );

        let declared_before = declared(fund.hotkey);
        let root_before = root_stake_of(&fund.hotkey, &claimant);
        let actual = claim(claimant, fund.hotkey);

        // The claim settled every debt and removed every conversion record.
        assert_eq!(
            SubtensorModule::root_claim_debts(&fund.hotkey, &claimant),
            0
        );
        assert_eq!(
            DeferredBasketConversions::<Test>::iter_prefix((fund.hotkey,)).count(),
            0
        );
        run = Some(DebtRun {
            debts,
            declared: declared_before,
            actual,
            paid: root_stake_of(&fund.hotkey, &claimant).saturating_sub(root_before),
        });
    });
    run.expect("the scenario ran")
}

/// A debt outlives the credit that made it. The claim that settles such debts reports
/// their settlement on top of its row work, and its declaration covers both.
#[test]
fn claim_reports_the_settlement_of_debts_that_outlived_their_credits() {
    let without = debts_of_settled_credits(false);
    let with = debts_of_settled_credits(true);
    assert_eq!(without.debts, 0);
    assert_eq!(with.debts, 12);

    // The fund is the same in both runs, so the declaration is the same.
    assert_eq!(with.declared, without.declared);
    // The two claims do the same row work. The claim with debts reports exactly the
    // settlement of its twelve debts more.
    let settlement = new_test_ext(1)
        .execute_with(|| SubtensorModule::root_claim_debt_settlement_weight(with.debts));
    assert!(settlement.ref_time() > 0);
    assert_eq!(with.actual, without.actual.saturating_add(settlement));
    assert!(
        with.actual.all_lte(with.declared),
        "actual {:?} above declared {:?}",
        with.actual,
        with.declared
    );
    // Both claimants are paid. The added stake earns nothing from the credits that were
    // deferred before it: the debts take that part out again, up to rounding.
    assert!(without.paid > 12 * TAO / 2);
    assert!(with.paid > 12 * TAO / 2);
    assert!(with.paid.abs_diff(without.paid) < TAO / 1_000);
}

/// A small fund with the most historical debts. Before the settlement bound, this claim
/// declared 34.4 ms, the floor of a fund with one row, and did 32.4 ms of settlement that
/// nothing declared and nothing reported.
#[test]
fn small_fund_with_the_most_historical_debts_fits_its_declaration() {
    new_test_ext(1).execute_with(|| {
        let fund = real_fund(&[5 * TAO], &[3, 1]);
        let claimant = fund.stakers[0];

        // 256 debts of the claimant, each with the conversion record that its credit left
        // when it converted. No credit is queued, and the fund has no row on those subnets.
        let generation = 7u64;
        let debts: BTreeMap<NetUid, DeferredBasketDebt> = (0..MAX_ROOT_CLAIM_WORK)
            .map(|index| {
                (
                    NetUid::from(1_000 + index as u16),
                    DeferredBasketDebt {
                        generation,
                        amount: I64F64::from_num(2),
                    },
                )
            })
            .collect();
        for netuid in debts.keys() {
            DeferredBasketConversions::<Test>::insert(
                (fund.hotkey, *netuid, generation),
                DeferredBasketConversion {
                    shares_per_alpha: I64F64::from_num(3),
                    cash_per_alpha: I64F64::from_num(0),
                    debt_count: 1,
                },
            );
        }
        DeferredBasketDebts::<Test>::insert(
            fund.hotkey,
            claimant,
            BoundedBTreeMap::try_from(debts).expect("256 debts fit the map"),
        );
        assert_eq!(
            SubtensorModule::root_claim_debts(&fund.hotkey, &claimant),
            MAX_ROOT_CLAIM_WORK
        );

        // The fund is small: one row, nothing queued. It declares the floor.
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&fund.hotkey),
            Some(RootClaimHotkeyWork {
                rows: 1,
                queued: 0,
                new_rows: 0,
            })
        );
        let floor = SubtensorModule::root_claim_hotkey_min_declared_weight();
        let declared_before = declared(fund.hotkey);
        assert_eq!(declared_before, floor);

        let claimed_before = BasketClaimed::<Test>::get(fund.hotkey, claimant);
        let owed_before = SubtensorModule::get_basket_owed_shares(&fund.hotkey, &claimant);
        let root_before = root_stake_of(&fund.hotkey, &claimant);
        let actual = claim(claimant, fund.hotkey);

        // Every debt is settled: 2 alpha at 3 shares for each alpha, 256 times, went into
        // the share watermark. The claim then redeemed what was left of the entitlement.
        // The watermark also moves by the paid stake, at the fund rate of 1.
        let correction = 256 * 2 * 3;
        assert_eq!(owed_before, 3 * SHARE - correction);
        let paid = root_stake_of(&fund.hotkey, &claimant) - root_before;
        assert!(paid > 0);
        assert_eq!(
            BasketClaimed::<Test>::get(fund.hotkey, claimant),
            claimed_before + i128::from(correction) + i128::from(owed_before) + i128::from(paid)
        );
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&fund.hotkey, &claimant),
            0
        );
        assert!(!DeferredBasketDebts::<Test>::contains_key(
            fund.hotkey,
            claimant
        ));
        assert_eq!(
            DeferredBasketConversions::<Test>::iter_prefix((fund.hotkey,)).count(),
            0
        );

        // The claim reports the settlement, and the declaration covers the claim.
        let bound = SubtensorModule::root_claim_debt_settlement_weight_bound();
        assert!(actual.ref_time() > bound.ref_time());
        assert!(
            actual.all_lte(declared_before),
            "actual {actual:?} above declared {declared_before:?}"
        );
        // The declaration without the settlement bound does not cover it.
        let without_bound = declared_before.saturating_sub(bound);
        assert!(
            actual.ref_time() > without_bound.ref_time(),
            "actual {actual:?} must exceed the declaration without the bound {without_bound:?}"
        );
    });
}

// =============================================================================
// The same-transaction guard.
// =============================================================================

struct TradedFund {
    /// Owns `hotkey`; the account that signs trades.
    coldkey: U256,
    hotkey: U256,
    /// Root staker entitled to the fund's dividends.
    staker: U256,
    /// The fund's holding lives here.
    netuid_a: NetUid,
    /// A second deep pool the fund does not hold.
    netuid_b: NetUid,
}

/// A trade comfortably above the minimum and inside the turnover budget.
const TRADE: u64 = 1_000_000_000;

/// A root validator whose fund holds one dividend-funded row on subnet A, with a second
/// deep pool B, prices pinned at 1, and trading enabled.
fn traded_fund() -> TradedFund {
    let coldkey = U256::from(1001);
    let hotkey = U256::from(1002);
    let staker = U256::from(1003);
    let netuid_a = add_dynamic_network(&hotkey, &coldkey);
    let netuid_b = add_dynamic_network(&U256::from(2002), &U256::from(2001));
    remove_owner_registration_stake(netuid_a);
    fund_pool(netuid_a);
    fund_pool(netuid_b);
    for netuid in [netuid_a, netuid_b] {
        let reserve = 1_000_000_000_000_000u64;
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(reserve));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(reserve));
        let account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        add_balance_to_coldkey_account(&account, TaoBalance::from(reserve));
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(1));
        SubnetFastMovingPrice::<Test>::insert(netuid, U64F64::from_num(1));
    }
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
        25_000_000_000u64.into(),
        AlphaBalance::ZERO,
    );
    flush_baskets();
    assert!(escrow_alpha(&hotkey, netuid_a) > 0);
    assert!(fund_shares(&hotkey) > 0);
    BasketTradingEnabled::<Test>::put(true);
    BasketDailyTurnoverCap::<Test>::put(u16::MAX);
    TradedFund {
        coldkey,
        hotkey,
        staker,
        netuid_a,
        netuid_b,
    }
}

fn swap_call(fund: &TradedFund, origin: NetUid, destination: NetUid) -> RuntimeCall {
    RuntimeCall::SubtensorModule(crate::Call::swap_basket {
        hotkey: fund.hotkey,
        origin_netuid: origin,
        destination_netuid: destination,
        amount: TRADE.into(),
        min_amount_out: 0,
    })
}

/// Dispatches the trade like a transaction does and returns its post-dispatch weight.
fn swap(fund: &TradedFund, origin: NetUid, destination: NetUid) -> Weight {
    swap_call(fund, origin, destination)
        .dispatch(RuntimeOrigin::signed(fund.coldkey))
        .expect("trade succeeds")
        .actual_weight
        .expect("trade reports actual weight")
}

/// A trade that opens a row is recorded, and a claim on that fund later in the same
/// transaction is refused. The next transaction can claim.
#[test]
fn claim_is_refused_after_a_trade_opened_a_row_in_the_same_transaction() {
    new_test_ext(1).execute_with(|| {
        let fund = traded_fund();
        let declared_before = declared(fund.hotkey);
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&fund.hotkey),
            Some(RootClaimHotkeyWork {
                rows: 1,
                queued: 0,
                new_rows: 0,
            })
        );

        enter_transaction(3);
        let trade_weight = swap(&fund, fund.netuid_a, fund.netuid_b);
        assert!(escrow_alpha(&fund.hotkey, fund.netuid_b) > 0);
        assert_eq!(
            BasketRowsOpenedAt::<Test>::get(fund.hotkey),
            Some(this_transaction(3))
        );
        // The trade pays for the record: one read and one write above a two-row trade.
        assert_eq!(
            trade_weight,
            SubtensorModule::swap_basket_weight(2)
                .saturating_add(SubtensorModule::basket_rows_opened_weight())
        );

        // The fund has a row now that `declared_before` did not count.
        assert_eq!(
            SubtensorModule::root_claim_hotkey_work(&fund.hotkey)
                .expect("the fund fits")
                .rows,
            2
        );
        assert_refused_as_too_heavy(fund.staker, fund.hotkey);

        // A second buy of the same subnet opens nothing, and the record stays.
        swap(&fund, fund.netuid_a, fund.netuid_b);
        assert_refused_as_too_heavy(fund.staker, fund.hotkey);

        // The next transaction sizes its claim from the fund as it is now.
        enter_transaction(4);
        assert!(declared_before.all_lte(declared(fund.hotkey)));
        let root_before = root_stake_of(&fund.hotkey, &fund.staker);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.staker),
            fund.hotkey
        ));
        assert!(root_stake_of(&fund.hotkey, &fund.staker) > root_before);
    });
}

/// The same extrinsic index in a later block is a different transaction.
#[test]
fn record_of_an_earlier_block_does_not_refuse_a_claim() {
    new_test_ext(1).execute_with(|| {
        let fund = traded_fund();
        enter_transaction(3);
        swap(&fund, fund.netuid_a, fund.netuid_b);
        assert_refused_as_too_heavy(fund.staker, fund.hotkey);

        System::set_block_number(System::block_number() + 1);
        enter_transaction(3);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.staker),
            fund.hotkey
        ));
    });
}

/// A buy of a subnet the fund holds, and a move into the root cash slot, open no row that
/// a claim had not counted. Nothing is recorded, and the claim runs in the same
/// transaction within the weight it declared before the trades.
#[test]
fn trade_into_a_held_row_or_into_root_cash_does_not_refuse_a_claim() {
    new_test_ext(1).execute_with(|| {
        let fund = traded_fund();
        // An earlier transaction opened subnet B.
        enter_transaction(1);
        swap(&fund, fund.netuid_a, fund.netuid_b);

        enter_transaction(2);
        let declared_before = declared(fund.hotkey);
        // B is held now: a buy of B records nothing.
        let held = swap(&fund, fund.netuid_a, fund.netuid_b);
        assert_eq!(held, SubtensorModule::swap_basket_weight(2));
        // Root cash is the slot every claim reserves: opening it records nothing.
        assert_eq!(escrow_alpha(&fund.hotkey, NetUid::ROOT), 0);
        swap(&fund, fund.netuid_a, NetUid::ROOT);
        assert!(escrow_alpha(&fund.hotkey, NetUid::ROOT) > 0);
        assert_eq!(
            BasketRowsOpenedAt::<Test>::get(fund.hotkey),
            Some(this_transaction(1))
        );

        let actual = claim(fund.staker, fund.hotkey);
        assert!(
            actual.all_lte(declared_before),
            "actual {actual:?} above declared {declared_before:?}"
        );
    });
}

/// Block hooks and inherents run outside a user transaction: no wrapper declared a weight
/// ahead of them, so nothing is recorded and nothing is refused.
#[test]
fn nothing_is_recorded_outside_a_user_transaction() {
    new_test_ext(1).execute_with(|| {
        let fund = traded_fund();
        assert_eq!(SubtensorModule::current_transaction(), None);
        let trade_weight = swap(&fund, fund.netuid_a, fund.netuid_b);
        assert_eq!(trade_weight, SubtensorModule::swap_basket_weight(2));
        assert_eq!(BasketRowsOpenedAt::<Test>::iter().count(), 0);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.staker),
            fund.hotkey
        ));
    });
}

/// In one batch, the claim was declared for the fund before the trade. The claim is
/// refused, so the whole atomic batch reverts. The coldkey-wide claim declares its fixed
/// envelope and is not affected.
#[test]
fn batch_of_a_trade_and_a_claim_on_the_same_fund_reverts() {
    new_test_ext(1).execute_with(|| {
        let fund = traded_fund();
        enter_transaction(7);
        let trade = swap_call(&fund, fund.netuid_a, fund.netuid_b);
        let batch = RuntimeCall::Utility(pallet_utility::Call::batch_all {
            calls: vec![trade.clone(), claim_call(fund.hotkey)],
        });
        let a_before = escrow_alpha(&fund.hotkey, fund.netuid_a);
        let err = batch
            .dispatch(RuntimeOrigin::signed(fund.coldkey))
            .expect_err("the claim inside the batch is refused");
        assert_eq!(err.error, Error::<Test>::RootClaimTooHeavy.into());
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_a), a_before);
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_b), 0);
        assert_eq!(BasketRowsOpenedAt::<Test>::get(fund.hotkey), None);

        // The same trade with the coldkey-wide claim goes through.
        let wide = RuntimeCall::SubtensorModule(crate::Call::claim_root {
            subnets: Default::default(),
        });
        let batch = RuntimeCall::Utility(pallet_utility::Call::batch_all {
            calls: vec![trade, wide],
        });
        assert_ok!(batch.dispatch(RuntimeOrigin::signed(fund.coldkey)));
        assert!(escrow_alpha(&fund.hotkey, fund.netuid_b) > 0);
    });
}

/// A hotkey swap moves the whole fund to the new hotkey. A claim on the new hotkey later
/// in the same transaction is refused; the next transaction can claim.
#[test]
fn claim_is_refused_after_a_hotkey_swap_moved_the_fund_in_the_same_transaction() {
    new_test_ext(1).execute_with(|| {
        let fund = traded_fund();
        let new_hotkey = U256::from(1009);
        // An earlier trade left a record on the old hotkey.
        enter_transaction(1);
        swap(&fund, fund.netuid_a, fund.netuid_b);
        assert!(BasketRowsOpenedAt::<Test>::contains_key(fund.hotkey));

        enter_transaction(2);
        let small = declared(new_hotkey);
        assert_eq!(
            small,
            SubtensorModule::root_claim_hotkey_min_declared_weight()
        );
        SubtensorModule::transfer_basket_for_new_hotkey(&fund.hotkey, &new_hotkey);
        assert_eq!(escrow_alpha(&fund.hotkey, fund.netuid_a), 0);
        assert!(escrow_alpha(&new_hotkey, fund.netuid_a) > 0);
        assert_eq!(
            BasketRowsOpenedAt::<Test>::get(new_hotkey),
            Some(this_transaction(2))
        );
        // The record follows the fund.
        assert_eq!(BasketRowsOpenedAt::<Test>::get(fund.hotkey), None);
        assert_refused_as_too_heavy(fund.staker, new_hotkey);

        enter_transaction(3);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(fund.staker),
            new_hotkey
        ));

        // A swap that moves no row and no queued credit records nothing.
        let empty_old = U256::from(4001);
        let empty_new = U256::from(4002);
        SubtensorModule::transfer_basket_for_new_hotkey(&empty_old, &empty_new);
        assert_eq!(BasketRowsOpenedAt::<Test>::get(empty_new), None);
    });
}
