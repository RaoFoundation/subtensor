//! Hotkey / coldkey identity swap (lineage + migration), not the TAO↔alpha AMM.
//!
//! This module lives under `pallets/subtensor/src/swap` and renames SS58 identity
//! across stake, ownership, and subnet membership. The AMM / liquidity pallet is
//! `pallets/swap` (`pallet-swap`) — different crate, different concern.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`swap_coldkey`] | Coldkey rename: stake, subnet ownership, owned hotkeys, locks, TAO |
//! | [`swap_hotkey`] | Hotkey rename on one subnet or all subnets (`keep_stake` paths) |
//! | [`coldkey_lineage`] | Global [`ColdkeyRoot`] / [`ColdkeySuccessor`] continuity maps |
//! | [`hotkey_lineage`] | Per-netuid [`HotkeyRoot`] / [`HotkeySuccessor`] + swap cooldown stamp |
//!
//! Extrinsics call [`Pallet::do_swap_coldkey`] / [`Pallet::do_swap_hotkey`]; lineage
//! helpers are the O(1) identity surface for bans/indexers after a successful swap.

use super::*;
pub mod coldkey_lineage;
pub mod hotkey_lineage;
pub mod swap_coldkey;
pub mod swap_hotkey;
