//! Position types and the pure arithmetic behind opening and settling.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::{
    Perbill, Percent, RuntimeDebug,
    traits::{Saturating, UniqueSaturatedInto, Zero},
};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, TaoBalance, Token};
use subtensor_swap_interface::Perquintill;

/// Blocks in one day at a 12-second block time. The rent is set per year and accrued per
/// block; a position carries it as a per-day amount, and each add books one day up front.
pub const BLOCKS_PER_DAY: u64 = 7_200;

/// Days the yearly rate is spread over.
pub const DAYS_PER_YEAR: u64 = 365;

/// Direction of a position.
///
/// * `Short`: the pool lends alpha; the user owes alpha back and holds the TAO it sold for.
/// * `Long`: the pool lends TAO; the user owes TAO back and holds the alpha it bought.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    RuntimeDebug,
)]
pub enum Side {
    Short,
    Long,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Short => Side::Long,
            Side::Long => Side::Short,
        }
    }
}

/// The lifted slice after the opening trade. The variant is the side, so every leg carries its
/// own token and no reader has to remember which is which.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    RuntimeDebug,
)]
pub enum Legs {
    /// The pool lent alpha, which was sold for TAO.
    Short {
        /// `N`: TAO the lifted alpha sold for. Held by the pallet until close.
        proceeds: TaoBalance,
        /// `Q`: alpha that must be bought back and returned to the pool.
        debt: AlphaBalance,
        /// `E`: the lifted TAO, held untouched and returned as-is.
        escrow: TaoBalance,
    },
    /// The pool lent TAO, which was spent on alpha.
    Long {
        /// `N`: alpha the lifted TAO bought. Held as stake until close.
        proceeds: AlphaBalance,
        /// `D`: TAO that must be repaid to the pool.
        debt: TaoBalance,
        /// `E`: the lifted alpha, held untouched and returned as-is.
        escrow: AlphaBalance,
    },
}

impl Legs {
    /// No slice lifted yet, on `side`.
    pub fn empty(side: Side) -> Self {
        match side {
            Side::Short => Legs::Short {
                proceeds: TaoBalance::ZERO,
                debt: AlphaBalance::ZERO,
                escrow: TaoBalance::ZERO,
            },
            Side::Long => Legs::Long {
                proceeds: AlphaBalance::ZERO,
                debt: TaoBalance::ZERO,
                escrow: AlphaBalance::ZERO,
            },
        }
    }

    pub fn side(&self) -> Side {
        match self {
            Legs::Short { .. } => Side::Short,
            Legs::Long { .. } => Side::Long,
        }
    }

    /// What the pallet holds for the pool, in the lent token: `proceeds + escrow`. Summed per
    /// side in `Footprint` and compared against `max_pool_share`.
    pub fn footprint(&self) -> u64 {
        match self {
            Legs::Short {
                proceeds, escrow, ..
            } => proceeds.saturating_add(*escrow).to_u64(),
            Legs::Long {
                proceeds, escrow, ..
            } => proceeds.saturating_add(*escrow).to_u64(),
        }
    }

    /// Alpha that changes hands when this settles: what a short must buy back, what a long has
    /// to sell. Summed per side in `AlphaToSettle` so a dissolution can net the two.
    pub fn alpha_to_settle(&self) -> u64 {
        match self {
            Legs::Short { debt, .. } => debt.to_u64(),
            Legs::Long { proceeds, .. } => proceeds.to_u64(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Legs::Short {
                proceeds,
                debt,
                escrow,
            } => proceeds.is_zero() && debt.is_zero() && escrow.is_zero(),
            Legs::Long {
                proceeds,
                debt,
                escrow,
            } => proceeds.is_zero() && debt.is_zero() && escrow.is_zero(),
        }
    }

    /// Legs are sums of lifted slices, so two of the same side add leg by leg. `None` when the
    /// sides differ.
    pub fn plus(&self, other: &Legs) -> Option<Legs> {
        match (self, other) {
            (
                Legs::Short {
                    proceeds,
                    debt,
                    escrow,
                },
                Legs::Short {
                    proceeds: p2,
                    debt: d2,
                    escrow: e2,
                },
            ) => Some(Legs::Short {
                proceeds: proceeds.saturating_add(*p2),
                debt: debt.saturating_add(*d2),
                escrow: escrow.saturating_add(*e2),
            }),
            (
                Legs::Long {
                    proceeds,
                    debt,
                    escrow,
                },
                Legs::Long {
                    proceeds: p2,
                    debt: d2,
                    escrow: e2,
                },
            ) => Some(Legs::Long {
                proceeds: proceeds.saturating_add(*p2),
                debt: debt.saturating_add(*d2),
                escrow: escrow.saturating_add(*e2),
            }),
            _ => None,
        }
    }

    /// The `fraction` of each leg that a partial settlement unwinds, rounded down so the
    /// remainder (`self` minus this) never goes negative.
    pub fn part(&self, fraction: Perquintill) -> Legs {
        match self {
            Legs::Short {
                proceeds,
                debt,
                escrow,
            } => Legs::Short {
                proceeds: TaoBalance::from(fraction.mul_floor(proceeds.to_u64())),
                debt: AlphaBalance::from(fraction.mul_floor(debt.to_u64())),
                escrow: TaoBalance::from(fraction.mul_floor(escrow.to_u64())),
            },
            Legs::Long {
                proceeds,
                debt,
                escrow,
            } => Legs::Long {
                proceeds: AlphaBalance::from(fraction.mul_floor(proceeds.to_u64())),
                debt: TaoBalance::from(fraction.mul_floor(debt.to_u64())),
                escrow: AlphaBalance::from(fraction.mul_floor(escrow.to_u64())),
            },
        }
    }

    /// `self` minus `part`, leg by leg. Saturating; `part` is expected to come from
    /// [`Legs::part`] of `self`.
    pub fn minus(&self, part: &Legs) -> Legs {
        match (self, part) {
            (
                Legs::Short {
                    proceeds,
                    debt,
                    escrow,
                },
                Legs::Short {
                    proceeds: p2,
                    debt: d2,
                    escrow: e2,
                },
            ) => Legs::Short {
                proceeds: proceeds.saturating_sub(*p2),
                debt: debt.saturating_sub(*d2),
                escrow: escrow.saturating_sub(*e2),
            },
            (
                Legs::Long {
                    proceeds,
                    debt,
                    escrow,
                },
                Legs::Long {
                    proceeds: p2,
                    debt: d2,
                    escrow: e2,
                },
            ) => Legs::Long {
                proceeds: proceeds.saturating_sub(*p2),
                debt: debt.saturating_sub(*d2),
                escrow: escrow.saturating_sub(*e2),
            },
            _ => *self,
        }
    }
}

/// An amount in the token the pool lent: alpha for a short, TAO for a long.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    RuntimeDebug,
)]
pub enum Lent {
    Alpha(AlphaBalance),
    Tao(TaoBalance),
}

impl Lent {
    pub fn is_zero(&self) -> bool {
        match self {
            Lent::Alpha(amount) => amount.is_zero(),
            Lent::Tao(amount) => amount.is_zero(),
        }
    }
}

/// What one `add` puts up as cushion. TAO comes from the caller's free balance; alpha is stake
/// the caller holds at `hotkey` on the position's subnet, and goes back there at close.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    PartialEq,
    Eq,
    RuntimeDebug,
)]
pub enum Deposit<AccountId> {
    Tao(TaoBalance),
    Alpha {
        hotkey: AccountId,
        amount: AlphaBalance,
    },
}

impl<AccountId: Clone> Deposit<AccountId> {
    pub fn is_zero(&self) -> bool {
        match self {
            Deposit::Tao(amount) => amount.is_zero(),
            Deposit::Alpha { amount, .. } => amount.is_zero(),
        }
    }

    /// The `fraction` of this deposit, rounded down. Used when an add flips through zero and
    /// only the part past the flip point opens the new position.
    pub fn part(&self, fraction: Perquintill) -> Self {
        match self {
            Deposit::Tao(amount) => {
                Deposit::Tao(TaoBalance::from(fraction.mul_floor(amount.to_u64())))
            }
            Deposit::Alpha { hotkey, amount } => Deposit::Alpha {
                hotkey: hotkey.clone(),
                amount: AlphaBalance::from(fraction.mul_floor(amount.to_u64())),
            },
        }
    }
}

/// What the owner has put up, in total: TAO and alpha side by side, since tranches may be added
/// in either. Returned in kind as the position is settled, minus fees and losses; alpha that
/// cannot go back to `alpha_hotkey` is sold and returned as TAO.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    PartialEq,
    Eq,
    RuntimeDebug,
)]
pub struct Cushion<AccountId> {
    pub tao: TaoBalance,
    pub alpha: AlphaBalance,
    /// Where the alpha goes back: the hotkey of the latest alpha deposit. `None` while the
    /// cushion holds no alpha.
    pub alpha_hotkey: Option<AccountId>,
}

impl<AccountId: Clone> Cushion<AccountId> {
    pub fn tao_only(tao: TaoBalance) -> Self {
        Self {
            tao,
            alpha: AlphaBalance::ZERO,
            alpha_hotkey: None,
        }
    }

    /// Fold a deposit in. An alpha deposit makes its hotkey the one the alpha goes back to.
    pub fn plus(&self, deposit: &Deposit<AccountId>) -> Self {
        match deposit {
            Deposit::Tao(amount) => Self {
                tao: self.tao.saturating_add(*amount),
                alpha: self.alpha,
                alpha_hotkey: self.alpha_hotkey.clone(),
            },
            Deposit::Alpha { hotkey, amount } => Self {
                tao: self.tao,
                alpha: self.alpha.saturating_add(*amount),
                alpha_hotkey: Some(hotkey.clone()),
            },
        }
    }

    /// The `fraction` of each leg, rounded down, keeping the hotkey.
    pub fn part(&self, fraction: Perquintill) -> Self {
        Self {
            tao: TaoBalance::from(fraction.mul_floor(self.tao.to_u64())),
            alpha: AlphaBalance::from(fraction.mul_floor(self.alpha.to_u64())),
            alpha_hotkey: self.alpha_hotkey.clone(),
        }
    }

    /// `self` minus `part`, leg by leg. Saturating; `part` is expected to come from
    /// [`Cushion::part`] of `self`.
    pub fn minus(&self, part: &Self) -> Self {
        Self {
            tao: self.tao.saturating_sub(part.tao),
            alpha: self.alpha.saturating_sub(part.alpha),
            alpha_hotkey: self.alpha_hotkey.clone(),
        }
    }

    /// The cushion in TAO, given what its alpha would sell for right now.
    pub fn value(&self, alpha_quote: TaoBalance) -> TaoBalance {
        self.tao.saturating_add(alpha_quote)
    }
}

/// One open position: the sum of every tranche the owner has added on this subnet, less
/// whatever has been settled. One per `(owner, netuid)`; the side is the sign of the exposure.
///
/// Every field but the block numbers is a plain sum, so adding a tranche is addition and
/// settling a fraction is multiplication. Nothing here is per tranche. A position has no term:
/// it lives until its owner closes it or it can no longer pay its rent, after which anyone may
/// close it.
#[freeze_struct("f34b637020d14de8")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    PartialEq,
    Eq,
    RuntimeDebug,
)]
pub struct Position<AccountId, BlockNumber> {
    /// `P`: what the owner has put up, in total, TAO and alpha. Returned in kind as the
    /// position is settled, minus fees and losses.
    pub cushion: Cushion<AccountId>,
    /// The borrowed slice: proceeds held, debt owed, escrow kept. Its variant is the side.
    pub legs: Legs,
    /// `sum(phi * T)` over tranches: the TAO value the pool has lent. The position's leverage
    /// is `exposure_tao / cushion`.
    pub exposure_tao: TaoBalance,
    /// Rent per day for the whole position: `rate_per_year * exposure / 365` of each tranche,
    /// fixed when it was added, summed.
    pub fee_per_day: TaoBalance,
    /// Rent owed and not yet paid, as of `last_touch`. Every add puts one day of the new
    /// tranche's rate here up front; every settlement pays it down.
    pub fee_accrued: TaoBalance,
    /// Block the rent was last brought up to date: the latest add or settlement.
    pub last_touch: BlockNumber,
    /// Block of the first add.
    pub opened_at: BlockNumber,
}

/// The two pool quotes a position's value depends on, both exact and fee-free as of now.
#[derive(Clone, Copy, PartialEq, Eq, RuntimeDebug)]
pub struct Quotes {
    /// TAO to buy back a short's debt, or TAO a long's proceeds sell for.
    pub legs: TaoBalance,
    /// TAO the cushion's alpha sells for. Zero for a TAO-only cushion.
    pub cushion_alpha: TaoBalance,
}

impl<AccountId: Clone, BlockNumber: Copy + Saturating + UniqueSaturatedInto<u64>>
    Position<AccountId, BlockNumber>
{
    /// A position with nothing in it yet, born at `now`. The first tranche folded in opens it.
    pub fn empty(side: Side, now: BlockNumber) -> Self {
        Self {
            cushion: Cushion::tao_only(TaoBalance::ZERO),
            legs: Legs::empty(side),
            exposure_tao: TaoBalance::ZERO,
            fee_per_day: TaoBalance::ZERO,
            fee_accrued: TaoBalance::ZERO,
            last_touch: now,
            opened_at: now,
        }
    }

    pub fn side(&self) -> Side {
        self.legs.side()
    }

    /// Fold a tranche of the same side in at `now`. Every field is a sum, so this is addition;
    /// the fee is brought up to date first so the new rate only runs from now, and one day of
    /// it is booked up front. `None` if the tranche is on the other side.
    pub fn fold(&mut self, tranche: Tranche<AccountId>, now: BlockNumber) -> Option<()> {
        let legs = self.legs.plus(&tranche.legs)?;
        self.fee_accrued = self.fee_owed(now).saturating_add(tranche.fee_per_day);
        self.last_touch = now;
        self.cushion = self.cushion.plus(&tranche.cushion);
        self.legs = legs;
        self.exposure_tao = self.exposure_tao.saturating_add(tranche.exposure_tao);
        self.fee_per_day = self.fee_per_day.saturating_add(tranche.fee_per_day);
        Some(())
    }

    /// Everything owed at `now`: the accrued balance plus the running rate since `last_touch`.
    pub fn fee_owed(&self, now: BlockNumber) -> TaoBalance {
        let blocks: u64 = now.saturating_sub(self.last_touch).unique_saturated_into();
        self.fee_accrued
            .saturating_add(fee_for_blocks(self.fee_per_day, blocks))
    }

    /// What a full close at `now` would leave the owner, in TAO, at the given quotes. Negative
    /// means underwater. Quoting the two alpha amounts separately understates what they would
    /// fetch together, so this errs on the side of calling a position unhealthy.
    pub fn equity(&self, now: BlockNumber, quotes: Quotes) -> i128 {
        let cushion = i128::from(self.cushion.value(quotes.cushion_alpha).to_u64());
        let fee = i128::from(self.fee_owed(now).to_u64());
        let quote = i128::from(quotes.legs.to_u64());
        let legs = match self.legs {
            Legs::Short { proceeds, .. } => i128::from(proceeds.to_u64()).saturating_sub(quote),
            Legs::Long { debt, .. } => quote.saturating_sub(i128::from(debt.to_u64())),
        };
        cushion.saturating_add(legs).saturating_sub(fee)
    }

    /// A position can pay its way while its equity covers one more day of rent. Below that
    /// anyone may close it and is paid the rent for doing so.
    pub fn is_healthy(&self, now: BlockNumber, quotes: Quotes) -> bool {
        self.equity(now, quotes) >= i128::from(self.fee_per_day.to_u64())
    }
}

/// What one `add` contributes to a position. Same shape as the sums it goes into.
pub struct Tranche<AccountId> {
    pub cushion: Deposit<AccountId>,
    pub legs: Legs,
    pub exposure_tao: TaoBalance,
    pub fee_per_day: TaoBalance,
}

/// Root-settable parameters. Two of them are the design: `max_pool_share` is how much of a pool
/// is for rent, `rate_per_year` is the rent. The rest are switches and safety bounds.
#[freeze_struct("4b841bde447129eb")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    PartialEq,
    Eq,
    RuntimeDebug,
)]
pub struct DerivativesParams {
    pub shorts_enabled: bool,
    pub longs_enabled: bool,
    /// Whether a short may put up alpha as cushion. A short holder posting alpha is betting
    /// against what they hold, which is the one alpha cushion a subnet team has no use for.
    pub alpha_cushion_shorts: bool,
    /// Whether a long may put up alpha as cushion. Off, a team cannot post self-minted alpha
    /// and long its own pool; on, that lever exists.
    pub alpha_cushion_longs: bool,
    /// Highest leverage a short may choose, as a percentage of its cushion. `100` = 1x. A
    /// short at leverage `L` costs the pool once the price rises by `1 / L`: 2x at 1x.
    pub max_short_leverage_percent: u16,
    /// Highest leverage a long may choose. `200` = 2x. A long at leverage `L` costs the pool
    /// once the price falls by `1 / L`: a halving at 2x. At 1x a long can never lose the pool
    /// anything, and is nothing a spot buy does not do better.
    pub max_long_leverage_percent: u16,
    /// `kappa`: the largest share of the lent reserve that all open positions of one side on
    /// one subnet may borrow together. A subnet's [`SubnetOverride`] can replace it.
    pub max_pool_share: Percent,
    /// `X`: the rent per year, as a fraction of `exposure_tao`, the same on both sides.
    /// Accrued per block; fixed for each tranche when it is added. A subnet's
    /// [`SubnetOverride`] can replace it.
    pub rate_per_year: Perbill,
    /// Smallest cushion, measured in TAO at the open price.
    pub min_deposit_tao: TaoBalance,
}

impl DerivativesParams {
    /// Mainnet defaults: shorts up to 1x, longs up to 2x, TAO cushions only, 10% of the pool
    /// for rent at 20% a year (about 0.055% a day, 1.6% a month), 0.1 TAO minimum cushion.
    ///
    /// The rent covers the pool's measured expected loss to pumps on pools above a few thousand
    /// TAO (`E[(theta - 2)+] * T ~= 86 TAO` per 30 days for a whole pool, over a year of Finney
    /// prices). Smaller pools carry more pump risk per unit of exposure; root prices them with
    /// the per-subnet rate override or a lower cap. There is no term: a position runs while it
    /// pays its rent, and the rent is what makes holding one indefinitely cost something.
    pub fn defaults() -> Self {
        Self {
            shorts_enabled: true,
            longs_enabled: true,
            alpha_cushion_shorts: false,
            alpha_cushion_longs: false,
            max_short_leverage_percent: 100,
            max_long_leverage_percent: 200,
            max_pool_share: Percent::from_percent(10),
            rate_per_year: Perbill::from_percent(20),
            min_deposit_tao: TaoBalance::from(100_000_000),
        }
    }

    pub fn max_leverage_percent(&self, side: Side) -> u16 {
        match side {
            Side::Short => self.max_short_leverage_percent,
            Side::Long => self.max_long_leverage_percent,
        }
    }

    /// Whether an owner may open `side` at `leverage_percent`: above zero and at most the
    /// side's maximum.
    pub fn leverage_allowed(&self, side: Side, leverage_percent: u16) -> bool {
        leverage_percent > 0 && leverage_percent <= self.max_leverage_percent(side)
    }

    pub fn side_enabled(&self, side: Side) -> bool {
        match side {
            Side::Short => self.shorts_enabled,
            Side::Long => self.longs_enabled,
        }
    }

    /// Whether `side` accepts an alpha cushion. TAO is always accepted.
    pub fn alpha_cushion_allowed(&self, side: Side) -> bool {
        match side {
            Side::Short => self.alpha_cushion_shorts,
            Side::Long => self.alpha_cushion_longs,
        }
    }

    /// Rent per day for a new tranche: `rate_per_year * exposure / 365`, whichever side.
    pub fn fee_per_day(rate_per_year: Perbill, exposure_tao: TaoBalance) -> TaoBalance {
        let per_year = rate_per_year.mul_floor(exposure_tao.to_u64());
        TaoBalance::from(per_year.checked_div(DAYS_PER_YEAR).unwrap_or(0))
    }

    /// A parameter set every add can act on. A zero maximum leverage or a zero pool share
    /// would make every add fail, and a zero rate would leave nobody paid to liquidate. Use
    /// `shorts_enabled` / `longs_enabled` to pause adds instead.
    pub fn is_valid(&self) -> bool {
        self.max_short_leverage_percent > 0
            && self.max_long_leverage_percent > 0
            && !self.max_pool_share.is_zero()
            && !self.rate_per_year.is_zero()
    }
}

/// Root-settable per-subnet overrides. Absent means the global parameters apply. Only adds
/// look at it: a paused side can still close and reduce.
#[freeze_struct("94414fc12b6c8ac7")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    RuntimeDebug,
)]
pub struct SubnetOverride {
    pub shorts_enabled: bool,
    pub longs_enabled: bool,
    /// Replaces the global `max_pool_share` on this subnet when set.
    pub max_pool_share: Option<Percent>,
    /// Replaces the global `rate_per_year` on this subnet when set: the lever for a small pool
    /// whose pump risk the flat rate underprices.
    pub rate_per_year: Option<Perbill>,
}

impl SubnetOverride {
    pub fn side_enabled(&self, side: Side) -> bool {
        match side {
            Side::Short => self.shorts_enabled,
            Side::Long => self.longs_enabled,
        }
    }

    /// A zero cap would make every add fail and a zero rate would leave nobody paid to
    /// liquidate; pause the side instead.
    pub fn is_valid(&self) -> bool {
        self.max_pool_share.is_none_or(|share| !share.is_zero())
            && self.rate_per_year.is_none_or(|rate| !rate.is_zero())
    }
}

/// TAO value of `amount` alpha at the spot ratio `tao_reserve / alpha_reserve`, rounded down.
/// Sizes an alpha cushion at open; settlement itself goes through the pool.
pub fn alpha_value_in_tao(amount: u64, tao_reserve: u64, alpha_reserve: u64) -> u64 {
    if alpha_reserve == 0 {
        return 0;
    }
    (amount as u128)
        .saturating_mul(tao_reserve as u128)
        .checked_div(alpha_reserve as u128)
        .unwrap_or(0)
        .min(u64::MAX as u128) as u64
}

/// `phi = L * amount / reserve`, as a fraction of the pool. `None` when the position would
/// take the whole pool or more.
pub fn pool_fraction(leverage_percent: u16, amount: u64, reserve: u64) -> Option<Perquintill> {
    let numer = (amount as u128).saturating_mul(leverage_percent as u128);
    let denom = (reserve as u128).saturating_mul(100);
    if denom == 0 || numer >= denom {
        return None;
    }
    let phi = Perquintill::from_rational(numer, denom);
    if phi.is_zero() { None } else { Some(phi) }
}

/// Projected footprint of a new position in the lent reserve: `phi * (2 - phi) * reserve`.
/// The lifted half is `phi * R`; swapping the other half back into the shrunken pool yields
/// about `phi * (1 - phi) * R` more.
pub fn projected_footprint(phi: Perquintill, lent_reserve: u64) -> u64 {
    let lifted = phi.mul_floor(lent_reserve);
    lifted
        .saturating_mul(2)
        .saturating_sub(phi.mul_floor(lifted))
}

/// Fee that `blocks` blocks accrue at `fee_per_day`, pro rata. The one-day minimum is not
/// here: each add books one day of its tranche's rate into `fee_accrued` up front.
pub fn fee_for_blocks(fee_per_day: TaoBalance, blocks: u64) -> TaoBalance {
    let fee = (fee_per_day.to_u64() as u128)
        .saturating_mul(blocks as u128)
        .checked_div(BLOCKS_PER_DAY as u128)
        .unwrap_or(0)
        .min(u64::MAX as u128) as u64;
    TaoBalance::from(fee)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn fraction_is_leverage_times_share() {
        let phi = pool_fraction(100, 10, 1_000).unwrap();
        assert_eq!(phi.mul_floor(1_000u64), 10);
        let phi = pool_fraction(200, 10, 1_000).unwrap();
        assert_eq!(phi.mul_floor(1_000u64), 20);
        assert!(pool_fraction(100, 1_000, 1_000).is_none());
        assert!(pool_fraction(100, 0, 1_000).is_none());
        assert!(pool_fraction(100, 10, 0).is_none());
    }

    #[test]
    fn footprint_is_phi_two_minus_phi() {
        let phi = Perquintill::from_percent(10);
        // 0.1 * 1.9 * 1000 = 190
        assert_eq!(projected_footprint(phi, 1_000), 190);
    }

    #[test]
    fn fee_accrues_pro_rata_per_block() {
        let per_day = TaoBalance::from(7_200_000);
        assert_eq!(fee_for_blocks(per_day, 0), TaoBalance::from(0));
        assert_eq!(fee_for_blocks(per_day, 1), TaoBalance::from(1_000));
        assert_eq!(fee_for_blocks(per_day, BLOCKS_PER_DAY), per_day);
        assert_eq!(
            fee_for_blocks(per_day, 30 * BLOCKS_PER_DAY),
            TaoBalance::from(216_000_000)
        );
    }

    #[test]
    fn legs_add_scale_and_subtract_leg_by_leg() {
        let a = Legs::Short {
            proceeds: TaoBalance::from(100),
            debt: AlphaBalance::from(400),
            escrow: TaoBalance::from(100),
        };
        let b = Legs::Short {
            proceeds: TaoBalance::from(50),
            debt: AlphaBalance::from(200),
            escrow: TaoBalance::from(50),
        };
        let sum = a.plus(&b).unwrap();
        assert_eq!(sum.footprint(), 300);
        let third = sum.part(Perquintill::from_rational(1u64, 3u64));
        assert_eq!(third.footprint(), 98); // 49 + 49: each leg rounds down on its own
        let rest = sum.minus(&third);
        assert_eq!(rest.plus(&third).unwrap(), sum);
        assert!(!rest.is_empty());
        assert!(sum.minus(&sum).is_empty());

        let long = Legs::Long {
            proceeds: AlphaBalance::from(1),
            debt: TaoBalance::from(1),
            escrow: AlphaBalance::from(1),
        };
        assert!(a.plus(&long).is_none());
    }

    #[test]
    fn position_fee_is_accrued_plus_running_rate() {
        let position = Position::<u64, u64> {
            cushion: Cushion::tao_only(TaoBalance::from(0)),
            legs: Legs::Short {
                proceeds: TaoBalance::from(0),
                debt: AlphaBalance::from(0),
                escrow: TaoBalance::from(0),
            },
            exposure_tao: TaoBalance::from(0),
            fee_per_day: TaoBalance::from(7_200),
            fee_accrued: TaoBalance::from(7_200),
            last_touch: 10,
            opened_at: 10,
        };
        assert_eq!(position.fee_owed(10), TaoBalance::from(7_200));
        assert_eq!(position.fee_owed(3_610), TaoBalance::from(10_800));
        // A clock that ran backwards owes nothing extra.
        assert_eq!(position.fee_owed(5), TaoBalance::from(7_200));
    }

    /// Quotes with a TAO-only cushion.
    fn q(legs: u64) -> Quotes {
        Quotes {
            legs: TaoBalance::from(legs),
            cushion_alpha: TaoBalance::ZERO,
        }
    }

    #[test]
    fn equity_is_what_a_close_would_pay_and_health_is_one_more_day_of_fee() {
        // Short: 100 cushion, sold for 100, owes alpha; fee 10/day, one day booked.
        let short = Position::<u64, u64> {
            cushion: Cushion::tao_only(TaoBalance::from(100)),
            legs: Legs::Short {
                proceeds: TaoBalance::from(100),
                debt: AlphaBalance::from(1_000),
                escrow: TaoBalance::from(100),
            },
            exposure_tao: TaoBalance::from(100),
            fee_per_day: TaoBalance::from(10),
            fee_accrued: TaoBalance::from(10),
            last_touch: 0,
            opened_at: 0,
        };
        // Buying the debt back costs what it sold for: equity is the cushion less the fee.
        assert_eq!(short.equity(0, q(100)), 90);
        assert!(short.is_healthy(0, q(100)));
        // Price doubled: equity is 200 - 200 - 10 = -10, underwater.
        assert_eq!(short.equity(0, q(200)), -10);
        assert!(!short.is_healthy(0, q(200)));
        // Exactly one day of fee left is still healthy; one rao less is not.
        assert!(short.is_healthy(0, q(180)));
        assert!(!short.is_healthy(0, q(181)));
        // Time alone can do it: one day booked plus eight accrued leaves exactly one day's
        // buffer; a ninth takes it.
        assert!(short.is_healthy(8 * BLOCKS_PER_DAY, q(100)));
        assert!(!short.is_healthy(9 * BLOCKS_PER_DAY, q(100)));

        // Long: 100 cushion, borrowed 100 and bought alpha; selling it back quotes `q`.
        let long = Position::<u64, u64> {
            cushion: Cushion::tao_only(TaoBalance::from(100)),
            legs: Legs::Long {
                proceeds: AlphaBalance::from(1_000),
                debt: TaoBalance::from(100),
                escrow: AlphaBalance::from(1_000),
            },
            exposure_tao: TaoBalance::from(200),
            fee_per_day: TaoBalance::from(10),
            fee_accrued: TaoBalance::from(10),
            last_touch: 0,
            opened_at: 0,
        };
        assert_eq!(long.equity(0, q(100)), 90);
        assert_eq!(long.equity(0, q(50)), 40);
        assert!(!long.is_healthy(0, q(19)));
        assert!(long.is_healthy(0, q(20)));
    }

    #[test]
    fn cushion_holds_both_tokens_and_counts_alpha_at_its_quote() {
        let cushion = Cushion::<u64>::tao_only(TaoBalance::from(100))
            .plus(&Deposit::Alpha {
                hotkey: 7,
                amount: AlphaBalance::from(1_000),
            })
            .plus(&Deposit::Tao(TaoBalance::from(50)));
        assert_eq!(cushion.tao, TaoBalance::from(150));
        assert_eq!(cushion.alpha, AlphaBalance::from(1_000));
        assert_eq!(cushion.alpha_hotkey, Some(7));
        // The latest alpha deposit decides where the alpha goes back.
        let moved = cushion.plus(&Deposit::Alpha {
            hotkey: 8,
            amount: AlphaBalance::from(0),
        });
        assert_eq!(moved.alpha_hotkey, Some(8));

        let half = cushion.part(Perquintill::from_percent(50));
        assert_eq!(half.tao, TaoBalance::from(75));
        assert_eq!(half.alpha, AlphaBalance::from(500));
        assert_eq!(half.alpha_hotkey, Some(7));
        let rest = cushion.minus(&half);
        assert_eq!(rest, half);
        assert_eq!(cushion.value(TaoBalance::from(40)), TaoBalance::from(190));

        let deposit = Deposit::<u64>::Alpha {
            hotkey: 7,
            amount: AlphaBalance::from(1_000),
        };
        assert_eq!(
            deposit.part(Perquintill::from_percent(25)),
            Deposit::Alpha {
                hotkey: 7,
                amount: AlphaBalance::from(250)
            }
        );
        assert!(Deposit::<u64>::Tao(TaoBalance::ZERO).is_zero());

        // An alpha cushion is worth what it sells for, so a falling price hurts a long twice.
        let long = Position::<u64, u64> {
            cushion: Cushion::tao_only(TaoBalance::ZERO).plus(&Deposit::Alpha {
                hotkey: 7,
                amount: AlphaBalance::from(1_000),
            }),
            legs: Legs::Long {
                proceeds: AlphaBalance::from(1_000),
                debt: TaoBalance::from(100),
                escrow: AlphaBalance::from(1_000),
            },
            exposure_tao: TaoBalance::from(200),
            fee_per_day: TaoBalance::from(10),
            fee_accrued: TaoBalance::from(10),
            last_touch: 0,
            opened_at: 0,
        };
        let at = |legs: u64, cushion_alpha: u64| Quotes {
            legs: TaoBalance::from(legs),
            cushion_alpha: TaoBalance::from(cushion_alpha),
        };
        assert_eq!(long.equity(0, at(100, 100)), 90);
        assert_eq!(long.equity(0, at(50, 50)), -10);
        assert!(!long.is_healthy(0, at(50, 50)));
    }

    #[test]
    fn alpha_is_valued_at_spot_for_sizing() {
        assert_eq!(alpha_value_in_tao(400, 1_000, 4_000), 100);
        assert_eq!(alpha_value_in_tao(1, 1_000, 4_000), 0);
        assert_eq!(alpha_value_in_tao(400, 1_000, 0), 0);
    }

    #[test]
    fn rent_is_one_yearly_rate_on_exposure_for_both_sides() {
        let params = DerivativesParams::defaults();
        let exposure = TaoBalance::from(1_000_000_000_000u64); // 1000 TAO
        // 20%/year of 1000 TAO is 200 TAO/year, 0.5479 TAO/day. The side and the pool share
        // do not enter.
        let per_day = DerivativesParams::fee_per_day(params.rate_per_year, exposure);
        assert_eq!(per_day, TaoBalance::from(547_945_205));
        // A year of days pays back the yearly rent, less rounding.
        assert_eq!(
            fee_for_blocks(per_day, DAYS_PER_YEAR * BLOCKS_PER_DAY),
            TaoBalance::from(199_999_999_825u64)
        );
        // Too small an exposure rounds to no rent; `min_deposit_tao` keeps this out of reach.
        assert_eq!(
            DerivativesParams::fee_per_day(params.rate_per_year, TaoBalance::from(1_824)),
            TaoBalance::ZERO
        );
    }

    #[test]
    fn a_position_has_no_term() {
        let position = Position::<u64, u64>::empty(Side::Short, 10);
        assert_eq!(position.opened_at, 10);
        assert_eq!(position.last_touch, 10);
        assert_eq!(position.fee_per_day, TaoBalance::ZERO);
    }

    #[test]
    fn defaults_are_valid_and_each_leverage_is_checked() {
        let params = DerivativesParams::defaults();
        assert!(params.is_valid());
        assert_eq!(params.max_leverage_percent(Side::Short), 100);
        assert_eq!(params.max_leverage_percent(Side::Long), 200);
        // Alpha cushions ship switched off on both sides.
        assert!(!params.alpha_cushion_allowed(Side::Short));
        assert!(!params.alpha_cushion_allowed(Side::Long));
        let mut no_long = params.clone();
        no_long.max_long_leverage_percent = 0;
        assert!(!no_long.is_valid());
        let mut no_short = params;
        no_short.max_short_leverage_percent = 0;
        assert!(!no_short.is_valid());
    }

    #[test]
    fn owner_picks_any_leverage_up_to_the_side_maximum() {
        let params = DerivativesParams::defaults();
        assert!(params.leverage_allowed(Side::Short, 1));
        assert!(params.leverage_allowed(Side::Short, 50));
        assert!(params.leverage_allowed(Side::Short, 100));
        assert!(!params.leverage_allowed(Side::Short, 101));
        assert!(!params.leverage_allowed(Side::Short, 0));
        assert!(params.leverage_allowed(Side::Long, 200));
        assert!(!params.leverage_allowed(Side::Long, 201));
        let mut raised = params;
        raised.max_long_leverage_percent = 1_000;
        assert!(raised.leverage_allowed(Side::Long, 1_000));
        assert!(!raised.leverage_allowed(Side::Short, 200));
    }

    #[test]
    fn subnet_override_rejects_a_zero_cap_or_rate() {
        let mut override_ = SubnetOverride {
            shorts_enabled: false,
            longs_enabled: true,
            max_pool_share: None,
            rate_per_year: None,
        };
        assert!(override_.is_valid());
        override_.max_pool_share = Some(Percent::from_percent(5));
        override_.rate_per_year = Some(Perbill::from_percent(1));
        assert!(override_.is_valid());
        override_.rate_per_year = Some(Perbill::zero());
        assert!(!override_.is_valid());
        override_.rate_per_year = None;
        override_.max_pool_share = Some(Percent::zero());
        assert!(!override_.is_valid());
    }
}
