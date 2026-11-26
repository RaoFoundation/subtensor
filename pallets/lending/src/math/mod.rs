//! Pure numeric core (DESIGN-v2 section 4): plain values in, plain values out.
//!
//! Nothing here reads storage or depends on the rest of the pallet. Every function is total:
//! it never panics, and an amount the spec calls infinite is reported as `None`. The functions
//! agree with the reference model (`model/v201`) to the rao; `tests/golden.rs` replays the
//! model's vectors.
//!
//! Rounding always favours the pool (DESIGN-v2 principle 4): costs and needs round up, values
//! and payouts round down, and each site names its direction through the helpers in [`fixed`].

pub mod coverage;
pub mod curve;
pub mod dereg;
pub mod fixed;
pub mod health;
pub mod interest;
pub mod pace;
pub mod ring;

use fixed::Price;

/// The three price references of a subnet (SPEC 1.1, P6, P13), all rao TAO per rao alpha.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PriceRefs {
    /// Live pool spot.
    pub spot: Price,
    /// Lower median of the TWAP ring (P6).
    pub twap: Price,
    /// 30-day SMA of raw spot used for dereg ranking (P13).
    pub sma: Price,
}
