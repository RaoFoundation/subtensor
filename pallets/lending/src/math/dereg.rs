//! Dereg settlement (SPEC 1.8, P12, P16, 7.4): the rate r, the zone test, a short's payment
//! into the pot and the lender-first split of the one-shot long sale.

use super::fixed::{BPS, Price, mul_price_floor, price, pro_rata_floor, scale_price_floor};

/// Dereg rate (SPEC 7.4 phase B): `r = floor(min(T_flag, T_ema, T_A) * 2^64 / all_alpha)`.
/// `all_alpha` is total issuance including synthetic and residual alpha (at least 1).
#[must_use]
pub fn rate(t_flag: u64, t_ema: u64, t_a: u64, all_alpha: u128) -> Price {
    price(t_flag.min(t_ema).min(t_a), all_alpha.max(1))
}

/// Dereg zone test (SPEC P16): `SMA30 * 1e4 <= zone_bps * lowest non-immune SMA30`.
#[must_use]
pub fn in_zone(sma: Price, lowest_other_sma: Price, zone_bps: u32) -> bool {
    scale_price_floor(lowest_other_sma, u128::from(zone_bps), BPS).is_none_or(|limit| sma <= limit)
}

/// What a short pays into the pot at dereg (SPEC 1.8, P16): zone-minted alpha settles at its
/// own proceeds, older alpha at r: `zone_proceeds + floor((q - zone_minted) * r)`.
/// The caller takes it from P first and then C.
#[must_use]
pub fn short_owed(q: u64, zone_minted: u64, zone_proceeds: u64, r: Price) -> u64 {
    zone_proceeds.saturating_add(mul_price_floor(q.saturating_sub(zone_minted), r))
}

/// What a staker is paid at dereg (SPEC 7.4 phase E): `floor(staked_alpha * r)`.
#[must_use]
pub fn staker_payout(staked_alpha: u64, r: Price) -> u64 {
    mul_price_floor(staked_alpha, r)
}

/// One long's share of the one-shot book sale (SPEC 7.4 phase A, step 3):
/// `p_pos = floor(P_book * a_pos / A_book)`. The dust `P_book - sum p_pos` stays in SubnetTAO.
#[must_use]
pub fn long_sale_share(p_book: u64, a_pos: u64, a_book: u64) -> u64 {
    pro_rata_floor(p_book, a_pos, a_book)
}

/// Lender-first settlement of one long at dereg (SPEC 7.4 phase A, step 4).
/// Invariants: `repay + shortfall == claim` and `repay + refund == proceeds + collateral`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LongSettlement {
    /// Credited to SubnetTAO: `min(claim, p_pos + C)`.
    pub repay: u64,
    /// Part of `repay` taken from the sale share (taken first).
    pub from_proceeds: u64,
    /// Part of `repay` taken from collateral.
    pub from_collateral: u64,
    /// Returned to the owner: `p_pos + C - repay`.
    pub refund: u64,
    /// Written off the lender claim: `claim - repay`.
    pub shortfall: u64,
}

/// Settles one long with sale share `p_pos`, `claim = D_pos + accrued interest` and
/// collateral `C` (SPEC 7.4 phase A, step 4).
#[must_use]
pub fn settle_long(p_pos: u64, claim: u64, collateral: u64) -> LongSettlement {
    let funds = u128::from(p_pos).saturating_add(u128::from(collateral));
    let repay = u64::try_from(funds).map_or(claim, |funds| funds.min(claim));
    let from_proceeds = repay.min(p_pos);
    let from_collateral = repay.saturating_sub(from_proceeds);
    LongSettlement {
        repay,
        from_proceeds,
        from_collateral,
        refund: p_pos
            .saturating_sub(from_proceeds)
            .saturating_add(collateral.saturating_sub(from_collateral)),
        shortfall: claim.saturating_sub(repay),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects)]
    use super::*;
    use crate::math::fixed::Q64;

    #[test]
    fn rate_uses_the_smallest_tao_snapshot() {
        for (flag, ema, a) in [(10, 20, 30), (20, 10, 30), (30, 20, 10)] {
            assert_eq!(rate(flag, ema, a, 40).to_bits(), Q64 / 4);
        }
        assert_eq!(rate(10, 10, 10, 3).to_bits(), 10 * Q64 / 3);
        assert_eq!(rate(10, 10, 10, 0).to_bits(), 10 * Q64);
    }

    #[test]
    fn zone_is_within_the_band_of_the_lowest_ranked_price() {
        let low = Price::from_bits(10_000);
        assert!(in_zone(Price::from_bits(12_500), low, 12_500));
        assert!(!in_zone(Price::from_bits(12_501), low, 12_500));
        assert!(in_zone(Price::max_value(), Price::max_value(), 12_500));
    }

    #[test]
    fn short_pays_r_on_old_alpha_and_its_proceeds_on_zone_alpha() {
        let r = Price::from_bits(Q64 / 4);
        assert_eq!(short_owed(103, 0, 0, r), 25);
        assert_eq!(short_owed(103, 3, 40, r), 65);
        assert_eq!(short_owed(103, 103, 40, r), 40);
        assert_eq!(short_owed(0, 0, 0, r), 0);
        assert_eq!(staker_payout(103, r), 25);
    }

    #[test]
    fn long_sale_shares_floor_and_leave_dust_below_the_position_count() {
        let positions = [3u64, 3, 1];
        let shares: u64 = positions.iter().map(|a| long_sale_share(100, *a, 7)).sum();
        assert_eq!(long_sale_share(100, 3, 7), 42);
        assert_eq!(100 - shares, 2);
        assert_eq!(long_sale_share(100, 3, 0), 0);
    }

    #[test]
    fn lender_is_repaid_from_proceeds_first_then_collateral() {
        let surplus = settle_long(120, 100, 30);
        assert_eq!(
            surplus,
            LongSettlement {
                repay: 100,
                from_proceeds: 100,
                from_collateral: 0,
                refund: 50,
                shortfall: 0
            }
        );
        let dips = settle_long(80, 100, 30);
        assert_eq!(
            dips,
            LongSettlement {
                repay: 100,
                from_proceeds: 80,
                from_collateral: 20,
                refund: 10,
                shortfall: 0
            }
        );
        let short = settle_long(60, 100, 30);
        assert_eq!(
            short,
            LongSettlement {
                repay: 90,
                from_proceeds: 60,
                from_collateral: 30,
                refund: 0,
                shortfall: 10
            }
        );
    }

    #[test]
    fn settlement_conserves_at_the_extremes() {
        for (p, claim, c) in [
            (u64::MAX, u64::MAX, u64::MAX),
            (0, 0, 0),
            (u64::MAX, 1, 0),
            (0, u64::MAX, 5),
        ] {
            let s = settle_long(p, claim, c);
            assert_eq!(
                u128::from(s.repay) + u128::from(s.shortfall),
                u128::from(claim)
            );
            assert_eq!(s.from_proceeds + s.from_collateral, s.repay);
            if p.checked_add(c).is_some() {
                assert_eq!(s.repay + s.refund, p + c);
            }
        }
    }
}
