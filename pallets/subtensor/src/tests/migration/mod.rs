#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! Unit tests for storage / runtime migrations.
//!
//! Split from the former monolithic `tests/migration.rs` into concept modules
//! named after the migration families they cover.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`helpers`] | shared fixtures (`close`, `test_remove_storage_item`, SS58 decode helpers) |
//! | [`associated_evm_address_index`] | AssociatedEvmAddress index + orphan subnet identity cleanup |
//! | [`fix_subnet_hotkey_lock_swaps`] | tao-in refund deployment block + subnet hotkey lock-swap repair |
//! | [`transfer_and_delete_subnets`] | foundation ownership transfer + delete subnet 3/21 |
//! | [`commit_reveal`] | commit-reveal v2/v3, settings, disable, timelocked CR |
//! | [`subnet_volume_emission_flags`] | subnet volume, first emission block, subtoken, zero hotkey alpha |
//! | [`remove_unused_storage`] | orphan / deprecated storage item removals |
//! | [`rate_limit_keys`] | rate-limit key migrations and last-tx block maps |
//! | [`populate_locking_coldkeys`] | populate LockingColdkeys aggregate |
//! | [`fix_staking_and_root_tao`] | staking hotkeys, root TAO/alpha, symbols, registration/nominator settings |
//! | [`auto_stake_destination`] | auto-stake destination migration |
//! | [`network_modality_and_locks`] | network modality removal, subnet limit, lock cost/decay, kappa |
//! | [`reset_unactive_sn`] | reset inactive subnet state |
//! | [`swap_cleanup`] | swap v3 cleanup, coldkey-swap announcements, registration map clear, axon/cert purge |
//! | [`fix_bad_hk_swap_genesis`] | bad hotkey-swap repair — genesis-only cases |
//! | [`fix_bad_hk_swap_mainnet`] | bad hotkey-swap repair — mainnet cases |
//! | [`fix_root_claimed`] | root claimed overclaim repair |
//! | [`subnet_balances_and_issuance`] | subnet balances + total issuance EVM fees |
//! | [`conviction_and_tempo`] | tnet conviction locks + dynamic tempo |

mod associated_evm_address_index;
mod auto_stake_destination;
mod commit_reveal;
mod conviction_and_tempo;
mod fix_bad_hk_swap_genesis;
mod fix_bad_hk_swap_mainnet;
mod fix_root_claimed;
mod fix_staking_and_root_tao;
mod fix_subnet_hotkey_lock_swaps;
mod helpers;
mod network_modality_and_locks;
mod populate_locking_coldkeys;
mod prelude;
mod rate_limit_keys;
mod remove_unused_storage;
mod reset_unactive_sn;
mod subnet_balances_and_issuance;
mod subnet_volume_emission_flags;
mod swap_cleanup;
mod transfer_and_delete_subnets;
