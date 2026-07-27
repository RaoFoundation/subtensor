#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! Shared imports for migration unit tests.

pub use super::super::mock::*;
pub use crate::staking::lock::LockState;
pub use crate::*;
pub use alloc::collections::BTreeMap;
pub use approx::{assert_abs_diff_eq, assert_relative_eq};
pub use codec::{Decode, Encode};
pub use frame_support::{
    StorageHasher, Twox64Concat, assert_ok,
    storage::unhashed::{get, get_raw, put, put_raw},
    storage_alias,
    traits::{Currency, StorageInstance, StoredMap, fungible::Inspect},
    weights::Weight,
};
pub use safe_math::SafeDiv;

pub use crate::migrations::migrate_coldkey_swap_scheduled_to_announcements::deprecated as coldkey_swap_deprecated;
pub use frame_support::traits::Bounded;
pub use frame_system::Config;
pub use pallet_drand::types::RoundNumber;
pub use pallet_scheduler::ScheduledOf;
pub use scale_info::prelude::collections::VecDeque;
pub use sp_core::{H160, H256, U256, crypto::Ss58Codec};
pub use sp_io::hashing::twox_128;
pub use sp_runtime::{
    AccountId32, PerU16,
    traits::{Hash, Zero},
};
pub use sp_std::marker::PhantomData;
pub use substrate_fixed::types::{I96F32, U64F64};
pub use substrate_fixed::{traits::ToFixed, types::extra::U2};
pub use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance};
