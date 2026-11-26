//! Price rings (SPEC P6, P12, P13, section 9): the TWAP ring, the rank ring with its published
//! SMA, and the pool-TAO EMA.
//!
//! Sampling runs in `on_initialize` for every live subnet whether lending is enabled or not.
//! Rings are created only by a seed (upgrade, registration, enable, window change), always
//! full. A missing ring is an error (I25): sampling never creates one and no read falls back
//! to another price.

use alloc::vec;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{BoundedVec, ensure, traits::ConstU32, traits::Get};
use scale_info::TypeInfo;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::NetUid;

use crate::{
    Config, DeregParamsStore, Enabled, Error, Event, LendingParamsStore, LendingPoolInterface,
    Pallet, PoolTaoEma, RankGeneration, RankPrice, RankRing, RankRingGeneration, TwapRing,
    math::{
        PriceRefs,
        fixed::Price,
        interest::BLOCKS_PER_DAY,
        ring::{ema_step, mean, median_lower},
    },
};

/// Bound of the TWAP ring: the upper bound of `twap_samples` (SPEC 2.1).
pub const MAX_TWAP_SAMPLES: u32 = 72;
/// Bound of the rank ring: the upper bound of `rank_window_days` (SPEC 2.1).
pub const MAX_RANK_DAYS: u32 = 30;
/// Blocks between rank samples: hourly (SPEC P13), independent of `twap_interval_blocks`.
pub const RANK_SAMPLE_INTERVAL_BLOCKS: u32 = 300;
/// Rank samples in one day.
pub const RANK_SAMPLES_PER_DAY: u32 = 24;

/// Overwrites the slot at `cursor` and advances it, wrapping at the ring's length.
fn overwrite<S: Get<u32>>(ring: &mut BoundedVec<Price, S>, cursor: &mut u32, value: Price) {
    if let Some(slot) = ring.get_mut(*cursor as usize) {
        *slot = value;
    }
    let len = u32::try_from(ring.len()).unwrap_or(u32::MAX);
    *cursor = cursor.saturating_add(1).checked_rem(len).unwrap_or(0);
}

/// A subnet's TWAP samples (SPEC P6). Always full: its length is the `twap_samples` it was
/// seeded with.
#[freeze_struct("e55b78b861f4ec50")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct TwapState {
    /// Spot samples, oldest at `cursor`.
    pub samples: BoundedVec<Price, ConstU32<MAX_TWAP_SAMPLES>>,
    /// Slot the next sample overwrites.
    pub cursor: u32,
}

impl TwapState {
    /// A ring of `len` copies of `price` (capped at the ring bound).
    #[must_use]
    pub fn seeded(price: Price, len: u32) -> Self {
        Self {
            samples: BoundedVec::truncate_from(vec![price; len as usize]),
            cursor: 0,
        }
    }

    /// Replaces the oldest sample with `spot`.
    pub fn push(&mut self, spot: Price) {
        overwrite(&mut self.samples, &mut self.cursor, spot);
    }

    /// TWAP_median: the lower median of the samples (SPEC P6). `None` for an empty ring.
    #[must_use]
    pub fn median(&self) -> Option<Price> {
        median_lower(&mut self.samples.to_vec())
    }
}

/// A subnet's rank ring (SPEC P13): daily means of hourly spot samples, plus the open day.
/// Always full: its length is the `rank_window_days` it was seeded with.
#[freeze_struct("2bb479d160710512")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct RankState {
    /// Daily means, oldest at `cursor`.
    pub days: BoundedVec<Price, ConstU32<MAX_RANK_DAYS>>,
    /// Slot the next daily mean overwrites.
    pub cursor: u32,
    /// Hourly samples of the open day.
    pub hourly: BoundedVec<Price, ConstU32<RANK_SAMPLES_PER_DAY>>,
}

impl RankState {
    /// A ring of `days` copies of `price` (capped at the ring bound) with an empty open day.
    #[must_use]
    pub fn seeded(price: Price, days: u32) -> Self {
        Self {
            days: BoundedVec::truncate_from(vec![price; days as usize]),
            cursor: 0,
            hourly: BoundedVec::new(),
        }
    }

    /// Adds an hourly spot sample to the open day. A day never holds more than
    /// `RANK_SAMPLES_PER_DAY` samples because it closes every `BLOCKS_PER_DAY` blocks.
    pub fn sample(&mut self, spot: Price) {
        if self.hourly.try_push(spot).is_err() {
            frame_support::defensive!("rank ring: open day already holds a full day of samples");
        }
    }

    /// Closes the open day: its mean replaces the oldest daily mean. Returns the new SMA, or
    /// `None` when the day holds no sample (a ring seeded since the last sample), in which
    /// case nothing changes.
    pub fn close_day(&mut self) -> Option<Price> {
        let day_mean = mean(&self.hourly)?;
        self.hourly = BoundedVec::new();
        overwrite(&mut self.days, &mut self.cursor, day_mean);
        self.sma()
    }

    /// SMA of the daily means (SPEC P13). `None` for an empty ring.
    #[must_use]
    pub fn sma(&self) -> Option<Price> {
        mean(&self.days)
    }
}

/// `now` is a positive multiple of `interval`.
fn is_due(now: u64, interval: u32) -> bool {
    now.checked_rem(u64::from(interval)) == Some(0)
}

impl<T: Config> Pallet<T> {
    /// The subnet's three price references (SPEC 1.1, P6, P13): the only reader. Fails with
    /// `PriceRingMissing` instead of substituting any other price (SPEC section 9 item 5).
    pub fn price_refs(netuid: NetUid) -> Result<PriceRefs, Error<T>> {
        let twap = TwapRing::<T>::get(netuid)
            .and_then(|ring| ring.median())
            .ok_or(Error::<T>::PriceRingMissing)?;
        ensure!(
            RankRingGeneration::<T>::get(netuid) == Some(RankGeneration::<T>::get()),
            Error::<T>::PriceRingMissing
        );
        let sma = RankPrice::<T>::get(netuid).ok_or(Error::<T>::PriceRingMissing)?;
        Ok(PriceRefs {
            spot: T::Pool::spot_price(netuid),
            twap,
            sma,
        })
    }

    /// Registration hook, called by subtensor once the new (or re-registered) subnet's pool
    /// has its first price: both rings are seeded at that price and `T_ema` at the pool's TAO
    /// (SPEC section 9 item 2 and "Cadence and immunity").
    pub fn on_subnet_registered(netuid: NetUid) {
        Self::seed_subnet(netuid, T::Pool::spot_price(netuid));
    }

    /// Removal hook, called by subtensor when a subnet is dissolved: drops its price state so
    /// a re-registered netuid starts from its own seed.
    pub fn on_subnet_removed(netuid: NetUid) {
        TwapRing::<T>::remove(netuid);
        RankRing::<T>::remove(netuid);
        RankPrice::<T>::remove(netuid);
        RankRingGeneration::<T>::remove(netuid);
        PoolTaoEma::<T>::remove(netuid);
    }

    /// Seeds all of a subnet's price state: the TWAP ring from spot, `T_ema` from pool TAO and
    /// the rank ring from `rank_price`.
    pub(crate) fn seed_subnet(netuid: NetUid, rank_price: Price) {
        let twap_samples = LendingParamsStore::<T>::get().twap_samples;
        TwapRing::<T>::insert(
            netuid,
            TwapState::seeded(T::Pool::spot_price(netuid), twap_samples),
        );
        PoolTaoEma::<T>::insert(netuid, T::Pool::pool_tao(netuid));
        Self::seed_rank_ring(
            netuid,
            rank_price,
            DeregParamsStore::<T>::get().rank_window_days,
            RankGeneration::<T>::get(),
        );
    }

    /// Fills a subnet's rank ring with `days` copies of `price`, publishes it and tags the
    /// ring with `generation`.
    fn seed_rank_ring(netuid: NetUid, price: Price, days: u32, generation: u32) {
        RankRing::<T>::insert(netuid, RankState::seeded(price, days));
        RankPrice::<T>::insert(netuid, price);
        RankRingGeneration::<T>::insert(netuid, generation);
    }

    /// Bumps `RankGeneration` and re-seeds every subnet's rank ring at `days` from `source`
    /// (SPEC section 9 item 3). A subnet for which `source` has no price is left on its old
    /// generation, which I25 reports.
    fn reseed_rank_rings(subnets: &[NetUid], days: u32, source: impl Fn(NetUid) -> Option<Price>) {
        let generation = RankGeneration::<T>::get().wrapping_add(1);
        RankGeneration::<T>::put(generation);
        for netuid in subnets.iter().copied() {
            if let Some(price) = source(netuid) {
                Self::seed_rank_ring(netuid, price, days, generation);
            }
        }
    }

    /// `set_enabled`: on false -> true every live subnet's rank ring is re-seeded from its
    /// current `SubnetMovingPrice` (SPEC section 9 item 3, I25). Disabling resets nothing
    /// (item 4).
    pub(crate) fn do_set_enabled(enabled: bool) {
        if enabled && !Enabled::<T>::get() {
            Self::reseed_rank_rings(
                &T::Pool::live_subnets(),
                DeregParamsStore::<T>::get().rank_window_days,
                |netuid| Some(T::Pool::moving_alpha_price(netuid)),
            );
        }
        Enabled::<T>::put(enabled);
        Self::deposit_event(Event::EnabledSet { enabled });
    }

    /// `rank_window_days` changed: every ring is resized to `days` and filled with its own
    /// published SMA, never the emission EMA (SPEC 2.1 re-seeding rules).
    pub(crate) fn reseed_rank_rings_from_published(subnets: &[NetUid], days: u32) {
        Self::reseed_rank_rings(subnets, days, RankPrice::<T>::get);
    }

    /// `twap_samples` or `twap_interval_blocks` changed: every TWAP ring is resized to
    /// `samples` and filled with its own current median, never spot (SPEC 2.1 re-seeding
    /// rules). A missing ring stays missing.
    pub(crate) fn reseed_twap_rings_from_median(subnets: &[NetUid], samples: u32) {
        for netuid in subnets.iter().copied() {
            TwapRing::<T>::mutate(netuid, |ring| {
                if let Some(median) = ring.as_ref().and_then(TwapState::median) {
                    *ring = Some(TwapState::seeded(median, samples));
                }
            });
        }
    }

    /// One block of sampling for every live subnet (SPEC P6, P12, P13): `T_ema` every block,
    /// the TWAP ring every `twap_interval_blocks`, the rank ring hourly with the day closing
    /// (and `RankPrice` republished) every `BLOCKS_PER_DAY`. Only existing state is advanced.
    pub(crate) fn sample_prices(now: u64, subnets: &[NetUid]) {
        let twap_due = is_due(now, LendingParamsStore::<T>::get().twap_interval_blocks);
        let rank_due = is_due(now, RANK_SAMPLE_INTERVAL_BLOCKS);
        let day_due = is_due(now, BLOCKS_PER_DAY);
        let ema_window = DeregParamsStore::<T>::get().pool_tao_ema_blocks;
        for netuid in subnets.iter().copied() {
            PoolTaoEma::<T>::mutate(netuid, |ema| {
                if let Some(prev) = ema {
                    *prev = ema_step(*prev, T::Pool::pool_tao(netuid), ema_window);
                }
            });
            if !twap_due && !rank_due {
                continue;
            }
            let spot = T::Pool::spot_price(netuid);
            if twap_due {
                TwapRing::<T>::mutate(netuid, |ring| {
                    if let Some(ring) = ring {
                        ring.push(spot);
                    }
                });
            }
            if rank_due {
                RankRing::<T>::mutate(netuid, |ring| {
                    let Some(ring) = ring else { return };
                    ring.sample(spot);
                    if !day_due {
                        return;
                    }
                    if let Some(sma) = ring.close_day() {
                        RankPrice::<T>::insert(netuid, sma);
                    }
                });
            }
        }
    }
}
