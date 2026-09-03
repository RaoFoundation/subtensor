//! Position types and the pure arithmetic behind opening and settling.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::{Perbill, Percent, RuntimeDebug, traits::Zero};
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

/// One open position.
#[freeze_struct("41b6fc92047d1227")]
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
    /// `P`: the TAO cushion. Returned at close minus the fee and any shortfall.
    pub cushion: TaoBalance,
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
#[freeze_struct("a3a44145cb64ae7e")]
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
    /// `L`: exposure as a percentage of the deposit. `100` = 1x.
    pub leverage_percent: u16,
    /// `kappa`: the largest share of the lent reserve that all open positions of one side on
    /// one subnet may borrow together.
    pub max_pool_share: Percent,
    /// `X`: how long a position may stay open.
    pub lifetime_blocks: BlockNumber,
    /// `c`: what a short pays per day for borrowing the whole pool, in TAO. A short that
    /// lifts a share `phi` pays `c * phi` per day. Pump risk in a constant-product pool
    /// scales with `1 / T`, so a fixed TAO amount per unit of pool share is the fair form.
    pub short_fee_per_day: TaoBalance,
    /// `r`: what a long pays per day, as a fraction of `exposure_tao`. Crash risk does not
    /// depend on pool size, so a plain rate on exposure is the fair form.
    pub long_rate_per_day: Perbill,
    /// Smallest cushion, measured in TAO at the open price.
    pub min_deposit_tao: TaoBalance,
}

impl<BlockNumber: From<u32>> DerivativesParams<BlockNumber> {
    /// Mainnet defaults: 1x, 10% of the pool, 30 days, 5 TAO/day per unit pool share on
    /// shorts, 0.02%/day of exposure on longs, 0.1 TAO minimum cushion.
    ///
    /// The fees are 1.5-2.5x the pool's measured expected loss over a year of Finney pool
    /// prices: `E[(theta - 2)+] * T ~= 96 TAO` per 30 days on shorts, `E[(1 - 2 theta)+]
    /// ~= 0.25%` of exposure per 30 days on longs.
    pub fn defaults() -> Self {
        Self {
            shorts_enabled: true,
            longs_enabled: true,
            leverage_percent: 100,
            max_pool_share: Percent::from_percent(10),
            lifetime_blocks: BlockNumber::from(216_000u32),
            short_fee_per_day: TaoBalance::from(5_000_000_000u64),
            long_rate_per_day: Perbill::from_rational(2u32, 10_000u32),
            min_deposit_tao: TaoBalance::from(100_000_000),
        }
    }

    /// Fee per day for a new position: `c * phi` on a short, `r * exposure` on a long.
    pub fn fee_per_day(
        &self,
        side: Side,
        phi: Perquintill,
        exposure_tao: TaoBalance,
    ) -> TaoBalance {
        match side {
            Side::Short => TaoBalance::from(phi.mul_floor(self.short_fee_per_day.to_u64())),
            Side::Long => TaoBalance::from(self.long_rate_per_day.mul_floor(exposure_tao.to_u64())),
        }
    }
}

impl<BlockNumber: Zero> DerivativesParams<BlockNumber> {
    /// A parameter set every open can act on. Zero leverage or a zero pool share would make
    /// every `open` fail; a zero lifetime would let anyone close a position the block it opens.
    /// Use `shorts_enabled` / `longs_enabled` to pause opens instead.
    pub fn is_valid(&self) -> bool {
        self.leverage_percent > 0
            && !self.max_pool_share.is_zero()
            && !self.lifetime_blocks.is_zero()
    }
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
        // 1% of any pool: 5 TAO/day * 1% = 0.05 TAO/day, whatever the exposure.
        assert_eq!(
            params.fee_per_day(Side::Short, one_percent, exposure),
            TaoBalance::from(50_000_000)
        );
        // Long: 0.02%/day of 1000 TAO = 0.2 TAO/day, whatever the pool share.
        assert_eq!(
            params.fee_per_day(Side::Long, one_percent, exposure),
            TaoBalance::from(200_000_000)
        );
    }
}
