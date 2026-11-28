//! Benchmarks of `pallet_lending`: the short calls, the root setters, the parts of
//! `on_initialize`, the coldkey-swap re-key and the v1 migration.
//!
//! The pools come from `Config::BenchmarkHelper`. In the runtime that is the real subnet pool,
//! so the short calls are measured through the adapter's swap legs with the fee settled to a
//! block author; in the pallet's mock it is `FakePool`, which reads and writes the same storage
//! of this pallet.
//!
//! The long calls are not benchmarked. HOLD(longs): their weights are the placeholders of
//! `long_weights`.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use alloc::{vec, vec::Vec};

use frame_benchmarking::{account, v2::*};
use frame_support::{
    BoundedVec,
    traits::{Get, Hooks},
};
use frame_system::{RawOrigin, pallet_prelude::BlockNumberFor};
use subtensor_runtime_common::NetUid;

use crate::{
    BenchmarkHelper, Call, Config, Custody, DeregParamsStore, Due, Enabled, HasMigrationRun,
    HolderPositions, InterestRetry, InterestTotals, LendingInterface, LendingParamsStore,
    LendingPoolInterface, LongParamsStore, LongsEnabled, NextDue, Pallet, PendingDeregParams,
    PendingLendingParams, PendingLongParams, PendingShortParams, PoolTaoEma, Positions, RankPrice,
    RankRing, ShortParamsStore, SwappedOutAt, TotalPositions, TwapRing,
    hooks::MAX_POSITIONS_PER_HOLDER,
    interest::MAX_DUE_BUCKETS_PER_BLOCK,
    math::interest::BLOCKS_PER_DAY,
    migrations,
    params::{DeregParams, LendingParams, LongParams, RAO, ShortParams},
    prices::{MAX_RANK_DAYS, MAX_TWAP_SAMPLES, RANK_SAMPLES_PER_DAY},
    types::{Leg, PositionId, Side},
};

/// Most subnets a per-subnet benchmark builds: the default registration `SubnetLimit`. The
/// weights are linear in the subnet count, so a higher limit extrapolates.
const MAX_SUBNETS: u32 = 128;

/// Reserves of every benchmark pool: spot 0.1 TAO per alpha, well above `min_pool_tao`.
const POOL_TAO: u64 = 50_000 * RAO;
const POOL_ALPHA: u64 = 500_000 * RAO;

/// Alpha one benchmark short mints: 20 TAO of notional, twice `min_notional`. Small against
/// the pool, so that many of them fit under `open_limit_bps` in one block.
const SHORT_Q: u64 = 200 * RAO;
/// Collateral of one benchmark short, far above its coverage need.
const COLLATERAL: u64 = 1_000 * RAO;
/// Free TAO of one benchmark holder: collateral and deposits for a full list of positions.
const HOLDER_TAO: u64 = 100_000 * RAO;

/// Blocks a position ages before a measured touch, so that the touch pays interest.
const AGE_BLOCKS: u32 = 100;

fn netuid_of(index: u32) -> NetUid {
    NetUid::from(u16::try_from(index).expect("benchmark netuids fit u16"))
}

/// Leaves exactly `subnets` live subnets, `1..=subnets`, each with a pool and no lending state.
fn pools<T: Config>(subnets: u32) -> Vec<NetUid> {
    // The benchmark runner starts a run at block 0 and moves to block 1 only after the setup.
    // Starting the setup at block 1 keeps "opened in this block" true in the measured code.
    if now::<T>() == 0u32.into() {
        frame_system::Pallet::<T>::set_block_number(1u32.into());
    }
    T::BenchmarkHelper::clear_subnets();
    (1..=subnets)
        .map(|index| {
            let netuid = netuid_of(index);
            T::BenchmarkHelper::setup_pool(netuid, POOL_TAO, POOL_ALPHA);
            netuid
        })
        .collect()
}

/// [`pools`] with every subnet's price rings seeded at full length, as registration leaves
/// them under the launch parameters.
fn seeded_pools<T: Config>(subnets: u32) -> Vec<NetUid> {
    let netuids = pools::<T>(subnets);
    for netuid in netuids.iter().copied() {
        Pallet::<T>::on_subnet_registered(netuid);
        assert_eq!(
            TwapRing::<T>::get(netuid).map(|ring| ring.samples.len()),
            Some(MAX_TWAP_SAMPLES as usize)
        );
        assert_eq!(
            RankRing::<T>::get(netuid).map(|ring| ring.days.len()),
            Some(MAX_RANK_DAYS as usize)
        );
    }
    netuids
}

/// [`seeded_pools`] with shorts enabled and a block author for the swap fees.
fn market<T: Config>(subnets: u32) -> Vec<NetUid> {
    let netuids = seeded_pools::<T>(subnets);
    Enabled::<T>::put(true);
    T::BenchmarkHelper::set_block_author();
    netuids
}

fn funded<T: Config>(who: T::AccountId) -> T::AccountId {
    T::BenchmarkHelper::fund(&who, HOLDER_TAO);
    who
}

fn holder<T: Config>(index: u32) -> T::AccountId {
    funded::<T>(account("holder", index, 0))
}

/// Opens a short of `who` on `netuid` and gives it the records that only some positions have,
/// so that every later touch reads and clears them: a custody record (kept by a coldkey swap)
/// and a retry count (left by a failed charge).
fn open<T: Config>(who: &T::AccountId, netuid: NetUid) -> PositionId<T::AccountId> {
    Pallet::<T>::open_short(
        RawOrigin::Signed(who.clone()).into(),
        netuid,
        SHORT_Q,
        COLLATERAL,
        0,
    )
    .expect("benchmark short opens");
    let id = (who.clone(), netuid, Side::Short);
    let custody = Pallet::<T>::custody_account(who, netuid, Side::Short).expect("position exists");
    Custody::<T>::insert(&id, custody);
    InterestRetry::<T>::insert(&id, 1);
    id
}

/// A market of `positions` subnets in which `who` holds a short on each.
fn holding<T: Config>(who: &T::AccountId, positions: u32) -> Vec<NetUid> {
    let netuids = market::<T>(positions);
    for netuid in netuids.iter().copied() {
        open::<T>(who, netuid);
    }
    netuids
}

fn now<T: Config>() -> BlockNumberFor<T> {
    frame_system::Pallet::<T>::block_number()
}

/// Moves the chain `blocks` ahead without running any hook.
fn age<T: Config>(blocks: u32) {
    frame_system::Pallet::<T>::set_block_number(now::<T>() + blocks.into());
}

fn minted<T: Config>(who: &T::AccountId, netuid: NetUid) -> u64 {
    match Positions::<T>::get((who.clone(), netuid, Side::Short)).map(|pos| pos.leg) {
        Some(Leg::Short { minted, .. }) => minted,
        _ => 0,
    }
}

fn collateral<T: Config>(who: &T::AccountId, netuid: NetUid) -> u64 {
    Positions::<T>::get((who.clone(), netuid, Side::Short)).map_or(0, |pos| pos.collateral)
}

/// Interest has reached the pool of `netuid`: the measured touch paid a charge.
fn assert_interest_paid<T: Config>(netuid: NetUid) {
    assert!(InterestTotals::<T>::get(netuid).charged > 0);
}

#[benchmarks]
mod benchmarks {
    use super::*;

    /// A holder one position short of the cap opens the last one, in a block after a coldkey
    /// swap marked the key (the marker is read and cleared).
    #[benchmark]
    fn open_short() {
        let caller: T::AccountId = funded::<T>(whitelisted_caller());
        let netuids = market::<T>(MAX_POSITIONS_PER_HOLDER);
        let (last, held) = netuids.split_last().unwrap();
        for netuid in held.iter().copied() {
            open::<T>(&caller, netuid);
        }
        SwappedOutAt::<T>::insert(&caller, now::<T>());
        age::<T>(1);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            *last,
            SHORT_Q,
            COLLATERAL,
            0,
        );

        assert_eq!(minted::<T>(&caller, *last), SHORT_Q);
        assert_eq!(
            HolderPositions::<T>::get(&caller).len(),
            MAX_POSITIONS_PER_HOLDER as usize
        );
        assert!(!SwappedOutAt::<T>::contains_key(&caller));
    }

    /// An aged short grows: the increase pays the interest accrued so far.
    #[benchmark]
    fn increase_short() {
        let caller: T::AccountId = funded::<T>(whitelisted_caller());
        let netuid = holding::<T>(&caller, 1)[0];
        age::<T>(AGE_BLOCKS);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            netuid,
            SHORT_Q,
            COLLATERAL,
            0,
        );

        assert_eq!(minted::<T>(&caller, netuid), 2 * SHORT_Q);
        assert_interest_paid::<T>(netuid);
    }

    /// A full close by a holder with a full list of positions: the buy-back, the interest,
    /// the refund, and every record of the position removed.
    #[benchmark]
    fn close_short() {
        let caller: T::AccountId = funded::<T>(whitelisted_caller());
        let netuids = holding::<T>(&caller, MAX_POSITIONS_PER_HOLDER);
        let netuid = netuids[0];
        let id = (caller.clone(), netuid, Side::Short);
        age::<T>(AGE_BLOCKS);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            netuid,
            u64::MAX,
            u64::MAX,
        );

        assert!(!Positions::<T>::contains_key(&id));
        assert!(!Custody::<T>::contains_key(&id));
        assert_eq!(InterestRetry::<T>::get(&id), 0);
        assert_eq!(
            HolderPositions::<T>::get(&caller).len(),
            MAX_POSITIONS_PER_HOLDER as usize - 1
        );
        assert_interest_paid::<T>(netuid);
    }

    /// A partial close: the buy-back, the interest and the coverage check of what is left.
    /// `close_short` is charged the larger of this and the full close.
    #[benchmark]
    fn close_short_partial() {
        let caller: T::AccountId = funded::<T>(whitelisted_caller());
        let netuid = holding::<T>(&caller, 1)[0];
        age::<T>(AGE_BLOCKS);

        #[block]
        {
            Pallet::<T>::close_short(
                RawOrigin::Signed(caller.clone()).into(),
                netuid,
                SHORT_Q / 2,
                u64::MAX,
            )
            .expect("partial close");
        }

        assert_eq!(minted::<T>(&caller, netuid), SHORT_Q / 2);
        assert_interest_paid::<T>(netuid);
    }

    #[benchmark]
    fn add_collateral() {
        let caller: T::AccountId = funded::<T>(whitelisted_caller());
        let netuid = holding::<T>(&caller, 1)[0];
        age::<T>(AGE_BLOCKS);
        let before = collateral::<T>(&caller, netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(caller.clone()), netuid, COLLATERAL);

        // The interest of `AGE_BLOCKS` is far below one TAO.
        assert!(collateral::<T>(&caller, netuid) > before + COLLATERAL - RAO);
        assert_interest_paid::<T>(netuid);
    }

    /// Enabling re-seeds the rank ring of each of the `s` live subnets.
    #[benchmark]
    fn set_enabled(s: Linear<1, MAX_SUBNETS>) {
        let netuids = seeded_pools::<T>(s);
        let generation = crate::RankGeneration::<T>::get();
        assert!(!Enabled::<T>::get());

        #[extrinsic_call]
        _(RawOrigin::Root, true);

        assert!(Enabled::<T>::get());
        for netuid in netuids {
            assert_eq!(
                crate::RankRingGeneration::<T>::get(netuid),
                Some(generation.wrapping_add(1))
            );
        }
    }

    #[benchmark]
    fn set_longs_enabled() {
        #[extrinsic_call]
        _(RawOrigin::Root, true);

        assert!(LongsEnabled::<T>::get());
    }

    #[benchmark]
    fn set_lending_params() {
        // The heaviest path: the call tightens the live group (a higher base APR), so it replaces
        // a pending group and is scheduled for the delay in force, with `LendingParamsScheduled`.
        let now = frame_system::Pallet::<T>::block_number();
        let live = LendingParams::default();
        let earlier = LendingParams {
            rate_cap_bps: live.rate_cap_bps.saturating_mul(2),
            ..live.clone()
        };
        PendingLendingParams::<T>::put((earlier, now));
        let params = LendingParams {
            apr_bps: live.apr_bps.saturating_add(1),
            deposit: live.deposit.saturating_add(1),
            ..live.clone()
        };
        let apply_at = now + BlockNumberFor::<T>::from(live.rate_delay_blocks);

        #[extrinsic_call]
        _(RawOrigin::Root, params.clone());

        assert_eq!(PendingLendingParams::<T>::get(), Some((params, apply_at)));
    }

    #[benchmark]
    fn set_short_params() {
        let params = ShortParams::default();

        #[extrinsic_call]
        _(RawOrigin::Root, params.clone());

        assert_eq!(PendingShortParams::<T>::get(), Some(params));
    }

    #[benchmark]
    fn set_long_params() {
        let params = LongParams::default();

        #[extrinsic_call]
        _(RawOrigin::Root, params.clone());

        assert_eq!(PendingLongParams::<T>::get(), Some(params));
    }

    #[benchmark]
    fn set_dereg_params() {
        let params = DeregParams::default();

        #[extrinsic_call]
        _(RawOrigin::Root, params.clone());

        assert_eq!(PendingDeregParams::<T>::get(), Some(params));
    }

    /// The whole hook in a block that samples no ring: `s` live subnets step `T_ema`, nothing
    /// is pending and no charge is scheduled.
    #[benchmark]
    fn on_initialize_ema(s: Linear<1, MAX_SUBNETS>) {
        let netuids = seeded_pools::<T>(s);
        // One block past a day boundary: no TWAP, rank or day sample is due.
        let block: BlockNumberFor<T> = (BLOCKS_PER_DAY + 1).into();
        frame_system::Pallet::<T>::set_block_number(block);
        let rings: Vec<_> = netuids.iter().map(|n| TwapRing::<T>::get(*n)).collect();

        #[block]
        {
            Pallet::<T>::on_initialize(block);
        }

        for (netuid, ring) in netuids.iter().zip(rings) {
            assert!(PoolTaoEma::<T>::get(*netuid).is_some());
            assert_eq!(TwapRing::<T>::get(*netuid), ring);
        }
    }

    /// The whole hook in the costliest sampling block: each of the `s` live subnets steps
    /// `T_ema`, pushes a TWAP sample and an hourly rank sample, and closes a full day, which
    /// republishes its SMA. Nothing is pending and no charge is scheduled.
    #[benchmark]
    fn on_initialize_sampling(s: Linear<1, MAX_SUBNETS>) {
        let netuids = seeded_pools::<T>(s);
        for netuid in netuids.iter().copied() {
            RankRing::<T>::mutate(netuid, |ring| {
                let ring = ring.as_mut().expect("seeded");
                let price = ring.days[0];
                ring.hourly =
                    BoundedVec::truncate_from(vec![price; RANK_SAMPLES_PER_DAY as usize - 1]);
            });
            RankPrice::<T>::remove(netuid);
        }
        let block: BlockNumberFor<T> = BLOCKS_PER_DAY.into();
        frame_system::Pallet::<T>::set_block_number(block);

        #[block]
        {
            Pallet::<T>::on_initialize(block);
        }

        for netuid in netuids {
            let twap = TwapRing::<T>::get(netuid).expect("seeded");
            let rank = RankRing::<T>::get(netuid).expect("seeded");
            assert_eq!(twap.cursor, 1);
            assert_eq!((rank.cursor, rank.hourly.len()), (1, 0));
            assert!(RankPrice::<T>::get(netuid).is_some());
        }
    }

    /// All four groups pending, with a TWAP window change and a rank window change: both rings
    /// of each of the `s` live subnets are re-seeded at full length.
    #[benchmark]
    fn apply_pending_params(s: Linear<1, MAX_SUBNETS>) {
        let netuids = seeded_pools::<T>(s);
        let generation = crate::RankGeneration::<T>::get();
        LendingParamsStore::<T>::put(LendingParams {
            twap_samples: MAX_TWAP_SAMPLES - 1,
            ..LendingParams::default()
        });
        DeregParamsStore::<T>::put(DeregParams {
            rank_window_days: MAX_RANK_DAYS - 1,
            ..DeregParams::default()
        });
        let now = frame_system::Pallet::<T>::block_number();
        PendingLendingParams::<T>::put((LendingParams::default(), now));
        PendingShortParams::<T>::put(ShortParams::default());
        PendingLongParams::<T>::put(LongParams::default());
        PendingDeregParams::<T>::put(DeregParams::default());
        let applied;

        #[block]
        {
            let lending = Pallet::<T>::apply_pending_lending_params(now, &netuids);
            let others = Pallet::<T>::apply_pending_params(&netuids);
            applied = lending || others;
        }

        assert!(applied);
        assert_eq!(LendingParamsStore::<T>::get(), LendingParams::default());
        assert_eq!(ShortParamsStore::<T>::get(), ShortParams::default());
        assert_eq!(LongParamsStore::<T>::get(), LongParams::default());
        assert_eq!(DeregParamsStore::<T>::get(), DeregParams::default());
        assert!(PendingDeregParams::<T>::get().is_none());
        for netuid in netuids {
            assert_eq!(
                crate::RankRingGeneration::<T>::get(netuid),
                Some(generation.wrapping_add(1))
            );
        }
    }

    /// `c` scheduled charges in the bucket of the current block, each of another holder and,
    /// up to the subnet count, on another subnet. Every charge takes interest to the pool,
    /// reads a custody record, clears a retry count and schedules the next charge.
    #[benchmark]
    fn collect_due(c: Linear<0, { T::MaxChargesPerBlock::get() }>) {
        let netuids = market::<T>(c.clamp(1, MAX_SUBNETS));
        let ids: Vec<_> = (0..c)
            .map(|index| {
                let netuid = netuids[index as usize % netuids.len()];
                open::<T>(&holder::<T>(index), netuid)
            })
            .collect();
        let interval = LendingParamsStore::<T>::get().charge_interval_blocks;
        age::<T>(interval);
        let block = now::<T>();
        for id in &ids {
            assert!(Due::<T>::contains_key(block, id));
        }
        let ran;

        #[block]
        {
            ran = Pallet::<T>::collect_due(block);
        }

        assert_eq!(ran.0, c);
        for id in &ids {
            let pos = Positions::<T>::get(id).expect("still open");
            assert_eq!(pos.charge_at, block + interval.into());
            assert!(Due::<T>::contains_key(pos.charge_at, id));
            assert_eq!(InterestRetry::<T>::get(id), 0);
            assert_interest_paid::<T>(id.1);
        }
    }

    /// `b` empty `Due` buckets read while the cursor catches up with the current block.
    #[benchmark]
    fn collect_due_buckets(b: Linear<1, MAX_DUE_BUCKETS_PER_BLOCK>) {
        let start: BlockNumberFor<T> = 10u32.into();
        let block: BlockNumberFor<T> = (10 + b - 1).into();
        frame_system::Pallet::<T>::set_block_number(block);
        NextDue::<T>::put(start);
        // An open position somewhere keeps the cursor instead of clearing it.
        TotalPositions::<T>::put(1);
        let ran;

        #[block]
        {
            ran = Pallet::<T>::collect_due(block);
        }

        assert_eq!(ran, (0, b));
        assert_eq!(NextDue::<T>::get(), Some(block + 1u32.into()));
    }

    /// The re-key of a coldkey swap for a holder of `p` positions opened in this block, each
    /// with a `Due` entry, a retry count and a custody record to move.
    #[benchmark]
    fn rekey_positions(p: Linear<1, MAX_POSITIONS_PER_HOLDER>) {
        let old: T::AccountId = holder::<T>(0);
        let new: T::AccountId = account("new", 0, 0);
        let netuids = holding::<T>(&old, p);

        #[block]
        {
            let _ = Pallet::<T>::rekey_positions(&old, &new);
        }

        assert!(HolderPositions::<T>::get(&old).is_empty());
        assert_eq!(HolderPositions::<T>::get(&new).len(), p as usize);
        assert_eq!(SwappedOutAt::<T>::get(&old), Some(now::<T>()));
        for netuid in netuids {
            let id = (new.clone(), netuid, Side::Short);
            let pos = Positions::<T>::get(&id).expect("moved");
            assert!(Due::<T>::contains_key(pos.charge_at, &id));
            assert!(Custody::<T>::contains_key(&id));
            assert_eq!(InterestRetry::<T>::get(&id), 1);
        }
    }

    /// The admission check of a coldkey swap away from a holder with a full list of positions.
    #[benchmark]
    fn coldkey_swap_destination_ok() {
        let old: T::AccountId = holder::<T>(0);
        let new: T::AccountId = account("new", 0, 0);
        holding::<T>(&old, MAX_POSITIONS_PER_HOLDER);
        let admitted;

        #[block]
        {
            admitted = <Pallet<T> as LendingInterface<T::AccountId>>::coldkey_swap_destination_ok(
                &old, &new,
            );
        }

        assert!(admitted);
    }

    /// The v1 initialisation with `s` live subnets, each seeded with both rings and `T_ema`.
    #[benchmark]
    fn migrate_to_v1(s: Linear<1, MAX_SUBNETS>) {
        let netuids = pools::<T>(s);
        let _ = HasMigrationRun::<T>::clear(u32::MAX, None);
        assert!(!migrations::has_run::<T>());

        #[block]
        {
            let _ = migrations::migrate_to_v1::<T>();
        }

        assert!(migrations::has_run::<T>());
        for netuid in netuids {
            assert!(Pallet::<T>::price_refs(netuid).is_ok());
            assert_eq!(
                PoolTaoEma::<T>::get(netuid),
                Some(T::Pool::pool_tao(netuid))
            );
        }
    }

    impl_benchmark_test_suite!(
        Pallet,
        crate::tests::mock::new_test_ext(),
        crate::tests::mock::Test
    );
}
