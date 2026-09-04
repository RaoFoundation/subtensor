//! Position types and the pure arithmetic behind opening and settling.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::{PerThing, Perbill, Percent, RuntimeDebug, traits::Zero};
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

/// One open position.
#[freeze_struct("cd9e5fdfbc8a1e58")]
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
    /// `P`: what the owner put up. Returned at close minus the fee and any shortfall.
    pub cushion: Cushion,
    /// The borrowed slice: proceeds held, debt owed, escrow kept. Its variant is the side.
    pub legs: Legs,
    /// `phi * T` at open: the TAO value the pool lent.
    pub exposure_tao: TaoBalance,
    /// Borrow fee per day, fixed at open from the parameters in force then. Shorts pay
    /// `short_fee_per_day * phi`; longs pay `long_rate_per_day * exposure_tao`.
    pub fee_per_day: TaoBalance,
    pub opened_at: BlockNumber,
    /// After this block anyone may close the position.
    pub expires_at: BlockNumber,
    /// Block whose `Expiring` queue holds this position. Starts as `expires_at`; moves later
    /// each time a sweep fails and is rescheduled.
    pub queued_at: BlockNumber,
    /// Sweeps that have failed so far. Rescheduling stops at `MAX_SETTLE_RETRIES`.
    pub failed_sweeps: u8,
}

impl<BlockNumber> Position<BlockNumber> {
    pub fn side(&self) -> Side {
        self.legs.side()
    }
}

/// Root-settable parameters.
#[freeze_struct("e263f11aa1ca8132")]
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
    /// `L_short`: a short's exposure as a percentage of its cushion. `100` = 1x. The pool is
    /// wiped when the price rises by `1 / L`: 2x at 1x leverage.
    pub short_leverage_percent: u16,
    /// `L_long`: a long's exposure as a percentage of its cushion. `200` = 2x. The pool is
    /// wiped when the price falls by `1 / L`: a halving at 2x. At 1x a long can never lose
    /// the pool anything, and is nothing a spot buy does not do better.
    pub long_leverage_percent: u16,
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
    /// Mainnet defaults: shorts 1x, longs 2x, 10% of the pool, 30 days, 6 TAO/day per unit
    /// pool share on shorts, 0.01%/day of exposure on longs, 0.1 TAO minimum cushion.
    ///
    /// Each fee is twice the pool's measured expected loss over a year of Finney pool prices:
    /// `E[(theta - 2)+] * T ~= 86 TAO` per 30 days on shorts (2.9 TAO/day), `E[(1/2 - theta)+]
    /// ~= 0.11%` of exposure per 30 days on longs at 2x (0.004%/day). The factor of two covers
    /// the sampling error on ~60 pump episodes and the book closing against the pool at the cap.
    pub fn defaults() -> Self {
        Self {
            shorts_enabled: true,
            longs_enabled: true,
            short_leverage_percent: 100,
            long_leverage_percent: 200,
            max_pool_share: Percent::from_percent(10),
            lifetime_blocks: BlockNumber::from(216_000u32),
            short_fee_per_day: TaoBalance::from(6_000_000_000u64),
            long_rate_per_day: Perbill::from_rational(1u32, 10_000u32),
            min_deposit_tao: TaoBalance::from(100_000_000),
        }
    }

    pub fn leverage_percent(&self, side: Side) -> u16 {
        match side {
            Side::Short => self.short_leverage_percent,
            Side::Long => self.long_leverage_percent,
        }
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
    /// A parameter set every open can act on. Zero leverage or a zero pool share would make
    /// every `open` fail; a zero lifetime would let anyone close a position the block it opens.
    /// Use `shorts_enabled` / `longs_enabled` to pause opens instead.
    pub fn is_valid(&self) -> bool {
        self.short_leverage_percent > 0
            && self.long_leverage_percent > 0
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

/// Borrow fee owed after `blocks_open` blocks, never less than one day's worth.
pub fn accrued_fee(fee_per_day: TaoBalance, blocks_open: u64) -> TaoBalance {
    let days_numer = blocks_open.max(BLOCKS_PER_DAY) as u128;
    let fee = (fee_per_day.to_u64() as u128)
        .saturating_mul(days_numer)
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
    fn fee_has_one_day_floor() {
        let per_day = TaoBalance::from(500_000);
        assert_eq!(accrued_fee(per_day, 1), per_day);
        assert_eq!(accrued_fee(per_day, BLOCKS_PER_DAY), per_day);
        assert_eq!(
            accrued_fee(per_day, 30 * BLOCKS_PER_DAY),
            TaoBalance::from(15_000_000)
        );
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
        assert_eq!(params.leverage_percent(Side::Short), 100);
        assert_eq!(params.leverage_percent(Side::Long), 200);
        let mut no_long = params.clone();
        no_long.long_leverage_percent = 0;
        assert!(!no_long.is_valid());
        let mut no_short = params;
        no_short.short_leverage_percent = 0;
        assert!(!no_short.is_valid());
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
