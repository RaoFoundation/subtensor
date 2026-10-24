//! The TAO/USD price that values gamma.
//!
//! One root-set oracle feeds the price. The accepted price falls to the feed at once and
//! rises toward it by at most [`crate::TAO_USD_PRICE_MAX_STEP`] per block.

use super::*;

impl<T: Config> Pallet<T> {
    /// The accepted price at `now`: `state.accepted` raised by `TAO_USD_PRICE_MAX_STEP` of
    /// itself, rounded up, per block since `state.fed_at`, up to the feed.
    pub(crate) fn tao_usd_price_at(state: &TaoUsdPriceState, now: u64) -> u64 {
        let blocks = now.saturating_sub(state.fed_at);
        let step = crate::TAO_USD_PRICE_MAX_STEP.mul_ceil(state.accepted);
        state
            .accepted
            .saturating_add(step.saturating_mul(blocks))
            .min(state.feed)
    }

    /// The TAO/USD price a mint uses at `now`, given a feed at most
    /// `TAO_USD_PRICE_MAX_AGE` old.
    pub fn mint_tao_usd_price(now: u64) -> Result<u64, Error<T>> {
        TaoUsdPrice::<T>::get()
            .filter(|state| now.saturating_sub(state.fed_at) <= crate::TAO_USD_PRICE_MAX_AGE)
            .map(|state| Self::tao_usd_price_at(&state, now))
            .ok_or(Error::<T>::TaoUsdPriceUnavailable)
    }

    /// Stores the oracle's feed. The accepted price settles at the current block, never above
    /// the feed; the first feed is accepted as it is.
    pub fn do_feed_tao_usd_price(origin: OriginFor<T>, price: u64) -> DispatchResult {
        let who = ensure_signed(origin)?;
        ensure!(
            TaoUsdPriceOracle::<T>::get().as_ref() == Some(&who),
            DispatchError::BadOrigin
        );
        ensure!(price != 0, Error::<T>::InvalidValue);
        let now = Self::get_current_block_as_u64();
        let accepted = TaoUsdPrice::<T>::get()
            .map_or(price, |state| Self::tao_usd_price_at(&state, now))
            .min(price);
        TaoUsdPrice::<T>::put(TaoUsdPriceState {
            accepted,
            feed: price,
            fed_at: now,
        });
        Self::deposit_event(Event::TaoUsdPriceFed { price });
        Ok(())
    }
}
