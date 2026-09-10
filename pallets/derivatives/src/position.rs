//! Position types and the pure arithmetic behind opening and settling.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::{
    Percent, RuntimeDebug,
    traits::{Saturating, UniqueSaturatedInto, Zero},
};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, TaoBalance, Token};
use subtensor_swap_interface::Perquintill;

/// Blocks in one year at a 12-second block time. The interest is a yearly rate accrued per block.
pub const BLOCKS_PER_YEAR: u64 = 365 * 7_200;

/// Blocks between two collections of one position's interest: one week. Each position is due
/// on its own block, so the book spreads itself over the week.
pub const INTEREST_PERIOD: u32 = 7 * 7_200;

/// Interest collections one block may run. Bounds the work `on_initialize` does; a slot with
/// more positions due than this is finished over the following blocks.
pub const COLLECTIONS_PER_BLOCK: u32 = 20;

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
        /// TAO the lifted alpha sold for. Held by the pallet until close.
        proceeds: TaoBalance,
        /// Alpha that must be bought back and returned to the pool.
        debt: AlphaBalance,
        /// The lifted TAO, held untouched and returned as-is.
        escrow: TaoBalance,
    },
    /// The pool lent TAO, which was spent on alpha.
    Long {
        /// Alpha the lifted TAO bought. Held as stake until close.
        proceeds: AlphaBalance,
        /// TAO that must be repaid to the pool.
        debt: TaoBalance,
        /// The lifted alpha, held untouched and returned as-is.
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
    /// side in `Footprint` and compared against `pool_share` of the lent reserve.
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

/// One open position: the sum of every tranche the owner has added on this subnet, less
/// whatever has been settled. One per `(owner, netuid)`; the side is the sign of the exposure.
///
/// Every field but the two block numbers is a plain sum, so adding a tranche is addition and
/// settling a fraction is multiplication. Nothing here is per tranche. A position has no term:
/// it lives until its owner closes it, or until its cushion can no longer pay its interest and
/// the chain forfeits it.
#[freeze_struct("d1bea716fab8bb55")]
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
    /// TAO the owner has put up, in total. Returned as the position is settled, minus interest
    /// and losses.
    pub cushion: TaoBalance,
    /// The borrowed slice: proceeds held, debt owed, escrow kept. Its variant is the side.
    pub legs: Legs,
    /// The TAO value the pool has lent, summed over tranches. The position's leverage is
    /// `exposure_tao / cushion`.
    pub exposure_tao: TaoBalance,
    /// Interest per year for the whole position: `interest_rate * exposure` of each tranche, fixed
    /// when it was added, summed.
    pub interest_per_year: TaoBalance,
    /// Interest owed and not yet paid, as of `since`. An add brings it up to date; a collection
    /// and every settlement pay it.
    pub interest_owed: TaoBalance,
    /// Block the interest was last brought up to date: the latest add, collection, or
    /// settlement.
    pub since: BlockNumber,
    /// Block the chain next collects this position's interest: one [`INTEREST_PERIOD`] after
    /// it opened or was last collected. Adds and reductions do not move it.
    pub due: BlockNumber,
}

impl<BlockNumber: Copy + Saturating + UniqueSaturatedInto<u64> + From<u32>> Position<BlockNumber> {
    /// A position with nothing in it yet, born at `now`. The first tranche folded in opens it.
    pub fn empty(side: Side, now: BlockNumber) -> Self {
        Self {
            cushion: TaoBalance::ZERO,
            legs: Legs::empty(side),
            exposure_tao: TaoBalance::ZERO,
            interest_per_year: TaoBalance::ZERO,
            interest_owed: TaoBalance::ZERO,
            since: now,
            due: now.saturating_add(INTEREST_PERIOD.into()),
        }
    }

    pub fn side(&self) -> Side {
        self.legs.side()
    }

    /// Fold a tranche of the same side in at `now`. Every field is a sum, so this is addition;
    /// the interest is brought up to date first so the new rate only runs from now. `None` if the
    /// tranche is on the other side.
    pub fn fold(&mut self, tranche: Tranche, now: BlockNumber) -> Option<()> {
        let legs = self.legs.plus(&tranche.legs)?;
        self.interest_owed = self.interest_due(now);
        self.since = now;
        self.cushion = self.cushion.saturating_add(tranche.deposit);
        self.legs = legs;
        self.exposure_tao = self.exposure_tao.saturating_add(tranche.exposure_tao);
        self.interest_per_year = self
            .interest_per_year
            .saturating_add(tranche.interest_per_year);
        Some(())
    }

    /// Everything owed at `now`: the carried balance plus the yearly rate since `since`.
    pub fn interest_due(&self, now: BlockNumber) -> TaoBalance {
        let blocks: u64 = now.saturating_sub(self.since).unique_saturated_into();
        self.interest_owed
            .saturating_add(interest_for_blocks(self.interest_per_year, blocks))
    }

    /// Collect at `now`: move the interest due out of the cushion and set the next collection
    /// one period ahead. Returns what was moved, or `None` if the cushion cannot cover it: the
    /// position is starved and must be forfeited.
    pub fn collect(&mut self, now: BlockNumber) -> Option<TaoBalance> {
        let due = self.interest_due(now);
        if self.cushion < due {
            return None;
        }
        self.cushion = self.cushion.saturating_sub(due);
        self.interest_owed = TaoBalance::ZERO;
        self.since = now;
        self.due = now.saturating_add(INTEREST_PERIOD.into());
        Some(due)
    }
}

/// What one `add` contributes to a position. Same shape as the sums it goes into.
pub struct Tranche {
    pub deposit: TaoBalance,
    pub legs: Legs,
    pub exposure_tao: TaoBalance,
    pub interest_per_year: TaoBalance,
}

/// The two root-set numbers that are the design: how much of a pool may be lent, and at what
/// interest. Everything else the pallet needs is a constant.
#[freeze_struct("519138526a63073a")]
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
pub struct DerivativesParams {
    /// `kappa`: the largest share of the lent reserve that all open positions of one side on
    /// one subnet may borrow together. Zero pauses new adds; open positions still settle.
    pub pool_share: Percent,
    /// The interest per year, as a fraction of a tranche's TAO exposure, the same on both sides.
    /// Accrued per block; fixed for each tranche when it is added.
    pub interest_rate: Percent,
}

impl DerivativesParams {
    /// Mainnet defaults: up to a quarter of the pool lent, at 25% a year.
    pub fn defaults() -> Self {
        Self {
            pool_share: Percent::from_percent(25),
            interest_rate: Percent::from_percent(25),
        }
    }

    /// Interest per year for a new tranche: `interest_rate * exposure`, whichever side.
    pub fn interest_for(&self, exposure_tao: TaoBalance) -> TaoBalance {
        TaoBalance::from(self.interest_rate.mul_floor(exposure_tao.to_u64()))
    }
}

/// The fraction of the pool a tranche lifts. The tranche wants `L * deposit` of TAO, and the
/// alpha that is worth at the smoothed price; `phi` is the share of the live reserves that
/// delivers no more than either. On an untouched pool the two agree at `L * deposit / tao`.
/// After a same-block dump the alpha bound binds, after a pump the TAO bound does, so the
/// swap buys nothing. Saturates at one (the whole pool), which the cap then rejects.
pub fn pool_fraction(
    leverage_percent: u16,
    deposit: u64,
    (tao, alpha): (u64, u64),
    (smoothed_tao, smoothed_alpha): (u64, u64),
) -> Perquintill {
    if tao == 0 || alpha == 0 || smoothed_tao == 0 {
        return Perquintill::one();
    }
    let want_tao = (deposit as u128).saturating_mul(leverage_percent as u128);
    let by_tao = Perquintill::from_rational(want_tao, (tao as u128).saturating_mul(100));
    // `want_tao * smoothed_alpha / smoothed_tao` alpha, over the live alpha reserve.
    let want_alpha = want_tao
        .saturating_mul(smoothed_alpha as u128)
        .checked_div(smoothed_tao as u128)
        .unwrap_or(u128::MAX);
    let by_alpha = Perquintill::from_rational(want_alpha, (alpha as u128).saturating_mul(100));
    by_tao.min(by_alpha)
}

/// Projected footprint of a new tranche in the lent reserve: `phi * (2 - phi) * reserve`.
/// The lifted half is `phi * R`; swapping the other half back into the shrunken pool yields
/// about `phi * (1 - phi) * R` more.
pub fn projected_footprint(phi: Perquintill, lent_reserve: u64) -> u64 {
    let lifted = phi.mul_floor(lent_reserve);
    lifted
        .saturating_mul(2)
        .saturating_sub(phi.mul_floor(lifted))
}

/// Interest that `blocks` blocks accrue at `interest_per_year`, pro rata.
pub fn interest_for_blocks(interest_per_year: TaoBalance, blocks: u64) -> TaoBalance {
    let interest = (interest_per_year.to_u64() as u128)
        .saturating_mul(blocks as u128)
        .checked_div(BLOCKS_PER_YEAR as u128)
        .unwrap_or(0)
        .min(u64::MAX as u128) as u64;
    TaoBalance::from(interest)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn fraction_is_leverage_times_share_and_saturates_at_the_whole_pool() {
        let pool = (1_000, 4_000);
        assert_eq!(pool_fraction(100, 10, pool, pool).mul_floor(1_000u64), 10);
        assert_eq!(pool_fraction(200, 10, pool, pool).mul_floor(1_000u64), 20);
        assert!(pool_fraction(100, 1_000, pool, pool).is_one());
        assert!(pool_fraction(100, 2_000, pool, pool).is_one());
        assert!(pool_fraction(100, 10, (0, 4_000), pool).is_one());
        assert!(pool_fraction(100, 0, pool, pool).is_zero());
    }

    #[test]
    fn fraction_is_bounded_by_both_tokens_at_the_smoothed_price() {
        let smoothed = (1_000, 4_000);
        // Dumped: a fifth of the TAO gone, alpha up. 10 TAO wants 40 alpha; on 5_000 alpha
        // that is 0.8%, below the 1.25% the TAO side would allow.
        let dumped = (800, 5_000);
        let phi = pool_fraction(100, 10, dumped, smoothed);
        assert_eq!(phi.mul_floor(5_000u64), 40);
        assert_eq!(phi.mul_floor(800u64), 6);
        // Pumped: TAO up by half, alpha down. The TAO side binds: 10 TAO of 1_500 is 0.67%,
        // which lifts fewer than the 40 alpha the smoothed price would allow.
        let pumped = (1_500, 2_667);
        let phi = pool_fraction(100, 10, pumped, smoothed);
        // Rounded down in the fraction: 9 or 10 TAO, never more.
        assert!((9..=10).contains(&phi.mul_floor(1_500u64)));
        assert!(phi.mul_floor(2_667u64) < 40);
    }

    #[test]
    fn footprint_is_phi_two_minus_phi() {
        let phi = Perquintill::from_percent(10);
        // 0.1 * 1.9 * 1000 = 190
        assert_eq!(projected_footprint(phi, 1_000), 190);
    }

    #[test]
    fn interest_is_a_yearly_rate_on_exposure_accrued_per_block() {
        let params = DerivativesParams::defaults();
        let exposure = TaoBalance::from(1_000_000_000_000u64); // 1000 TAO
        // 25%/year of 1000 TAO is 250 TAO/year. The side does not enter.
        let per_year = params.interest_for(exposure);
        assert_eq!(per_year, TaoBalance::from(250_000_000_000u64));
        assert_eq!(interest_for_blocks(per_year, 0), TaoBalance::ZERO);
        assert_eq!(interest_for_blocks(per_year, BLOCKS_PER_YEAR), per_year);
        // One day is 1/365 of it.
        assert_eq!(
            interest_for_blocks(per_year, 7_200),
            TaoBalance::from(684_931_506u64)
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
        assert!(sum.minus(&sum).footprint() == 0);

        let long = Legs::Long {
            proceeds: AlphaBalance::from(1),
            debt: TaoBalance::from(1),
            escrow: AlphaBalance::from(1),
        };
        assert!(a.plus(&long).is_none());
    }

    /// A 1x short: 100 cushion, sold for 100, owes 1_000 alpha; 3_650/year of interest so a day
    /// costs 10.
    fn short() -> Position<u64> {
        Position {
            cushion: TaoBalance::from(100),
            legs: Legs::Short {
                proceeds: TaoBalance::from(100),
                debt: AlphaBalance::from(1_000),
                escrow: TaoBalance::from(100),
            },
            exposure_tao: TaoBalance::from(100),
            interest_per_year: TaoBalance::from(3_650),
            interest_owed: TaoBalance::ZERO,
            since: 0,
            due: 7 * 7_200,
        }
    }

    #[test]
    fn interest_due_is_the_carried_balance_plus_the_rate_since() {
        let mut position = short();
        position.interest_owed = TaoBalance::from(7);
        assert_eq!(position.interest_due(0), TaoBalance::from(7));
        assert_eq!(position.interest_due(7_200), TaoBalance::from(17));
        // A clock that ran backwards owes nothing extra.
        position.since = 10;
        assert_eq!(position.interest_due(5), TaoBalance::from(7));
    }

    #[test]
    fn folding_brings_the_rent_up_to_date_and_sums_the_rest() {
        let mut position = short();
        let folded = position.fold(
            Tranche {
                deposit: TaoBalance::from(50),
                legs: Legs::Short {
                    proceeds: TaoBalance::from(50),
                    debt: AlphaBalance::from(500),
                    escrow: TaoBalance::from(50),
                },
                exposure_tao: TaoBalance::from(50),
                interest_per_year: TaoBalance::from(1_825),
            },
            7_200,
        );
        assert!(folded.is_some());
        assert_eq!(position.cushion, TaoBalance::from(150));
        assert_eq!(position.exposure_tao, TaoBalance::from(150));
        assert_eq!(position.interest_per_year, TaoBalance::from(5_475));
        assert_eq!(position.interest_owed, TaoBalance::from(10));
        assert_eq!(position.since, 7_200);
        assert_eq!(position.legs.footprint(), 300);

        let other_side = position.fold(
            Tranche {
                deposit: TaoBalance::from(1),
                legs: Legs::empty(Side::Long),
                exposure_tao: TaoBalance::ZERO,
                interest_per_year: TaoBalance::ZERO,
            },
            7_200,
        );
        assert!(other_side.is_none());
    }

    #[test]
    fn collecting_draws_the_cushion_down_and_books_the_next_collection() {
        const WEEK: u64 = 7 * 7_200;
        let mut position = short();
        assert_eq!(position.due, WEEK);
        // A week in: seventy comes out of the cushion, the clock restarts, the next week is
        // booked.
        assert_eq!(position.collect(WEEK), Some(TaoBalance::from(70)));
        assert_eq!(position.cushion, TaoBalance::from(30));
        assert_eq!(position.interest_owed, TaoBalance::ZERO);
        assert_eq!(position.since, WEEK);
        assert_eq!(position.due, 2 * WEEK);
        // Collected late: the interest is exact for the time passed, and the next collection is
        // one period after the late one.
        assert_eq!(
            position.collect(WEEK + 3 * 7_200),
            Some(TaoBalance::from(30))
        );
        assert_eq!(position.cushion, TaoBalance::ZERO);
        assert_eq!(position.due, 2 * WEEK + 3 * 7_200);
        // Nothing left: the next collection cannot be paid.
        assert_eq!(position.collect(3 * WEEK), None);
        assert_eq!(position.since, WEEK + 3 * 7_200);
    }

    #[test]
    fn a_position_has_no_term_and_is_first_collected_a_week_in() {
        let position = Position::<u64>::empty(Side::Short, 10);
        assert_eq!(position.since, 10);
        assert_eq!(position.due, 10 + 7 * 7_200);
        assert_eq!(position.interest_per_year, TaoBalance::ZERO);
        assert_eq!(position.side(), Side::Short);
    }
}
