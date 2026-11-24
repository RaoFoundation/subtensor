//! Slow deregistration-ranking price (Jacob P13).
//!
//! `get_network_to_prune` ranks subnets by [`DeregMovingPrice`], a slow EMA of
//! live spot with time constant [`SynthParams::dereg_ma_blocks`]. Spot is raw
//! AMM spot with no add-back of minted short alpha, so sustained short selling
//! may push a subnet out, while a one-block dump moves the ranking by only
//! `1 / dereg_ma_blocks` of the gap.

use super::*;

/// Default [`SynthParams::dereg_ma_blocks`]: 30 days of 12 s blocks.
pub const DEFAULT_DEREG_MA_BLOCKS: u64 = 216_000;

/// One EMA step `prev + (spot − prev) / window`. The first sample seeds at spot;
/// a zero window tracks spot.
pub fn dereg_ema_step(prev: Option<U64F64>, spot: U64F64, window: u64) -> U64F64 {
    let Some(prev) = prev else {
        return spot;
    };
    let window = U64F64::saturating_from_num(window.max(1));
    if spot >= prev {
        prev.saturating_add(
            spot.saturating_sub(prev)
                .checked_div(window)
                .unwrap_or_default(),
        )
    } else {
        prev.saturating_sub(
            prev.saturating_sub(spot)
                .checked_div(window)
                .unwrap_or_default(),
        )
    }
}

impl<T: Config> Pallet<T> {
    /// Advance `netuid`'s [`DeregMovingPrice`] one block toward live spot.
    pub fn update_dereg_moving_price(netuid: NetUid) {
        let spot = T::Pool::current_alpha_price(netuid);
        let window = SynthParamsStore::<T>::get().dereg_ma_blocks;
        DeregMovingPrice::<T>::mutate(netuid, |price| {
            *price = Some(dereg_ema_step(*price, spot, window));
        });
    }

    pub fn dereg_moving_price(netuid: NetUid) -> Option<U64F64> {
        DeregMovingPrice::<T>::get(netuid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: f64) -> U64F64 {
        U64F64::from_num(v)
    }

    #[test]
    fn first_sample_seeds_at_spot() {
        assert_eq!(
            dereg_ema_step(None, p(0.25), DEFAULT_DEREG_MA_BLOCKS),
            p(0.25)
        );
    }

    #[test]
    fn one_block_dump_moves_rank_price_by_one_window_fraction() {
        let next = dereg_ema_step(Some(p(1.0)), p(0.0), 1_000);
        assert!((next.to_num::<f64>() - 0.999).abs() < 1e-12);
    }

    #[test]
    fn sustained_pressure_converges_to_spot() {
        let window = 100;
        let mut price = Some(p(1.0));
        for _ in 0..window * 10 {
            price = Some(dereg_ema_step(price, p(0.5), window));
        }
        let price = price.unwrap_or_default().to_num::<f64>();
        assert!((price - 0.5).abs() < 1e-4, "{price}");
    }

    #[test]
    fn rises_symmetrically_and_zero_window_tracks_spot() {
        let up = dereg_ema_step(Some(p(0.5)), p(1.5), 4);
        assert!((up.to_num::<f64>() - 0.75).abs() < 1e-12);
        assert_eq!(dereg_ema_step(Some(p(0.5)), p(2.0), 0), p(2.0));
    }
}
