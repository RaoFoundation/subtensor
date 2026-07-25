#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::AlphaBalance;

use super::mock::*;

/// Marks `locked` alpha as miner collateral so fee logic can only spend the free slice.
pub(super) fn lock_test_miner_collateral(
    netuid: NetUid,
    hotkey: &U256,
    coldkey: &U256,
    locked: AlphaBalance,
) {
    MinerCollateral::<Test>::insert(
        (netuid, hotkey, coldkey),
        MinerCollateralState {
            locked,
            drain_ratio: U64F64::from_num(1),
            min_locked: AlphaBalance::ZERO,
            earned: AlphaBalance::ZERO,
        },
    );
    ColdkeyMinerCollateral::<Test>::insert(netuid, coldkey, locked);
}

/// Drains free TAO down to the existential deposit so fees must fall back to alpha.
pub(super) fn drain_coldkey_to_existential(coldkey: &U256) {
    let current = Balances::free_balance(*coldkey);
    remove_balance_from_coldkey_account(coldkey, current.saturating_sub(ExistentialDeposit::get()));
}
