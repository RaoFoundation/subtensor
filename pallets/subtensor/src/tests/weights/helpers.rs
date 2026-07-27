#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Shared helpers for weights extrinsic tests.

use frame_support::dispatch::DispatchResult;
use sp_core::H256;
use sp_core::U256;
use sp_runtime::traits::{BlakeTwo256, Hash};

use crate::tests::mock::*;
use crate::*;

pub(super) fn commit_reveal_set_weights(
    hotkey: U256,
    netuid: NetUid,
    uids: Vec<u16>,
    weights: Vec<u16>,
    salt: Vec<u16>,
    version_key: u64,
) -> DispatchResult {
    SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

    let commit_hash: H256 = BlakeTwo256::hash_of(&(
        hotkey,
        netuid,
        uids.clone(),
        weights.clone(),
        salt.clone(),
        version_key,
    ));

    SubtensorModule::commit_weights(RuntimeOrigin::signed(hotkey), netuid, commit_hash)?;

    step_epochs(1, netuid);

    SubtensorModule::reveal_weights(
        RuntimeOrigin::signed(hotkey),
        netuid,
        uids,
        weights,
        salt,
        version_key,
    )?;

    Ok(())
}
