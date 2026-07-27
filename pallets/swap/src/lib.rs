//! # Subtensor Swap Pallet
//!
//! Weighted-balancer AMM for TAO↔alpha swaps on dynamic subnets (`mechanism == 1`).
//!
//! Core surfaces agents search for:
//! - [`Pallet::do_swap`] / [`SwapHandler`] — execute or simulate a swap
//! - [`Pallet::adjust_protocol_liquidity`] — inject protocol TAO/alpha without moving price
//! - [`Balancer`] — pool math (weights, price, reserve deltas)
//! - [`FeeRate`] / [`SwapBalancer`] — per-`netuid` fee and weight state
//!
//! User LP extrinsics (`add_liquidity`, etc.) are deprecated stubs; liquidity is protocol-owned.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod pallet;
pub mod weights;

pub use pallet::*;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;

#[cfg(test)]
pub(crate) mod mock;
