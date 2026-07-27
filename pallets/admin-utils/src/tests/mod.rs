//! Unit tests for `pallet-admin-utils`, split by concept for discoverability.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

/// Test runtime / helpers shared by admin-utils unit tests and benchmarks.
pub mod mock;

mod admin_windows_rate_limits;
mod alpha_commit_reveal;
mod consensus_hyperparams;
mod evm_grandpa_precompile;
mod mechanisms;
mod registration_burn;
mod stake_delegate_take;
mod subnet_owner_misc;
mod uids_validators;
mod weights_difficulty;

#[allow(unused_imports)] // used by `impl_benchmark_test_suite!` in benchmarking.rs
pub use mock::{Test, new_test_ext};

/// Shared imports for concept test modules under this directory.
pub(crate) mod prelude {
    pub(crate) use crate::{Error, pallet::PrecompileEnable};
    pub(crate) use frame_support::{
        assert_err, assert_noop, assert_ok,
        dispatch::{DispatchClass, GetDispatchInfo, Pays},
        sp_runtime::DispatchError,
        traits::{Currency as _, Hooks},
    };
    pub(crate) use frame_system::Config;
    pub(crate) use pallet_subtensor::{
        Error as SubtensorError, Event, MaxRegistrationsPerBlock, SubnetOwner,
        TargetRegistrationsPerInterval, Tempo, WeightsVersionKeyRateLimit,
        subnets::mechanism::MAX_MECHANISM_COUNT_PER_SUBNET, utils::rate_limiting::TransactionType,
        *,
    };
    pub(crate) use sp_consensus_grandpa::AuthorityId as GrandpaId;
    pub(crate) use sp_core::{Get, Pair, U256, ed25519};
    pub(crate) use sp_runtime::PerU16;
    pub(crate) use substrate_fixed::types::I96F32;
    pub(crate) use subtensor_runtime_common::{MechId, NetUid, TaoBalance, Token};

    pub(crate) use super::mock::*;
}
