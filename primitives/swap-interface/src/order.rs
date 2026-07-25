//! Typed AMM orders: pay TAO for alpha ([`GetAlphaForTao`]) or alpha for TAO ([`GetTaoForAlpha`]).

use core::marker::PhantomData;

use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, TaoBalance, Token, TokenReserve};

/// Directional swap order: fixed paid-in amount against a subnet's TAO/alpha reserves.
pub trait Order: Clone {
    type PaidIn: Token;
    type PaidOut: Token;
    type ReserveIn: TokenReserve<Self::PaidIn>;
    type ReserveOut: TokenReserve<Self::PaidOut>;

    /// Build an order that pays exactly `amount` of [`Self::PaidIn`].
    fn with_amount(amount: impl Into<Self::PaidIn>) -> Self;
    /// Paid-in size of this order.
    fn amount(&self) -> Self::PaidIn;
    /// True when the current spot price has moved past `limit_price` for this direction.
    fn is_beyond_price_limit(&self, current_price: U64F64, limit_price: U64F64) -> bool;
}

/// Buy order: pay TAO into the pool, receive alpha out.
#[derive(Clone, Default)]
pub struct GetAlphaForTao<ReserveIn, ReserveOut>
where
    ReserveIn: TokenReserve<TaoBalance>,
    ReserveOut: TokenReserve<AlphaBalance>,
{
    amount: TaoBalance,
    _phantom: PhantomData<(ReserveIn, ReserveOut)>,
}

impl<ReserveIn, ReserveOut> Order for GetAlphaForTao<ReserveIn, ReserveOut>
where
    ReserveIn: TokenReserve<TaoBalance> + Clone,
    ReserveOut: TokenReserve<AlphaBalance> + Clone,
{
    type PaidIn = TaoBalance;
    type PaidOut = AlphaBalance;
    type ReserveIn = ReserveIn;
    type ReserveOut = ReserveOut;

    fn with_amount(amount: impl Into<Self::PaidIn>) -> Self {
        Self {
            amount: amount.into(),
            _phantom: PhantomData,
        }
    }

    fn amount(&self) -> TaoBalance {
        self.amount
    }

    fn is_beyond_price_limit(&self, current_price: U64F64, limit_price: U64F64) -> bool {
        // Buying alpha: reject when spot is already below the caller's minimum TAO/alpha.
        current_price < limit_price
    }
}

/// Sell order: pay alpha into the pool, receive TAO out.
#[derive(Clone, Default)]
pub struct GetTaoForAlpha<ReserveIn, ReserveOut>
where
    ReserveIn: TokenReserve<AlphaBalance>,
    ReserveOut: TokenReserve<TaoBalance>,
{
    amount: AlphaBalance,
    _phantom: PhantomData<(ReserveIn, ReserveOut)>,
}

impl<ReserveIn, ReserveOut> Order for GetTaoForAlpha<ReserveIn, ReserveOut>
where
    ReserveIn: TokenReserve<AlphaBalance> + Clone,
    ReserveOut: TokenReserve<TaoBalance> + Clone,
{
    type PaidIn = AlphaBalance;
    type PaidOut = TaoBalance;
    type ReserveIn = ReserveIn;
    type ReserveOut = ReserveOut;

    fn with_amount(amount: impl Into<Self::PaidIn>) -> Self {
        Self {
            amount: amount.into(),
            _phantom: PhantomData,
        }
    }

    fn amount(&self) -> AlphaBalance {
        self.amount
    }

    fn is_beyond_price_limit(&self, current_price: U64F64, limit_price: U64F64) -> bool {
        // Selling alpha: reject when spot is already above the caller's maximum TAO/alpha.
        current_price > limit_price
    }
}
