//! Position types (DESIGN-v2 section 3): the side, the leg, accrual, the stored position and the
//! per-subnet market snapshot the operations quote against.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::traits::UniqueSaturatedInto;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::NetUid;

use crate::math::{
    PriceRefs,
    curve::PoolState,
    fixed::{Price, mul_price_floor},
    interest::{accrue, short_notional},
    rate::RateCurve,
};

/// The two sides of a position. A holder has at most one open side per subnet.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub enum Side {
    /// Synthetic alpha minted and sold into the pool; the holder is paid the TAO proceeds.
    Short,
    /// TAO lifted from the pool and swapped for alpha that stays locked in custody.
    Long,
}

impl Side {
    /// The opposite side: a holder may not hold both on one subnet.
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Side::Short => Side::Long,
            Side::Long => Side::Short,
        }
    }
}

/// Key of a position: one per account, subnet and side (the `Positions` map key).
pub type PositionId<AccountId> = (AccountId, NetUid, Side);

/// The side-specific book of a position.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub enum Leg {
    /// `minted` is `q`, the synthetic alpha owed back; `proceeds` is `P`, the TAO paid out at
    /// the mint and held in custody against the buy-back.
    Short { minted: u64, proceeds: u64 },
    /// `locked_alpha` is `A`, the alpha bought with the lifted TAO and held in custody;
    /// `debt` is `D`, the lifted TAO owed back to the pool.
    Long { locked_alpha: u64, debt: u64 },
}

impl Leg {
    /// The side this leg belongs to.
    #[must_use]
    pub fn side(&self) -> Side {
        match self {
            Leg::Short { .. } => Side::Short,
            Leg::Long { .. } => Side::Long,
        }
    }

    /// A leg with nothing on it yet.
    #[must_use]
    pub fn empty(side: Side) -> Self {
        match side {
            Side::Short => Leg::Short {
                minted: 0,
                proceeds: 0,
            },
            Side::Long => Leg::Long {
                locked_alpha: 0,
                debt: 0,
            },
        }
    }

    /// The alpha this leg must give back: `q` for a short, `A` for a long.
    #[must_use]
    pub fn alpha(&self) -> u64 {
        match *self {
            Leg::Short { minted, .. } => minted,
            Leg::Long { locked_alpha, .. } => locked_alpha,
        }
    }

    /// The leg after growing by `size` (`q` or `L`) that bought `value` (`P` or `A`).
    #[must_use]
    pub fn grown(self, size: u64, value: u64) -> Self {
        match self {
            Leg::Short { minted, proceeds } => Leg::Short {
                minted: minted.saturating_add(size),
                proceeds: proceeds.saturating_add(value),
            },
            Leg::Long { locked_alpha, debt } => Leg::Long {
                locked_alpha: locked_alpha.saturating_add(value),
                debt: debt.saturating_add(size),
            },
        }
    }

    /// The leg after closing `alpha` of it, with `consumed` taken from its TAO book: the
    /// proceeds a short spent on its buy-back, or the debt a long repaid.
    #[must_use]
    pub fn shrunk(self, alpha: u64, consumed: u64) -> Self {
        match self {
            Leg::Short { minted, proceeds } => Leg::Short {
                minted: minted.saturating_sub(alpha),
                proceeds: proceeds.saturating_sub(consumed),
            },
            Leg::Long { locked_alpha, debt } => Leg::Long {
                locked_alpha: locked_alpha.saturating_sub(alpha),
                debt: debt.saturating_sub(consumed),
            },
        }
    }

    /// The interest notional (SPEC 1.2): `floor(q * TWAP_median)` for a short, the debt for a
    /// long.
    #[must_use]
    pub fn notional(&self, twap: Price) -> u64 {
        match *self {
            Leg::Short { minted, .. } => short_notional(minted, twap),
            Leg::Long { debt, .. } => debt,
        }
    }

    /// The two terms of `u`, the debt over the holdings the rate curve reads (SPEC section 1
    /// item 2, 7.1 item 4), with `collateral` the position's `C`. The alpha is valued at the
    /// slow price, the one of the TWAP median and the 30-day rank SMA that makes `u` smaller:
    /// a short has `floor(q * min(TWAP_median, SMA30_rank))` over `C + P`, a long has `D` over
    /// `floor(A * max(TWAP_median, SMA30_rank)) + C`. The notional interest accrues on stays at
    /// the TWAP median ([`Leg::notional`]).
    #[must_use]
    pub fn debt_over_holdings(&self, collateral: u64, refs: &PriceRefs) -> (u128, u128) {
        let collateral = u128::from(collateral);
        match *self {
            Leg::Short { minted, proceeds } => (
                u128::from(mul_price_floor(minted, refs.twap.min(refs.sma))),
                collateral.saturating_add(u128::from(proceeds)),
            ),
            Leg::Long { locked_alpha, debt } => (
                u128::from(debt),
                u128::from(mul_price_floor(locked_alpha, refs.twap.max(refs.sma)))
                    .saturating_add(collateral),
            ),
        }
    }

    /// The TAO a short holds against its buy-back (`P`); a long holds none.
    #[must_use]
    pub fn proceeds(&self) -> u64 {
        match *self {
            Leg::Short { proceeds, .. } => proceeds,
            Leg::Long { .. } => 0,
        }
    }
}

/// Interest accrual of one position (SPEC 1.2, P15). Charge-on-touch: every operation settles
/// the accrual to the current block before it reads or moves collateral.
#[freeze_struct("8cb4590d8ccfd129")]
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub struct Accrual<BlockNumber> {
    /// Block the accrual was last settled to.
    pub last_accrued: BlockNumber,
    /// Fraction of a rao carried to the next accrual (see `math::interest::accrue`).
    pub remainder: u128,
    /// Interest accrued but not yet charged to the pool (the model's `due`).
    pub due: u64,
}

/// A stored position (DESIGN-v2 section 3). Its custody account is derived from the side, the
/// holder, the subnet and `opened_at`, so a closed position's custody is never reused.
#[freeze_struct("89d22f06a06265cc")]
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub struct Position<BlockNumber> {
    /// Collateral `C` in custody, in rao TAO.
    pub collateral: u64,
    /// Refundable open deposit (SPEC P19), returned on a full close.
    pub deposit: u64,
    /// Block the position was opened.
    pub opened_at: BlockNumber,
    /// Interest accrual.
    pub accrual: Accrual<BlockNumber>,
    /// Block of the position's one scheduled `Due` entry (SPEC P15, I5).
    pub charge_at: BlockNumber,
    /// Side-specific book.
    pub leg: Leg,
}

/// One interest charge: what an accrual takes from the collateral, and the rate it accrued at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Charge {
    /// `min(due, C)` in rao TAO, which the caller moves to the pool.
    pub amount: u64,
    /// The yearly rate in bps the accrual read from the position (SPEC I23).
    pub rate_bps: u32,
}

impl<BlockNumber: Copy + UniqueSaturatedInto<u64>> Position<BlockNumber> {
    /// The yearly rate in bps of the position as it stands (SPEC section 1 item 2): the curve
    /// at its debt over its holdings, the alpha valued at the slow price of `refs`.
    #[must_use]
    pub fn rate_bps(&self, refs: &PriceRefs, curve: &RateCurve) -> u32 {
        let (debt, holdings) = self.leg.debt_over_holdings(self.collateral, refs);
        curve.rate(debt, holdings)
    }

    /// Charge-on-touch settlement (SPEC section 1 item 2, P15): reads the rate from the position
    /// as stored, before anything changes it, accrues interest at that rate on the leg's notional
    /// (at the TWAP median) up to `now`, then takes `min(due, collateral)` from the collateral. Returns the settled
    /// position and the charge, which the caller moves to the pool at commit (pool-internal, not
    /// flow).
    ///
    /// The rate is read here and nowhere else, so whatever the caller does to the position
    /// next (a top-up, a resize) changes the rate only from this block on, and a
    /// change of the curve applies from each position's next accrual.
    #[must_use]
    pub fn settle(self, refs: &PriceRefs, curve: &RateCurve, now: BlockNumber) -> (Self, Charge) {
        let now_blocks: u64 = now.unique_saturated_into();
        let last: u64 = self.accrual.last_accrued.unique_saturated_into();
        let elapsed = now_blocks.saturating_sub(last);
        let rate_bps = self.rate_bps(refs, curve);
        let owed = accrue(
            self.leg.notional(refs.twap),
            rate_bps,
            elapsed,
            self.accrual.remainder,
        );
        let due = self.accrual.due.saturating_add(owed.owed);
        let amount = due.min(self.collateral);
        let settled = Self {
            collateral: self.collateral.saturating_sub(amount),
            accrual: Accrual {
                last_accrued: now,
                remainder: owed.remainder,
                due: due.saturating_sub(amount),
            },
            ..self
        };
        (settled, Charge { amount, rate_bps })
    }
}

impl<BlockNumber: Copy> Position<BlockNumber> {
    /// A position opened at `now` with nothing deposited yet and its first charge scheduled at
    /// `charge_at`.
    #[must_use]
    pub fn fresh(side: Side, now: BlockNumber, charge_at: BlockNumber) -> Self {
        Self {
            collateral: 0,
            deposit: 0,
            opened_at: now,
            charge_at,
            accrual: Accrual {
                last_accrued: now,
                remainder: 0,
                due: 0,
            },
            leg: Leg::empty(side),
        }
    }
}

/// Cumulative interest of one subnet (SPEC I19).
#[freeze_struct("6559790839bae251")]
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub struct InterestTotal {
    /// Σ interest taken from the collateral of the subnet's positions, in rao.
    pub charged: u128,
    /// Σ rise of the pool's TAO reserve measured across those credits, in rao.
    pub to_pool: u128,
}

/// One subnet's state at the start of an operation, read once and then used for every gate and
/// quote of that operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Market {
    /// Live pool reserves (`SubnetTAO`, `SubnetAlphaIn`).
    pub pool: PoolState,
    /// The three price references (SPEC 1.1, P6, P13).
    pub refs: PriceRefs,
    /// `SyntheticOutstanding`: the short book `Q`.
    pub short_book: u64,
    /// `LongLockedAlpha`: the long book, alpha held by all longs.
    pub long_book: u64,
    /// `LongLiftedTao`: the long debt, TAO lifted from the pool and not yet repaid.
    pub long_debt: u64,
}

impl Market {
    /// The book of one side: `Q` for shorts, the alpha locked by longs otherwise.
    #[must_use]
    pub fn book(&self, side: Side) -> u64 {
        match side {
            Side::Short => self.short_book,
            Side::Long => self.long_book,
        }
    }
}
