//! Position types and the pure arithmetic behind opening and settling.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::{
    PerThing, Perbill, Percent, RuntimeDebug,
    traits::{Saturating, UniqueSaturatedInto, Zero},
};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, TaoBalance, Token};
use subtensor_swap_interface::Perquintill;

/// Blocks in one day at a 12-second block time. The borrow fee is quoted per day and never
/// charged for less than one day.
pub const BLOCKS_PER_DAY: u64 = 7_200;

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

/// What the owner put up. Only TAO today. An enum so that an alpha variant can be added later
/// without migrating stored positions: SCALE encodes the variant index first, so existing
/// `Tao` values keep decoding.
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
pub enum Cushion {
    Tao(TaoBalance),
}

impl Cushion {
    /// The cushion's TAO, once any alpha variant is valued. Today it is the deposit itself.
    pub fn tao(&self) -> TaoBalance {
        match self {
            Cushion::Tao(amount) => *amount,
        }
    }
}

/// One open position: the sum of every tranche the owner has added on this subnet, less
/// whatever has been settled. One per `(owner, netuid)`; the side is the sign of the exposure.
///
/// Every field but the clocks is a plain sum, so adding a tranche is addition and settling a
/// fraction is multiplication. Nothing here is per tranche.
#[freeze_struct("b4c605e08da65a24")]
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
pub struct Position<BlockNumber> {
    /// `P`: what the owner has put up, in total. Returned as the position is settled, minus
    /// fees and losses.
    pub cushion: Cushion,
    /// The borrowed slice: proceeds held, debt owed, escrow kept. Its variant is the side.
    pub legs: Legs,
    /// `sum(phi * T)` over tranches: the TAO value the pool has lent. The position's leverage
    /// is `exposure_tao / cushion`.
    pub exposure_tao: TaoBalance,
    /// Borrow fee per day for the whole position: each tranche's rate, fixed when it was added,
    /// summed. Shorts pay `short_fee_per_day * phi`; longs pay `long_rate_per_day * exposure`.
    pub fee_per_day: TaoBalance,
    /// Fee owed and not yet paid, as of `last_touch`. Every add puts one day of the new
    /// tranche's rate here up front; every settlement pays it down.
    pub fee_accrued: TaoBalance,
    /// Block the fee was last brought up to date: the latest add or settlement.
    pub last_touch: BlockNumber,
    /// Block of the first add.
    pub opened_at: BlockNumber,
    /// After this block anyone may close the position, and nothing more can be added to it.
    /// Set by the first add; adds do not extend it.
    pub expires_at: BlockNumber,
    /// Block whose `Expiring` queue holds this position. Starts as `expires_at`; moves later
    /// each time a sweep fails and is rescheduled.
    pub queued_at: BlockNumber,
    /// Sweeps that have failed so far. Rescheduling stops at `MAX_SETTLE_RETRIES`.
    pub failed_sweeps: u8,
}

impl<BlockNumber: Copy + Saturating + UniqueSaturatedInto<u64>> Position<BlockNumber> {
    pub fn side(&self) -> Side {
        self.legs.side()
    }

    /// Everything owed at `now`: the accrued balance plus the running rate since `last_touch`.
    pub fn fee_owed(&self, now: BlockNumber) -> TaoBalance {
        let blocks: u64 = now.saturating_sub(self.last_touch).unique_saturated_into();
        self.fee_accrued
            .saturating_add(fee_for_blocks(self.fee_per_day, blocks))
    }
}

/// What one `add` contributes to a position. Same shape as the sums it goes into.
pub struct Tranche {
    pub cushion: TaoBalance,
    pub legs: Legs,
    pub exposure_tao: TaoBalance,
    pub fee_per_day: TaoBalance,
}

/// Root-settable parameters.
#[freeze_struct("d506fe231a223242")]
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
pub struct DerivativesParams<BlockNumber> {
    pub shorts_enabled: bool,
    pub longs_enabled: bool,
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
    /// `X`: how long a position may stay open.
    pub lifetime_blocks: BlockNumber,
    /// `C`: what a short pays per day for borrowing the whole pool, in TAO. A short that
    /// lifts a share `phi` pays `C * phi` per day. Pump risk in a constant-product pool
    /// scales with `1 / T`, so a fixed TAO amount per unit of pool share is the fair form.
    pub short_fee_per_day: TaoBalance,
    /// `r`: what a long pays per day, as a fraction of `exposure_tao`. Crash risk does not
    /// depend on pool size, so a plain rate on exposure is the fair form.
    pub long_rate_per_day: Perbill,
    /// Smallest cushion, measured in TAO at the open price.
    pub min_deposit_tao: TaoBalance,
}

impl<BlockNumber: From<u32>> DerivativesParams<BlockNumber> {
    /// Mainnet defaults: shorts up to 1x, longs up to 2x, 10% of the pool, 30 days, 6 TAO/day
    /// per unit pool share on shorts, 0.01%/day of exposure on longs, 0.1 TAO minimum cushion.
    ///
    /// Each fee is twice the pool's measured expected loss over a year of Finney pool prices:
    /// `E[(theta - 2)+] * T ~= 86 TAO` per 30 days on shorts (2.9 TAO/day), `E[(1/2 - theta)+]
    /// ~= 0.11%` of exposure per 30 days on longs at 2x (0.004%/day). The factor of two covers
    /// the sampling error on ~60 pump episodes and the book closing against the pool at the cap.
    pub fn defaults() -> Self {
        Self {
            shorts_enabled: true,
            longs_enabled: true,
            max_short_leverage_percent: 100,
            max_long_leverage_percent: 200,
            max_pool_share: Percent::from_percent(10),
            lifetime_blocks: BlockNumber::from(216_000u32),
            short_fee_per_day: TaoBalance::from(6_000_000_000u64),
            long_rate_per_day: Perbill::from_rational(1u32, 10_000u32),
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

    /// Fee per day for a new position: `C * phi` on a short, `r * exposure` on a long, both
    /// times [`size_factor`] for the position's own slippage.
    pub fn fee_per_day(
        &self,
        side: Side,
        phi: Perquintill,
        exposure_tao: TaoBalance,
    ) -> TaoBalance {
        let base = match side {
            Side::Short => phi.mul_floor(self.short_fee_per_day.to_u64()),
            Side::Long => self.long_rate_per_day.mul_floor(exposure_tao.to_u64()),
        };
        TaoBalance::from(size_factor(phi, base))
    }
}

impl<BlockNumber: Zero> DerivativesParams<BlockNumber> {
    /// A parameter set every open can act on. A zero maximum leverage or a zero pool share
    /// would make every `open` fail; a zero lifetime would let anyone close a position the
    /// block it opens. Use `shorts_enabled` / `longs_enabled` to pause opens instead.
    pub fn is_valid(&self) -> bool {
        self.max_short_leverage_percent > 0
            && self.max_long_leverage_percent > 0
            && !self.max_pool_share.is_zero()
            && !self.lifetime_blocks.is_zero()
    }
}

/// Root-settable per-subnet overrides. Absent means the global parameters apply. Only opens
/// look at it: a paused side can still close, roll cannot reopen.
#[freeze_struct("c897b7b9addbdd09")]
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
}

impl SubnetOverride {
    pub fn side_enabled(&self, side: Side) -> bool {
        match side {
            Side::Short => self.shorts_enabled,
            Side::Long => self.longs_enabled,
        }
    }

    /// A zero cap would make every open fail; pause the side instead.
    pub fn is_valid(&self) -> bool {
        self.max_pool_share.is_none_or(|share| !share.is_zero())
    }
}

/// `base / (1 - phi)^4`: the fee scaled for the position's own slippage at close.
///
/// The linear fee laws assume a small slice. A position that lifts `phi` of the pool must buy
/// back (or sell) into a pool that is `phi` smaller, and its exact expected loss over the
/// measured price history is `(1 - phi)^-4` times the small-slice value to within 3% for
/// `phi` up to 25% (x1.23 at 5%, x2.4 at 20%). With this factor the fee stays fair at any
/// pool-share cap without a separate per-position limit. Saturates as `phi` nears one.
pub fn size_factor(phi: Perquintill, base: u64) -> u64 {
    let one_minus = phi.left_from_one();
    let denom = one_minus.square().square();
    if denom.is_zero() {
        return u64::MAX;
    }
    (base as u128)
        .saturating_mul(Perquintill::ACCURACY as u128)
        .checked_div(denom.deconstruct() as u128)
        .unwrap_or(u64::MAX as u128)
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
        let position = Position::<u64> {
            cushion: Cushion::Tao(TaoBalance::from(0)),
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
            expires_at: 20,
            queued_at: 20,
            failed_sweeps: 0,
        };
        assert_eq!(position.fee_owed(10), TaoBalance::from(7_200));
        assert_eq!(position.fee_owed(3_610), TaoBalance::from(10_800));
        // A clock that ran backwards owes nothing extra.
        assert_eq!(position.fee_owed(5), TaoBalance::from(7_200));
    }

    #[test]
    fn short_fee_scales_with_pool_share_and_long_fee_with_exposure() {
        let params = DerivativesParams::<u64>::defaults();
        let one_percent = Perquintill::from_percent(1);
        let exposure = TaoBalance::from(1_000_000_000_000u64); // 1000 TAO
        // 1% of any pool: 6 TAO/day * 1% = 0.06 TAO/day, whatever the exposure, times the
        // size factor at 1%.
        assert_eq!(
            params.fee_per_day(Side::Short, one_percent, exposure),
            TaoBalance::from(size_factor(one_percent, 60_000_000))
        );
        // Long: 0.01%/day of 1000 TAO = 0.1 TAO/day, whatever the pool share, times the same
        // factor.
        assert_eq!(
            params.fee_per_day(Side::Long, one_percent, exposure),
            TaoBalance::from(size_factor(one_percent, 100_000_000))
        );
    }

    #[test]
    fn size_factor_is_inverse_fourth_power_of_the_remaining_pool() {
        let base = 1_000_000;
        // (1 - phi)^-4: 1.0410 at 1%, 1.0842 at 2%, 2.4414 at 20%.
        assert_eq!(size_factor(Perquintill::from_percent(1), base), 1_041_020);
        assert_eq!(size_factor(Perquintill::from_percent(2), base), 1_084_165);
        assert_eq!(size_factor(Perquintill::from_percent(20), base), 2_441_406);
        assert_eq!(size_factor(Perquintill::zero(), base), base);
        assert_eq!(size_factor(Perquintill::one(), base), u64::MAX);
    }

    #[test]
    fn defaults_are_valid_and_each_leverage_is_checked() {
        let params = DerivativesParams::<u64>::defaults();
        assert!(params.is_valid());
        assert_eq!(params.max_leverage_percent(Side::Short), 100);
        assert_eq!(params.max_leverage_percent(Side::Long), 200);
        let mut no_long = params.clone();
        no_long.max_long_leverage_percent = 0;
        assert!(!no_long.is_valid());
        let mut no_short = params;
        no_short.max_short_leverage_percent = 0;
        assert!(!no_short.is_valid());
    }

    #[test]
    fn owner_picks_any_leverage_up_to_the_side_maximum() {
        let params = DerivativesParams::<u64>::defaults();
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
    fn subnet_override_rejects_a_zero_cap() {
        let mut override_ = SubnetOverride {
            shorts_enabled: false,
            longs_enabled: true,
            max_pool_share: None,
        };
        assert!(override_.is_valid());
        override_.max_pool_share = Some(Percent::from_percent(5));
        assert!(override_.is_valid());
        override_.max_pool_share = Some(Percent::zero());
        assert!(!override_.is_valid());
    }
}
