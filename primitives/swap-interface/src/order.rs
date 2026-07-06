use core::marker::PhantomData;

use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, TaoBalance, Token, TokenReserve};

pub trait Order: Clone {
    type PaidIn: Token;
    type PaidOut: Token;
    type ReserveIn: TokenReserve<Self::PaidIn>;
    type ReserveOut: TokenReserve<Self::PaidOut>;

    fn with_amount(amount: impl Into<Self::PaidIn>) -> Self;
    fn amount(&self) -> Self::PaidIn;
    fn is_beyond_price_limit(&self, current_price: U64F64, limit_price: U64F64) -> bool;
}

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
        current_price < limit_price
    }
}

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
        current_price > limit_price
    }
}
