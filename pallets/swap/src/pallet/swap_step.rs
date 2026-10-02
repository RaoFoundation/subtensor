use core::marker::PhantomData;

use frame_support::{ensure, traits::Get};
use safe_math::*;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token, TokenReserve};

use super::pallet::*;

pub(crate) const MAX_SWAP_INPUT_RESERVE_MULTIPLIER: u64 = 1_000;

/// A struct representing a single swap step with all its parameters and state
pub(crate) struct BasicSwapStep<T, PaidIn, PaidOut>
where
    T: Config,
    PaidIn: Token,
    PaidOut: Token,
{
    // Input parameters
    netuid: NetUid,
    drop_fees: bool,
    requested_delta_in: PaidIn,
    limit_price: U64F64,

    // Intermediate calculations
    target_price: U64F64,
    current_price: U64F64,

    // Result values
    delta_in: PaidIn,
    /// Output for swapping the full `requested_delta_in`, computed once at construction.
    /// `convert_deltas` runs a 256-bit bignum pow, by far the most expensive part of a
    /// swap, so the full-fill execution reuses this instead of recomputing it.
    requested_delta_out: PaidOut,
    final_price: U64F64,
    fee: PaidIn,
    endpoint_clamped: bool,

    _phantom: PhantomData<(T, PaidIn, PaidOut)>,
}

impl<T, PaidIn, PaidOut> BasicSwapStep<T, PaidIn, PaidOut>
where
    T: Config,
    PaidIn: Token,
    PaidOut: Token,
    Self: SwapStep<T, PaidIn, PaidOut>,
{
    /// Creates and initializes a new swap step
    pub(crate) fn new(
        netuid: NetUid,
        amount_remaining: PaidIn,
        limit_price: U64F64,
        drop_fees: bool,
    ) -> Result<Self, Error<T>> {
        let fee = Pallet::<T>::calculate_fee_amount(netuid, amount_remaining, drop_fees);
        let net_requested = amount_remaining.saturating_sub(fee);
        let mut requested_delta_in = net_requested.min(Self::max_input(netuid)?);
        let mut endpoint_clamped = requested_delta_in < net_requested;

        // Full-fill output amount (one integer square root), shared by target-price
        // computation here and the execution in `process_swap`.
        let mut requested_delta_out = Self::convert_deltas(netuid, requested_delta_in)?;

        // Target and current prices
        let current_price = Self::price_target(netuid, PaidIn::ZERO, PaidOut::ZERO)?;
        let target_price = match Self::price_target(netuid, requested_delta_in, requested_delta_out)
        {
            Ok(price) => price,
            Err(_) => {
                // A near-endpoint full buy can exceed the price type's range.
                // Solve a representable partial fill before pricing that input;
                // errors on the reduced fill still propagate normally.
                requested_delta_in =
                    requested_delta_in.min(Self::delta_in(netuid, current_price, limit_price)?);
                requested_delta_out = Self::convert_deltas(netuid, requested_delta_in)?;
                endpoint_clamped = true;
                Self::price_target(netuid, requested_delta_in, requested_delta_out)?
            }
        };

        Ok(Self {
            netuid,
            drop_fees,
            requested_delta_in,
            limit_price,
            target_price,
            current_price,
            delta_in: PaidIn::ZERO,
            requested_delta_out,
            final_price: target_price,
            fee,
            endpoint_clamped,
            _phantom: PhantomData,
        })
    }

    /// Execute the swap step and return the result
    pub(crate) fn execute(&mut self) -> Result<SwapStepResult<PaidIn, PaidOut>, Error<T>> {
        self.determine_action()?;
        self.process_swap()
    }

    /// Determine the appropriate action for this swap step
    fn determine_action(&mut self) -> Result<(), Error<T>> {
        let mut recalculate_fee = self.endpoint_clamped;

        // Calculate the stopping price: The price at which we either reach the limit price,
        // or exchange the full amount.
        if Self::price_is_closer(&self.target_price, &self.limit_price) {
            // Case 1. target_quantity is the lowest, execute in full
            self.final_price = self.target_price;
            self.delta_in = self.requested_delta_in;
        } else {
            // Case 2. lim_quantity is the lowest
            self.final_price = self.limit_price;
            self.delta_in = Self::delta_in(self.netuid, self.current_price, self.limit_price)?
                .min(self.requested_delta_in);
            recalculate_fee = true;
        }

        log::trace!("\tCurrent Price    : {}", self.current_price);
        log::trace!("\tTarget Price     : {}", self.target_price);
        log::trace!("\tLimit Price      : {}", self.limit_price);
        log::trace!("\tDelta In         : {}", self.delta_in);

        // Because on step creation we calculate fee off the total amount, we might need to
        // recalculate it in case if we hit the limit price.
        if recalculate_fee {
            let u16_max = U64F64::saturating_from_num(u16::MAX);
            let fee_rate = if self.drop_fees {
                U64F64::saturating_from_num(0)
            } else {
                U64F64::saturating_from_num(FeeRate::<T>::get(self.netuid))
            };
            let delta_fixed = U64F64::saturating_from_num(self.delta_in);
            self.fee = delta_fixed
                .saturating_mul(fee_rate.safe_div(u16_max.saturating_sub(fee_rate)))
                .saturating_to_num::<u64>()
                .into();
        }
        Ok(())
    }

    /// Process a single step of a swap
    fn process_swap(&self) -> Result<SwapStepResult<PaidIn, PaidOut>, Error<T>> {
        // Convert amounts, actual swap happens here. The full-fill case (limit price not
        // hit) reuses the output computed at construction; only a limit-clamped partial
        // fill pays for a second conversion.
        let delta_out = if self.delta_in == self.requested_delta_in {
            self.requested_delta_out
        } else {
            Self::convert_deltas(self.netuid, self.delta_in)?
        };
        log::trace!("\tDelta Out        : {delta_out}");
        let mut fee_to_block_author = 0.into();
        if !self.delta_in.is_zero() {
            ensure!(!delta_out.is_zero(), Error::<T>::ReservesTooLow);

            // 100% of swap fees to to block builder
            fee_to_block_author = self.fee;
        }

        Ok(SwapStepResult {
            fee_paid: self.fee,
            delta_in: self.delta_in,
            delta_out,
            fee_to_block_author,
        })
    }
}

impl<T: Config> SwapStep<T, TaoBalance, AlphaBalance>
    for BasicSwapStep<T, TaoBalance, AlphaBalance>
{
    fn max_input(netuid: NetUid) -> Result<TaoBalance, Error<T>> {
        let curve = Pallet::<T>::superellipse(netuid)?;
        curve
            .max_buy_input_with_reserve_floor(
                T::AlphaReserve::reserve(netuid).into(),
                T::TaoReserve::reserve(netuid).into(),
                T::MinimumReserve::get().get(),
            )
            .map(TaoBalance::from)
            .map_err(|_| Error::<T>::ReservesOutOfBalance)
    }

    fn delta_in(
        netuid: NetUid,
        _price_curr: U64F64,
        target: U64F64,
    ) -> Result<TaoBalance, Error<T>> {
        Pallet::<T>::superellipse(netuid)?
            .quote_delta_to_price(
                T::AlphaReserve::reserve(netuid).into(),
                T::TaoReserve::reserve(netuid).into(),
                target,
            )
            .map(TaoBalance::from)
            .map_err(|_| Error::<T>::ReservesOutOfBalance)
    }

    fn price_target(
        netuid: NetUid,
        delta_in: TaoBalance,
        delta_out: AlphaBalance,
    ) -> Result<U64F64, Error<T>> {
        let alpha = u64::from(T::AlphaReserve::reserve(netuid))
            .checked_sub(delta_out.into())
            .ok_or(Error::<T>::InsufficientLiquidity)?;
        let tao = u64::from(T::TaoReserve::reserve(netuid))
            .checked_add(delta_in.into())
            .ok_or(Error::<T>::ReservesOutOfBalance)?;
        Pallet::<T>::superellipse(netuid)?
            .calculate_price(alpha, tao)
            .map_err(|_| Error::<T>::ReservesOutOfBalance)
    }

    fn price_is_closer(price1: &U64F64, price2: &U64F64) -> bool {
        price1 <= price2
    }

    fn convert_deltas(netuid: NetUid, input: TaoBalance) -> Result<AlphaBalance, Error<T>> {
        Pallet::<T>::superellipse(netuid)?
            .buy_output(
                T::AlphaReserve::reserve(netuid).into(),
                T::TaoReserve::reserve(netuid).into(),
                input.into(),
            )
            .map(AlphaBalance::from)
            .map_err(|_| Error::<T>::ReservesOutOfBalance)
    }
}

impl<T: Config> SwapStep<T, AlphaBalance, TaoBalance>
    for BasicSwapStep<T, AlphaBalance, TaoBalance>
{
    fn max_input(netuid: NetUid) -> Result<AlphaBalance, Error<T>> {
        Pallet::<T>::superellipse(netuid)?
            .max_sell_input_with_reserve_floor(
                T::AlphaReserve::reserve(netuid).into(),
                T::TaoReserve::reserve(netuid).into(),
                T::MinimumReserve::get().get(),
            )
            .map(AlphaBalance::from)
            .map_err(|_| Error::<T>::ReservesOutOfBalance)
    }

    fn delta_in(
        netuid: NetUid,
        _price_curr: U64F64,
        target: U64F64,
    ) -> Result<AlphaBalance, Error<T>> {
        Pallet::<T>::superellipse(netuid)?
            .base_delta_to_price(
                T::AlphaReserve::reserve(netuid).into(),
                T::TaoReserve::reserve(netuid).into(),
                target,
            )
            .map(AlphaBalance::from)
            .map_err(|_| Error::<T>::ReservesOutOfBalance)
    }

    fn price_target(
        netuid: NetUid,
        delta_in: AlphaBalance,
        delta_out: TaoBalance,
    ) -> Result<U64F64, Error<T>> {
        let alpha = u64::from(T::AlphaReserve::reserve(netuid))
            .checked_add(delta_in.into())
            .ok_or(Error::<T>::ReservesOutOfBalance)?;
        let tao = u64::from(T::TaoReserve::reserve(netuid))
            .checked_sub(delta_out.into())
            .ok_or(Error::<T>::InsufficientLiquidity)?;
        Pallet::<T>::superellipse(netuid)?
            .calculate_price(alpha, tao)
            .map_err(|_| Error::<T>::ReservesOutOfBalance)
    }

    fn price_is_closer(price1: &U64F64, price2: &U64F64) -> bool {
        price1 >= price2
    }

    fn convert_deltas(netuid: NetUid, input: AlphaBalance) -> Result<TaoBalance, Error<T>> {
        Pallet::<T>::superellipse(netuid)?
            .sell_output(
                T::AlphaReserve::reserve(netuid).into(),
                T::TaoReserve::reserve(netuid).into(),
                input.into(),
            )
            .map(TaoBalance::from)
            .map_err(|_| Error::<T>::ReservesOutOfBalance)
    }
}

pub(crate) trait SwapStep<T, PaidIn, PaidOut>
where
    T: Config,
    PaidIn: Token,
    PaidOut: Token,
{
    /// Largest input supported by the current curve branch and real reserves.
    fn max_input(netuid: NetUid) -> Result<PaidIn, Error<T>>;

    /// Get the input amount needed to reach the target price
    fn delta_in(
        netuid: NetUid,
        price_curr: U64F64,
        price_target: U64F64,
    ) -> Result<PaidIn, Error<T>>;

    /// Get the target price based on the input amount and its precomputed output
    /// (`delta_out` must be `Self::convert_deltas(netuid, delta_in)`; it is passed in so
    /// the expensive conversion is computed once and shared with swap execution)
    fn price_target(
        netuid: NetUid,
        delta_in: PaidIn,
        delta_out: PaidOut,
    ) -> Result<U64F64, Error<T>>;

    /// Returns True if price1 is closer to the current price than price2
    ///    For buying:  price1 <= price2
    ///    For selling: price1 >= price2
    fn price_is_closer(price1: &U64F64, price2: &U64F64) -> bool;

    /// Convert input amount (delta_in) to output amount (delta_out)
    ///
    /// This is the core method of the swap that tells how much output token is given for an
    /// amount of input token within one price tick.
    fn convert_deltas(netuid: NetUid, delta_in: PaidIn) -> Result<PaidOut, Error<T>>;
}

#[derive(Debug, PartialEq)]
pub(crate) struct SwapStepResult<PaidIn, PaidOut>
where
    PaidIn: Token,
    PaidOut: Token,
{
    pub(crate) fee_paid: PaidIn,
    pub(crate) delta_in: PaidIn,
    pub(crate) delta_out: PaidOut,
    pub(crate) fee_to_block_author: PaidIn,
}
