//! Where an interest charge takes its TAO from, and when a short defaults (SPEC section 1
//! items 2 and 4, I5).
//!
//! Both rules live here and nowhere else, so that they can only change together.
//!
//! Decision (Jacob, Nov 27): interest is paid from the collateral `C` only, and a short
//! defaults when a scheduled charge finds `C == 0` with interest still due. That is
//! [`CollateralOnly`], the launch policy [`Launch`].
//!
//! The alternative that was considered is [`CollateralThenProceeds`]: a charge takes `C` first
//! and then the proceeds `P` (paid to the pool's TAO reserve as interest, like any charge), and
//! the short defaults only when both are exhausted with interest still due. It is compiled for
//! the tests only. `Position::settle_with` and the scheduled charge are generic over
//! [`ChargePolicy`], and the tests run both policies through them, so switching is the one-line
//! edit of the [`Launch`] alias.
//!
//! A touch (open, increase, close, add collateral) charges with [`ChargePolicy::charge_sources`]
//! and never consults [`ChargePolicy::should_default`]: only the scheduled charge of the `Due`
//! queue defaults a position, as in the model (`model/v21/core.py`, `end_block`).

/// The payment-source order of a charge and the default trigger.
pub trait ChargePolicy {
    /// Splits a charge of `due` between the collateral `c` and the proceeds `p`. Returns
    /// `(from_c, from_p, remaining_due)` with `from_c <= c`, `from_p <= p` and
    /// `from_c + from_p + remaining_due == due`.
    fn charge_sources(due: u64, c: u64, p: u64) -> (u64, u64, u64);

    /// Whether a short left with collateral `c_after`, proceeds `p_after` and `remaining_due`
    /// unpaid interest after its scheduled charge defaults.
    fn should_default(c_after: u64, p_after: u64, remaining_due: u64) -> bool;
}

/// The launch policy (Jacob, Nov 27): a charge takes `min(due, C)` from the collateral and
/// nothing from the proceeds; the short defaults iff `C == 0` and interest is still due.
pub struct CollateralOnly;

impl ChargePolicy for CollateralOnly {
    fn charge_sources(due: u64, c: u64, _p: u64) -> (u64, u64, u64) {
        let from_c = due.min(c);
        (from_c, 0, due.saturating_sub(from_c))
    }

    fn should_default(c_after: u64, _p_after: u64, remaining_due: u64) -> bool {
        c_after == 0 && remaining_due > 0
    }
}

/// The alternative, not live: a charge takes the collateral first and then the proceeds; the
/// short defaults only when `C == 0` and `P == 0` with interest still due.
#[cfg(test)]
pub struct CollateralThenProceeds;

#[cfg(test)]
impl ChargePolicy for CollateralThenProceeds {
    fn charge_sources(due: u64, c: u64, p: u64) -> (u64, u64, u64) {
        let from_c = due.min(c);
        let from_p = due.saturating_sub(from_c).min(p);
        (
            from_c,
            from_p,
            due.saturating_sub(from_c).saturating_sub(from_p),
        )
    }

    fn should_default(c_after: u64, p_after: u64, remaining_due: u64) -> bool {
        c_after == 0 && p_after == 0 && remaining_due > 0
    }
}

/// The policy the pallet runs. Changing this alias is the whole switch.
pub type Launch = CollateralOnly;

/// [`ChargePolicy::charge_sources`] of the launch policy.
#[must_use]
pub fn charge_sources(due: u64, c: u64, p: u64) -> (u64, u64, u64) {
    Launch::charge_sources(due, c, p)
}

/// [`ChargePolicy::should_default`] of the launch policy.
#[must_use]
pub fn should_default(c_after: u64, p_after: u64, remaining_due: u64) -> bool {
    Launch::should_default(c_after, p_after, remaining_due)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects)]
    use super::*;

    /// What every policy must satisfy for the charge code to stay sound: the split never takes
    /// more than a source holds and accounts for every rao of `due`.
    fn split_is_sound<P: ChargePolicy>(due: u64, c: u64, p: u64) {
        let (from_c, from_p, remaining) = P::charge_sources(due, c, p);
        assert!(from_c <= c);
        assert!(from_p <= p);
        assert_eq!(
            u128::from(from_c) + u128::from(from_p) + u128::from(remaining),
            u128::from(due)
        );
        // Nothing stays due while the first source still holds TAO.
        if remaining > 0 {
            assert_eq!(c - from_c, 0);
        }
    }

    const EDGES: [u64; 7] = [0, 1, 2, 999, 1_000, u64::MAX - 1, u64::MAX];

    #[test]
    fn both_policies_split_soundly_at_the_edges() {
        for due in EDGES {
            for c in EDGES {
                for p in EDGES {
                    split_is_sound::<CollateralOnly>(due, c, p);
                    split_is_sound::<CollateralThenProceeds>(due, c, p);
                }
            }
        }
    }

    #[test]
    fn launch_takes_collateral_only_and_defaults_at_zero_collateral() {
        assert_eq!(charge_sources(100, 100, 500), (100, 0, 0));
        assert_eq!(charge_sources(101, 100, 500), (100, 0, 1));
        assert_eq!(charge_sources(40, 100, 500), (40, 0, 0));
        assert_eq!(charge_sources(0, 0, 500), (0, 0, 0));
        // Collateral exactly covering the charge: nothing due, no default.
        assert!(!should_default(0, 500, 0));
        // One rao short: default, whatever the proceeds.
        assert!(should_default(0, 500, 1));
        assert!(should_default(0, 0, 1));
        // Interest due against collateral a touch added back is not a default.
        assert!(!should_default(1, 500, 1));
    }

    #[test]
    fn the_alternative_spends_proceeds_before_it_defaults() {
        type Alt = CollateralThenProceeds;
        assert_eq!(Alt::charge_sources(100, 100, 500), (100, 0, 0));
        assert_eq!(Alt::charge_sources(101, 100, 500), (100, 1, 0));
        assert_eq!(Alt::charge_sources(700, 100, 500), (100, 500, 100));
        assert!(!Alt::should_default(0, 499, 0));
        assert!(!Alt::should_default(0, 1, 1));
        assert!(Alt::should_default(0, 0, 1));
        assert!(!Alt::should_default(0, 0, 0));
    }

    #[test]
    fn a_default_follows_from_the_split_only_when_the_sources_are_spent() {
        for due in EDGES {
            for c in EDGES {
                for p in EDGES {
                    let (from_c, from_p, remaining) = CollateralOnly::charge_sources(due, c, p);
                    assert_eq!(
                        CollateralOnly::should_default(c - from_c, p - from_p, remaining),
                        due > c
                    );
                    let (from_c, from_p, remaining) =
                        CollateralThenProceeds::charge_sources(due, c, p);
                    assert_eq!(
                        CollateralThenProceeds::should_default(c - from_c, p - from_p, remaining),
                        u128::from(due) > u128::from(c) + u128::from(p)
                    );
                }
            }
        }
    }
}
