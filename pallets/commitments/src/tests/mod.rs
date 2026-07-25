#![allow(clippy::expect_used, clippy::indexing_slicing)]

//! Unit tests for the commitments pallet.

use codec::Encode;
use sp_std::prelude::*;
use subtensor_runtime_common::{NetUid, TaoBalance};

#[cfg(test)]
use crate::{
    BalanceOf, CommitmentInfo, CommitmentOf, Config, Data, Error, Event, LastBondsReset,
    LastCommitment, MaxSpace, Pallet, Registration, RevealedCommitments, TimelockedIndex,
    UsageTracker, UsedSpaceOf, WeightInfo,
    mock::{
        Balances, DRAND_QUICKNET_SIG_2000_HEX, DRAND_QUICKNET_SIG_HEX, RuntimeEvent, RuntimeOrigin,
        Test, TestMaxFields, insert_drand_pulse, new_test_ext, produce_ciphertext,
    },
};
use frame_support::pallet_prelude::Hooks;
use frame_support::{
    BoundedVec, assert_noop, assert_ok,
    traits::{Currency, Get, ReservableCurrency},
    weights::{Weight, constants::RocksDbWeight},
};
use frame_system::{Pallet as System, RawOrigin};

mod data_type_info;
mod purge_netuid;
mod revealed_commitments;
mod set_commitment;
mod space_limit;
mod timelock_mixed_fields;
mod timelock_reveal;
mod timelocked_index;

/// Runs [`Pallet::purge_netuid`] under a weight meter capped at `limit`.
fn purge_netuid_with_meter(netuid: NetUid, limit: Weight) -> bool {
    let mut weight_meter = frame_support::weights::WeightMeter::with_limit(limit);
    Pallet::<Test>::purge_netuid(netuid, &mut weight_meter)
}
