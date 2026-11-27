//! Price rings (SPEC P6, P12, P13, section 9, I25): sampling, seeds, re-seeds, the reader and
//! the v1 migration.

use frame_support::{
    assert_ok,
    traits::{Get, GetStorageVersion, Hooks, StorageVersion},
    weights::{RuntimeDbWeight, Weight},
};
use subtensor_runtime_common::NetUid;

use super::mock::*;
use crate::{
    DeregParamsStore, Enabled, Error, LendingParamsStore, LongParamsStore, LongsEnabled,
    PoolTaoEma, RankGeneration, RankPrice, RankRing, RankRingGeneration, ShortParamsStore,
    TwapRing,
    math::{
        PriceRefs,
        fixed::Price,
        ring::{mean, median_lower},
    },
    params::{DeregParams, LendingParams, LongParams, ShortParams},
    prices::{RankState, TwapState},
    weights::{SubstrateWeight, WeightInfo},
};

const DAY: u64 = 7_200;

fn sn(netuid: u16) -> NetUid {
    NetUid::from(netuid)
}

fn twap(netuid: u16) -> TwapState {
    TwapRing::<Test>::get(sn(netuid)).expect("TWAP ring")
}

fn rank(netuid: u16) -> RankState {
    RankRing::<Test>::get(sn(netuid)).expect("rank ring")
}

fn refs(netuid: u16) -> PriceRefs {
    Lending::price_refs(sn(netuid)).expect("price refs")
}

/// The reader fails with `PriceRingMissing`.
fn ring_missing(netuid: u16) -> bool {
    matches!(
        Lending::price_refs(sn(netuid)),
        Err(Error::<Test>::PriceRingMissing)
    )
}

/// The floor mean of `old` repeated `n_old` times and `new` repeated `n_new` times.
fn mix(old: Price, n_old: usize, new: Price, n_new: usize) -> Price {
    let mut all = vec![old; n_old];
    all.extend(vec![new; n_new]);
    mean(&all).expect("non-empty")
}

#[test]
fn migration_prefills_every_live_subnet_with_lending_disabled() {
    new_test_ext().execute_with(|| {
        set_pool(1, 1_000, 10_000);
        set_moving_price(1, px(800, 10_000));
        set_pool(2, 5_000, 10_000);
        set_moving_price(2, px(4_000, 10_000));
        assert_eq!(Lending::on_chain_storage_version(), StorageVersion::new(0));
        assert!(Lending::do_try_state().is_err());

        let weight = Lending::on_runtime_upgrade();

        assert_eq!(weight, SubstrateWeight::<Test>::migrate_to_v1(2));
        assert_eq!(Lending::on_chain_storage_version(), StorageVersion::new(1));
        assert_eq!(Lending::in_code_storage_version(), StorageVersion::new(1));
        assert!(!Enabled::<Test>::get());
        assert!(!LongsEnabled::<Test>::get());
        assert_eq!(LendingParamsStore::<Test>::get(), LendingParams::default());
        assert_eq!(ShortParamsStore::<Test>::get(), ShortParams::default());
        assert_eq!(LongParamsStore::<Test>::get(), LongParams::default());
        assert_eq!(DeregParamsStore::<Test>::get(), DeregParams::default());
        for (netuid, tao, spot, moving) in [
            (1, 1_000, px(1_000, 10_000), px(800, 10_000)),
            (2, 5_000, px(5_000, 10_000), px(4_000, 10_000)),
        ] {
            // Rank ring: 30 copies of SubnetMovingPrice. TWAP ring: 72 copies of spot.
            assert_eq!(rank(netuid), RankState::seeded(moving, 30));
            assert_eq!(rank(netuid).days.len(), 30);
            assert_eq!(RankPrice::<Test>::get(sn(netuid)), Some(moving));
            assert_eq!(
                RankRingGeneration::<Test>::get(sn(netuid)),
                Some(RankGeneration::<Test>::get())
            );
            assert_eq!(twap(netuid), TwapState::seeded(spot, 72));
            assert_eq!(twap(netuid).samples.len(), 72);
            assert_eq!(PoolTaoEma::<Test>::get(sn(netuid)), Some(tao));
            assert_eq!(
                refs(netuid),
                PriceRefs {
                    spot,
                    twap: spot,
                    sma: moving
                }
            );
        }
        assert_ok!(Lending::do_try_state());
    });
}

/// FRAME sets the storage version of a pallet that is new to a chain before any migration
/// runs. The initialisation still runs then: it is guarded by `HasMigrationRun`.
#[test]
fn migration_runs_on_a_chain_whose_storage_version_is_already_set() {
    new_test_ext().execute_with(|| {
        set_pool(1, 1_000, 10_000);
        set_moving_price(1, px(800, 10_000));
        StorageVersion::new(1).put::<Lending>();
        assert!(!crate::migrations::has_run::<Test>());

        let weight = Lending::on_runtime_upgrade();

        assert_eq!(weight, SubstrateWeight::<Test>::migrate_to_v1(1));
        assert!(crate::migrations::has_run::<Test>());
        assert_eq!(twap(1), TwapState::seeded(px(1_000, 10_000), 72));
        assert_eq!(RankPrice::<Test>::get(sn(1)), Some(px(800, 10_000)));
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn migration_runs_once() {
    migrated_ext().execute_with(|| {
        set_pool(1, 9_000, 10_000);
        set_moving_price(1, px(9_000, 10_000));
        let before = (twap(1), rank(1));
        let weight = Lending::on_runtime_upgrade();
        let db: RuntimeDbWeight = <Test as frame_system::Config>::DbWeight::get();
        assert_eq!(weight, db.reads(1));
        assert_eq!((twap(1), rank(1)), before);
    });
}

#[test]
fn twap_samples_only_on_interval_blocks_and_wraps_around() {
    migrated_ext().execute_with(|| {
        let old = px(1_000, 10_000);
        let new = px(2_000, 10_000);
        set_pool(1, 2_000, 10_000);

        run_to(299);
        assert_eq!(twap(1), TwapState::seeded(old, 72));
        run_to(300);
        assert_eq!(twap(1).cursor, 1);
        assert_eq!(twap(1).samples.first(), Some(&new));
        assert_eq!(twap(1).samples.iter().filter(|s| **s == new).count(), 1);
        run_to(599);
        assert_eq!(twap(1).cursor, 1);

        // The 72nd sample fills the last slot and the cursor wraps to the oldest.
        run_to(72 * 300);
        assert_eq!(twap(1), TwapState::seeded(new, 72));

        // The 73rd overwrites slot 0, and only slot 0.
        let newest = px(3_000, 10_000);
        set_pool(1, 3_000, 10_000);
        run_to(73 * 300);
        let ring = twap(1);
        assert_eq!(ring.cursor, 1);
        assert_eq!(ring.samples.len(), 72);
        assert_eq!(ring.samples.first(), Some(&newest));
        assert_eq!(ring.samples.iter().filter(|s| **s == new).count(), 71);
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn a_one_block_x145_spike_at_the_sample_block_leaves_the_twap_median_unchanged() {
    migrated_ext().execute_with(|| {
        let base = px(1_000, 10_000);
        run_to(299);
        // The spike lands exactly on the sample block and is gone one block later.
        set_pool(1, 145_000, 10_000);
        run_to(300);
        set_pool(1, 1_000, 10_000);
        run_to(301);

        let ring = twap(1);
        assert_eq!(ring.samples.first(), Some(&px(145_000, 10_000)));
        assert_eq!(ring.median(), Some(base));
        assert_eq!(refs(1).twap, base);
        // Control (SPEC P6): the same ring read as a mean moves 3x (71 + 145 = 216 = 3 * 72).
        let as_mean = mean(&ring.samples).expect("non-empty");
        assert_eq!(as_mean, mix(base, 71, px(145_000, 10_000), 1));
        assert!(as_mean > px(2_999, 10_000));
    });
}

#[test]
fn a_sustained_move_reaches_the_median_only_after_half_the_window() {
    migrated_ext().execute_with(|| {
        let base = px(1_000, 10_000);
        let high = px(3_000, 10_000);
        set_pool(1, 3_000, 10_000);
        run_to(36 * 300);
        assert_eq!(refs(1).twap, base);
        run_to(37 * 300);
        assert_eq!(refs(1).twap, high);
        // The reader's median is the pure core's lower median of the stored samples.
        assert_eq!(median_lower(&mut twap(1).samples.to_vec()), Some(high));
    });
}

#[test]
fn rank_ring_averages_hourly_samples_into_daily_means_and_publishes_the_sma() {
    migrated_ext().execute_with(|| {
        let seed = px(800, 10_000);
        let a = px(1_000, 10_000);
        let b = px(3_000, 10_000);

        // Half a day at a, half at b: 12 hourly samples each.
        run_to(12 * 300);
        assert_eq!(rank(1).hourly.len(), 12);
        assert_eq!(RankPrice::<Test>::get(sn(1)), Some(seed));
        set_pool(1, 3_000, 10_000);
        run_to(DAY - 1);
        assert_eq!(rank(1).hourly.len(), 23);
        assert_eq!(RankPrice::<Test>::get(sn(1)), Some(seed));

        run_to(DAY);
        let day_mean = mix(a, 12, b, 12);
        let ring = rank(1);
        assert!(ring.hourly.is_empty());
        assert_eq!(ring.cursor, 1);
        assert_eq!(ring.days.first(), Some(&day_mean));
        assert_eq!(ring.days.iter().filter(|d| **d == seed).count(), 29);
        // One day moves the SMA by a thirtieth of the window.
        let sma = mix(seed, 29, day_mean, 1);
        assert_eq!(RankPrice::<Test>::get(sn(1)), Some(sma));
        assert_eq!(refs(1).sma, sma);
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn rank_ring_wraps_after_a_full_window_and_keeps_sampling_while_disabled() {
    migrated_ext().execute_with(|| {
        assert!(!Enabled::<Test>::get());
        let seed = px(800, 10_000);
        let spot = px(1_000, 10_000);

        run_to(29 * DAY);
        assert_eq!(rank(1).cursor, 29);
        assert_eq!(RankPrice::<Test>::get(sn(1)), Some(mix(seed, 1, spot, 29)));

        // Day 30 replaces the last seeded day and the cursor wraps.
        run_to(30 * DAY);
        assert_eq!(rank(1), RankState::seeded(spot, 30));
        assert_eq!(RankPrice::<Test>::get(sn(1)), Some(spot));

        // Day 31 overwrites day 1's slot.
        let newest = px(4_000, 10_000);
        set_pool(1, 4_000, 10_000);
        run_to(31 * DAY);
        let ring = rank(1);
        assert_eq!(ring.cursor, 1);
        assert_eq!(ring.days.len(), 30);
        assert_eq!(ring.days.first(), Some(&newest));
        assert_eq!(
            RankPrice::<Test>::get(sn(1)),
            Some(mix(spot, 29, newest, 1))
        );
        // The TWAP ring sampled all along too: day 31's 24 samples sit at the newest price.
        assert_eq!(twap(1).cursor, 24);
        assert_eq!(twap(1).samples.iter().filter(|s| **s == newest).count(), 24);
        assert!(!Enabled::<Test>::get());
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn enabling_reseeds_every_rank_ring_from_the_moving_price() {
    migrated_ext().execute_with(|| {
        // Let real samples roll in so the rings differ from any seed.
        run_to(2 * DAY + 600);
        assert_eq!(rank(1).hourly.len(), 2);
        let generation = RankGeneration::<Test>::get();
        let moving_1 = px(700, 10_000);
        let moving_2 = px(3_500, 10_000);
        set_moving_price(1, moving_1);
        set_moving_price(2, moving_2);
        let twap_before = twap(1);

        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));

        // I25: in the same call, every ring is 30 copies of that block's SubnetMovingPrice.
        assert_eq!(RankGeneration::<Test>::get(), generation + 1);
        for (netuid, moving) in [(1, moving_1), (2, moving_2)] {
            assert_eq!(rank(netuid), RankState::seeded(moving, 30));
            assert_eq!(RankPrice::<Test>::get(sn(netuid)), Some(moving));
            assert_eq!(
                RankRingGeneration::<Test>::get(sn(netuid)),
                Some(generation + 1)
            );
            assert_eq!(refs(netuid).sma, moving);
        }
        assert_eq!(twap(1), twap_before);
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn enabling_when_enabled_and_disabling_do_not_reseed() {
    migrated_ext().execute_with(|| {
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        let generation = RankGeneration::<Test>::get();
        run_to(DAY);
        let ring = rank(1);
        assert_ne!(ring, RankState::seeded(px(800, 10_000), 30));

        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_eq!(RankGeneration::<Test>::get(), generation);
        assert_eq!(rank(1), ring);

        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), false));
        assert_eq!(RankGeneration::<Test>::get(), generation);
        assert_eq!(rank(1), ring);

        // Re-enabling re-seeds again.
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_eq!(RankGeneration::<Test>::get(), generation + 1);
        assert_eq!(rank(1), RankState::seeded(px(800, 10_000), 30));
    });
}

#[test]
fn a_subnet_registered_mid_window_is_seeded_at_its_first_price() {
    migrated_ext().execute_with(|| {
        run_to(DAY + 1_000);
        // SN3 registers: first price 0.2, and an emission moving price that must not be read.
        set_pool(3, 2_000, 10_000);
        set_moving_price(3, px(9_999, 10_000));
        let first = px(2_000, 10_000);
        assert!(ring_missing(3));

        Lending::on_subnet_registered(sn(3));

        assert_eq!(twap(3), TwapState::seeded(first, 72));
        assert_eq!(rank(3), RankState::seeded(first, 30));
        assert_eq!(RankPrice::<Test>::get(sn(3)), Some(first));
        assert_eq!(PoolTaoEma::<Test>::get(sn(3)), Some(2_000));
        assert_eq!(
            refs(3),
            PriceRefs {
                spot: first,
                twap: first,
                sma: first
            }
        );
        assert_ok!(Lending::do_try_state());

        // Its first day closes on its own 21 samples (blocks 8,400..=14,400).
        set_pool(3, 4_000, 10_000);
        run_to(2 * DAY);
        assert_eq!(rank(3).days.first(), Some(&px(4_000, 10_000)));
        assert_eq!(
            RankPrice::<Test>::get(sn(3)),
            Some(mix(first, 29, px(4_000, 10_000), 1))
        );
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn a_ring_seeded_on_a_day_boundary_closes_no_empty_day() {
    migrated_ext().execute_with(|| {
        run_to(DAY - 1);
        set_pool(3, 2_000, 10_000);
        Lending::on_subnet_registered(sn(3));
        let first = px(2_000, 10_000);
        // Block 7,200 samples once and closes the day on that one sample.
        run_to(DAY);
        assert_eq!(rank(3), {
            let mut ring = RankState::seeded(first, 30);
            ring.cursor = 1;
            ring
        });

        // A ring with an empty open day is left untouched by a close.
        let mut empty_day = RankState::seeded(first, 30);
        assert_eq!(empty_day.close_day(), None);
        assert_eq!(empty_day, RankState::seeded(first, 30));
    });
}

#[test]
fn a_removed_and_reregistered_netuid_starts_from_its_own_seed() {
    migrated_ext().execute_with(|| {
        run_to(DAY);
        assert_ne!(rank(1), RankState::seeded(px(800, 10_000), 30));

        remove_pool(1);
        Lending::on_subnet_removed(sn(1));
        assert_eq!(TwapRing::<Test>::get(sn(1)), None);
        assert_eq!(RankRing::<Test>::get(sn(1)), None);
        assert_eq!(RankPrice::<Test>::get(sn(1)), None);
        assert_eq!(RankRingGeneration::<Test>::get(sn(1)), None);
        assert_eq!(PoolTaoEma::<Test>::get(sn(1)), None);
        assert_ok!(Lending::do_try_state());

        set_pool(1, 300, 10_000);
        Lending::on_subnet_registered(sn(1));
        let first = px(300, 10_000);
        assert_eq!(twap(1), TwapState::seeded(first, 72));
        assert_eq!(rank(1), RankState::seeded(first, 30));
        assert_eq!(PoolTaoEma::<Test>::get(sn(1)), Some(300));
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn a_missing_ring_is_an_error_and_sampling_never_creates_one() {
    migrated_ext().execute_with(|| {
        // SN3 is live but was never seeded.
        set_pool(3, 2_000, 10_000);
        set_moving_price(3, px(2_000, 10_000));
        run_to(DAY);
        assert_eq!(TwapRing::<Test>::get(sn(3)), None);
        assert_eq!(RankRing::<Test>::get(sn(3)), None);
        assert_eq!(RankPrice::<Test>::get(sn(3)), None);
        assert_eq!(PoolTaoEma::<Test>::get(sn(3)), None);
        assert!(ring_missing(3));

        // Each missing piece fails the read on its own; none falls back to spot or the EMA.
        let twap_ring = twap(1);
        TwapRing::<Test>::remove(sn(1));
        assert!(ring_missing(1));
        TwapRing::<Test>::insert(sn(1), twap_ring);
        assert_ok!(Lending::price_refs(sn(1)));

        let published = RankPrice::<Test>::take(sn(1)).expect("published");
        assert!(ring_missing(1));
        RankPrice::<Test>::insert(sn(1), published);
        assert_ok!(Lending::price_refs(sn(1)));

        // A ring of an older generation is not read.
        RankGeneration::<Test>::mutate(|g| *g += 1);
        assert!(ring_missing(1));
    });
}

#[test]
fn pool_tao_ema_tracks_pool_tao_every_block() {
    migrated_ext().execute_with(|| {
        assert_eq!(PoolTaoEma::<Test>::get(sn(1)), Some(1_000));
        // A step is floor(|T - ema| / 7,200), truncated toward the old value.
        set_pool(1, 1_000 + 72_000, 10_000);
        next_block();
        assert_eq!(PoolTaoEma::<Test>::get(sn(1)), Some(1_010));
        set_pool(1, 0, 10_000);
        next_block();
        assert_eq!(PoolTaoEma::<Test>::get(sn(1)), Some(1_010));
        set_pool(1, 1_010 + 7_199, 10_000);
        next_block();
        assert_eq!(PoolTaoEma::<Test>::get(sn(1)), Some(1_010));
        // SN2 is unaffected by SN1's pool.
        assert_eq!(PoolTaoEma::<Test>::get(sn(2)), Some(5_000));
    });
}

/// The hook's weight in a block that samples no ring, applies nothing and collects nothing.
fn idle_block_weight(subnets: u32) -> Weight {
    SubstrateWeight::<Test>::on_initialize_ema(subnets)
        .saturating_add(SubstrateWeight::<Test>::collect_due(0))
        .saturating_add(SubstrateWeight::<Test>::collect_due_buckets(0))
}

#[test]
fn on_initialize_charges_for_every_live_subnet() {
    migrated_ext().execute_with(|| {
        assert_eq!(Lending::on_initialize(2), idle_block_weight(2));
        set_pool(3, 1, 1);
        assert_eq!(Lending::on_initialize(3), idle_block_weight(3));
    });
}
