#![allow(unused, clippy::indexing_slicing, clippy::panic, clippy::unwrap_used)]
//! Integration tests for subnet-scoped hotkey swap ([`crate::swap::swap_hotkey`]).
//!
//! Layout mirrors `swap/swap_hotkey.rs` concepts: ownership, membership/serve
//! metadata, stake transfer, parent/child maps, rate limits, revert paths, and
//! root claims.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`owner_identity`] | Owner / OwnedHotkeys / Delegates / ownership errors / subnet-owner hotkey |
//! | [`membership_serve`] | membership, UIDs/keys, Prometheus, axons, certificates, weight commits, loaded emission |
//! | [`stake_transfer`] | total stake, staking-hotkey indexes, V1/V2 alpha, keep_stake, multi coldkey/subnet |
//! | [`parent_child_maps`] | ChildKeys / ParentKeys maps and auto parent-delegation |
//! | [`rate_limits`] | `HotkeySwapOnSubnetInterval` / `LastHotkeySwapOnNetuid` |
//! | [`revert_swap`] | swap-back / revert preserves stake, maps, dividends, voting power, claims |
//! | [`root_claims`] | root claim rows transfer on root / all-subnet vs non-root |

mod membership_serve;
mod owner_identity;
mod parent_child_maps;
mod rate_limits;
mod revert_swap;
mod root_claims;
mod stake_transfer;
