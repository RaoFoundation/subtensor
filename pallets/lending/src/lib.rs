//! Synthetic-mint shorts and pool-lifted longs.
//!
//! The normative spec is `docs/SPEC.md` and the code design is `docs/DESIGN-v2.md`.
//! This crate currently holds the pure numeric core ([`math`]) and the root-settable
//! parameter groups ([`params`]); the storage-facing modules listed in the design build on them.
#![cfg_attr(not(feature = "std"), no_std)]

pub mod math;
pub mod params;

#[cfg(test)]
mod tests;
