//! Deregistration inputs owned by lending.
//!
//! * **P13 ranking.** `get_network_to_prune` ranks subnets by
//!   [`RankPrice`], a [`SynthParams::rank_days`]-day SMA of raw spot (no
//!   add-back of minted short alpha). Each block adds spot to a day
//!   accumulator; every [`SynthParams::rank_day_blocks`] the day's mean enters
//!   a ring of daily means and the SMA is recomputed. Rings start empty and
//!   stay dark until a full window of real daily samples; prune selection
//!   falls back to the emission moving price until then.
//! * **P12 `T_ema`.** An EMA of pool TAO with time constant
//!   [`SynthParams::tao_ema_blocks`], seeded at pool TAO.

use super::*;

/// Daily means behind the dereg-ranking SMA, plus the open day.
#[freeze_struct("ee0b9366c30a655")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
)]
pub struct RankState {
    pub days: BoundedVec<U64F64, ConstU32<MAX_RANK_DAYS>>,
    /// Slot the next daily mean overwrites once the ring is full.
    pub cursor: u32,
    pub day_sum: U64F64,
    pub day_blocks: u32,
}

/// Fill every SMA day slot with `price` (registration / enable / migration).
pub fn seed_rank_ring(state: &mut RankState, price: U64F64, days: u32) {
    let cap = days.clamp(1, MAX_RANK_DAYS) as usize;
    while state.days.pop().is_some() {}
    state.day_sum = U64F64::from_num(0);
    state.day_blocks = 0;
    state.cursor = 0;
    for _ in 0..cap {
        let _ = state.days.try_push(price);
    }
}

/// Mean of `values`; zero when empty.
pub fn mean(values: &[U64F64]) -> U64F64 {
    if values.is_empty() {
        return U64F64::from_num(0);
    }
    let sum = values
        .iter()
        .fold(U64F64::from_num(0), |acc, v| acc.saturating_add(*v));
    sum.checked_div(U64F64::saturating_from_num(values.len()))
        .unwrap_or_default()
}

/// Add one block's `spot` to `state`. Returns the new SMA only when the ring
/// holds a full window of real samples (`days.len() == cap`). An empty ring
/// warms through partial fills before publishing; `seed_rank_ring` pre-fills
/// the ring for migration and benchmarks.
pub fn rank_step(
    state: &mut RankState,
    spot: U64F64,
    days: u32,
    day_blocks: u32,
) -> Option<U64F64> {
    let cap = days.clamp(1, MAX_RANK_DAYS) as usize;
    state.day_sum = state.day_sum.saturating_add(spot);
    state.day_blocks = state.day_blocks.saturating_add(1);
    if state.day_blocks < day_blocks.max(1) {
        return None;
    }
    let day_mean = state
        .day_sum
        .checked_div(U64F64::saturating_from_num(state.day_blocks))
        .unwrap_or(spot);
    state.day_sum = U64F64::from_num(0);
    state.day_blocks = 0;
    if state.days.len() > cap {
        state.days.truncate(cap);
        state.cursor = 0;
    }
    if state.days.len() < cap {
        let _ = state.days.try_push(day_mean);
    } else {
        if let Some(slot) = state.days.get_mut(state.cursor as usize) {
            *slot = day_mean;
        }
        state.cursor = state
            .cursor
            .saturating_add(1)
            .checked_rem(cap as u32)
            .unwrap_or(0);
    }
    // Only publish once the ring is fully warm with real samples.
    if state.days.len() == cap {
        Some(mean(&state.days))
    } else {
        None
    }
}

/// One EMA step `prev + (value − prev) / window`, seeded at `value`.
pub fn ema_step(prev: Option<u64>, value: u64, window: u64) -> u64 {
    let Some(prev) = prev else {
        return value;
    };
    let window = window.max(1);
    if value >= prev {
        prev.saturating_add(value.saturating_sub(prev).checked_div(window).unwrap_or(0))
    } else {
        prev.saturating_sub(prev.saturating_sub(value).checked_div(window).unwrap_or(0))
    }
}

/// Fixed-point EMA step for uncapped prices.
pub fn ema_price_step(prev: Option<U64F64>, value: U64F64, window: u64) -> U64F64 {
    let Some(prev) = prev else {
        return value;
    };
    let window = U64F64::saturating_from_num(window.max(1));
    let delta = if value >= prev {
        value.saturating_sub(prev)
    } else {
        prev.saturating_sub(value)
    };
    let step = delta.checked_div(window).unwrap_or_default();
    if value >= prev {
        prev.saturating_add(step)
    } else {
        prev.saturating_sub(step)
    }
}

impl<T: Config> Pallet<T> {
    /// Advance `netuid`'s ranking SMA, pool-TAO EMA, and uncapped notional EMA.
    pub fn update_dereg_moving_price(netuid: NetUid) {
        let params = SynthParamsStore::<T>::get();
        let tao = T::Pool::pool_tao(netuid).to_u64();
        let ema = ema_step(
            PoolTaoEma::<T>::get(netuid),
            tao,
            u64::from(params.tao_ema_blocks),
        );
        PoolTaoEma::<T>::insert(netuid, ema);
        // Uncapped slow EMA of spot for notional / interest (never the
        // emission EMA's 1.0-capped feed).
        let spot = T::Pool::current_alpha_price(netuid);
        let notional = ema_price_step(
            NotionalEma::<T>::get(netuid),
            spot,
            u64::from(params.tao_ema_blocks),
        );
        NotionalEma::<T>::insert(netuid, notional);
        if !Enabled::<T>::get() {
            return;
        }
        let mut state = RankRing::<T>::get(netuid).unwrap_or_default();
        if let Some(sma) = rank_step(&mut state, spot, params.rank_days, params.rank_day_blocks) {
            RankPrice::<T>::insert(netuid, sma);
        }
        RankRing::<T>::insert(netuid, state);
    }

    pub fn dereg_moving_price(netuid: NetUid) -> Option<U64F64> {
        RankPrice::<T>::get(netuid)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn p(v: f64) -> U64F64 {
        U64F64::from_num(v)
    }

    #[test]
    fn seed_fills_the_whole_window() {
        let mut s = RankState::default();
        seed_rank_ring(&mut s, p(0.25), 30);
        assert_eq!(s.days.len(), 30);
        assert!(s.days.iter().all(|d| *d == p(0.25)));
        assert_eq!(rank_step(&mut s, p(0.25), 30, 10), None);
    }

    #[test]
    fn one_bad_day_moves_the_sma_by_a_window_fraction() {
        let mut s = RankState::default();
        seed_rank_ring(&mut s, p(1.0), 30);
        let mut sma = None;
        for _ in 0..10 {
            sma = rank_step(&mut s, p(0.0), 30, 10).or(sma);
        }
        let sma = sma.unwrap().to_num::<f64>();
        assert!((sma - 29.0 / 30.0).abs() < 1e-9, "{sma}");
    }

    #[test]
    fn sustained_pressure_replaces_the_window_and_wraps() {
        let mut s = RankState::default();
        seed_rank_ring(&mut s, p(1.0), 3);
        let mut sma = None;
        for _ in 0..(4 * 2) {
            sma = rank_step(&mut s, p(0.5), 3, 2).or(sma);
        }
        assert_eq!(sma, Some(p(0.5)));
        assert_eq!(s.cursor, 1);
        assert_eq!(s.days.len(), 3);
    }

    #[test]
    fn ema_seeds_and_tracks() {
        assert_eq!(ema_step(None, 100, 7_200), 100);
        assert_eq!(ema_step(Some(1_000), 0, 10), 900);
        assert_eq!(ema_step(Some(1_000), 2_000, 4), 1_250);
        assert_eq!(ema_step(Some(1_000), 2_000, 0), 2_000);
    }

    /// An empty ring warms through partial fills; `rank_step` returns None until
    /// the ring holds a full window of real daily means, then Some.
    #[test]
    fn empty_ring_warms_until_full_then_publishes() {
        // cap = 3, day_blocks = 2 → 6 updates fill the ring.
        let mut s = RankState::default();
        assert!(s.days.is_empty());

        // Day 1: blocks 1-2.
        assert_eq!(rank_step(&mut s, p(1.0), 3, 2), None, "mid-day 1");
        let r = rank_step(&mut s, p(1.0), 3, 2);
        assert_eq!(r, None, "day 1 close: ring not full yet (len=1)");
        assert_eq!(s.days.len(), 1);

        // Day 2: blocks 3-4.
        rank_step(&mut s, p(1.0), 3, 2);
        let r = rank_step(&mut s, p(1.0), 3, 2);
        assert_eq!(r, None, "day 2 close: ring not full yet (len=2)");
        assert_eq!(s.days.len(), 2);

        // Day 3: blocks 5-6 → ring full → Some.
        rank_step(&mut s, p(1.0), 3, 2);
        let r = rank_step(&mut s, p(1.0), 3, 2);
        assert_eq!(r, Some(p(1.0)), "day 3 close: ring full, SMA published");
        assert_eq!(s.days.len(), 3);

        // Subsequent day closes continue publishing (ring wraps).
        rank_step(&mut s, p(2.0), 3, 2);
        let r = rank_step(&mut s, p(2.0), 3, 2);
        assert!(r.is_some(), "subsequent days keep publishing");
    }
}
