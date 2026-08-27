//! Tests for the on-chain standardized beta pricing layer: the paged beta-index sweep
//! and its published snapshot, baseline stamping/retirement/transfer, the staker
//! total-return accumulator, and the `migrate_stamp_beta_baselines` seed migration.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use codec::Decode;
use sp_core::U256;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::NetUid;

use crate::migrations::beta_baseline_table::{
    BETA_BASELINES, BETA_INDEX_LEVEL_BITS, BETA_TR_INDEX_LEVEL_BITS,
};
use crate::migrations::migrate_stamp_beta_baselines::migrate_stamp_beta_baselines;
use crate::staking::beta_pricing::{
    BETA_INDEX_REFRESH_INTERVAL_BLOCKS, BETA_INDEX_SWEEP_ROWS_PER_BLOCK, MIN_INDEX_NAV_RAO,
};
use crate::tests::mock::*;
use crate::{
    BasketRate, BasketShares, BasketTwr, BetaBaseline, BetaBaselineOf, BetaIndexFundSample,
    BetaIndexFundSampleOf, BetaIndexSnapshot, BetaIndexSweep, Event, HasMigrationRun,
};

fn one() -> U64F64 {
    U64F64::saturating_from_num(1)
}

/// A stamped baseline with zero pre-period rate (so the fund's staker yield is zero
/// while `BasketRate` stays at its default).
fn baseline(divisor: U64F64, tr_splice: U64F64) -> BetaBaselineOf {
    BetaBaselineOf {
        price_divisor: divisor,
        rate0: I96F32::from_num(0),
        tr_splice,
        first_block: 1,
    }
}

/// Give a fund `shares` outstanding shares and a root (netuid 0) holding of `root_nav`
/// rao. Root holdings realize 1:1, so the fund's NAV — spot and realizable — is exactly
/// `root_nav`, which makes index levels exact.
fn seed_fund_holdings(hotkey: &U256, shares: u64, root_nav: u64) {
    let escrow = SubtensorModule::get_beta_escrow_account_id();
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey,
        &escrow,
        NetUid::ROOT,
        root_nav.into(),
    );
    BasketShares::<Test>::insert(hotkey, shares);
}

/// Grow a fund's root holding by `root_nav` rao without minting shares: a pure price
/// move (performance), never a flow.
fn grow_fund_nav(hotkey: &U256, root_nav: u64) {
    let escrow = SubtensorModule::get_beta_escrow_account_id();
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey,
        &escrow,
        NetUid::ROOT,
        root_nav.into(),
    );
}

/// Deposit into a fund at NAV: holdings and shares grow by the same factor, so the raw
/// price is unchanged — a pure flow, never performance.
fn deposit_at_nav(hotkey: &U256, root_nav: u64, shares: u64) {
    grow_fund_nav(hotkey, root_nav);
    BasketShares::<Test>::mutate(hotkey, |s| *s = s.saturating_add(shares));
}

/// Run sweep pages at `block` until the in-progress pass completes. The caller must
/// space calls at least [`BETA_INDEX_REFRESH_INTERVAL_BLOCKS`] apart (or have no
/// snapshot yet), otherwise no pass starts.
fn complete_sweep_pass(block: u64) {
    System::set_block_number(block);
    loop {
        SubtensorModule::advance_beta_index_sweep();
        if BetaIndexSweep::<Test>::get().is_none() {
            break;
        }
    }
}

/// Block at which the `n`-th completed pass (1-based) is run in these tests.
fn pass_block(n: u64) -> u64 {
    1 + n.saturating_sub(1) * BETA_INDEX_REFRESH_INTERVAL_BLOCKS
}

// =============================================================================
// Paged index sweep
// =============================================================================

#[test]
fn test_index_sweep_empty_map_publishes_unit_levels() {
    new_test_ext(1).execute_with(|| {
        assert!(BetaIndexSnapshot::<Test>::get().is_none());

        SubtensorModule::advance_beta_index_sweep();

        let snapshot = BetaIndexSnapshot::<Test>::get().expect("pass over empty map completes");
        assert_eq!(snapshot.bag_level, one());
        assert_eq!(snapshot.stake_level, one());
        assert_eq!(snapshot.block, 1);
        assert!(BetaIndexSweep::<Test>::get().is_none());
    });
}

#[test]
fn test_index_sweep_respects_refresh_interval() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::advance_beta_index_sweep();
        assert_eq!(BetaIndexSnapshot::<Test>::get().unwrap().block, 1);

        // Fresh snapshot: no new pass starts, no work done.
        System::set_block_number(2);
        assert_eq!(SubtensorModule::advance_beta_index_sweep(), 0);
        assert_eq!(BetaIndexSnapshot::<Test>::get().unwrap().block, 1);

        // Stale snapshot: a new pass starts and republishes.
        System::set_block_number(1 + BETA_INDEX_REFRESH_INTERVAL_BLOCKS);
        SubtensorModule::advance_beta_index_sweep();
        assert_eq!(
            BetaIndexSnapshot::<Test>::get().unwrap().block,
            1 + BETA_INDEX_REFRESH_INTERVAL_BLOCKS
        );
    });
}

#[test]
fn test_index_sweep_first_pass_records_samples_without_moving_levels() {
    new_test_ext(1).execute_with(|| {
        // Two funds at different display prices (1.0 and 2.0). A cross-sectional mean
        // would publish 1.75; the chained index has no previous samples to chain
        // against, so the first pass only records start-of-period state.
        let fund_a = U256::from(9001);
        seed_fund_holdings(&fund_a, 400_000_000, 400_000_000);
        BetaBaseline::<Test>::insert(fund_a, baseline(one(), one()));
        let fund_b = U256::from(9002);
        seed_fund_holdings(&fund_b, 600_000_000, 1_200_000_000);
        BetaBaseline::<Test>::insert(fund_b, baseline(one(), one()));

        complete_sweep_pass(pass_block(1));

        let snapshot = BetaIndexSnapshot::<Test>::get().expect("snapshot published");
        assert_eq!(snapshot.bag_level, one());
        assert_eq!(snapshot.stake_level, one());
        assert_eq!(
            BetaIndexFundSample::<Test>::get(fund_b),
            Some(BetaIndexFundSampleOf {
                nav: 1_200_000_000,
                display: U64F64::saturating_from_num(2),
                stake: one(),
            })
        );
    });
}

#[test]
fn test_index_sweep_chains_price_relatives() {
    new_test_ext(1).execute_with(|| {
        // Fund A: raw 1.0 over NAV 4e8; fund B: raw 2.0 over NAV 12e8.
        let fund_a = U256::from(9001);
        seed_fund_holdings(&fund_a, 400_000_000, 400_000_000);
        BetaBaseline::<Test>::insert(fund_a, baseline(one(), one()));
        let fund_b = U256::from(9002);
        seed_fund_holdings(&fund_b, 600_000_000, 1_200_000_000);
        BetaBaseline::<Test>::insert(fund_b, baseline(one(), one()));
        complete_sweep_pass(pass_block(1));

        // B's price doubles (pure performance: NAV grows, shares don't) and B's TWR
        // doubles (a dividend worth its claimants' whole stake). A unchanged. Both
        // aggregates weight by start-of-period NAV:
        // (4e8 × 1.0 + 12e8 × 2.0) / 16e8 = 1.75.
        grow_fund_nav(&fund_b, 1_200_000_000);
        SubtensorModule::accrue_basket_twr(&fund_b, 1_000, 1_000);
        complete_sweep_pass(pass_block(2));
        let snapshot = BetaIndexSnapshot::<Test>::get().expect("snapshot published");
        assert_eq!(snapshot.bag_level, U64F64::saturating_from_num(1.75));
        // The stake index chains TWR relatives (stake price = tr_splice × TWR): B's
        // relative is 2.0 from the accumulator alone, price moves don't enter.
        assert_eq!(snapshot.stake_level, U64F64::saturating_from_num(1.75));

        // Next period both funds are flat: the levels must not drift. B's weight is
        // now its new NAV (24e8), but both relatives are 1.0.
        complete_sweep_pass(pass_block(3));
        let snapshot = BetaIndexSnapshot::<Test>::get().expect("snapshot republished");
        assert_eq!(snapshot.bag_level, U64F64::saturating_from_num(1.75));
        assert_eq!(snapshot.stake_level, U64F64::saturating_from_num(1.75));
    });
}

/// The auditor's mandatory invariant: capital flows between unchanged-price funds must
/// not move the index. A deposit mints shares at NAV (size changes, price doesn't); the
/// old cross-sectional mean jumped 11% on this exact scenario.
#[test]
fn test_index_sweep_is_flow_neutral() {
    new_test_ext(1).execute_with(|| {
        // The auditor's example: A at NAV 1e9 price 1.0, B at NAV 1e9 price 2.0.
        let fund_a = U256::from(9001);
        seed_fund_holdings(&fund_a, 1_000_000_000, 1_000_000_000);
        BetaBaseline::<Test>::insert(fund_a, baseline(one(), one()));
        let fund_b = U256::from(9002);
        seed_fund_holdings(&fund_b, 500_000_000, 1_000_000_000);
        BetaBaseline::<Test>::insert(fund_b, baseline(one(), one()));
        complete_sweep_pass(pass_block(1));
        let before = BetaIndexSnapshot::<Test>::get().expect("baseline pass");

        // Deposit doubles B's size at NAV; no price moved anywhere.
        deposit_at_nav(&fund_b, 1_000_000_000, 500_000_000);
        complete_sweep_pass(pass_block(2));
        let after = BetaIndexSnapshot::<Test>::get().expect("post-flow pass");
        assert_eq!(after.bag_level, before.bag_level);
        assert_eq!(after.stake_level, before.stake_level);

        // And the deposit's weight change must not leak into later periods either.
        complete_sweep_pass(pass_block(3));
        let later = BetaIndexSnapshot::<Test>::get().expect("steady-state pass");
        assert_eq!(later.bag_level, before.bag_level);
    });
}

#[test]
fn test_index_sweep_skips_dust_and_zero_share_funds() {
    new_test_ext(1).execute_with(|| {
        // Qualifying fund: raw 2.0 over NAV 4e8.
        let live = U256::from(9010);
        seed_fund_holdings(&live, 200_000_000, 400_000_000);
        BetaBaseline::<Test>::insert(live, baseline(one(), one()));

        // Dust fund (below the index NAV floor at both ends): its relative must not
        // count, or it would drag the aggregate below the live fund's.
        let dust = U256::from(9011);
        seed_fund_holdings(&dust, MIN_INDEX_NAV_RAO / 4, MIN_INDEX_NAV_RAO / 4);
        BetaBaseline::<Test>::insert(dust, baseline(one(), one()));

        // Baseline entry with no outstanding shares: must be ignored entirely.
        let drained = U256::from(9012);
        BetaBaseline::<Test>::insert(drained, baseline(one(), one()));

        complete_sweep_pass(pass_block(1));

        // Live fund's price goes 2.0 -> 4.0 (relative 2.0); dust fund's price goes
        // 1.0 -> 2.0 too but stays under the floor (nav MIN/2 < MIN).
        grow_fund_nav(&live, 400_000_000);
        grow_fund_nav(&dust, MIN_INDEX_NAV_RAO / 4);
        complete_sweep_pass(pass_block(2));

        let snapshot = BetaIndexSnapshot::<Test>::get().expect("snapshot published");
        assert_eq!(snapshot.bag_level, U64F64::saturating_from_num(2));
    });
}

#[test]
fn test_index_sweep_never_chains_across_unpriceable_gap() {
    new_test_ext(1).execute_with(|| {
        let fund = U256::from(9020);
        seed_fund_holdings(&fund, 400_000_000, 400_000_000);
        BetaBaseline::<Test>::insert(fund, baseline(one(), one()));
        complete_sweep_pass(pass_block(1));
        assert!(BetaIndexFundSample::<Test>::get(fund).is_some());

        // The fund drains: unpriceable, so its stale sample is removed.
        BasketShares::<Test>::insert(fund, 0u64);
        complete_sweep_pass(pass_block(2));
        assert!(BetaIndexFundSample::<Test>::get(fund).is_none());

        // It revives at a different price. With no previous sample there is no
        // relative to chain — the level must not move on the revival pass.
        BasketShares::<Test>::insert(fund, 100_000_000u64);
        complete_sweep_pass(pass_block(3));
        let snapshot = BetaIndexSnapshot::<Test>::get().expect("snapshot published");
        assert_eq!(snapshot.bag_level, one());
        assert!(BetaIndexFundSample::<Test>::get(fund).is_some());
    });
}

#[test]
fn test_index_sweep_paginates_and_resumes_across_blocks() {
    new_test_ext(1).execute_with(|| {
        // More funds than one page: each holdings-free entry costs one row.
        let count = BETA_INDEX_SWEEP_ROWS_PER_BLOCK + 44;
        for i in 0..count {
            let hotkey = U256::from(20_000 + i);
            BasketShares::<Test>::insert(hotkey, 1u64);
            BetaBaseline::<Test>::insert(hotkey, baseline(one(), one()));
        }

        // First page: stops at the row budget, stores a cursor, publishes nothing.
        let work = SubtensorModule::advance_beta_index_sweep();
        assert_eq!(work, BETA_INDEX_SWEEP_ROWS_PER_BLOCK);
        assert!(BetaIndexSweep::<Test>::get().is_some());
        assert!(BetaIndexSnapshot::<Test>::get().is_none());

        // Second page: finishes the remaining rows and publishes (all funds are below
        // the dust floor, so levels default to 1.0).
        System::set_block_number(2);
        let work = SubtensorModule::advance_beta_index_sweep();
        assert_eq!(work, count - BETA_INDEX_SWEEP_ROWS_PER_BLOCK);
        assert!(BetaIndexSweep::<Test>::get().is_none());
        let snapshot = BetaIndexSnapshot::<Test>::get().expect("pass completed");
        assert_eq!(snapshot.bag_level, one());
        assert_eq!(snapshot.block, 2);
    });
}

// =============================================================================
// Paginated pricing enumeration
// =============================================================================

#[test]
fn test_get_all_beta_pricing_paginates_with_cursor() {
    new_test_ext(1).execute_with(|| {
        // Three live funds plus one drained fund life (zero-share row): the dead row
        // is walked over but never priced.
        let funds: Vec<U256> = (0..3).map(|i| U256::from(9600 + i)).collect();
        for fund in &funds {
            seed_fund_holdings(fund, 200_000_000, 200_000_000);
        }
        BasketShares::<Test>::insert(U256::from(9650), 0u64);

        // Page of 2, then resume from the cursor: every live fund exactly once.
        let first = SubtensorModule::get_all_beta_pricing(None, 2);
        assert_eq!(first.pricing.len(), 2);
        let cursor = first.next.expect("more rows remain after a capped page");
        let second = SubtensorModule::get_all_beta_pricing(Some(cursor), 2);
        assert!(second.next.is_none());
        let mut seen: Vec<U256> = first
            .pricing
            .iter()
            .chain(second.pricing.iter())
            .map(|p| p.hotkey)
            .collect();
        seen.sort();
        let mut want = funds.clone();
        want.sort();
        assert_eq!(seen, want);

        // limit 0 = one full page (everything fits under the cap here).
        let all = SubtensorModule::get_all_beta_pricing(None, 0);
        assert_eq!(all.pricing.len(), 3);
        assert!(all.next.is_none());
    });
}

// =============================================================================
// Baseline stamping and fund-life display state
// =============================================================================

#[test]
fn test_stamp_waits_for_snapshot_then_stamps_once() {
    new_test_ext(1).execute_with(|| {
        let fund = U256::from(9100);
        seed_fund_holdings(&fund, 300_000_000, 300_000_000);

        // No published snapshot yet: the fund stays unstamped (prices provisionally)
        // and the next mint retries.
        SubtensorModule::stamp_beta_baseline_if_new(&fund);
        assert!(BetaBaseline::<Test>::get(fund).is_none());

        // Publish a snapshot (empty index -> unit levels), then stamp.
        SubtensorModule::advance_beta_index_sweep();
        SubtensorModule::stamp_beta_baseline_if_new(&fund);

        let stamped = BetaBaseline::<Test>::get(fund).expect("stamped after snapshot");
        // raw = 3e8 / 3e8 = 1.0 against bag level 1.0 -> divisor 1.0.
        assert_eq!(stamped.price_divisor, one());
        assert_eq!(stamped.tr_splice, one());
        assert_eq!(stamped.rate0, BasketRate::<Test>::get(fund));
        assert_eq!(stamped.first_block, 1);
        assert!(System::events().iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::SubtensorModule(Event::BetaBaselineStamped { hotkey }) if *hotkey == fund
        )));

        // Idempotent: a later mint is one read, never a restamp.
        BasketShares::<Test>::insert(fund, 600_000_000u64);
        assert_eq!(SubtensorModule::stamp_beta_baseline_if_new(&fund), 0);
        assert_eq!(
            BetaBaseline::<Test>::get(fund).unwrap().price_divisor,
            one()
        );
    });
}

#[test]
fn test_stamp_divisor_splices_to_index_level() {
    new_test_ext(1).execute_with(|| {
        // Drive the chained index to 2.0: the reference fund's price doubles between
        // the first pass (records the sample) and the second (chains the relative).
        let reference = U256::from(9200);
        seed_fund_holdings(&reference, 200_000_000, 200_000_000);
        BetaBaseline::<Test>::insert(reference, baseline(one(), one()));
        complete_sweep_pass(pass_block(1));
        grow_fund_nav(&reference, 200_000_000);
        complete_sweep_pass(pass_block(2));
        assert_eq!(
            BetaIndexSnapshot::<Test>::get().unwrap().bag_level,
            U64F64::saturating_from_num(2)
        );

        // Newborn at raw 1.0 splices onto the level: divisor = raw / bag = 0.5, so its
        // display price starts exactly on the index line. Its TWR already compounded
        // to 2.0 before the (deferred) stamp, so the splice normalizes by it:
        // tr_splice = stake_level / twr = 1.0 / 2.0, and the fund's stake price
        // (tr_splice × TWR) starts exactly at the stake-index level.
        let newborn = U256::from(9201);
        seed_fund_holdings(&newborn, 300_000_000, 300_000_000);
        BasketTwr::<Test>::insert(newborn, U64F64::saturating_from_num(2));
        SubtensorModule::stamp_beta_baseline_if_new(&newborn);

        let stamped = BetaBaseline::<Test>::get(newborn).expect("stamped");
        assert_eq!(stamped.price_divisor, U64F64::saturating_from_num(0.5));
        assert_eq!(stamped.tr_splice, U64F64::saturating_from_num(0.5));
    });
}

#[test]
fn test_stamp_skips_zero_shares_and_unpriceable_funds() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::advance_beta_index_sweep();

        // No outstanding shares: nothing to stamp.
        let empty = U256::from(9300);
        SubtensorModule::stamp_beta_baseline_if_new(&empty);
        assert!(BetaBaseline::<Test>::get(empty).is_none());

        // Shares but no priceable holdings (NAV 0): stays unstamped for a later,
        // priceable mint.
        let unpriceable = U256::from(9301);
        BasketShares::<Test>::insert(unpriceable, 1_000u64);
        SubtensorModule::stamp_beta_baseline_if_new(&unpriceable);
        assert!(BetaBaseline::<Test>::get(unpriceable).is_none());
    });
}

#[test]
fn test_retire_and_transfer_display_state() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(9400);
        let new = U256::from(9401);
        let divisor = U64F64::saturating_from_num(0.25);
        let sample = BetaIndexFundSampleOf {
            nav: 500_000_000,
            display: U64F64::saturating_from_num(1.5),
            stake: one(),
        };
        BetaBaseline::<Test>::insert(old, baseline(divisor, one()));
        BasketTwr::<Test>::insert(old, U64F64::saturating_from_num(1.5));
        BetaIndexFundSample::<Test>::insert(old, sample);

        // Hotkey swap: a pure move of all three entries.
        SubtensorModule::transfer_beta_display_state(&old, &new);
        assert!(BetaBaseline::<Test>::get(old).is_none());
        assert!(!BasketTwr::<Test>::contains_key(old));
        assert!(BetaIndexFundSample::<Test>::get(old).is_none());
        assert_eq!(
            BetaBaseline::<Test>::get(new).unwrap().price_divisor,
            divisor
        );
        assert_eq!(
            BasketTwr::<Test>::get(new),
            U64F64::saturating_from_num(1.5)
        );
        assert_eq!(BetaIndexFundSample::<Test>::get(new), Some(sample));

        // End of the fund life (last claim or dust revival): every entry retires, so
        // a revived fund can neither keep its divisor nor chain across lives.
        SubtensorModule::retire_beta_display_state(&new);
        assert!(BetaBaseline::<Test>::get(new).is_none());
        assert!(!BasketTwr::<Test>::contains_key(new));
        assert!(BetaIndexFundSample::<Test>::get(new).is_none());
        // The accumulator reads at its 1.0 default for the next life.
        assert_eq!(BasketTwr::<Test>::get(new), one());
    });
}

// =============================================================================
// Staker total-return accumulator
// =============================================================================

#[test]
fn test_accrue_basket_twr_compounds_exactly() {
    new_test_ext(1).execute_with(|| {
        let fund = U256::from(9500);
        assert_eq!(BasketTwr::<Test>::get(fund), one());

        // +25% then +50%: 1.0 * 1.25 * 1.5 = 1.875 (all exact in binary).
        SubtensorModule::accrue_basket_twr(&fund, 250, 1_000);
        assert_eq!(
            BasketTwr::<Test>::get(fund),
            U64F64::saturating_from_num(1.25)
        );
        SubtensorModule::accrue_basket_twr(&fund, 500, 1_000);
        assert_eq!(
            BasketTwr::<Test>::get(fund),
            U64F64::saturating_from_num(1.875)
        );

        // Zero claimant base: no gain, accumulator unchanged.
        SubtensorModule::accrue_basket_twr(&fund, 100, 0);
        assert_eq!(
            BasketTwr::<Test>::get(fund),
            U64F64::saturating_from_num(1.875)
        );
    });
}

// =============================================================================
// Seed migration
// =============================================================================

#[test]
fn test_migrate_stamp_beta_baselines_seeds_live_funds_only() {
    new_test_ext(1).execute_with(|| {
        let (pubkey_live, divisor_bits, rate0_bits, tr_splice_bits, first_block) =
            &BETA_BASELINES[0];
        let (pubkey_drained, ..) = &BETA_BASELINES[1];
        let (pubkey_stamped, ..) = &BETA_BASELINES[2];
        let live = U256::decode(&mut &pubkey_live[..]).expect("32-byte key decodes");
        let drained = U256::decode(&mut &pubkey_drained[..]).expect("32-byte key decodes");
        let stamped = U256::decode(&mut &pubkey_stamped[..]).expect("32-byte key decodes");

        // Live fund: gets the frozen table row, with its pre-upgrade staker
        // entitlement folded into the splice. Root holdings realize 1:1, so its raw
        // price is 1e6 / 1e6 = 1.0; a rate delta of 0.5 over `rate0` folds the
        // table's splice by 1 + 0.5 × 1.0 = 1.5, making the stake series
        // (tr_splice × TWR, with TWR starting at 1.0 here) continuous at the upgrade.
        seed_fund_holdings(&live, 1_000_000, 1_000_000);
        BasketRate::<Test>::insert(
            live,
            I96F32::from_bits(*rate0_bits).saturating_add(I96F32::from_num(0.5)),
        );
        // Drained fund (no shares): skipped.
        // Already-stamped fund: its existing baseline must never be overwritten.
        BasketShares::<Test>::insert(stamped, 1_000_000u64);
        let existing = baseline(U64F64::saturating_from_num(7), one());
        BetaBaseline::<Test>::insert(stamped, existing);

        migrate_stamp_beta_baselines::<Test>();

        let seeded = BetaBaseline::<Test>::get(live).expect("live fund seeded");
        assert_eq!(seeded.price_divisor, U64F64::from_bits(*divisor_bits));
        assert_eq!(seeded.rate0, I96F32::from_bits(*rate0_bits));
        assert_eq!(
            seeded.tr_splice,
            U64F64::from_bits(*tr_splice_bits).saturating_mul(U64F64::saturating_from_num(1.5))
        );
        assert_eq!(seeded.first_block, *first_block);

        assert!(BetaBaseline::<Test>::get(drained).is_none());
        assert_eq!(BetaBaseline::<Test>::get(stamped).unwrap(), existing);
        assert!(HasMigrationRun::<Test>::get(
            b"migrate_stamp_beta_baselines".to_vec()
        ));

        // A seeding run publishes the initial index snapshot at the frozen SDK levels,
        // so the on-chain chained index continues the series the baselines were
        // stamped against.
        let snapshot = BetaIndexSnapshot::<Test>::get().expect("initial snapshot seeded");
        assert_eq!(snapshot.bag_level, U64F64::from_bits(BETA_INDEX_LEVEL_BITS));
        assert_eq!(
            snapshot.stake_level,
            U64F64::from_bits(BETA_TR_INDEX_LEVEL_BITS)
        );
    });
}

#[test]
fn test_migrate_stamp_beta_baselines_skips_snapshot_when_nothing_seeds() {
    new_test_ext(1).execute_with(|| {
        // No live table fund on this chain (fresh chain / testnet): no baseline is
        // seeded and the index must start at 1.0 via the sweep, not at mainnet's
        // frozen level.
        migrate_stamp_beta_baselines::<Test>();
        assert!(HasMigrationRun::<Test>::get(
            b"migrate_stamp_beta_baselines".to_vec()
        ));
        assert!(BetaIndexSnapshot::<Test>::get().is_none());
    });
}

#[cfg(feature = "try-runtime")]
#[test]
fn test_migrate_stamp_beta_baselines_try_runtime_hooks() {
    use crate::migrations::migrate_stamp_beta_baselines::stamp_beta_baselines::Migration;
    use frame_support::traits::OnRuntimeUpgrade;

    new_test_ext(1).execute_with(|| {
        // One live fund the migration must seed, one fund whose pre-existing baseline
        // must survive the upgrade verbatim.
        let (pubkey_live, ..) = &BETA_BASELINES[0];
        let (pubkey_stamped, ..) = &BETA_BASELINES[1];
        let live = U256::decode(&mut &pubkey_live[..]).expect("32-byte key decodes");
        let stamped = U256::decode(&mut &pubkey_stamped[..]).expect("32-byte key decodes");
        BasketShares::<Test>::insert(live, 1_000_000u64);
        BasketShares::<Test>::insert(stamped, 1_000_000u64);
        BetaBaseline::<Test>::insert(stamped, baseline(U64F64::saturating_from_num(7), one()));

        let state = Migration::<Test>::pre_upgrade().expect("pre_upgrade");
        Migration::<Test>::on_runtime_upgrade();
        Migration::<Test>::post_upgrade(state).expect("post_upgrade validates the seed");

        // Second execution (try-runtime double-run): still a no-op that validates.
        let state = Migration::<Test>::pre_upgrade().expect("pre_upgrade after run");
        Migration::<Test>::on_runtime_upgrade();
        Migration::<Test>::post_upgrade(state).expect("post_upgrade after idempotent re-run");
    });
}

#[test]
fn test_migrate_stamp_beta_baselines_is_idempotent() {
    new_test_ext(1).execute_with(|| {
        let (pubkey, ..) = &BETA_BASELINES[0];
        let hotkey = U256::decode(&mut &pubkey[..]).expect("32-byte key decodes");
        BasketShares::<Test>::insert(hotkey, 1_000_000u64);

        migrate_stamp_beta_baselines::<Test>();
        assert!(BetaBaseline::<Test>::get(hotkey).is_some());

        // A second run is a no-op: it must not resurrect a retired baseline.
        BetaBaseline::<Test>::remove(hotkey);
        migrate_stamp_beta_baselines::<Test>();
        assert!(BetaBaseline::<Test>::get(hotkey).is_none());
    });
}
